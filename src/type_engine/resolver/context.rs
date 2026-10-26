//! Context types and the thread-local chain resolution cache shared by
//! [`super::resolve_target_classes`] and the functions it delegates to.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crate::Backend;
use crate::class_lookup::{ResolvedClassLoader, SourceClassLoader};

use crate::php_type::PhpType;
use crate::type_engine::variable::forward_walk::{ExprTypes, ScopeProofs};
use crate::type_engine::variable::resolution::VarQuery;
use crate::types::*;

use super::WrittenNames;

// ─── Thread-local chain resolution cache ────────────────────────────────────
//
// During a single diagnostic pass a file may contain many chain expressions
// that share common prefixes (e.g. `$model->where(...)` is the prefix of
// `$model->where(...)->whereNotNull(...)` which is the prefix of
// `$model->where(...)->whereNotNull(...)->orderBy(...)`, etc.).
//
// Without caching, each chain link re-resolves the entire prefix from
// scratch via recursive calls to `resolve_target_classes_expr`.  For a
// 6-link Eloquent chain this means the base variable is resolved 6 times,
// the first method call 5 times, etc. — O(depth²) total work.
//
// The chain cache stores `resolve_target_classes` results keyed by the
// subject text (see [`ChainKey`]).  It is activated per-request for all LSP
// handlers (completion, hover, definition, diagnostics, etc.) via
// [`with_chain_resolution_cache`] and consulted by
// `resolve_target_classes` before doing any work.

thread_local! {
    /// When `Some`, `resolve_target_classes` will consult and populate
    /// this cache.  Set by [`with_chain_resolution_cache`], cleared on
    /// guard drop.
    pub(super) static CHAIN_CACHE: RefCell<Option<ChainCache>> = const { RefCell::new(None) };
}

/// The chain cache: resolved results, and the table that names each chain
/// text by a number.
#[derive(Default)]
pub(crate) struct ChainCache {
    /// Interned chain texts.  A chain is named by the number of the chain
    /// one link shorter plus the text its last link adds, so naming every
    /// link of a chain costs the chain's length once rather than once per
    /// link.
    chains: HashMap<String, u32>,
    pub(super) entries: HashMap<ChainKey, Vec<ResolvedType>>,
}

impl ChainCache {
    /// The number naming the chain that extends `parent` (or starts a
    /// chain, when `None`) with `link`.  Two chains get the same number
    /// only when their texts are equal.
    pub(super) fn intern(&mut self, parent: Option<u32>, link: &str, scratch: &mut String) -> u32 {
        use std::fmt::Write;
        scratch.clear();
        match parent {
            Some(parent) => {
                let _ = write!(scratch, "{parent:x}:");
            }
            None => scratch.push(':'),
        }
        scratch.push_str(link);
        if let Some(&id) = self.chains.get(scratch.as_str()) {
            return id;
        }
        let id = self.chains.len() as u32;
        self.chains.insert(scratch.clone(), id);
        id
    }
}

/// What a chain cache entry is keyed by.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct ChainKey {
    /// Tells files apart: the same text can name different classes in two
    /// files (see `SpineKeys::new`).
    pub(super) file_id: usize,
    /// Tells apart the places in one file where the same text names
    /// different things.
    pub(super) names: NameScope,
    /// The subject text, as numbered by [`ChainCache::intern`].
    pub(super) chain: u32,
    pub(super) site: ChainSite,
}

/// Where the names in a chain's text are read.
///
/// Inside a class, `self`, `static` and `parent` name that class and its
/// parent; everywhere, a class or function name is read through the
/// imports and namespace of the `namespace` block it is written in.  Code
/// in one class always sits in one block, so the class alone settles both.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum NameScope {
    /// Inside the class with this FQN.
    Class(crate::atom::Atom),
    /// Outside any class, in the `namespace` block declared at this offset
    /// (`None` before the file's first declaration).
    Block(Option<usize>),
}

impl NameScope {
    /// Where the names of a chain resolved through `ctx` are read.
    pub(super) fn of(ctx: &ResolutionCtx<'_>) -> Self {
        match ctx.current_class {
            // The stand-in class for code outside one has no name.
            Some(cls) if !cls.name.is_empty() => NameScope::Class(cls.fqn()),
            _ => NameScope::Block(crate::parser::namespace_decl_at(
                ctx.file.content,
                ctx.cursor_offset as usize,
            )),
        }
    }
}

/// Which occurrences of a chain text share a cache entry.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) enum ChainSite {
    /// Every occurrence whose names read the same (see [`NameScope`]).
    Anywhere,
    /// Occurrences whose local variables hold these types.
    Typed(std::rc::Rc<str>),
    /// Only the occurrence at this offset.
    At(u32),
    /// The occurrences that read the diagnostic scope snapshot recorded at
    /// this offset.
    Scope(u32),
}

/// RAII guard that clears the thread-local chain cache on drop.
pub(crate) type ChainCacheGuard = crate::type_engine::MemoGuard<ChainCache>;

/// Activate the thread-local chain resolution cache.
///
/// While the returned guard is alive, `resolve_target_classes` caches
/// its results by subject text so that shared chain prefixes are
/// resolved only once.
///
/// Nested activations are no-ops — the outermost guard owns the cache.
pub(crate) fn with_chain_resolution_cache() -> ChainCacheGuard {
    crate::type_engine::activate_memo(&CHAIN_CACHE)
}

/// Puts back the chain cache [`with_isolated_chain_cache`] set aside.
pub(crate) struct IsolatedChainCacheGuard(Option<ChainCache>);

impl Drop for IsolatedChainCacheGuard {
    fn drop(&mut self) {
        CHAIN_CACHE.with(|cell| {
            *cell.borrow_mut() = self.0.take();
        });
    }
}

/// Swap the chain cache for an empty one until the returned guard drops.
///
/// A cached chain is keyed by the text of the expression and the types of
/// the variables in it, falling back to the file offset it was written
/// at.  Reading a method body with its parameters seeded from a call site
/// asks about the same text at the same offset as every other call site
/// does, so the entries either side of that walk describe a different
/// scope than the one being resolved and neither may be shared with it.
pub(crate) fn with_isolated_chain_cache() -> IsolatedChainCacheGuard {
    IsolatedChainCacheGuard(
        CHAIN_CACHE.with(|cell| cell.borrow_mut().replace(ChainCache::default())),
    )
}

/// Type alias for the optional function-loader closure passed through
/// the resolution chain.  Reduces clippy `type_complexity` warnings.
pub(crate) type FunctionLoaderFn<'a> = Option<&'a dyn Fn(&str, u32) -> Option<FunctionInfo>>;

/// Type alias for the optional constant-value-loader closure passed
/// through the resolution chain.  Given a constant name as written and
/// the byte offset it was written at, returns `Some(Some(value))` when
/// the constant exists with a known value, `Some(None)` when it exists
/// but the value is unknown, and `None` when the constant was not found.
///
/// The offset is what lets the closure resolve the name against the
/// file it appears in, the way a class or function name is resolved;
/// pass `0` when no offset is available.
pub(crate) type ConstantLoaderFn<'a> = Option<&'a dyn Fn(&str, u32) -> Option<Option<String>>>;

/// Type alias for the optional config-value resolver closure.  Given a
/// dotted Laravel config key (e.g. `"database.default"`), returns the
/// inferred [`PhpType`] of the config value.
pub(crate) type ConfigResolverFn<'a> = Option<&'a dyn Fn(&str) -> Option<crate::php_type::PhpType>>;

/// Type alias for the optional translation-key resolver closure.  Given a
/// translation key (e.g. `"messages.welcome"`), returns `Some(string)`
/// when the key names a scalar translation entry, or `None` when it names
/// a group or cannot be resolved.
pub(crate) type TransResolverFn<'a> = Option<&'a dyn Fn(&str) -> Option<crate::php_type::PhpType>>;

/// Type alias for the optional scope-based variable resolver from the
/// forward walker.  When set on a [`VarResolutionCtx`], variable
/// lookups read from the forward walker's in-progress `ScopeState`
/// instead of re-entering `resolve_variable_types`.
pub(crate) type ScopeVarResolverFn<'a> =
    Option<&'a dyn Fn(&str) -> Vec<crate::types::ResolvedType>>;

/// Type alias for the optional scope-membership resolver from the
/// forward walker.  Unlike [`ScopeVarResolverFn`], which returns an empty
/// vec both when a variable was never assigned on any surviving path and
/// when it was assigned but its type could not be resolved, this answers
/// which of the two it is: `true` when the variable has *some* entry in
/// the walker's `ScopeState` (even an empty/unresolved one), `false` when
/// it has none at all.
pub(crate) type ScopeContainsResolverFn<'a> = Option<&'a dyn Fn(&str) -> bool>;

/// Optional Laravel macro callback `$this` resolver.
pub(crate) type LaravelMacroThisResolverFn<'a> = Option<&'a dyn Fn(&str) -> Option<Arc<ClassInfo>>>;

/// The `use` imports (alias → FQN) of the namespace block being resolved,
/// when the caller has a file context to read them from.
pub(crate) type SourceImports<'a> = Option<&'a HashMap<String, String>>;

/// Bundles optional cross-file loader callbacks so they can be threaded
/// through the resolution chain as a single argument instead of one
/// parameter per loader.
#[derive(Clone, Copy, Default)]
pub(crate) struct Loaders<'a> {
    /// Cross-file function resolution callback (optional).
    pub function_loader: FunctionLoaderFn<'a>,
    /// Cross-file constant value resolution callback (optional).
    ///
    /// Given a global constant name (e.g. `"PHP_EOL"`), returns the
    /// constant's value string so that the type can be inferred from
    /// the literal value.
    pub constant_loader: ConstantLoaderFn<'a>,
    /// Resolve a dotted Laravel config key (e.g. `"database.default"`)
    /// to the inferred type of its static value.  Used by the
    /// `config()` return-type interception in function call resolution.
    pub config_resolver: ConfigResolverFn<'a>,
    /// Resolve a translation key to `string` when it names a scalar
    /// entry.  Used by the `__()`/`trans()`/`Lang::get()` return-type
    /// interception in function/static-call resolution.
    pub trans_resolver: TransResolverFn<'a>,
}

impl<'a> Loaders<'a> {
    /// Create a `Loaders` with only a function loader.
    pub fn with_function(fl: FunctionLoaderFn<'a>) -> Self {
        Self {
            function_loader: fl,
            constant_loader: None,
            config_resolver: None,
            trans_resolver: None,
        }
    }
}

/// Anything that owns the closures a [`Loaders`] borrows and can lend
/// one out.
///
/// The closures a diagnostic collector passes down capture the
/// `Backend`, which makes them unnameable, so a factory cannot return a
/// `Loaders` directly: the references inside it would outlive the
/// temporaries they point at. A caller binds one of these to a local
/// instead and calls [`LendsLoaders::loaders`] on it.
/// [`crate::Backend::diagnostic_loaders`] is the usual way to get one.
pub(crate) trait LendsLoaders {
    /// Borrow the owned closures as a [`Loaders`].
    fn loaders(&self) -> Loaders<'_>;
}

/// The four closures a [`Loaders`] borrows, held by value.
pub(crate) struct OwnedLoaders<F, C, G, T> {
    function_loader: F,
    constant_loader: C,
    config_resolver: G,
    trans_resolver: T,
}

impl<F, C, G, T> OwnedLoaders<F, C, G, T> {
    /// Bundle four already-built loaders.
    pub(crate) fn new(
        function_loader: F,
        constant_loader: C,
        config_resolver: G,
        trans_resolver: T,
    ) -> Self {
        Self {
            function_loader,
            constant_loader,
            config_resolver,
            trans_resolver,
        }
    }
}

impl<F, C, G, T> LendsLoaders for OwnedLoaders<F, C, G, T>
where
    F: Fn(&str, u32) -> Option<crate::types::FunctionInfo>,
    C: Fn(&str, u32) -> Option<Option<String>>,
    G: Fn(&str) -> Option<crate::php_type::PhpType>,
    T: Fn(&str) -> Option<crate::php_type::PhpType>,
{
    fn loaders(&self) -> Loaders<'_> {
        Loaders {
            function_loader: Some(&self.function_loader),
            constant_loader: Some(&self.constant_loader),
            config_resolver: Some(&self.config_resolver),
            trans_resolver: Some(&self.trans_resolver),
        }
    }
}

/// The file a resolution reads: its source text, the classes declared in
/// it, the loaders that turn the names written there into classes, and the
/// server state.
///
/// Every type-engine context ([`ResolutionCtx`], [`VarResolutionCtx`],
/// [`VarQuery`] and the forward walker's context) holds one, so moving
/// from one context to another carries the file across whole instead of
/// field by field.  Not to be confused with
/// [`FileContext`](crate::types::FileContext), the parsed file a request
/// handler builds this from.
#[derive(Clone, Copy)]
pub(crate) struct FileCtx<'a> {
    /// The full source text of the current file.
    pub content: &'a str,
    /// All classes known in the current file.
    pub all_classes: &'a [Arc<ClassInfo>],
    /// Cross-file class resolution callback for already-resolved names.
    pub class_loader: &'a ResolvedClassLoader<'a>,
    /// Cross-file class resolution callback for names as written in
    /// source (`new Foo`, `Foo::bar()`, `@var Foo`), which resolves them
    /// through the file's imports.  [`class_loader`](Self::class_loader)
    /// is for already-resolved names, where a bare `Redis` is the global
    /// class even when the file imports another `Redis`.
    pub source_class_loader: &'a SourceClassLoader<'a>,
    /// The `use` imports of the namespace block the cursor is in, which
    /// [`resolve_code_class_name`](crate::names::resolve_code_class_name)
    /// qualifies a written name no class answers to with.  `None` where
    /// no file context exists.
    pub source_imports: SourceImports<'a>,
    /// The server state the type engine consults for answers that need
    /// the whole project rather than the current file: inferring a
    /// method's return type from its body, the model configured for an
    /// auth guard, and the validation rules describing a request.
    ///
    /// `None` only where no server state exists (free-function callers
    /// and unit tests), in which case those answers are unavailable —
    /// not wrong.
    pub backend: Option<&'a Backend>,
}

impl<'a> FileCtx<'a> {
    /// A file with no imports and no server state.
    pub(crate) fn new(
        content: &'a str,
        all_classes: &'a [Arc<ClassInfo>],
        class_loader: &'a ResolvedClassLoader<'a>,
        source_class_loader: &'a SourceClassLoader<'a>,
    ) -> Self {
        Self {
            content,
            all_classes,
            class_loader,
            source_class_loader,
            source_imports: None,
            backend: None,
        }
    }

    /// The same file with `imports` as the `use` imports in force.
    pub(crate) fn with_imports(self, imports: SourceImports<'a>) -> Self {
        Self {
            source_imports: imports,
            ..self
        }
    }

    /// The same file with `backend` as the server state.
    pub(crate) fn with_backend(self, backend: Option<&'a Backend>) -> Self {
        Self { backend, ..self }
    }

    /// The same file with another source text and set of classes, as when
    /// a request re-parses a patched copy of the file.
    pub(crate) fn with_source(self, content: &'a str, all_classes: &'a [Arc<ClassInfo>]) -> Self {
        Self {
            content,
            all_classes,
            ..self
        }
    }

    /// How a class name written at `cursor_offset` reads, inside
    /// `current_class` when there is one.
    pub(in crate::type_engine) fn written_names(
        &self,
        current_class: Option<&'a ClassInfo>,
        cursor_offset: u32,
    ) -> WrittenNames<'a> {
        WrittenNames::new(
            current_class,
            self.all_classes,
            self.content,
            cursor_offset,
            self.source_imports,
            self.source_class_loader,
            self.class_loader,
        )
    }
}

/// Bundles the context needed by [`super::resolve_target_classes`] and
/// the functions it delegates to, avoiding an unwieldy multi-parameter
/// signature. Also used directly by
/// `resolve_call_return_types_expr_with_hint` and `resolve_arg_text_to_type`.
pub(crate) struct ResolutionCtx<'a> {
    /// The file being resolved in.
    pub file: FileCtx<'a>,
    /// The class the cursor is inside, if any.
    pub current_class: Option<&'a ClassInfo>,
    /// Byte offset of the cursor in the file's content.
    pub cursor_offset: u32,
    /// Optional Laravel macro callback `$this` resolver.
    pub laravel_macro_this_resolver: LaravelMacroThisResolverFn<'a>,
    /// Shared cache of fully-resolved classes, keyed by FQN.
    ///
    /// When `Some`, [`resolve_class_fully_cached`](crate::virtual_members::resolve_class_fully_cached)
    /// is used instead of the uncached variant, eliminating redundant
    /// full-resolution work within a single request cycle.  `None` in
    /// contexts where no `Backend` (and therefore no cache) is available
    /// (e.g. standalone free-function callers, some test helpers).
    pub resolved_class_cache: Option<&'a crate::virtual_members::ResolvedClassCache>,
    /// Cross-file function resolution callback (optional).
    pub function_loader: FunctionLoaderFn<'a>,
    /// Optional scope-based variable resolver carried from the forward
    /// walker.  When set, `resolve_variable_fallback` reads variable
    /// types from this closure (which reads the forward walker's
    /// in-progress `ScopeState`) instead of calling
    /// `resolve_variable_types` which would trigger a full method-body
    /// re-walk.
    pub scope_var_resolver: ScopeVarResolverFn<'a>,
    /// Whether the cursor is inside a `static` method body.
    /// When `true`, `$this` is not available and `SubjectExpr::This`
    /// resolves to nothing.  Precomputed from the `SymbolMap` at the
    /// call site to avoid re-parsing the AST.
    pub is_in_static_method: bool,
    /// When `true`, `$this` / `self` / `static` resolve to their
    /// keyword form rather than the concrete class name, and method
    /// chains use the last method's declared return type directly.
    ///
    /// Used by macro return-type inference so that `return $this;` in
    /// a macro closure produces `$this` — preserving polymorphism and
    /// generics that the general resolver would flatten.
    pub preserve_static: bool,
}

/// The loader closures a [`ResolutionCtx`] carries beyond its file's.
///
/// They are built at the call site rather than inside
/// [`Backend::resolution_ctx_at`](crate::Backend::resolution_ctx_at):
/// each borrows the file context it resolves names against, and the
/// Laravel macro resolver borrows the class loader in turn, so they
/// have to outlive the context they are handed to.
#[derive(Clone, Copy)]
pub(crate) struct CtxLoaders<'a> {
    pub function_loader: FunctionLoaderFn<'a>,
    pub laravel_macro_this_resolver: LaravelMacroThisResolverFn<'a>,
}

impl<'a> ResolutionCtx<'a> {
    /// A context at `cursor_offset` of `file`, inside `current_class`,
    /// with no loaders beyond the file's, no resolved-class cache, no
    /// forward-walker scope, and a cursor that is neither inside a static
    /// method nor preserving `static`.  Callers that differ name the
    /// fields they set with struct-update syntax.
    pub(crate) fn new(
        file: FileCtx<'a>,
        current_class: Option<&'a ClassInfo>,
        cursor_offset: u32,
    ) -> Self {
        Self {
            file,
            current_class,
            cursor_offset,
            laravel_macro_this_resolver: None,
            resolved_class_cache: None,
            function_loader: None,
            scope_var_resolver: None,
            is_in_static_method: false,
            preserve_static: false,
        }
    }

    /// How a class name written at the cursor reads.  Outside a class the
    /// namespace is the one in force at the cursor.
    pub(in crate::type_engine) fn written_names(&self) -> WrittenNames<'a> {
        self.file
            .written_names(self.current_class, self.cursor_offset)
    }

    /// A forward-walker query for `var_name` at the cursor, read inside
    /// the current class, or inside a placeholder for the cursor's
    /// namespace outside one.
    pub(crate) fn var_query<'q>(&'q self, var_name: &'q str) -> VarQuery<'q> {
        VarQuery {
            loaders: Loaders::with_function(self.function_loader),
            ..VarQuery::new(var_name, self.file, self.current_class, self.cursor_offset)
        }
    }

    /// [`resolve_variable_types`](crate::type_engine::variable::resolution::resolve_variable_types)
    /// for `var_name` at the cursor (see [`Self::var_query`]).
    pub(crate) fn resolve_variable_at_cursor(&self, var_name: &str) -> Vec<ResolvedType> {
        crate::type_engine::variable::resolution::resolve_variable_types(self.var_query(var_name))
    }
}

impl<'a> CtxLoaders<'a> {
    /// Both loaders, as a request handler with a file context builds
    /// them.
    pub(crate) fn new(
        function_loader: &'a dyn Fn(&str, u32) -> Option<FunctionInfo>,
        laravel_macro_this_resolver: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Self {
        Self {
            function_loader: Some(function_loader),
            laravel_macro_this_resolver: Some(laravel_macro_this_resolver),
        }
    }

    /// Without a Laravel macro `$this` resolver, for the paths that
    /// cannot reach a macro closure body.
    pub(crate) fn without_macro_this(
        function_loader: &'a dyn Fn(&str, u32) -> Option<FunctionInfo>,
    ) -> Self {
        Self {
            function_loader: Some(function_loader),
            laravel_macro_this_resolver: None,
        }
    }
}

/// Bundles the common parameters threaded through variable-type resolution.
///
/// Introducing this struct avoids passing 7–10 individual arguments to
/// every helper in the resolution chain, which keeps clippy happy and
/// makes call-sites much easier to read.
///
/// Build one with [`VarResolutionCtx::new`] and name only the optional
/// fields that differ, rather than restating them all.
#[derive(Clone)]
pub(crate) struct VarResolutionCtx<'a> {
    /// The file being resolved in.
    pub file: FileCtx<'a>,
    pub var_name: &'a str,
    pub current_class: &'a ClassInfo,
    pub cursor_offset: u32,
    /// Cross-file loader callbacks (function loader, constant loader).
    pub loaders: Loaders<'a>,
    /// Shared cache of fully-resolved classes, keyed by FQN.
    ///
    /// See [`ResolutionCtx::resolved_class_cache`] for details.
    pub resolved_class_cache: Option<&'a crate::virtual_members::ResolvedClassCache>,
    /// The `@return` type annotation of the enclosing function/method,
    /// if known.  Used inside generator bodies to reverse-infer variable
    /// types from `Generator<TKey, TValue, TSend, TReturn>`.
    pub enclosing_return_type: Option<PhpType>,
    /// Pre-computed top-level scope for resolving `global` variable imports.
    /// When a function body contains `global $x;`, the walker looks up
    /// `$x` in this map to seed the local scope with the top-level type.
    pub top_level_scope: Option<crate::type_engine::variable::forward_walk::Locals>,
    /// Match-arm instanceof narrowings: var name → narrowed types.
    /// Empty outside of match(true) arm bodies.
    pub match_arm_narrowing: HashMap<String, Vec<crate::types::ResolvedType>>,
    /// Optional scope-based variable resolver from the forward walker.
    ///
    /// When set, `resolve_var_types` in `rhs_resolution/mod.rs` reads
    /// variable types from this closure instead of re-entering
    /// `resolve_variable_types`, which would trigger a redundant
    /// forward walk of the method body.
    ///
    /// The closure takes a `$`-prefixed variable name and returns the
    /// variable's types from the forward walker's in-progress
    /// `ScopeState`.
    pub scope_var_resolver: ScopeVarResolverFn<'a>,
    /// Optional scope-membership resolver, set alongside
    /// `scope_var_resolver`.
    ///
    /// `resolve_null_coalesce_chain` uses this to tell a bare variable
    /// that was never assigned on any surviving path (contributes
    /// nothing to a `??` chain, like a stripped `null`) apart from one
    /// that was assigned but whose type resolution failed (still
    /// contributes `mixed`, per `widen_unresolved_branch`).
    pub scope_contains_resolver: ScopeContainsResolverFn<'a>,
    /// The proofs that scope holds which are not variable types: what a
    /// boolean stands for, which `preg_match` outcome a variable is, and
    /// whose null a value's null stands for.
    ///
    /// Set alongside `scope_var_resolver`; `None` where no walker scope
    /// exists.  Narrowing a ternary arm or a `match` arm reads these, so
    /// a condition that tests a boolean recording an earlier check
    /// narrows that check's subject instead of only the boolean.
    pub scope_proofs: Option<ScopeProofs<'a>>,
    /// The types of sub-expressions the forward walker evaluated against a
    /// different scope than the one this resolution reads, keyed by span.
    ///
    /// An expression that writes partway through (`[$b, $b = 1]`,
    /// `[$d++, $d]`) is resolved against the scope its writes leave
    /// behind, which is wrong for whatever ran before them and for the old
    /// value a `$d++` yields.  Those nodes are typed where they ran, and
    /// the resolver reads them back from here instead of re-deriving them.
    pub expr_types: Option<&'a ExprTypes>,
}

impl<'a> VarResolutionCtx<'a> {
    /// A context over the fields no caller can do without, with every
    /// optional one at its neutral value: no loaders beyond the file's, no
    /// resolved-class cache, no enclosing return type, no top-level scope,
    /// no match-arm narrowing and no forward-walk scope.
    ///
    /// Callers that do have one of those name it with struct-update
    /// syntax:
    ///
    /// ```ignore
    /// VarResolutionCtx {
    ///     loaders,
    ///     ..VarResolutionCtx::new("", file, class, offset)
    /// }
    /// ```
    pub(crate) fn new(
        var_name: &'a str,
        file: FileCtx<'a>,
        current_class: &'a ClassInfo,
        cursor_offset: u32,
    ) -> Self {
        Self {
            file,
            var_name,
            current_class,
            cursor_offset,
            loaders: Loaders::default(),
            resolved_class_cache: None,
            enclosing_return_type: None,
            top_level_scope: None,
            match_arm_narrowing: HashMap::new(),
            scope_var_resolver: None,
            scope_contains_resolver: None,
            scope_proofs: None,
            expr_types: None,
        }
    }

    /// A forward-walker query for `var_name` at `cursor_offset` with this
    /// context's file, class and loaders (none of its scope state).
    pub(crate) fn var_query<'q>(&'q self, var_name: &'q str, cursor_offset: u32) -> VarQuery<'q> {
        VarQuery {
            loaders: self.loaders,
            ..VarQuery::new(var_name, self.file, Some(self.current_class), cursor_offset)
        }
    }

    /// How a class name written at the cursor reads.
    pub(in crate::type_engine) fn written_names(&self) -> WrittenNames<'a> {
        self.file
            .written_names(Some(self.current_class), self.cursor_offset)
    }

    /// The type the forward walker recorded for `expr` where it ran, if
    /// it recorded one (see [`Self::expr_types`]).
    pub(crate) fn recorded_expr_type(
        &self,
        expr: &mago_syntax::cst::Expression<'_>,
    ) -> Option<&[crate::types::ResolvedType]> {
        self.expr_types?.get(expr)
    }

    /// Create a [`ResolutionCtx`] from this variable resolution context.
    ///
    /// The non-optional `current_class` is wrapped in `Some(…)`.  A
    /// variable-resolution context does not track whether it is inside a
    /// static method or preserving `static`, and carries no Laravel macro
    /// `$this` resolver, so the result has neither.
    pub(crate) fn as_resolution_ctx(&self) -> ResolutionCtx<'a> {
        ResolutionCtx {
            function_loader: self.loaders.function_loader,
            resolved_class_cache: self.resolved_class_cache,
            scope_var_resolver: self.scope_var_resolver,
            ..ResolutionCtx::new(self.file, Some(self.current_class), self.cursor_offset)
        }
    }

    /// Convenience accessor for the function loader.
    pub fn function_loader(&self) -> FunctionLoaderFn<'a> {
        self.loaders.function_loader
    }

    /// The value of the constant written as `name` at `offset`, as
    /// `Some(Some(value))` when it is known, `Some(None)` when the constant
    /// exists without a readable value, and `None` when it is not found.
    ///
    /// Falls back to the attached `Backend`, which is where the loader
    /// closure reads from anyway: a context built from a [`ResolutionCtx`]
    /// (which carries no constant loader) would otherwise report every
    /// global constant unknown, so a variable assigned one would resolve to
    /// nothing on that path while resolving fine on others.  That fallback
    /// has no file to resolve `name` against, so it only finds the constant
    /// under the name as written.
    pub(crate) fn lookup_constant(&self, name: &str, offset: u32) -> Option<Option<String>> {
        match self.loaders.constant_loader {
            Some(loader) => loader(name, offset),
            None => self.file.backend?.lookup_global_constant(name),
        }
    }

    /// Clone this context with a different `cursor_offset`.
    ///
    /// All other fields (including `enclosing_return_type`) are preserved.
    /// This is useful when resolving a right-hand-side expression at a
    /// position earlier than the original cursor to avoid infinite
    /// recursion on self-referential assignments.
    pub(crate) fn with_cursor_offset(&self, cursor_offset: u32) -> VarResolutionCtx<'a> {
        VarResolutionCtx {
            cursor_offset,
            ..self.clone()
        }
    }

    /// The types an enclosing ternary branch or `match` arm proved about a
    /// subject, if it proved anything.
    ///
    /// `key` is a subject key: a variable name (with or without the leading
    /// `$`) or a property path such as `$a->b`.  Every consumer that reads a
    /// subject's type has to go through here, or it silently resolves the
    /// branch against the type the subject had *before* the condition was
    /// tested.
    pub(crate) fn arm_narrowed(&self, key: &str) -> Option<&Vec<crate::types::ResolvedType>> {
        if self.match_arm_narrowing.is_empty() {
            return None;
        }
        match self.match_arm_narrowing.get(key) {
            Some(found) => Some(found),
            None if key.starts_with('$') => None,
            None => self.match_arm_narrowing.get(&format!("${key}")),
        }
    }

    /// Clone this context with match-arm instanceof narrowings applied.
    ///
    /// All other fields are preserved.  This is used when descending
    /// into a `match(true)` arm body whose conditions narrow one or
    /// more variables via `instanceof`.
    pub(crate) fn with_match_arm_narrowing(
        &self,
        match_arm_narrowing: HashMap<String, Vec<crate::types::ResolvedType>>,
    ) -> VarResolutionCtx<'a> {
        VarResolutionCtx {
            match_arm_narrowing,
            ..self.clone()
        }
    }
}

// ── Helpers to convert between ResolvedType and Arc<ClassInfo> ──────
//
// Many internal callers (property chain bases, call resolution, etc.)
// still operate on `Vec<Arc<ClassInfo>>`.  These thin wrappers avoid
// repeating the conversion at every call site inside this module.

/// Convert `Vec<ResolvedType>` to `Vec<Arc<ClassInfo>>`, discarding
/// entries without class info (scalars, shapes, unresolvable types).
pub(super) fn resolved_to_arcs(resolved: Vec<ResolvedType>) -> Vec<Arc<ClassInfo>> {
    ResolvedType::into_arced_classes(resolved)
}
