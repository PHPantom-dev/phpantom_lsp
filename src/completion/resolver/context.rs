/// Context types and the thread-local chain resolution cache shared by
/// [`super::resolve_target_classes`] and the functions it delegates to.
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crate::atom::AtomMap;

use crate::php_type::PhpType;
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
pub(crate) struct ChainCacheGuard {
    /// `true` when this guard owns the cache (outermost activation).
    owns: bool,
}

impl Drop for ChainCacheGuard {
    fn drop(&mut self) {
        if self.owns {
            CHAIN_CACHE.with(|cell| {
                *cell.borrow_mut() = None;
            });
        }
    }
}

/// Activate the thread-local chain resolution cache.
///
/// While the returned guard is alive, `resolve_target_classes` caches
/// its results by subject text so that shared chain prefixes are
/// resolved only once.
///
/// Nested activations are no-ops — the outermost guard owns the cache.
pub(crate) fn with_chain_resolution_cache() -> ChainCacheGuard {
    let already_active = CHAIN_CACHE.with(|cell| cell.borrow().is_some());
    if already_active {
        return ChainCacheGuard { owns: false };
    }
    CHAIN_CACHE.with(|cell| {
        *cell.borrow_mut() = Some(HashMap::new());
    });
    ChainCacheGuard { owns: true }
}

/// Type alias for the optional function-loader closure passed through
/// the resolution chain.  Reduces clippy `type_complexity` warnings.
pub(crate) type FunctionLoaderFn<'a> = Option<&'a dyn Fn(&str, u32) -> Option<FunctionInfo>>;

/// Type alias for the optional constant-value-loader closure passed
/// through the resolution chain.  Given a constant name, returns
/// `Some(Some(value))` when the constant exists with a known value,
/// `Some(None)` when it exists but the value is unknown, and `None`
/// when the constant was not found.
pub(crate) type ConstantLoaderFn<'a> = Option<&'a dyn Fn(&str) -> Option<Option<String>>>;

/// Type alias for the optional scope-based variable resolver from the
/// forward walker.  When set on a [`VarResolutionCtx`], variable
/// lookups read from the forward walker's in-progress `ScopeState`
/// instead of re-entering `resolve_variable_types`.
pub(crate) type ScopeVarResolverFn<'a> =
    Option<&'a dyn Fn(&str) -> Vec<crate::types::ResolvedType>>;

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
}

impl<'a> Loaders<'a> {
    /// Create a `Loaders` with only a function loader.
    pub fn with_function(fl: FunctionLoaderFn<'a>) -> Self {
        Self {
            function_loader: fl,
            constant_loader: None,
        }
    }
}

/// Bundles the context needed by [`super::resolve_target_classes`] and
/// the functions it delegates to.
///
/// Introduced to replace the 8-parameter signature of
/// `resolve_target_classes` with a cleaner `(subject, access_kind, ctx)`
/// triple.  Also used directly by `resolve_call_return_types_expr` and
/// `resolve_arg_text_to_type` (formerly `CallResolutionCtx`).
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

/// Bundles the common parameters threaded through variable-type resolution.
///
/// Introducing this struct avoids passing 7–10 individual arguments to
/// every helper in the resolution chain, which keeps clippy happy and
/// makes call-sites much easier to read.
pub(crate) struct VarResolutionCtx<'a> {
    pub var_name: &'a str,
    pub current_class: &'a ClassInfo,
    pub all_classes: &'a [Arc<ClassInfo>],
    pub content: &'a str,
    pub cursor_offset: u32,
    pub class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
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
    /// Legacy flag: historically selected branch-aware resolution for
    /// hover vs union-all resolution for completion.  The forward
    /// walker now inherently produces position-accurate types, so both
    /// paths behave identically.  Kept for API compatibility with
    /// callers that set it to `true` (hover, diagnostics).
    pub branch_aware: bool,
    /// Match-arm instanceof narrowings: var name → narrowed types.
    /// Empty outside of match(true) arm bodies.
    pub match_arm_narrowing: HashMap<String, Vec<crate::types::ResolvedType>>,
    /// Optional scope-based variable resolver from the forward walker.
    ///
    /// When set, `resolve_var_types` in `rhs_resolution.rs` reads
    /// variable types from this closure instead of re-entering
    /// `resolve_variable_types`, which would trigger a redundant
    /// forward walk of the method body.
    ///
    /// The closure takes a `$`-prefixed variable name and returns the
    /// variable's types from the forward walker's in-progress
    /// `ScopeState`.
    pub scope_var_resolver: ScopeVarResolverFn<'a>,
}

impl<'a> VarResolutionCtx<'a> {
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

    /// Convenience accessor for the constant loader.
    pub fn constant_loader(&self) -> ConstantLoaderFn<'a> {
        self.loaders.constant_loader
    }

    /// Clone this context with a different `cursor_offset`.
    ///
    /// All other fields (including `enclosing_return_type`) are preserved.
    /// This is useful when resolving a right-hand-side expression at a
    /// position earlier than the original cursor to avoid infinite
    /// recursion on self-referential assignments.
    pub(crate) fn with_cursor_offset(&self, cursor_offset: u32) -> VarResolutionCtx<'a> {
        VarResolutionCtx {
            var_name: self.var_name,
            current_class: self.current_class,
            all_classes: self.all_classes,
            content: self.content,
            cursor_offset,
            class_loader: self.class_loader,
            loaders: self.loaders,
            resolved_class_cache: self.resolved_class_cache,
            enclosing_return_type: self.enclosing_return_type.clone(),
            top_level_scope: self.top_level_scope.clone(),
            branch_aware: self.branch_aware,
            match_arm_narrowing: self.match_arm_narrowing.clone(),
            scope_var_resolver: self.scope_var_resolver,
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
            var_name: self.var_name,
            current_class: self.current_class,
            all_classes: self.all_classes,
            content: self.content,
            cursor_offset: self.cursor_offset,
            class_loader: self.class_loader,
            loaders: self.loaders,
            resolved_class_cache: self.resolved_class_cache,
            enclosing_return_type: self.enclosing_return_type.clone(),
            top_level_scope: self.top_level_scope.clone(),
            branch_aware: self.branch_aware,
            match_arm_narrowing,
            scope_var_resolver: self.scope_var_resolver,
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
