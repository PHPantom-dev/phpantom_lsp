//! Precomputed symbol-location map for a single PHP file.
//!
//! During `update_ast`, every navigable symbol occurrence (class reference,
//! member access, variable, function call, etc.) is recorded as a
//! [`SymbolSpan`] in a flat, sorted vec.  At request time a binary search
//! on this vec replaces character-level backward-walking and
//! provides instant rejection when the cursor lands on whitespace, a
//! string literal, a comment, or any other non-navigable token.
//!
//! The map also stores variable definition sites ([`VarDefSite`]) and
//! scope boundaries so that go-to-definition for `$variable` can be
//! answered entirely from precomputed data without re-parsing.
//!
//! Docblock type references (from `@param`, `@return`, `@var`,
//! `@template`, `@method`, etc.) are extracted by parsing each
//! docblock's own PHPDoc CST during the AST walk, since docblocks are
//! trivia in the `mago_syntax` AST and produce no expression/statement
//! nodes.
//!
//! The module is split into submodules:
//!
//! - [`docblock`] — Docblock symbol extraction helpers (type span
//!   emission, `@template` / `@method` tag scanning, navigability
//!   filtering, and `get_docblock_text_with_offset`)
//! - [`extraction`] — AST walk that builds a [`SymbolMap`] from a
//!   parsed PHP program (`extract_symbol_map` and all
//!   `extract_from_*` helpers)
//! - [`call_sites`] — the call sites signature help reads, stored in
//!   shared arenas
//! - [`laravel_spans`] — Laravel string keys and the spans that depend
//!   on a workspace symbol being absent

mod call_sites;
pub(crate) mod docblock;
pub(crate) mod extraction;
pub(crate) mod laravel_resources;
mod laravel_spans;
mod morph_columns;

use crate::atom::Atom;
use crate::php_type::PhpType;

pub(crate) use call_sites::*;
#[cfg(test)]
pub(crate) use extraction::extract_symbol_map;
pub(crate) use extraction::extract_symbol_map_for_index;
pub(crate) use laravel_spans::*;
pub(crate) use morph_columns::{MorphColumnReceiver, MorphColumnSite};

// ─── Data structures ────────────────────────────────────────────────────────

/// A single navigable symbol occurrence in a file.
///
/// Stored in a sorted vec keyed by `start` offset so that a binary
/// search can locate the symbol (or gap) at any byte position in O(log n).
#[derive(Debug, Clone)]
pub(crate) struct SymbolSpan {
    /// Byte offset of the first character of this symbol token.
    pub start: u32,
    /// Byte offset one past the last character of this symbol token.
    pub end: u32,
    /// What kind of navigable symbol this is.
    pub kind: SymbolKind,
}

/// Which flavour of class-self-reference keyword a `SelfStaticParent`
/// span represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelfStaticParentKind {
    /// The `self` keyword.
    Self_,
    /// The `static` keyword (late static binding).
    Static,
    /// The `parent` keyword.
    Parent,
    /// The `$this` pseudo-variable.
    This,
}

pub(crate) fn self_static_parent_kind(name: &str) -> Option<SelfStaticParentKind> {
    match name {
        "self" => Some(SelfStaticParentKind::Self_),
        "static" => Some(SelfStaticParentKind::Static),
        "parent" => Some(SelfStaticParentKind::Parent),
        "$this" => Some(SelfStaticParentKind::This),
        _ => None,
    }
}

/// The syntactic context in which a `ClassReference` appears.
///
/// Used by the invalid-class-kind diagnostic to check whether the
/// referenced class's kind (class, interface, trait, enum) is valid
/// for the position it appears in.  The completion system uses the
/// parallel [`ClassNameContext`](crate::completion::context::class_completion::ClassNameContext)
/// enum for the same purpose at completion time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ClassRefContext {
    /// Context not determined or not relevant for diagnostics.
    #[default]
    Other,
    /// After `new` keyword — only concrete (non-abstract) classes and enums.
    New,
    /// After `extends` in a class declaration.
    ExtendsClass,
    /// After `extends` in an interface declaration.
    ExtendsInterface,
    /// After `implements` in a class or enum declaration.
    Implements,
    /// After `use` inside a class body — trait use statement.
    TraitUse,
    /// RHS of `instanceof` operator.
    Instanceof,
    /// In a `catch (X $e)` type hint.
    Catch,
    /// In a native type-hint position (parameter type, return type,
    /// property type).
    TypeHint,
    /// In a `use` import statement at file level.
    UseImport,
    /// As a PHP attribute (`#[Foo(...)]`).  Like `New`, this invokes the
    /// class constructor, but — unlike `New` — it is valid on any
    /// instantiable class and does not produce "cannot instantiate"
    /// diagnostics.
    Attribute,
    /// As the subject of PHPUnit coverage metadata: a `@covers` / `@uses` /
    /// `@coversDefaultClass` tag, or a `#[CoversClass]` / `#[UsesClass]` /
    /// `#[CoversTrait]` / `#[UsesTrait]` / `#[CoversMethod]` /
    /// `#[UsesMethod]` attribute argument.
    ///
    /// The reference names a code unit rather than using it, so it
    /// constrains nothing about the class' kind and does not autoload.  It
    /// is tagged so the "which tests cover this class" code lens can pick
    /// coverage references out of the reference index without re-parsing.
    CoversTarget,
    /// As the operand of a docblock type operator: `key-of<X>`,
    /// `value-of<X>`, or the base of `X[K]`.
    ///
    /// A bare name there is an *array-typed* thing rather than a class most
    /// of the time — a constant holding an array literal, a `@template`
    /// parameter, or a type alias — so the unknown-class diagnostic checks
    /// for those readings before reporting the name as a missing class. A
    /// backed enum (`value-of<Suit>`) is the one genuine class operand, so
    /// the reference is still recorded and still navigable.
    TypeOperatorOperand,
    /// As the target of a `@see` or `{@see …}` docblock tag.
    ///
    /// `@see` legally carries URIs, prose, and naming suggestions in
    /// addition to FQSENs, so an unresolvable target is not an error.
    DocblockSee,
}

/// How a [`SymbolKind::MemberAccess`] span names its member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DocblockMemberRef {
    /// A real `->` / `?->` / `::` access written in PHP code.
    No,
    /// The target of a `@see` or `{@see …}` tag.
    ///
    /// `@see` legally carries URIs, prose, and naming suggestions in
    /// addition to FQSENs, so a target that resolves to nothing is not an
    /// error.
    See,
    /// PHPUnit metadata naming a member: coverage (a `@covers` / `@uses`
    /// tag, a `@covers ::name` under a `@coversDefaultClass`, or a
    /// `#[CoversMethod]` / `#[UsesMethod]` attribute), a data provider
    /// (`@dataProvider`, `#[DataProvider]`, `#[DataProviderExternal]`), or
    /// a test dependency (`@depends`, `#[Depends]` and its variants).
    ///
    /// Unlike `@see`, the target names a code unit that has to exist —
    /// PHPUnit errors out on metadata that names nothing — so it is still
    /// checked.
    PhpUnit,
}

impl DocblockMemberRef {
    /// Whether the member is *named* rather than accessed, which is what
    /// earns the relaxed member lookup: the subject is a class name rather
    /// than a runtime expression, and a property hook or an `@method` is a
    /// legitimate target.
    pub(crate) fn is_reference(self) -> bool {
        !matches!(self, Self::No)
    }

    /// Whether a target that resolves to nothing is acceptable rather than
    /// a diagnostic.
    pub(crate) fn tolerates_missing_target(self) -> bool {
        matches!(self, Self::See)
    }
}

/// Text a [`SymbolMap`]'s offsets are known to index into.
///
/// A map records byte offsets into whatever content it was extracted
/// from.  Slicing them against a *different* revision of the same file
/// reads unrelated bytes, and runs off the end as soon as the other
/// revision is shorter — which panics, taking the whole worker thread
/// with it.  The two revisions are easy to mix up: the map for a Blade
/// template describes the virtual PHP it lowers to rather than the
/// template's own bytes, and a map cached per URI outlives the buffer it
/// was built from.
///
/// `MappedSource` is the proof that a map and a string belong together.
/// It can only be obtained from [`SymbolMap::source`], which checks the
/// pairing, so a consumer that holds one cannot be holding the wrong
/// text.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MappedSource<'a>(&'a str);

impl<'a> MappedSource<'a> {
    /// The source text itself, for the parts of a consumer that need a
    /// plain `&str`.
    pub(crate) fn text(self) -> &'a str {
        self.0
    }
}

/// The subject (LHS) text of a [`SymbolKind::MemberAccess`] span.
///
/// Most subjects are a verbatim slice of the source (a plain variable,
/// `$this`, a bare class name), so `Range` stores just the byte offsets
/// and re-slices the file content on demand instead of paying for an
/// allocation per span. `Owned` is the fallback for subjects synthesised
/// from the AST that diverge from the raw bytes at that span: `(new
/// Foo($x))->bar()` lowers to `"Foo"` (dropping `new` and the constructor
/// arguments), whitespace around `->`/`::` is normalised away, and
/// expressions the lowering can't represent collapse to `""`.
#[derive(Debug, Clone)]
pub(crate) enum SubjectText {
    /// Byte range `[start, end)` into the file content whose raw slice is
    /// exactly the synthesised subject text.
    Range { start: u32, end: u32 },
    /// The synthesised subject text, stored because it diverges from (or
    /// has no corresponding) raw source span.
    Owned(Box<str>),
}

impl SubjectText {
    /// Build from a synthesised `text` and the raw span `[start, end)` of
    /// the expression it was derived from. Reuses the raw bytes when they
    /// match `text` exactly, falling back to an owned allocation otherwise.
    pub(crate) fn new(text: String, start: u32, end: u32, content: &str) -> Self {
        match content.get(start as usize..end as usize) {
            Some(raw) if raw == text => Self::Range { start, end },
            _ => Self::Owned(text.into_boxed_str()),
        }
    }

    /// Build from an already-synthesised `text` with no known matching
    /// source span (e.g. a literal `"self"` fallback, or text resolved
    /// from elsewhere in a chain).
    pub(crate) fn owned(text: String) -> Self {
        Self::Owned(text.into_boxed_str())
    }

    /// The subject text, slicing the source the owning map was extracted
    /// from for the `Range` case.
    ///
    /// Taking a [`MappedSource`] rather than a bare `&str` is what keeps
    /// the offsets and the text in step: see the type's documentation.
    pub(crate) fn as_str<'a>(&'a self, source: MappedSource<'a>) -> &'a str {
        match self {
            Self::Range { start, end } => &source.text()[*start as usize..*end as usize],
            Self::Owned(s) => s,
        }
    }

    /// Memory-audit tooling: heap bytes held by this subject text.
    #[cfg(feature = "mem-audit")]
    pub(crate) fn heap_bytes(&self) -> usize {
        match self {
            Self::Range { .. } => 0,
            Self::Owned(s) => s.len(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum SymbolKind {
    /// Class/interface/trait/enum name in a type context:
    /// type hint, `new Foo`, `extends Foo`, `implements Foo`,
    /// `use` statement target, `catch (Foo $e)`, etc.
    ClassReference {
        name: Atom,
        /// `true` when the original PHP source used a leading `\`
        /// (fully-qualified name).  When set, the resolver should use the
        /// name as-is without prepending the file's namespace.
        is_fqn: bool,
        /// The syntactic context this reference appears in.  Used by
        /// the invalid-class-kind diagnostic to validate that the
        /// referenced class's kind matches the position.
        context: ClassRefContext,
    },
    /// Class/interface/trait/enum name at its *declaration* site
    /// (`class Foo`, `interface Bar`, etc.).  Go-to-definition returns
    /// the symbol's own location so editors can fall back to
    /// Find References.  Also useful for document highlights.
    ClassDeclaration { name: Atom },

    /// Member name on the RHS of `->`, `?->`, or `::`.
    /// `subject_text` is the source text of the LHS expression.
    MemberAccess {
        subject_text: SubjectText,
        member_name: Atom,
        is_static: bool,
        is_method_call: bool,
        /// Whether this span was extracted from a docblock tag or a
        /// coverage attribute (e.g. `@see Order::$channel_type`) rather
        /// than real PHP code, and if so which kind.  A named member gets
        /// a relaxed lookup because the subject is a class name rather
        /// than a runtime expression.
        docblock_ref: DocblockMemberRef,
        /// `true` when this span was synthesised from a two-element
        /// `[Class::class, 'method']` / `[$obj, 'method']` array literal
        /// rather than a real `->`/`::` access.  These spans are a
        /// best-effort navigation aid: without type-flow analysis we
        /// cannot tell a callable array from a plain data pair (e.g.
        /// `return [[Foo::class, 'name'], ...]`), so diagnostics skip
        /// them to avoid false "method not found" errors.  Navigation,
        /// hover, and semantic tokens still use them because they only
        /// surface information when the member actually resolves.
        is_array_callable: bool,
        /// `true` when this access used the nullsafe operator (`?->`)
        /// rather than plain `->`.  A nullsafe access short-circuits to
        /// `null` without touching the member when its subject is
        /// `null`, so diagnostics must not flag that one case the way
        /// they flag a plain `->` on a `null`-typed subject.
        is_nullsafe: bool,
    },

    /// A `$variable` token (usage or definition site).
    Variable {
        /// Name without `$` prefix.
        name: Atom,
    },

    /// A local variable referenced by a bare name rather than by `$name`:
    /// a string literal argument inside `compact('var')`, or the name in a
    /// `"${var}"` string interpolation.
    ///
    /// The span covers just the name so rename and go-to-definition can
    /// treat it as a variable reference while still replacing only the bare
    /// name text.
    CompactVariable {
        /// Variable name without `$` prefix.
        name: Atom,
    },

    /// Standalone function call name (not a method call).
    ///
    /// When `is_definition` is `true`, the span covers the function name
    /// at its *declaration* site (`function foo() {}`).  When `false`, it
    /// covers a call site (`foo()`).  The distinction is needed by the
    /// unknown-function diagnostic (which must skip definitions) and by
    /// find-references / document-highlight (which may want to include
    /// both).
    FunctionCall {
        name: Atom,
        is_definition: bool,
        /// `true` when this span was extracted from an unqualified `@see`
        /// reference (`{@see covers()}`) rather than real PHP code.  Such a
        /// reference names a member of the class the docblock documents
        /// before it names a global function, which is how phpDocumentor
        /// and PHPStorm read it, and `@see` legally carries prose besides
        /// an FQSEN, so a name that resolves to nothing is not an error.
        ///
        /// PHPUnit coverage metadata shares the `@see` emitter but neither
        /// reading applies to it, so `retag_as_covers_target` clears this;
        /// see [`DocblockMemberRef`] for the member equivalent.
        is_docblock_reference: bool,
    },

    /// `self`, `static`, `parent`, or `$this` in a navigable context.
    SelfStaticParent(SelfStaticParentKind),

    /// A constant name in a navigable context (`define()` name,
    /// class constant access, standalone constant reference).
    ///
    /// `is_definition` follows the same convention as `FunctionCall`: `true`
    /// at the declaration site (`const FOO = 1;`), `false` everywhere else,
    /// so find-references can exclude the declaration when asked to.
    ConstantReference { name: Atom, is_definition: bool },

    /// A method, property, or constant name at its *declaration* site.
    ///
    /// Go-to-definition returns the symbol's own location so editors
    /// can fall back to Find References.  Also needed for
    /// find-references and rename so that the declaration site
    /// participates in the match.
    MemberDeclaration {
        /// The member name (e.g. `"save"`, `"name"`, `"MAX_SIZE"`).
        /// For properties this is the name WITHOUT the `$` prefix.
        name: Atom,
        /// Whether this is a static member (`static function`, `static $prop`,
        /// or class constant — constants are always accessed statically).
        is_static: bool,
    },

    /// A namespace name at its declaration site (`namespace App\Models;`).
    /// Used by the rename handler to support namespace renaming.
    NamespaceDeclaration {
        /// The full namespace name (e.g. `"App\\Models"`).
        name: Atom,
    },

    /// A PHP keyword token (e.g. `as`, `foreach`, `if`, `new`, `use`).
    /// Emitted so that semantic tokens can highlight keywords in Blade
    /// files where Tree-sitter's PHP grammar is not available.
    Keyword,

    /// A cast-type name inside a cast expression (e.g. `string` in
    /// `(string)$x`).  Tree-sitter marks these as `type.builtin`.
    CastType,

    /// A comment token (single-line `//`, one line of a multi-line `/* */`
    /// or docblock `/** */`, or hash `#`).  Emitted from AST trivia so that
    /// Blade files get comment highlighting.  Multi-line block comments are
    /// split into one span per source line at extraction time.
    Comment,

    /// A Laravel string-key literal (config key, route name, view name, etc.)
    /// inside a framework helper call such as `config()`, `route()`, `view()`,
    /// `Config::get()`, or `Config::set()`.
    ///
    /// The span covers the string content *inside* the quotes so that
    /// go-to-definition and find-references can work without re-parsing
    /// the file at request time.
    LaravelStringKey {
        /// The category of the key (e.g. `Config`, `Route`, `View`).
        kind: LaravelStringKind,
        /// The key value, e.g. `"app.name"` or `"users.index"`.
        key: String,
        /// Whether the call *defines* the key rather than reading it, as
        /// `Config::set('services.acme.token', …)` does.  A write is a
        /// declaration site: it may name a key that exists nowhere else.
        is_write: bool,
        /// Whether the call tolerates the key naming nothing, as one
        /// candidate of an `@includeFirst(['custom.header', 'partials.header'])`
        /// does: the directive renders whichever exists, so a candidate that
        /// does not is the point rather than a mistake.
        is_optional: bool,
    },

    /// The string-literal name argument inside a Laravel `::macro('name', ...)`
    /// registration.
    ///
    /// The span covers the string content inside the quotes so find-references
    /// and rename can link the registration site with macro call sites.
    LaravelMacroString {
        /// Macro method name, e.g. `"sumPrices"`.
        name: String,
    },

    /// The string-literal parameter name inside a console command's own
    /// `$this->argument('user')` / `$this->option('queue')` call.
    ///
    /// The span covers the string content inside the quotes.  Hover and
    /// diagnostics resolve it against the enclosing command class's parsed
    /// `$signature` (looked up on demand via the span offset).
    CommandOwnParam {
        /// The parameter name, e.g. `"user"` or `"queue"`.
        name: String,
        /// `true` for `option()`, `false` for `argument()`.
        is_option: bool,
    },
}

impl SymbolKind {
    /// Memory-audit tooling: heap bytes held by this span's strings.
    ///
    /// `Atom` fields are globally interned (stored once in the intern
    /// table, not per-span), so they contribute nothing here.
    #[cfg(feature = "mem-audit")]
    pub(crate) fn audit_heap(&self) -> usize {
        match self {
            Self::ClassReference { .. }
            | Self::ClassDeclaration { .. }
            | Self::Variable { .. }
            | Self::CompactVariable { .. }
            | Self::FunctionCall { .. }
            | Self::ConstantReference { .. }
            | Self::MemberDeclaration { .. }
            | Self::NamespaceDeclaration { .. } => 0,
            Self::LaravelMacroString { name, .. } | Self::CommandOwnParam { name, .. } => {
                name.capacity()
            }
            Self::MemberAccess { subject_text, .. } => subject_text.heap_bytes(),
            Self::LaravelStringKey { key, .. } => key.capacity(),
            Self::SelfStaticParent(_) | Self::Keyword | Self::CastType | Self::Comment => 0,
        }
    }
}

// ─── Template parameter definition site structures ──────────────────────────

/// A `@template` parameter definition site discovered during docblock extraction.
///
/// Stored in `SymbolMap::template_defs`, sorted by `name_offset`.
/// When a `ClassReference` cannot be resolved to an actual class, the
/// resolver checks whether it matches a template parameter in scope and
/// jumps to the `@template` tag that declares it.
#[derive(Debug, Clone)]
pub(crate) struct TemplateParamDef {
    /// Byte offset of the template parameter *name* token (e.g. the `T`
    /// in `@template T of Foo`).
    pub name_offset: u32,
    /// Template parameter name (e.g. `"TKey"`, `"TModel"`).
    pub name: String,
    /// Upper bound from the `of` clause (e.g. `PhpType::named("array-key")`
    /// for `@template TKey of array-key`), or `None` when unbounded.
    pub bound: Option<PhpType>,
    /// Variance annotation from the `@template` tag.
    pub variance: crate::types::TemplateVariance,
    /// Start of the scope where this template parameter is visible: the
    /// docblock's own start offset, whether the template is declared at
    /// class level or method/function level.
    pub scope_start: u32,
    /// End of the scope where this template parameter is visible.
    /// For class-level templates this is the class closing-brace offset;
    /// for method-level templates it is the method closing-brace offset;
    /// for function-level templates it is the function closing-brace offset.
    /// For an abstract or interface method (no body), this is the end of
    /// the method's own signature span instead.
    pub scope_end: u32,
}

// ─── Nested range index ─────────────────────────────────────────────────────

/// Ranges `(start, end)`, both inclusive, sorted by start and indexed so
/// the innermost one containing an offset is found without testing every
/// range in the file.
///
/// A long top-level script holds thousands of sibling blocks, and the
/// diagnostic pass asks for the innermost one at every member access, so
/// a linear scan here makes the pass quadratic in the file's length.
#[derive(Debug, Clone, Default)]
pub(crate) struct NestedRanges {
    ranges: Vec<(u32, u32)>,
    /// For each range, the index of the closest earlier range that ends
    /// after it does, or [`Self::NONE`].  A range that ends before an
    /// offset rules out every range between it and this one, since those
    /// all end no later than it.
    wider: Vec<u32>,
}

impl NestedRanges {
    const NONE: u32 = u32::MAX;

    pub(crate) fn new(mut ranges: Vec<(u32, u32)>) -> Self {
        ranges.sort_by_key(|r| r.0);
        let mut wider = Vec::with_capacity(ranges.len());
        for (i, &(_, end)) in ranges.iter().enumerate() {
            let mut candidate = i.checked_sub(1).map_or(Self::NONE, |p| p as u32);
            while candidate != Self::NONE && ranges[candidate as usize].1 <= end {
                candidate = wider[candidate as usize];
            }
            wider.push(candidate);
        }
        Self { ranges, wider }
    }

    /// The start of the range with the greatest start that contains
    /// `offset`, or `0` when none does.
    pub(crate) fn innermost_start(&self, offset: u32) -> u32 {
        let mut candidate = match self.ranges.partition_point(|r| r.0 <= offset) {
            0 => return 0,
            n => (n - 1) as u32,
        };
        while candidate != Self::NONE {
            let (start, end) = self.ranges[candidate as usize];
            if offset <= end {
                return start;
            }
            candidate = self.wider[candidate as usize];
        }
        0
    }

    /// Heap bytes held, for the memory audit.
    #[cfg(feature = "mem-audit")]
    pub(crate) fn heap_bytes(&self) -> usize {
        self.ranges.capacity() * size_of::<(u32, u32)>() + self.wider.capacity() * size_of::<u32>()
    }
}

impl std::ops::Deref for NestedRanges {
    type Target = [(u32, u32)];

    fn deref(&self) -> &Self::Target {
        &self.ranges
    }
}

impl<'a> IntoIterator for &'a NestedRanges {
    type Item = &'a (u32, u32);
    type IntoIter = std::slice::Iter<'a, (u32, u32)>;

    fn into_iter(self) -> Self::IntoIter {
        self.ranges.iter()
    }
}

// ─── Variable definition site structures ────────────────────────────────────

/// A variable definition site discovered during the AST walk.
///
/// Stored in `SymbolMap::var_defs`, sorted by `(scope_start, offset)`,
/// so that go-to-definition for `$var` can be answered entirely from
/// the precomputed map without any scanning at request time.
#[derive(Debug, Clone)]
pub(crate) struct VarDefSite {
    /// Byte offset of the `$var` token at the definition site.
    pub offset: u32,
    /// Variable name *without* `$` prefix.
    pub name: String,
    /// What kind of definition this is.
    pub kind: VarDefKind,
    /// Byte offset of the enclosing scope's opening brace (method body,
    /// function body, closure body) or `0` for top-level code.  Used to
    /// scope the backward search to the correct function/method.
    pub scope_start: u32,
    /// Byte offset from which this definition becomes "visible".
    ///
    /// For **assignments** (`$x = expr;`), this is the end of the
    /// statement — the RHS of an assignment still sees the *previous*
    /// definition of the variable, not the one being written.
    ///
    /// For **parameters**, **foreach**, **catch**, **static**, **global**,
    /// and **destructuring** definitions this equals `offset` (the
    /// definition is immediately visible).
    pub effective_from: u32,
    /// How many conditional nesting levels deep this definition is
    /// (if/else, switch, while, etc.) relative to the enclosing scope.
    /// Top-level statements in the function body have depth 0.
    /// Used to prefer outer-scope definitions over conditional ones.
    pub nesting_depth: u16,
    /// End byte offset of the innermost conditional block containing
    /// this definition.  When the cursor is past this offset, this
    /// definition should not override a shallower one.  `u32::MAX`
    /// for top-level (depth 0) definitions.
    pub block_end: u32,
}

/// A closure or arrow function passed as an argument to a callable-typed
/// parameter.  Used by inlay hints to show:
/// - **Parameter type hints** for untyped closure parameters when the type
///   can be inferred from the enclosing callable signature.
/// - **Return type hints** when the closure lacks an explicit return type
///   and the callable signature specifies one.
#[derive(Debug, Clone)]
pub(crate) struct UntypedClosureSite {
    /// The call expression string of the parent call (same format as
    /// `CallSite::call_expression`).
    pub parent_call_expression: String,
    /// 0-based index of the closure argument within the parent call's
    /// argument list.
    pub arg_index_in_parent: usize,
    /// Byte offset of the closing `)` of the closure's parameter list.
    /// Used to place the return type hint.  `None` when the closure
    /// already has an explicit return type declaration.
    pub close_paren_offset: Option<u32>,
    /// Untyped parameters: `(param_index, param_variable_offset)`.
    /// Only parameters that lack a native type hint are included.
    pub untyped_params: Vec<(usize, u32)>,
}

/// The kind of variable definition site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VarDefKind {
    Assignment,
    /// Compound assignment (`+=`, `-=`, `.=`, `??=`, etc.).  Semantically
    /// the variable is modified in place rather than rebound to a
    /// completely different value, so it does not start a new
    /// definition region.
    CompoundAssignment,
    Parameter,
    Property,
    Foreach,
    Catch,
    StaticDecl,
    GlobalDecl,
    ArrayDestructuring,
    ListDestructuring,
    ClosureCapture,
    /// A `@param $varName` mention in a docblock.  Not a real variable
    /// definition, but recorded so that `find_variable_scope` can map
    /// the pre-body offset to the correct function body scope for
    /// rename and find-references.
    DocblockParam,
    /// A `@var Type $varName` mention in an inline docblock.  Recorded
    /// so that the forward walker picks up the type annotation as a
    /// variable definition site.
    DocblockVar,
    /// An `unset($var)` call.  Recorded so that variable completion can
    /// suppress the variable after the unset point.
    Unset,
}

/// Per-file symbol location index.
///
/// The `spans` vec is sorted by `start` offset.  Gaps between spans
/// represent non-navigable regions (whitespace, operators, string
/// literal interiors, comment interiors, numeric literals, etc.).
/// When the cursor falls in a gap, the lookup returns `None`
/// immediately — no parsing, no text scanning.
#[derive(Debug, Clone, Default)]
pub(crate) struct SymbolMap {
    pub spans: Vec<SymbolSpan>,
    /// Laravel string spans gated by cross-file function/class membership.
    /// Empty for ordinary PHP files and never consulted by request handlers.
    pub conditional_laravel_spans: Vec<ConditionalLaravelStringSpan>,
    /// Member-access span indices keyed by member name.
    ///
    /// This lets references/rename jump straight to relevant `->name` /
    /// `::name` spans without rescanning the full `spans` vec for each file.
    pub member_access_indices: crate::atom::AtomMap<Vec<usize>>,
    /// Variable definition sites, sorted by `(scope_start, offset)`.
    pub var_defs: Vec<VarDefSite>,
    /// Indices into [`Self::var_defs`] sorted by `(scope_start, name,
    /// offset)`, so the definitions of one variable in one scope are a
    /// contiguous run found by binary search.
    pub var_defs_by_name: Vec<u32>,
    /// Scope boundaries `(start_offset, end_offset)` for functions,
    /// methods, closures, and arrow functions.  Used by
    /// `find_enclosing_scope` to determine which scope the cursor is in.
    pub scopes: NestedRanges,
    /// Scope start offsets of arrow functions, sorted.  Arrow functions
    /// inherit the enclosing scope's variables (unlike closures which
    /// isolate their scope).  Used by variable name completion to walk up
    /// through arrow function boundaries while stopping at closure
    /// and function boundaries.
    pub arrow_fn_scopes: Vec<u32>,
    /// Body boundaries `(body_start_offset, body_end_offset)` for
    /// closures and arrow functions only.
    ///
    /// For closures, `body_start` is the opening `{` offset (same as
    /// the scope start).  For arrow functions, `body_start` is the
    /// `=>` token offset, which is later than the scope start (the
    /// `fn` keyword).
    ///
    /// Used by signature help to suppress the outer call's popup once
    /// the cursor has entered a closure or arrow function body that is
    /// itself an argument to the call.  Separate from `scopes` because
    /// variable resolution needs the full `fn`..`end` range for arrow
    /// function parameter lookups.
    pub body_scopes: Vec<(u32, u32)>,
    /// Narrowing block boundaries `(start_offset, end_offset)` for
    /// if-body, elseif-body, else-body, match-arm, and switch-case
    /// blocks.  Sorted by start offset.
    ///
    /// Used by the diagnostic subject cache to determine whether two
    /// variable accesses are in the same narrowing context.  Accesses
    /// in the same block get the same instanceof narrowing applied and
    /// can share a cache entry, while accesses in different blocks
    /// (e.g. different if/else branches) may resolve to different types
    /// and need independent entries.
    pub narrowing_blocks: NestedRanges,
    /// Offsets of `assert($var instanceof ...)` statements, sorted.
    ///
    /// These act as sequential narrowing boundaries: accesses before and
    /// after an assert-instanceof in the same flat statement list should
    /// get different diagnostic cache entries because the assert changes
    /// the variable's resolved type.  Unlike `narrowing_blocks` (which
    /// model block-scoped if/else branches), these are point boundaries
    /// in a linear statement sequence.
    pub assert_narrowing_offsets: Vec<u32>,
    /// Template parameter definition sites from `@template` docblock tags,
    /// sorted by `name_offset`.  Used to resolve template parameter names
    /// (e.g. `TKey`, `TModel`) that appear in docblock types but are not
    /// actual class names.
    pub template_defs: Vec<TemplateParamDef>,
    /// Call expression sites, sorted by `args_start`.
    /// Used by signature help to find the innermost call containing the
    /// cursor and to compute the active parameter index from AST data.
    pub call_sites: CallSites,
    /// Breakable block boundaries `(start_offset, end_offset)` where
    /// `break` is valid (loops and `switch`).
    pub breakable_scopes: Vec<(u32, u32)>,
    /// Loop block boundaries `(start_offset, end_offset)` where
    /// `continue` is valid (`while`, `do/while`, `for`, `foreach`).
    pub loop_scopes: Vec<(u32, u32)>,
    /// Switch body boundaries `(start_offset, end_offset)` where
    /// `case` / `default` labels are valid.
    pub switch_scopes: Vec<(u32, u32)>,
    /// Ranges of static method bodies `(start_offset, end_offset)`.
    /// Sorted by start offset.  Used to determine whether `$this` is
    /// unavailable at a given cursor position without re-parsing the AST.
    pub static_method_scopes: Vec<(u32, u32)>,
    /// Ranges of non-static (instance) method bodies `(start_offset,
    /// end_offset)`.  Used by variable name completion to determine
    /// whether `$this` is available at a given cursor position.
    pub instance_method_scopes: Vec<(u32, u32)>,
    /// Closures and arrow functions passed as arguments to callable-typed
    /// parameters.  Used by inlay hints to show inferred parameter types
    /// and return types from the enclosing callable signature.
    pub untyped_closure_sites: Vec<UntypedClosureSite>,
    /// Render sites whose receiver only a type settles, sorted by `start`.
    ///
    /// Empty for all but the handful of files that call a view factory or
    /// a mailable through something other than the spellings
    /// [`SymbolKind::LaravelStringKey`] is emitted for.
    pub view_receiver_sites: Vec<ViewReceiverSite>,
    /// Literal comparisons whose receivers and columns must be confirmed
    /// as Eloquent morph type columns, sorted by `start`.
    pub morph_column_sites: Vec<MorphColumnSite>,
    /// The model argument of each gate check that named one, keyed back to
    /// its ability span by [`GateSubject::ability_start`].  Empty for every
    /// file that performs no authorization checks.
    pub gate_subjects: Vec<GateSubject>,
    /// Byte length of the source text this map was extracted from, or
    /// `u32::MAX` for a file larger than 4 GiB (whose offsets do not fit
    /// in the `u32` fields above anyway).
    ///
    /// Every offset in this map indexes the exact text it was built from.
    /// The map is refreshed on a background task after each keystroke and
    /// is left untouched when a parse panics, so a request that reads
    /// `symbol_maps` may get a map built from *older* text than the buffer
    /// it was handed.  See [`Self::matches_source`].
    pub source_len: u32,
    /// A hash of the source text, recorded only for a file open in the
    /// editor when it was parsed.  See [`Self::built_from`].
    pub source_hash: Option<u64>,
}

impl SymbolMap {
    /// Whether this map's offsets are valid indices into `content`.
    ///
    /// A length match means no text was inserted or removed since the map
    /// was built, so every offset still points at the same token.  Callers
    /// that turn map offsets into edits the editor will apply (linked
    /// editing, rename) **must** check this: converting stale offsets
    /// against a newer buffer yields ranges over unrelated code, and the
    /// editor has no way to tell that they are nonsense.
    ///
    /// Length alone is the right signal because only insertion and removal
    /// shift offsets.  A same-length replacement leaves every offset
    /// outside the replaced span correct, and callers validate the token
    /// text at each offset they use.  Hashing the content instead would
    /// mean an extra full pass over every file at index time for a
    /// vanishingly small gain.
    pub fn matches_source(&self, content: &str) -> bool {
        u32::try_from(content.len()).is_ok_and(|len| len == self.source_len)
    }

    /// Pair this map with the content its offsets index into, or `None`
    /// when `content` is a different revision (see [`MappedSource`]).
    ///
    /// A consumer that cannot produce the matching text has nothing
    /// trustworthy to say about the file and should skip it rather than
    /// slice a span against text it does not describe.
    pub(crate) fn source<'a>(&self, content: &'a str) -> Option<MappedSource<'a>> {
        self.matches_source(content)
            .then_some(MappedSource(content))
    }

    /// The hash [`Self::source_hash`] records for `content`.
    pub(crate) fn hash_source(content: &str) -> u64 {
        use std::hash::Hasher;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        hasher.write(content.as_bytes());
        hasher.finish()
    }

    /// Whether this map was built from exactly `content`, as far as can be
    /// told: `false` whenever no hash was recorded.
    ///
    /// Stricter than [`Self::matches_source`], which a same-length edit
    /// passes.  A diagnostic pass that keeps its results to build the next
    /// one on needs every offset it reported to describe `content`, not
    /// just the ones it happened to re-validate.  Only open files pay for
    /// the hash.
    pub(crate) fn built_from(&self, content: &str) -> bool {
        self.matches_source(content) && self.source_hash == Some(Self::hash_source(content))
    }

    /// Indices into [`Self::spans`] for member accesses named `name`.
    pub fn member_access_indices(&self, name: &str) -> &[usize] {
        self.member_access_indices
            .get(&crate::atom::atom(name))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// The model a gate check named, for the ability span starting at
    /// `ability_start`.
    pub fn gate_subject(&self, ability_start: u32) -> Option<&GateSubject> {
        self.gate_subjects
            .iter()
            .find(|subject| subject.ability_start == ability_start)
    }

    /// Find the symbol span (if any) that contains `offset`.
    ///
    /// Uses binary search on the sorted `spans` vec.  Returns `None`
    /// when the offset falls in a gap between spans (whitespace,
    /// string interior, comment interior, etc.).
    pub fn lookup(&self, offset: u32) -> Option<&SymbolSpan> {
        let idx = self.spans.partition_point(|s| s.start <= offset);
        if idx == 0 {
            return None;
        }
        let candidate = &self.spans[idx - 1];
        if offset < candidate.end {
            Some(candidate)
        } else {
            None
        }
    }

    /// The span whose extent is exactly `[start, end)`, if the map holds
    /// one.
    ///
    /// Unlike [`lookup`](Self::lookup), which answers "what is at this
    /// offset", this answers "is this range a symbol of its own".  A
    /// caller that holds the range of a sub-expression — the subject of a
    /// member access, say — uses it to find the span that sub-expression
    /// emitted for itself, and so learns what it is without the file's
    /// text.  Several spans can start at the same offset (a chain and its
    /// base both start where the base does), so the whole equal-start run
    /// is checked.
    pub(crate) fn span_covering_exactly(&self, start: u32, end: u32) -> Option<&SymbolSpan> {
        let first = self.spans.partition_point(|span| span.start < start);
        self.spans[first..]
            .iter()
            .take_while(|span| span.start == start)
            .find(|span| span.end == end)
    }

    /// Find the innermost scope that contains `offset`.
    ///
    /// Returns the `scope_start` (opening brace offset) of the innermost
    /// function/method/closure body that contains the cursor, or `0` when
    /// the cursor is in top-level code.
    pub fn find_enclosing_scope(&self, offset: u32) -> u32 {
        self.scopes.innermost_start(offset)
    }

    /// Whether the scope starting at `scope_start` is an arrow function's.
    pub fn is_arrow_fn_scope(&self, scope_start: u32) -> bool {
        self.arrow_fn_scopes.binary_search(&scope_start).is_ok()
    }

    /// Determine the effective scope for a variable reference at `offset`.
    ///
    /// For most variable spans this is the same as
    /// [`find_enclosing_scope`].  However, **parameters** and
    /// **docblock `@param $var` mentions** sit physically before the
    /// opening `{` of the function/method/closure body, so
    /// `find_enclosing_scope` returns the *parent* scope for them.
    ///
    /// This method detects those cases and returns the correct body
    /// scope instead:
    ///
    /// 1. If `offset` is on a `VarDefSite` with `VarDefKind::Parameter`,
    ///    return that definition's `scope_start`.
    /// 2. Otherwise, if `offset` is before a scope boundary and there is
    ///    a parameter `VarDefSite` for `var_name` whose `scope_start` is
    ///    the next scope after `offset`, return that scope.  This covers
    ///    docblock `@param` variable tokens that precede the parameter
    ///    list.
    /// 3. Otherwise, fall back to `find_enclosing_scope`.
    pub fn find_variable_scope(&self, var_name: &str, offset: u32) -> u32 {
        // Case 1: cursor is directly on a parameter or docblock @param
        // definition token.  Both sit physically before the body `{`,
        // but their `VarDefSite.scope_start` points to the correct
        // body scope.
        if let Some(def) = self.var_defs.iter().find(|d| {
            d.name == var_name
                && (d.kind == VarDefKind::Parameter || d.kind == VarDefKind::DocblockParam)
                && offset >= d.offset
                && offset < d.offset + 1 + d.name.len() as u32
        }) {
            return def.scope_start;
        }

        self.find_enclosing_scope(offset)
    }

    /// Find the innermost narrowing block (if/elseif/else body, match
    /// arm, switch case) that contains `offset`.
    ///
    /// Returns the block's start offset, or `0` when the offset is not
    /// inside any narrowing block.  Two variable accesses that return
    /// the same value from this method will have identical instanceof
    /// narrowing applied and can safely share a diagnostic cache entry.
    pub fn find_narrowing_block(&self, offset: u32) -> u32 {
        self.narrowing_blocks.innermost_start(offset)
    }

    /// Find the offset of the last `assert($var instanceof …)` statement
    /// that precedes `offset`, or `0` if there is none.
    ///
    /// This is used as a cache discriminator: accesses before and after
    /// an assert-instanceof in the same flat statement list must get
    /// separate cache entries because the assert changes the variable's
    /// resolved type.
    pub fn find_preceding_assert_offset(&self, offset: u32) -> u32 {
        // `assert_narrowing_offsets` is sorted, so binary search for
        // the last element that is strictly less than `offset`.
        match self
            .assert_narrowing_offsets
            .partition_point(|&o| o < offset)
        {
            0 => 0,
            i => self.assert_narrowing_offsets[i - 1],
        }
    }

    /// Find the `@template` definition for a template parameter name at
    /// the given cursor offset.
    ///
    /// Returns the closest (most specific) `TemplateParamDef` whose scope
    /// covers `cursor_offset` and whose name matches.  Method-level
    /// template params are preferred over class-level ones because their
    /// `scope_start` is larger (they are defined later in the file).
    pub fn find_template_def(&self, name: &str, cursor_offset: u32) -> Option<&TemplateParamDef> {
        // Iterate in reverse so that narrower / later-defined scopes
        // (method-level) are checked before broader ones (class-level).
        self.template_defs.iter().rev().find(|d| {
            d.name == name && cursor_offset >= d.scope_start && cursor_offset <= d.scope_end
        })
    }

    /// Find the most recent definition of `$var_name` before
    /// `cursor_offset` within the same scope.
    ///
    /// The caller should obtain `scope_start` via
    /// [`find_enclosing_scope`].
    pub fn find_var_definition(
        &self,
        var_name: &str,
        cursor_offset: u32,
        scope_start: u32,
    ) -> Option<&VarDefSite> {
        // Find all visible definitions for this variable in this scope.
        // Prefer the most recent one, but if a shallower (outer) definition
        // exists, prefer it over a deeper (conditional) one when the cursor
        // is outside the conditional block.
        let key = |&i: &u32| {
            let d = &self.var_defs[i as usize];
            (d.scope_start, d.name.as_str())
        };
        let first = self
            .var_defs_by_name
            .partition_point(|i| key(i) < (scope_start, var_name));
        let run = self.var_defs_by_name[first..]
            .iter()
            .take_while(|i| key(i) == (scope_start, var_name))
            .map(|&i| &self.var_defs[i as usize]);

        let mut best: Option<&VarDefSite> = None;
        for d in run {
            if d.effective_from > cursor_offset {
                continue;
            }
            match best {
                None => best = Some(d),
                Some(prev) => {
                    if d.nesting_depth <= prev.nesting_depth {
                        // Same or shallower depth: more recent wins.
                        best = Some(d);
                    } else if cursor_offset <= d.block_end {
                        // Deeper, but cursor is inside the block: use it.
                        best = Some(d);
                    }
                    // Deeper and cursor is past the block: keep prev.
                }
            }
        }
        best
    }

    /// Return the `effective_from` offset of the most recent definition
    /// of `$var_name` that is visible at `cursor_offset`, or `0` if no
    /// definition is found.
    ///
    /// This is used as a cache-key discriminator: two accesses to the
    /// same variable that fall under the same definition share a cache
    /// entry, but accesses before vs. after a reassignment get different
    /// entries.
    pub fn active_var_def_offset(&self, var_name: &str, cursor_offset: u32) -> u32 {
        let scope_start = self.find_enclosing_scope(cursor_offset);
        self.find_var_definition(var_name, cursor_offset, scope_start)
            .map(|d| d.effective_from)
            .unwrap_or(0)
    }

    /// Check whether `cursor_offset` is physically sitting on a variable
    /// definition token (the `$var` token of an assignment LHS, parameter,
    /// foreach binding, etc.).
    ///
    /// This is used to detect the "already at definition" case *before*
    /// the `effective_from`-based lookup, because the assignment LHS token
    /// exists at the definition site even though the definition hasn't
    /// "taken effect" yet (its `effective_from` is past the cursor).
    pub fn is_at_var_definition(&self, var_name: &str, cursor_offset: u32) -> bool {
        self.var_def_kind_at(var_name, cursor_offset).is_some()
    }

    /// If the cursor is physically on a variable definition token, return
    /// the [`VarDefKind`] of that definition.
    ///
    /// This is a more informative variant of [`is_at_var_definition`] that
    /// lets the caller decide how to handle different definition kinds
    /// (e.g. skip type-hint navigation for parameters and catch variables).
    pub fn var_def_kind_at(&self, var_name: &str, cursor_offset: u32) -> Option<&VarDefKind> {
        self.var_def_at(var_name, cursor_offset).map(|d| &d.kind)
    }

    /// If the cursor is physically on a variable definition token, return
    /// the full [`VarDefSite`].
    ///
    /// This is used by hover to retrieve the `effective_from` offset so
    /// that hovering on the `$` sign of `$x = new Foo()` uses a cursor
    /// offset that includes the assignment itself.
    pub fn var_def_at(&self, var_name: &str, cursor_offset: u32) -> Option<&VarDefSite> {
        // No scope check needed: if the cursor is physically within a
        // VarDefSite's `$var` token, it IS that definition — two different
        // definitions cannot occupy the same bytes.  This also correctly
        // handles parameters, which are physically before the opening
        // brace of the function body (outside `find_enclosing_scope`'s
        // range) but whose VarDefSite has scope_start set to that brace.
        self.var_defs.iter().find(|d| {
            d.name == var_name
                && cursor_offset >= d.offset
                && cursor_offset < d.offset + 1 + d.name.len() as u32
        })
    }

    /// Find the innermost call site whose argument list contains `offset`.
    ///
    /// `call_sites` is sorted by `args_start`.  We want the innermost
    /// (last) one whose range contains the cursor, so we iterate in
    /// reverse and return the first match.
    pub fn find_enclosing_call_site(&self, offset: u32) -> Option<CallSite<'_>> {
        self.call_sites
            .iter()
            .rev()
            .find(|cs| offset >= cs.args_start && offset <= cs.args_end)
    }

    /// Check whether `offset` is inside a closure or arrow-function body
    /// that is nested within a call's argument list.
    ///
    /// Returns `true` when there is a scope (closure/arrow-fn) whose
    /// opening boundary falls inside (`args_start`..`args_end`) and
    /// whose range contains `offset`.  In that case the cursor is
    /// writing code *inside* the closure body, not filling in arguments
    /// to the outer call.
    ///
    /// Used by signature help to suppress the outer call's popup once
    /// the user has entered a closure or arrow function body argument.
    pub fn is_inside_nested_scope_of_call(&self, offset: u32, call: CallSite<'_>) -> bool {
        self.body_scopes.iter().any(|&(body_start, body_end)| {
            body_start > call.args_start
                && body_start < call.args_end
                && offset >= body_start
                && offset <= body_end
        })
    }

    /// Whether `offset` is inside a function-like scope
    /// (function/method/closure/arrow function body).
    pub fn is_inside_function_like_scope(&self, offset: u32) -> bool {
        self.find_enclosing_scope(offset) != 0
    }

    /// Binary-search helper: check whether `offset` falls inside any
    /// `(start, end)` range in a vec sorted by start offset.
    fn offset_in_sorted_ranges(ranges: &[(u32, u32)], offset: u32) -> bool {
        // Find the first range whose start is past `offset`.
        let idx = ranges.partition_point(|&(start, _)| start <= offset);
        // Check all candidate ranges (those with start <= offset) from
        // the closest one backward.  Usually only one or two iterations
        // are needed since scopes are rarely deeply nested.
        ranges[..idx].iter().rev().any(|&(_, end)| offset <= end)
    }

    /// Whether `offset` is inside a breakable scope where `break` is valid.
    pub fn is_inside_breakable_scope(&self, offset: u32) -> bool {
        Self::offset_in_sorted_ranges(&self.breakable_scopes, offset)
    }

    /// Whether `offset` is inside a loop scope where `continue` is valid.
    pub fn is_inside_loop_scope(&self, offset: u32) -> bool {
        Self::offset_in_sorted_ranges(&self.loop_scopes, offset)
    }

    /// Whether `offset` is inside a switch scope where `case/default`
    /// labels are valid.
    pub fn is_inside_switch_scope(&self, offset: u32) -> bool {
        Self::offset_in_sorted_ranges(&self.switch_scopes, offset)
    }

    /// Returns `true` when `offset` falls inside a `static` method body.
    pub fn is_in_static_method(&self, offset: u32) -> bool {
        Self::offset_in_sorted_ranges(&self.static_method_scopes, offset)
    }
}

#[cfg(test)]
mod tests;
