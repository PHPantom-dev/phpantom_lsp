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
//! `@template`, `@method`, etc.) are extracted by a dedicated string
//! scanner during the AST walk, since docblocks are trivia in the
//! `mago_syntax` AST and produce no expression/statement nodes.
//!
//! The module is split into submodules:
//!
//! - [`docblock`] — Docblock symbol extraction helpers (type span
//!   emission, `@template` / `@method` tag scanning, navigability
//!   filtering, and `get_docblock_text_with_offset`)
//! - [`extraction`] — AST walk that builds a [`SymbolMap`] from a
//!   parsed PHP program (`extract_symbol_map` and all
//!   `extract_from_*` helpers)

pub(crate) mod docblock;
mod extraction;

use crate::php_type::PhpType;

// Re-export the public entry point from extraction.
pub(crate) use extraction::extract_symbol_map;

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
}

#[derive(Debug, Clone)]
pub(crate) enum SymbolKind {
    /// Class/interface/trait/enum name in a type context:
    /// type hint, `new Foo`, `extends Foo`, `implements Foo`,
    /// `use` statement target, `catch (Foo $e)`, etc.
    ClassReference {
        name: String,
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
    ClassDeclaration { name: String },

    /// Member name on the RHS of `->`, `?->`, or `::`.
    /// `subject_text` is the source text of the LHS expression.
    MemberAccess {
        subject_text: String,
        member_name: String,
        is_static: bool,
        is_method_call: bool,
        /// `true` when this span was extracted from a docblock reference
        /// (e.g. `@see Order::$channel_type`) rather than real PHP code.
        /// Diagnostics skip these because the subject is a class name,
        /// not a runtime expression.
        is_docblock_reference: bool,
    },

    /// A `$variable` token (usage or definition site).
    Variable {
        /// Name without `$` prefix.
        name: String,
    },

    /// Standalone function call name (not a method call).
    ///
    /// When `is_definition` is `true`, the span covers the function name
    /// at its *declaration* site (`function foo() {}`).  When `false`, it
    /// covers a call site (`foo()`).  The distinction is needed by the
    /// unknown-function diagnostic (which must skip definitions) and by
    /// find-references / document-highlight (which may want to include
    /// both).
    FunctionCall { name: String, is_definition: bool },

    /// `self`, `static`, `parent`, or `$this` in a navigable context.
    SelfStaticParent(SelfStaticParentKind),

    /// A constant name in a navigable context (`define()` name,
    /// class constant access, standalone constant reference).
    ConstantReference { name: String },

    /// A method, property, or constant name at its *declaration* site.
    ///
    /// Go-to-definition returns the symbol's own location so editors
    /// can fall back to Find References.  Also needed for
    /// find-references and rename so that the declaration site
    /// participates in the match.
    MemberDeclaration {
        /// The member name (e.g. `"save"`, `"name"`, `"MAX_SIZE"`).
        /// For properties this is the name WITHOUT the `$` prefix.
        name: String,
        /// Whether this is a static member (`static function`, `static $prop`,
        /// or class constant — constants are always accessed statically).
        is_static: bool,
    },

    /// A namespace name at its declaration site (`namespace App\Models;`).
    /// Used by the rename handler to support namespace renaming.
    NamespaceDeclaration {
        /// The full namespace name (e.g. `"App\\Models"`).
        name: String,
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
    },
}

/// Identifies the category of a [`SymbolKind::LaravelStringKey`] span.
///
/// Adding a new Laravel navigation feature only requires adding a variant
/// here and updating the extraction and dispatch paths — the exhaustive
/// match arms in `highlight`, `hover`, `rename`, `semantic_tokens`, and
/// `type_definition` do not need to change.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum LaravelStringKind {
    /// A `config('dot.key')` or `Config::get('dot.key')` call.
    Config,
    /// A `view('name')` or `View::make('name')` call.
    View,
    /// A `route('name')` call.
    Route,
    /// A `__('key')`, `trans('key')`, or `Lang::get('key')` call.
    Trans,
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
    /// Upper bound from the `of` clause (e.g. `PhpType::Named("array-key")`
    /// for `@template TKey of array-key`), or `None` when unbounded.
    pub bound: Option<PhpType>,
    /// Variance annotation from the `@template` tag.
    pub variance: crate::types::TemplateVariance,
    /// Start of the scope where this template parameter is visible.
    /// For class-level templates this is the docblock start offset;
    /// for method/function-level templates it is the docblock start offset.
    pub scope_start: u32,
    /// End of the scope where this template parameter is visible.
    /// For class-level templates this is the class closing-brace offset;
    /// for method-level templates it is the method closing-brace offset;
    /// for function-level templates it is the function closing-brace offset.
    /// When the scope end cannot be determined (e.g. abstract method), this
    /// is set to `u32::MAX` so the parameter is visible to end-of-file.
    pub scope_end: u32,
}

// ─── Call site structures ───────────────────────────────────────────────────

/// A call expression site discovered during the AST walk.
///
/// Stored in `SymbolMap::call_sites`, sorted by `args_start`.
/// Used by signature help to find the innermost call whose argument
/// list contains the cursor and to compute the active parameter index
/// from precomputed comma offsets.
#[derive(Debug, Clone)]
pub(crate) struct CallSite {
    /// Byte offset immediately after the opening `(`.
    /// The cursor must be > `args_start` to be "inside" the call.
    pub args_start: u32,
    /// Byte offset of the closing `)`.
    /// When the parser recovered from an unclosed paren, this is the
    /// span end the parser chose.
    pub args_end: u32,
    /// The call expression in the format `resolve_callable` expects:
    ///   - `"functionName"` for standalone function calls
    ///   - `"$subject->method"` for instance/null-safe method calls
    ///   - `"ClassName::method"` for static method calls
    ///   - `"new ClassName"` for constructor calls
    pub call_expression: String,
    /// Byte offsets of each top-level comma separator inside the
    /// argument list.  Used to compute the active parameter index:
    /// count how many comma offsets are < cursor offset.
    pub comma_offsets: Vec<u32>,
    /// Byte offset of each argument expression's start token.
    ///
    /// One entry per argument in source order.  Used by inlay hints
    /// to place parameter-name annotations immediately before each
    /// argument.
    pub arg_offsets: Vec<u32>,
    /// Number of arguments passed at the call site.
    ///
    /// Computed from the AST argument list length during extraction.
    /// Unlike `comma_offsets.len() + 1`, this correctly handles empty
    /// argument lists (0) and trailing commas.
    pub arg_count: u32,
    /// Whether any argument uses the `...` spread/unpacking operator.
    ///
    /// When `true`, argument count diagnostics are suppressed because
    /// the actual number of arguments is unknown at static analysis time.
    pub has_unpacking: bool,
    /// Indices (into `arg_offsets`) of arguments that use named syntax
    /// (e.g. `name: $value`).  Inlay hints are suppressed for these
    /// because the parameter name is already visible in source.
    pub named_arg_indices: Vec<u32>,
    /// Parameter names (without `$` prefix) for each named argument,
    /// in the same order as `named_arg_indices`.  Used by inlay hints
    /// to determine which parameters are already consumed by named
    /// arguments so that positional arguments map to the correct
    /// remaining parameters.
    pub named_arg_names: Vec<String>,
    /// Indices (into `arg_offsets`) of arguments that use the `...`
    /// spread/unpacking operator.  Inlay hints are suppressed for these
    /// because a single spread argument may expand into multiple parameters.
    pub spread_arg_indices: Vec<u32>,
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
    /// completely different value.  Linked editing treats this the same
    /// as a read (it does not start a new definition region).
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
    /// Variable definition sites, sorted by `(scope_start, offset)`.
    pub var_defs: Vec<VarDefSite>,
    /// Scope boundaries `(start_offset, end_offset)` for functions,
    /// methods, closures, and arrow functions.  Used by
    /// `find_enclosing_scope` to determine which scope the cursor is in.
    pub scopes: Vec<(u32, u32)>,
    /// Scope start offsets of arrow functions.  Arrow functions inherit
    /// the enclosing scope's variables (unlike closures which isolate
    /// their scope).  Used by variable name completion to walk up
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
    pub narrowing_blocks: Vec<(u32, u32)>,
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
    pub call_sites: Vec<CallSite>,
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
}

impl SymbolMap {
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

    /// Find the innermost scope that contains `offset`.
    ///
    /// Returns the `scope_start` (opening brace offset) of the innermost
    /// function/method/closure body that contains the cursor, or `0` when
    /// the cursor is in top-level code.
    pub fn find_enclosing_scope(&self, offset: u32) -> u32 {
        let mut best: u32 = 0;
        for &(start, end) in &self.scopes {
            if start <= offset && offset <= end && start > best {
                best = start;
            }
        }
        best
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
        let mut best: u32 = 0;
        for &(start, end) in &self.narrowing_blocks {
            if start <= offset && offset <= end && start > best {
                best = start;
            }
        }
        best
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
        let mut best: Option<&VarDefSite> = None;
        for d in self.var_defs.iter() {
            if d.name != var_name
                || d.scope_start != scope_start
                || d.effective_from > cursor_offset
            {
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
    pub fn find_enclosing_call_site(&self, offset: u32) -> Option<&CallSite> {
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
    pub fn is_inside_nested_scope_of_call(&self, offset: u32, call: &CallSite) -> bool {
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
