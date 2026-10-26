//! The navigable symbol occurrences a [`SymbolMap`] records and their kinds.

use crate::atom::Atom;
use crate::php_type::PhpType;

use super::*;

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
    /// A Symfony form field, naming the property path of the form's data
    /// class it reads and writes.
    ///
    /// The property accessor reaches a path through a getter, an isser, or
    /// a hasser as readily as through a property, so a field with no
    /// property of its name is not an error.
    PropertyPath,
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
        matches!(self, Self::See | Self::PropertyPath)
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
pub(crate) struct MappedSource<'a>(pub(super) &'a str);

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
        /// Whether this declares a method (a real one or an `@method` tag)
        /// rather than a property, constant, or enum case.  A method and a
        /// property can share a name, and references must not mix them up.
        is_method: bool,
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

    /// A framework string-key literal (config key, route name, view name, etc.)
    /// inside a framework helper call such as `config()`, `route()`, `view()`,
    /// `Config::get()`, or `Config::set()`.
    ///
    /// The span covers the string content *inside* the quotes so that
    /// go-to-definition and find-references can work without re-parsing
    /// the file at request time.
    StringKey {
        /// The category of the key (e.g. `Config`, `Route`, `View`).
        kind: StringKeyKind,
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
            Self::StringKey { key, .. } => key.capacity(),
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
