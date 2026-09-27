/// Context types and the thread-local chain resolution cache shared by
/// [`super::resolve_target_classes`] and the functions it delegates to.
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crate::Backend;
use crate::atom::AtomMap;

use crate::php_type::PhpType;
use crate::type_engine::variable::forward_walk::ScopeProofs;
use crate::types::*;

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
// raw subject text string.  It is activated per-request for all LSP
// handlers (completion, hover, definition, diagnostics, etc.) via
// [`with_chain_resolution_cache`] and consulted by
// `resolve_target_classes` before doing any work.

thread_local! {
    /// When `Some`, `resolve_target_classes` will consult and populate
    /// this map.  Set by [`with_chain_resolution_cache`], cleared on
    /// guard drop.
    pub(super) static CHAIN_CACHE: RefCell<Option<HashMap<String, Vec<ResolvedType>>>> =
        const { RefCell::new(None) };
}

/// RAII guard that clears the thread-local chain cache on drop.
pub(crate) type ChainCacheGuard = crate::type_engine::MemoGuard<HashMap<String, Vec<ResolvedType>>>;

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
pub(crate) struct IsolatedChainCacheGuard(Option<HashMap<String, Vec<ResolvedType>>>);

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
    IsolatedChainCacheGuard(CHAIN_CACHE.with(|cell| cell.borrow_mut().replace(HashMap::new())))
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

/// Bundles the context needed by [`super::resolve_target_classes`] and
/// the functions it delegates to, avoiding an unwieldy multi-parameter
/// signature. Also used directly by
/// `resolve_call_return_types_expr_with_hint` and `resolve_arg_text_to_type`.
pub(crate) struct ResolutionCtx<'a> {
    /// The class the cursor is inside, if any.
    pub current_class: Option<&'a ClassInfo>,
    /// All classes known in the current file.
    pub all_classes: &'a [Arc<ClassInfo>],
    /// The full source text of the current file.
    pub content: &'a str,
    /// Byte offset of the cursor in `content`.
    pub cursor_offset: u32,
    /// Cross-file class resolution callback.
    pub class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    /// The server state the type engine consults for answers that need
    /// the whole project rather than the current file: inferring a
    /// method's return type from its body, the model configured for an
    /// auth guard, and the validation rules describing a request.
    ///
    /// `None` only where no server state exists (free-function callers
    /// and unit tests), in which case those answers are unavailable —
    /// not wrong.
    pub backend: Option<&'a Backend>,
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

/// The cross-file loader closures a [`ResolutionCtx`] carries.
///
/// They are built at the call site rather than inside
/// [`Backend::resolution_ctx_at`](crate::Backend::resolution_ctx_at):
/// each borrows the file context it resolves names against, and the
/// Laravel macro resolver borrows the class loader in turn, so they
/// have to outlive the context they are handed to.
pub(crate) struct CtxLoaders<'a> {
    pub class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    pub function_loader: FunctionLoaderFn<'a>,
    pub laravel_macro_this_resolver: LaravelMacroThisResolverFn<'a>,
}

impl<'a> CtxLoaders<'a> {
    /// The full trio, as a request handler with a file context builds
    /// them.
    pub(crate) fn new(
        class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        function_loader: &'a dyn Fn(&str, u32) -> Option<FunctionInfo>,
        laravel_macro_this_resolver: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Self {
        Self {
            class_loader,
            function_loader: Some(function_loader),
            laravel_macro_this_resolver: Some(laravel_macro_this_resolver),
        }
    }

    /// Without a Laravel macro `$this` resolver, for the paths that
    /// cannot reach a macro closure body.
    pub(crate) fn without_macro_this(
        class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        function_loader: &'a dyn Fn(&str, u32) -> Option<FunctionInfo>,
    ) -> Self {
        Self {
            class_loader,
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
/// fields that differ, rather than restating all fifteen.
#[derive(Clone)]
pub(crate) struct VarResolutionCtx<'a> {
    pub var_name: &'a str,
    pub current_class: &'a ClassInfo,
    pub all_classes: &'a [Arc<ClassInfo>],
    pub content: &'a str,
    pub cursor_offset: u32,
    pub class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    /// See [`ResolutionCtx::backend`].
    pub backend: Option<&'a Backend>,
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
    pub top_level_scope: Option<AtomMap<Vec<crate::types::ResolvedType>>>,
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
}

impl<'a> VarResolutionCtx<'a> {
    /// A context over the fields no caller can do without, with every
    /// optional one at its neutral value: no backend, no loaders, no
    /// resolved-class cache, no enclosing return type, no top-level scope,
    /// no match-arm narrowing and no forward-walk scope.
    ///
    /// Callers that do have one of those name it with struct-update
    /// syntax:
    ///
    /// ```ignore
    /// VarResolutionCtx {
    ///     backend: Some(backend),
    ///     ..VarResolutionCtx::new("", class, classes, content, offset, &loader)
    /// }
    /// ```
    pub(crate) fn new(
        var_name: &'a str,
        current_class: &'a ClassInfo,
        all_classes: &'a [Arc<ClassInfo>],
        content: &'a str,
        cursor_offset: u32,
        class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Self {
        Self {
            var_name,
            current_class,
            all_classes,
            content,
            cursor_offset,
            class_loader,
            backend: None,
            loaders: Loaders::default(),
            resolved_class_cache: None,
            enclosing_return_type: None,
            top_level_scope: None,
            match_arm_narrowing: HashMap::new(),
            scope_var_resolver: None,
            scope_contains_resolver: None,
            scope_proofs: None,
        }
    }

    /// Create a [`ResolutionCtx`] from this variable resolution context.
    ///
    /// The non-optional `current_class` is wrapped in `Some(…)`.
    pub(crate) fn as_resolution_ctx(&self) -> ResolutionCtx<'a> {
        ResolutionCtx {
            current_class: Some(self.current_class),
            all_classes: self.all_classes,
            content: self.content,
            cursor_offset: self.cursor_offset,
            class_loader: self.class_loader,
            backend: self.backend,
            laravel_macro_this_resolver: None,
            function_loader: self.loaders.function_loader,
            resolved_class_cache: self.resolved_class_cache,
            scope_var_resolver: self.scope_var_resolver,
            is_in_static_method: false,
            preserve_static: false,
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
            None => self.backend?.lookup_global_constant(name),
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
