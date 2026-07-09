/// Type resolution for completion subjects.
///
/// This module contains the core entry points for resolving a completion
/// subject (e.g. `$this`, `self`, `static`, `$var`, `$this->prop`,
/// `ClassName`) to a concrete `ClassInfo` so that the correct completion
/// items can be offered.
///
/// The resolution logic is split across several sibling modules:
///
/// - [`super::call_resolution`]: Call expression and callable target
///   resolution (method calls, static calls, function calls, constructor
///   calls, signature help, named-argument completion).
/// - [`super::type_resolution`]: Type-hint string to `ClassInfo` mapping
///   (unions, intersections, generics, type aliases, object shapes).
/// - [`super::source_helpers`]: Source-text scanning helpers (closure return
///   types, first-class callable resolution, `new` expression parsing,
///   array access segment walking).
/// - [`super::variable_resolution`]: Variable type resolution via
///   assignment scanning and parameter type hints.
/// - [`super::type_narrowing`]: instanceof / assert / custom type guard
///   narrowing.
/// - [`super::closure_resolution`]: Closure and arrow-function parameter
///   resolution.
/// - [`crate::inheritance`]: Class inheritance merging (traits, mixins,
///   parent chain).
/// - [`super::conditional_resolution`]: PHPStan conditional return type
///   resolution at call sites.
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crate::atom::AtomMap;

use crate::Backend;
use crate::docblock;
use crate::inheritance::resolve_property_type_hint;
use crate::php_type::PhpType;
use crate::subject_expr::BracketSegment;
use crate::subject_expr::SubjectExpr;
use crate::types::*;
use crate::util::{find_class_by_name, is_self_or_static, resolve_class_keyword};
use crate::virtual_members::resolve_class_fully_maybe_cached;

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
    static CHAIN_CACHE: RefCell<Option<HashMap<String, Vec<ResolvedType>>>> =
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
pub(crate) type FunctionLoaderFn<'a> = Option<&'a dyn Fn(&str) -> Option<FunctionInfo>>;

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

/// Bundles the context needed by [`resolve_target_classes`] and
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
            function_loader: self.loaders.function_loader,
            resolved_class_cache: self.resolved_class_cache,
            scope_var_resolver: self.scope_var_resolver,
            is_in_static_method: false,
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
fn resolved_to_arcs(resolved: Vec<ResolvedType>) -> Vec<Arc<ClassInfo>> {
    ResolvedType::into_arced_classes(resolved)
}

/// Resolve a completion subject to all candidate types, preserving
/// both class info and type strings.
///
/// This is the primary entry point for subject resolution.  It returns
/// `Vec<ResolvedType>` which carries both the structured type string
/// (e.g. `PhpType::Named("Collection")`) and the optional `ClassInfo`.
/// Callers that only need classes can call
/// `ResolvedType::into_arced_classes()` on the result.
pub(crate) fn resolve_target_classes(
    subject: &str,
    access_kind: AccessKind,
    ctx: &ResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let expr = SubjectExpr::parse(subject);
    resolve_target_classes_expr(&expr, access_kind, ctx)
}

/// Core dispatch for [`resolve_target_classes`], operating on a
/// pre-parsed [`SubjectExpr`].
pub(crate) fn resolve_target_classes_expr(
    expr: &SubjectExpr,
    access_kind: AccessKind,
    ctx: &ResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    // ── Chain cache lookup ───────────────────────────────────────
    // During diagnostic passes the chain cache is active and stores
    // results by subject text.  This eliminates O(depth²) re-resolution
    // of shared chain prefixes (e.g. `$model->where(...)` resolved once
    // and reused by `$model->where(...)->whereNotNull(...)` etc.).
    //
    // The cache is NOT used for variable-only subjects (no `->` or `::`
    // in the expression) because those are context-sensitive: the same
    // `$var` may resolve to different types at different cursor offsets
    // due to reassignment or narrowing.
    //
    // PropertyChain expressions rooted in a variable (e.g. `$this->pet`,
    // `$obj->prop`) are also excluded because instanceof narrowing can
    // change the resolved type at different positions within the same
    // method body.  For example, `$this->pet` may resolve to `Dog`
    // inside `if ($this->pet instanceof Dog)` but to `Cat` after
    // `if (!$this->pet instanceof Cat) { return; }`.
    //
    // Call expressions and static accesses are safe to cache because
    // their return types are deterministic (method signatures don't
    // change based on narrowing context).
    let is_cacheable_chain = match expr {
        SubjectExpr::CallExpr { .. }
        | SubjectExpr::MethodCall { .. }
        | SubjectExpr::StaticMethodCall { .. }
        | SubjectExpr::StaticAccess { .. } => true,
        // PropertyChain is only cacheable when the base is NOT a
        // bare variable — e.g. `$this->method()->prop` (CallExpr
        // base) is safe, but `$this->pet` (This/Variable base) is
        // subject to narrowing.
        SubjectExpr::PropertyChain { base, .. } => !matches!(
            base.as_ref(),
            SubjectExpr::This
                | SubjectExpr::SelfKw
                | SubjectExpr::StaticKw
                | SubjectExpr::Parent
                | SubjectExpr::Variable(_)
        ),
        _ => false,
    };
    if is_cacheable_chain {
        let cache_key = expr.to_subject_text();
        let cached = CHAIN_CACHE.with(|cell| {
            let borrow = cell.borrow();
            borrow.as_ref().and_then(|map| map.get(&cache_key).cloned())
        });
        if let Some(result) = cached {
            return result;
        }

        let result = resolve_target_classes_expr_inner(expr, access_kind, ctx);

        CHAIN_CACHE.with(|cell| {
            let mut borrow = cell.borrow_mut();
            if let Some(ref mut map) = *borrow {
                map.insert(cache_key, result.clone());
            }
        });

        return result;
    }

    resolve_target_classes_expr_inner(expr, access_kind, ctx)
}

/// Inner implementation of [`resolve_target_classes_expr`] without
/// chain caching.  The outer function handles cache lookup/store.
fn resolve_target_classes_expr_inner(
    expr: &SubjectExpr,
    access_kind: AccessKind,
    ctx: &ResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    thread_local! {
        static RESOLVE_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }
    let depth = RESOLVE_DEPTH.with(|d| {
        let v = d.get() + 1;
        d.set(v);
        v
    });
    // Maximum nesting depth for `resolve_target_classes_expr_inner`.
    // Breaks infinite recursion between subject resolution, call-return
    // resolution, and variable resolution that can occur on files with
    // deeply intertwined class hierarchies and virtual members.
    const MAX_RESOLVE_TARGET_DEPTH: u32 = 60;
    if depth > MAX_RESOLVE_TARGET_DEPTH {
        RESOLVE_DEPTH.with(|d| d.set(depth - 1));
        return vec![];
    }
    let result = resolve_target_classes_expr_inner_impl(expr, access_kind, ctx);
    RESOLVE_DEPTH.with(|d| d.set(depth - 1));
    result
}

fn resolve_target_classes_expr_inner_impl(
    expr: &SubjectExpr,
    access_kind: AccessKind,
    ctx: &ResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let current_class = ctx.current_class;
    let all_classes = ctx.all_classes;
    let class_loader = ctx.class_loader;

    match expr {
        // ── Keywords that always mean "current class" ────────────
        SubjectExpr::This => {
            // `$this` is not available inside static methods.
            if current_class.is_some() && ctx.is_in_static_method {
                return vec![];
            }

            // Check for `@param-closure-this` override: when the cursor
            // is inside a closure passed as an argument to a function
            // whose parameter carries `@param-closure-this`, resolve
            // `$this` to the declared type instead of the lexical class.
            if let Some(override_cls) =
                super::variable::closure_resolution::find_closure_this_override(ctx)
            {
                return vec![ResolvedType::from_class(override_cls)];
            }
            current_class
                .map(|cc| ResolvedType::from_class(cc.clone()))
                .into_iter()
                .collect()
        }
        SubjectExpr::SelfKw | SubjectExpr::StaticKw => current_class
            .map(|cc| ResolvedType::from_class(cc.clone()))
            .into_iter()
            .collect(),

        // ── `parent::` — resolve to the current class's parent ──
        SubjectExpr::Parent => {
            if let Some(cc) = current_class
                && let Some(ref parent_name) = cc.parent_class
            {
                if let Some(cls) = find_class_by_name(all_classes, parent_name) {
                    return vec![ResolvedType::from_arc(Arc::clone(cls))];
                }
                return class_loader(parent_name)
                    .map(ResolvedType::from_arc)
                    .into_iter()
                    .collect();
            }
            vec![]
        }

        // ── Inline array literal with index access ──────────────
        SubjectExpr::InlineArray { elements, .. } => {
            let mut element_types = Vec::new();
            for elem_text in elements {
                let elem = elem_text.trim();
                if elem.is_empty() {
                    continue;
                }
                let elem_expr = SubjectExpr::parse(elem);
                let resolved = resolve_target_classes_expr(&elem_expr, AccessKind::Arrow, ctx);
                ResolvedType::extend_unique(&mut element_types, resolved);
            }
            element_types
        }

        // ── Enum case / static member access ────────────────────
        SubjectExpr::StaticAccess { class, member } => {
            // Handle self/static/parent keywords — SubjectExpr::parse
            // produces StaticAccess for "self::MONTH", "static::FOO",
            // etc., but "self"/"static"/"parent" are keywords, not
            // class names, so find_class_by_name / class_loader won't
            // find them.
            let owner_classes: Vec<Arc<ClassInfo>> = if is_self_or_static(class) {
                current_class
                    .map(|cc| Arc::new(cc.clone()))
                    .into_iter()
                    .collect()
            } else if let Some(parent_name) = resolve_class_keyword(class, current_class) {
                // parent — resolve via all_classes first, then class_loader
                if let Some(cls) = find_class_by_name(all_classes, &parent_name) {
                    vec![Arc::clone(cls)]
                } else {
                    class_loader(&parent_name).into_iter().collect()
                }
            } else {
                if let Some(cls) = find_class_by_name(all_classes, class) {
                    vec![Arc::clone(cls)]
                } else {
                    class_loader(class).into_iter().collect()
                }
            };

            // When the member is a static property (starts with `$`),
            // resolve to the property's declared type instead of the
            // owning class.  This makes `self::$instance->method()`
            // resolve `method()` on the property's type, not on the
            // class that declares the static property.
            if let Some(prop_name) = member.strip_prefix('$') {
                let mut results: Vec<ResolvedType> = Vec::new();
                for cls in &owner_classes {
                    let resolved = super::type_resolution::resolve_property_types(
                        prop_name,
                        cls,
                        all_classes,
                        class_loader,
                    );
                    ResolvedType::extend_unique(
                        &mut results,
                        resolved.into_iter().map(ResolvedType::from_arc).collect(),
                    );
                }
                if !results.is_empty() {
                    return results;
                }
            }

            owner_classes
                .into_iter()
                .map(ResolvedType::from_arc)
                .collect()
        }

        // ── Bare class name ─────────────────────────────────────
        SubjectExpr::ClassName(name) => {
            if let Some(cls) = find_class_by_name(all_classes, name) {
                return vec![ResolvedType::from_arc(Arc::clone(cls))];
            }
            class_loader(name)
                .map(ResolvedType::from_arc)
                .into_iter()
                .collect()
        }

        // ── `new ClassName` (without trailing call parens) ───────
        SubjectExpr::NewExpr { class_name } => {
            if let Some(cls) = find_class_by_name(all_classes, class_name) {
                return vec![ResolvedType::from_arc(Arc::clone(cls))];
            }
            class_loader(class_name)
                .map(ResolvedType::from_arc)
                .into_iter()
                .collect()
        }

        // ── Call expression ─────────────────────────────────────
        SubjectExpr::CallExpr { callee, args_text } => {
            let mut hint: Option<PhpType> = None;
            let classes = Backend::resolve_call_return_types_expr_with_hint(
                callee,
                args_text,
                ctx,
                Some(&mut hint),
            );
            // Use the raw return type hint only when at least one
            // resolved class has template parameters — non-generic
            // classes don't benefit from it.
            if let Some(h) = hint
                && classes.iter().any(|c| !c.template_params.is_empty())
            {
                return ResolvedType::from_classes_with_hint(classes, h);
            }

            classes.into_iter().map(ResolvedType::from_arc).collect()
        }

        // ── Property chain ──────────────────────────────────────
        SubjectExpr::PropertyChain { base, property } => {
            let base_arcs = resolved_to_arcs(resolve_target_classes_expr(base, access_kind, ctx));
            let mut arc_results: Vec<Arc<ClassInfo>> = Vec::new();
            for cls in &base_arcs {
                let resolved = super::type_resolution::resolve_property_types(
                    property,
                    cls,
                    all_classes,
                    class_loader,
                );

                ClassInfo::extend_unique_arc(&mut arc_results, resolved);
            }

            // ── Property-level narrowing ────────────────────────
            // When the property chain resolves to a union (or a
            // broad interface type), an enclosing `instanceof`
            // check like `if ($this->prop instanceof Foo)` should
            // narrow the result set, just as it does for plain
            // variables.  Build the full access path (e.g.
            // `$this->timeline`) and run the narrowing walk.
            //
            // This also handles untyped properties: when the
            // property has no type hint, `results` is empty but
            // an `instanceof` check or `assert()` can still
            // provide a type via `apply_instanceof_inclusion`.
            //
            // Use a dummy class when outside a class body so that
            // property narrowing works in standalone functions and
            // top-level code (e.g. `$arg->value instanceof Foo`
            // inside a foreach).
            {
                let dummy_class;
                let effective_class = match current_class {
                    Some(cc) => cc,
                    None => {
                        dummy_class = ClassInfo::default();
                        &dummy_class
                    }
                };
                let full_path = format!("{}->{}", base.to_subject_text(), property);
                apply_property_narrowing(&full_path, effective_class, ctx, &mut arc_results);
            }

            arc_results
                .into_iter()
                .map(ResolvedType::from_arc)
                .collect()
        }

        // ── Array access on variable or call expression ─────────
        SubjectExpr::ArrayAccess { base, segments } => {
            // Check if the scope has a narrowed type for this array
            // access (e.g. `$row['page']` narrowed via `instanceof`).
            if let Some(scope_resolver) = ctx.scope_var_resolver {
                // Build the scope key with double-quote format used by
                // `expr_to_subject_key` (e.g. `$row["page"]`).
                let scope_key = {
                    let mut k = base.to_subject_text();
                    for seg in segments {
                        match seg {
                            BracketSegment::StringKey(s) => {
                                k.push_str(&format!("[\"{}\"]", s));
                            }
                            BracketSegment::ElementAccess => {
                                k.push_str("[]");
                            }
                        }
                    }
                    k
                };
                let from_scope = scope_resolver(&scope_key);
                if !from_scope.is_empty() {
                    return from_scope;
                }
            }
            // When no scope resolver is available (top-level completion),
            // try resolving the full array access key through the forward
            // walker.  This picks up instanceof narrowing on array elements
            // (e.g. `$row['page'] instanceof Page` narrows `$row["page"]`).
            if ctx.scope_var_resolver.is_none() && matches!(base.as_ref(), SubjectExpr::Variable(_))
            {
                let scope_key = {
                    let mut k = base.to_subject_text();
                    for seg in segments {
                        match seg {
                            BracketSegment::StringKey(s) => {
                                k.push_str(&format!("[\"{}\"]", s));
                            }
                            BracketSegment::ElementAccess => {
                                k.push_str("[]");
                            }
                        }
                    }
                    k
                };
                let dummy_class;
                let effective_class = match current_class {
                    Some(cc) => cc,
                    None => {
                        dummy_class = ClassInfo::default();
                        &dummy_class
                    }
                };
                let resolved = crate::completion::variable::resolution::resolve_variable_types(
                    &scope_key,
                    effective_class,
                    all_classes,
                    ctx.content,
                    ctx.cursor_offset,
                    class_loader,
                    Loaders::with_function(ctx.function_loader),
                );
                if !resolved.is_empty() {
                    return resolved;
                }
            }

            // When the base is a call expression (e.g. `$c->items()[0]`),
            // resolve the call's raw return type and use it as a candidate
            // for array-segment walking.  This mirrors the variable path
            // but sources the raw type from the method/function signature
            // instead of from docblock annotations or assignments.
            if let SubjectExpr::CallExpr { callee, args_text } = base.as_ref() {
                let call_raw = resolve_call_raw_return_type(callee, args_text, ctx);
                if let Some(raw) = call_raw {
                    let candidates = std::iter::once(raw);
                    if let Some(resolved) =
                        super::source::helpers::try_chained_array_access_with_candidates(
                            candidates,
                            segments,
                            current_class,
                            all_classes,
                            class_loader,
                        )
                    {
                        return resolved.into_iter().map(ResolvedType::from_arc).collect();
                    }
                }
                // If raw-type approach didn't work, fall back to resolving
                // the call normally (handles cases like `getItems()[0]`
                // where the return type is already a class with ArrayAccess).
                return vec![];
            }

            let base_var = base.to_subject_text();

            // Build candidate raw types from multiple strategies.
            // Each is tried as a complete pipeline (raw type →
            // segment walk → ClassInfo); the first that succeeds
            // through all segments wins.

            // ── Property chain raw type ─────────────────────────
            // When the base is a property chain (e.g. `$this->cache`,
            // `$obj->items`), resolve the owning class and extract
            // the property's raw type hint.  This preserves generic
            // parameters like `array<string, IntCollection>` or
            // `Collection<int, Translation>` that would be lost if
            // we resolved through `type_hint_to_classes_typed` first.
            let property_raw_type: Option<PhpType> = if let SubjectExpr::PropertyChain {
                base: prop_base,
                property,
            } = base.as_ref()
            {
                let owner_arcs =
                    resolved_to_arcs(resolve_target_classes_expr(prop_base, access_kind, ctx));
                owner_arcs.iter().find_map(|cls| {
                    crate::inheritance::resolve_property_type_hint(cls, property, class_loader)
                })
            } else {
                None
            };

            let docblock_type: Option<PhpType> = docblock::find_iterable_raw_type_in_source(
                ctx.content,
                ctx.cursor_offset as usize,
                &base_var,
            )
            .map(|t| crate::util::resolve_php_type_names(&t, ctx.class_loader));
            // resolve_variable_types is designed for bare `$variable` names;
            // property chains like `$this->query->joins` are handled by the
            // property_raw_type strategy above.  Skip this strategy for
            // non-variable expressions (chains, array access, comparisons,
            // null coalescing, boolean expressions) to avoid polluting
            // the scope cache with unsupported keys.
            let is_bare_variable = !base_var.contains("->")
                && !base_var.contains("::")
                && !base_var.contains('[')
                && !base_var.contains("===")
                && !base_var.contains("&&")
                && !base_var.contains("??")
                && !base_var.contains("||");
            let ast_type: Option<PhpType> = if is_bare_variable {
                // When a scope_var_resolver is available (i.e. we are
                // inside the forward walker), read the variable type
                // from the in-progress ScopeState instead of calling
                // resolve_variable_types which would re-enter the
                // forward walker and cause stack overflow.
                if let Some(scope_resolver) = ctx.scope_var_resolver {
                    let prefixed = if base_var.starts_with('$') {
                        base_var.clone()
                    } else {
                        format!("${}", base_var)
                    };
                    let from_scope = scope_resolver(&prefixed);
                    if from_scope.is_empty() {
                        None
                    } else {
                        Some(ResolvedType::types_joined(&from_scope))
                    }
                } else {
                    let dummy_class;
                    let effective_class = match current_class {
                        Some(cc) => cc,
                        None => {
                            dummy_class = ClassInfo::default();
                            &dummy_class
                        }
                    };
                    let resolved = crate::completion::variable::resolution::resolve_variable_types(
                        &base_var,
                        effective_class,
                        all_classes,
                        ctx.content,
                        ctx.cursor_offset,
                        class_loader,
                        Loaders::with_function(ctx.function_loader),
                    );
                    if resolved.is_empty() {
                        None
                    } else {
                        Some(ResolvedType::types_joined(&resolved))
                    }
                }
            } else {
                None
            };

            let candidates = property_raw_type
                .into_iter()
                .chain(docblock_type)
                .chain(ast_type);

            if let Some(resolved) = super::source::helpers::try_chained_array_access_with_candidates(
                candidates,
                segments,
                current_class,
                all_classes,
                class_loader,
            ) {
                return resolved.into_iter().map(ResolvedType::from_arc).collect();
            }
            // Segment walk failed — the base type does not have
            // array-shape, generic, or iterable annotations that
            // cover bracket access.  Return empty: `$var['key']` is
            // never the same type as `$var`.
            vec![]
        }

        // ── Bare variable ───────────────────────────────────────
        SubjectExpr::Variable(var_name) => resolve_variable_fallback(var_name, access_kind, ctx),

        // ── Callee-only variants (MethodCall, StaticMethodCall,
        //    FunctionCall) should not appear as top-level subjects;
        //    they are wrapped in CallExpr.  If they do appear
        //    (e.g. from a partial parse), treat as class name. ────
        SubjectExpr::MethodCall { .. }
        | SubjectExpr::StaticMethodCall { .. }
        | SubjectExpr::FunctionCall(_) => {
            let text = expr.to_subject_text();
            if let Some(cls) = find_class_by_name(all_classes, &text) {
                return vec![ResolvedType::from_arc(Arc::clone(cls))];
            }
            class_loader(&text)
                .map(ResolvedType::from_arc)
                .into_iter()
                .collect()
        }
    }
}

/// Extract the raw return type string from a call expression's callee.
///
/// Given a `CallExpr`'s callee and arguments, resolves the owning class
/// (for method/static-method calls) or the function info (for standalone
/// functions), finds the matching method/function, and returns its raw
/// return type string (e.g. `"Item[]"`).  This is used by the
/// `ArrayAccess` handler to strip array dimensions and resolve the
/// element type when the base of `[0]` is a call expression.
fn resolve_call_raw_return_type(
    callee: &SubjectExpr,
    _args_text: &str,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    match callee {
        SubjectExpr::MethodCall { base, method } => {
            let base_classes =
                resolved_to_arcs(resolve_target_classes_expr(base, AccessKind::Arrow, ctx));
            for cls in &base_classes {
                // Use a fully-resolved class so that inherited docblock
                // return types (e.g. `list<Pen>` from an interface or
                // parent) are visible instead of the bare native hint.
                let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                    cls,
                    ctx.class_loader,
                    ctx.resolved_class_cache,
                );
                let found = merged.get_method_ci(method);
                if let Some(m) = found {
                    if let Some(ref ret) = m.return_type {
                        return Some(ret.clone());
                    }
                    // Method exists but has no return type.
                    // Only fall through to __call for virtual methods
                    // (from @method tags or @mixin). Real methods are
                    // invoked directly at runtime, not through __call.
                    if !m.is_virtual {
                        continue;
                    }
                }
                // __call fallback: method not found, or virtual method
                // without a return type.  Use __call's return type so
                // that chains through dynamic calls (e.g. Builder
                // where{Column}) preserve the type.
                if let Some(m) = merged.get_method_ci("__call")
                    && let Some(ref ret) = m.return_type
                {
                    return Some(ret.clone());
                }
            }
            None
        }
        SubjectExpr::StaticMethodCall { class, method } => {
            let owner = resolve_static_owner_class(class, ctx);
            if let Some(ref cls) = owner {
                let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                    cls,
                    ctx.class_loader,
                    ctx.resolved_class_cache,
                );
                let found = merged.get_method_ci(method);
                if let Some(m) = found {
                    if let Some(ref ret) = m.return_type {
                        return Some(ret.clone());
                    }
                    // Method exists but has no return type.
                    // Only fall through to __callStatic for virtual methods.
                    if !m.is_virtual {
                        return None;
                    }
                }
                // __callStatic fallback: method not found, or virtual
                // method without a return type.
                if let Some(m) = merged.get_method_ci("__callStatic")
                    && let Some(ref ret) = m.return_type
                {
                    return Some(ret.clone());
                }
            }
            None
        }
        SubjectExpr::FunctionCall(fn_name) => {
            if let Some(fl) = ctx.function_loader
                && let Some(func_info) = fl(fn_name)
            {
                return func_info.return_type.clone();
            }
            None
        }
        _ => None,
    }
}

// ─── Enriched subject resolution for diagnostics ────────────────────────────

/// The outcome of resolving a subject for diagnostic purposes.
///
/// [`resolve_target_classes`] only returns `Vec<Arc<ClassInfo>>` and
/// silently drops scalar types and type-string-only entries.
/// Diagnostics need to know *why* resolution returned empty — was the
/// subject a scalar type (runtime crash), an unresolvable class name
/// (likely typo / missing import), or truly untyped?  This enum
/// carries that distinction so the diagnostic collector can emit the
/// right message and severity.
///
/// ## Architectural invariant
///
/// Every `SubjectOutcome` **must** be derived from the same resolution
/// pass that completion and hover use.  Re-resolving a variable
/// through a secondary helper (e.g. `resolve_variable_type`)
/// bypasses narrowing (instanceof, assert, ternary, `&&`) and
/// produces false positives.  See [`resolve_subject_outcome`] for
/// how this is enforced for each subject variant.
#[derive(Clone, Debug)]
pub(crate) enum SubjectOutcome {
    /// Subject resolved to one or more classes.
    Resolved(Vec<Arc<ClassInfo>>),
    /// Subject resolved to a scalar type — member access is always a
    /// runtime crash.  The `PhpType` is the resolved scalar type
    /// (e.g. `int`, `string`, `bool|int`) with null stripped.
    Scalar(PhpType),
    /// Subject resolved to a class name that couldn't be loaded.
    UnresolvableClass(PhpType),
    /// Subject type could not be resolved — no class information
    /// available.
    Untyped,
}

/// Resolve a subject to a [`SubjectOutcome`] in a single pass.
///
/// This is the unified entry point for diagnostic subject resolution.
/// It resolves the subject to `Vec<ResolvedType>` (the same pipeline
/// used by completion and hover) and classifies the result:
///
///   - If any entry has `class_info`, return `Resolved`.
///   - If all entries are primitive scalars, return `Scalar`.
///   - If a type string refers to an unloadable class, return
///     `UnresolvableClass`.
///   - If the result is empty, return `Untyped`.
pub(crate) fn resolve_subject_outcome(
    subject: &str,
    access_kind: AccessKind,
    ctx: &ResolutionCtx<'_>,
) -> SubjectOutcome {
    let resolved = resolve_target_classes(subject, access_kind, ctx);
    if !resolved.is_empty() {
        // ── Check for class-bearing entries ──────────────────────
        let arced: Vec<Arc<ClassInfo>> = ResolvedType::into_arced_classes(resolved.clone());
        if !arced.is_empty() {
            return SubjectOutcome::Resolved(arced);
        }

        // ── All entries are type-string-only (no class info) ────
        let joined = ResolvedType::types_joined(&resolved);

        // Pure scalar — member access is a runtime crash.
        if joined.all_members_primitive_scalar() {
            let scalar = joined.non_null_type().unwrap_or(joined);
            return SubjectOutcome::Scalar(scalar);
        }

        // stdClass / object — synthetic resolution.
        if resolved
            .iter()
            .any(|rt| rt.type_string.is_named_ci("stdclass") || rt.type_string.is_object())
        {
            let synthetic = Arc::new(ClassInfo {
                name: crate::atom::atom("stdClass"),
                ..ClassInfo::default()
            });
            return SubjectOutcome::Resolved(vec![synthetic]);
        }

        // Non-scalar, non-class type — check for unresolvable class.
        if let Some(unresolved) = check_unresolvable_class_name(&joined, ctx.class_loader) {
            return SubjectOutcome::UnresolvableClass(unresolved);
        }
        return SubjectOutcome::Untyped;
    }

    // ── Result is empty — classify why ──────────────────────────
    let expr = SubjectExpr::parse(subject);

    // For call expressions, check the raw return type hint.
    if let SubjectExpr::CallExpr {
        callee,
        args_text: _,
    } = &expr
    {
        if let Some(scalar) = resolve_call_scalar_return(callee, access_kind, ctx) {
            return SubjectOutcome::Scalar(scalar);
        }
        // Try unresolvable class detection for function calls.
        if let SubjectExpr::FunctionCall(fn_name) = callee.as_ref()
            && let Some(fl) = ctx.function_loader
            && let Some(func_info) = fl(fn_name.as_str())
            && let Some(ref raw_type) = func_info.return_type
            && let Some(unresolved) = check_unresolvable_class_name(raw_type, ctx.class_loader)
        {
            return SubjectOutcome::UnresolvableClass(unresolved);
        }
    }

    // For property chains, check the property's type hint.
    if let SubjectExpr::PropertyChain { base, property } = &expr {
        let base_arcs = resolved_to_arcs(resolve_target_classes_expr(base, access_kind, ctx));
        for cls in &base_arcs {
            let merged =
                resolve_class_fully_maybe_cached(cls, ctx.class_loader, ctx.resolved_class_cache);
            if let Some(parsed) = resolve_property_type_hint(&merged, property, ctx.class_loader) {
                if parsed.all_members_primitive_scalar() {
                    let scalar = parsed.non_null_type().unwrap_or(parsed);
                    return SubjectOutcome::Scalar(scalar);
                }
                return SubjectOutcome::Untyped;
            }
        }
    }

    // For bare variables, try the hover fallback for UnresolvableClass
    // detection only.
    if let SubjectExpr::Variable(var_name) = &expr
        && let Some(resolved_type) =
            crate::completion::variable::resolution::resolve_variable_php_type(
                var_name,
                ctx.content,
                ctx.cursor_offset,
                ctx.current_class,
                ctx.all_classes,
                ctx.class_loader,
                Loaders::with_function(ctx.function_loader),
            )
        && let Some(unresolved) = check_unresolvable_class_name(&resolved_type, ctx.class_loader)
    {
        return SubjectOutcome::UnresolvableClass(unresolved);
    }

    SubjectOutcome::Untyped
}

/// Check whether a call expression's return type is a scalar.
///
/// Inspects the raw return type hint on the method or function without
/// going through the full class resolution pipeline.
fn resolve_call_scalar_return(
    callee: &SubjectExpr,
    access_kind: AccessKind,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    match callee {
        // Instance method call: $obj->getAge()
        SubjectExpr::MethodCall { base, method } => {
            let base_arcs = resolved_to_arcs(resolve_target_classes_expr(base, access_kind, ctx));
            for cls in &base_arcs {
                let resolved = resolve_class_fully_maybe_cached(
                    cls,
                    ctx.class_loader,
                    ctx.resolved_class_cache,
                );
                if let Some(m) = resolved.get_method_ci(method)
                    && let Some(ref hint) = m.return_type
                    && hint.all_members_primitive_scalar()
                {
                    let scalar = hint.non_null_type().unwrap_or_else(|| hint.clone());
                    return Some(scalar);
                }
            }
            None
        }
        // Standalone function call: getInt()
        SubjectExpr::FunctionCall(fn_name) => {
            if let Some(fl) = ctx.function_loader
                && let Some(func_info) = fl(fn_name)
                && let Some(ref hint) = func_info.return_type
                && hint.all_members_primitive_scalar()
            {
                let scalar = hint.non_null_type().unwrap_or_else(|| hint.clone());
                return Some(scalar);
            }
            None
        }
        // Static method call: Foo::getInt()
        SubjectExpr::StaticMethodCall { class, method } => {
            let cls = (ctx.class_loader)(class);
            if let Some(cls) = cls {
                let resolved = resolve_class_fully_maybe_cached(
                    &cls,
                    ctx.class_loader,
                    ctx.resolved_class_cache,
                );
                if let Some(m) = resolved.get_method_ci(method)
                    && let Some(ref hint) = m.return_type
                    && hint.all_members_primitive_scalar()
                {
                    let scalar = hint.non_null_type().unwrap_or_else(|| hint.clone());
                    return Some(scalar);
                }
            }
            None
        }
        _ => None,
    }
}

/// Check whether a raw type string refers to a class that cannot be
/// loaded.
///
/// Returns `Some(class_name)` when the type looks like a class name
/// (not scalar, not a PHPDoc pseudo-type) but the class loader cannot
/// find it.  Returns `None` for scalars, unions, shapes, and types
/// that resolve successfully.
fn check_unresolvable_class_name(
    raw_type: &PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    if raw_type.all_members_scalar() || raw_type.is_mixed() {
        return None;
    }

    let effective = raw_type.non_null_type().unwrap_or_else(|| raw_type.clone());
    let base = effective.base_name()?;

    if class_loader(base).is_none() {
        Some(PhpType::Named(base.to_string()))
    } else {
        None
    }
}

/// Shared variable-resolution logic extracted from the former
/// bare-`$var` branch of `resolve_target_classes`.
///
/// Resolves a variable to its classes by running the full variable
/// resolution pipeline (including narrowing from instanceof, assert,
/// ternary, and `&&` chains) and converting the result to
/// `Vec<Arc<ClassInfo>>` (dropping type-string-only entries).
fn resolve_variable_fallback(
    var_name: &str,
    access_kind: AccessKind,
    ctx: &ResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let current_class = ctx.current_class;
    let all_classes = ctx.all_classes;
    let class_loader = ctx.class_loader;
    let function_loader = ctx.function_loader;

    let dummy_class;
    let effective_class = match current_class {
        Some(cc) => cc,
        None => {
            dummy_class = ClassInfo::default();
            &dummy_class
        }
    };

    // ── `$var::` where `$var` holds a class-string ──
    if access_kind == AccessKind::DoubleColon {
        let class_string_targets =
            crate::completion::variable::class_string_resolution::resolve_class_string_targets(
                var_name,
                effective_class,
                all_classes,
                ctx.content,
                ctx.cursor_offset,
                class_loader,
            );
        if !class_string_targets.is_empty() {
            return class_string_targets
                .into_iter()
                .map(ResolvedType::from_class)
                .collect();
        }
    }

    // Guard: resolve_variable_types is designed for bare `$variable`
    // names.  SubjectExpr::Variable can carry complex expressions
    // (array access like `$arr['key']`, null coalescing, comparisons)
    // that will never match a scope entry.  Skip them to avoid wasted
    // backward scans and fallthrough noise.
    let is_bare_variable = !var_name.contains("->")
        && !var_name.contains("::")
        && !var_name.contains('[')
        && !var_name.contains("===")
        && !var_name.contains("&&")
        && !var_name.contains("??")
        && !var_name.contains("||");
    let resolved_types = if is_bare_variable {
        // When a scope variable resolver is available (i.e. we are
        // inside the forward walker's scope-building pass), read the
        // variable's type directly from the in-progress ScopeState.
        // This avoids calling resolve_variable_types which would
        // trigger a full forward walk of the method body for every
        // variable access — an O(N²) blowup on files with closures.
        if let Some(scope_resolver) = ctx.scope_var_resolver {
            let prefixed = if var_name.starts_with('$') {
                var_name.to_string()
            } else {
                format!("${}", var_name)
            };
            scope_resolver(&prefixed)
        } else {
            super::variable::resolution::resolve_variable_types(
                var_name,
                effective_class,
                all_classes,
                ctx.content,
                ctx.cursor_offset,
                class_loader,
                Loaders::with_function(function_loader),
            )
        }
    } else {
        vec![]
    };

    // ── @var docblock fallback ───────────────────────────────────
    // When the statement walk found no assignments for this variable,
    // check for a standalone `/** @var Type $var */` annotation above
    // the cursor.  This handles Blade templates and files where the
    // only type source is a docblock assertion.
    let resolved_types = if resolved_types.is_empty() && is_bare_variable {
        let prefixed = if var_name.starts_with('$') {
            var_name.to_string()
        } else {
            format!("${}", var_name)
        };
        if let Some(var_type) = crate::docblock::find_var_raw_type_in_source(
            ctx.content,
            ctx.cursor_offset as usize,
            &prefixed,
        ) {
            let classes = super::type_resolution::type_hint_to_classes_typed(
                &var_type,
                &effective_class.name,
                all_classes,
                class_loader,
            );
            classes.into_iter().map(ResolvedType::from_arc).collect()
        } else {
            vec![]
        }
    } else {
        resolved_types
    };

    // ── `class-string<T>` unwrapping for `$var::` access ────────
    // When the variable's type is `class-string<T>` (e.g. from a
    // `@param class-string<BackedEnum> $class` annotation) and the
    // access kind is `::`, unwrap the inner type `T` and resolve it
    // to classes so that static members are offered against `T`.
    if access_kind == AccessKind::DoubleColon {
        let mut class_string_results: Vec<ResolvedType> = Vec::new();
        for rt in &resolved_types {
            let inner = match &rt.type_string {
                PhpType::ClassString(Some(inner)) => Some(inner.as_ref()),
                // Handle `?class-string<T>` — unwrap nullable first.
                PhpType::Nullable(inner) => match inner.as_ref() {
                    PhpType::ClassString(Some(cs_inner)) => Some(cs_inner.as_ref()),
                    _ => None,
                },
                // Handle union types containing class-string<T>.
                PhpType::Union(members) => {
                    for member in members {
                        let cs_inner = match member {
                            PhpType::ClassString(Some(inner)) => Some(inner.as_ref()),
                            PhpType::Nullable(inner) => match inner.as_ref() {
                                PhpType::ClassString(Some(cs_inner)) => Some(cs_inner.as_ref()),
                                _ => None,
                            },
                            _ => None,
                        };
                        if let Some(inner_ty) = cs_inner {
                            let resolved = super::type_resolution::type_hint_to_classes_typed(
                                inner_ty,
                                &effective_class.name,
                                all_classes,
                                class_loader,
                            );
                            for cls in resolved {
                                ResolvedType::push_unique(
                                    &mut class_string_results,
                                    ResolvedType::from_arc(cls),
                                );
                            }
                        }
                    }
                    None // already handled inline
                }
                _ => None,
            };
            if let Some(inner_ty) = inner {
                let resolved = super::type_resolution::type_hint_to_classes_typed(
                    inner_ty,
                    &effective_class.name,
                    all_classes,
                    class_loader,
                );
                for cls in resolved {
                    ResolvedType::push_unique(
                        &mut class_string_results,
                        ResolvedType::from_arc(cls),
                    );
                }
            }
        }
        if !class_string_results.is_empty() {
            return class_string_results;
        }
    }

    resolved_types
}

// ── Static owner class resolution ───────────────────────────────────

/// Resolve a static class reference (`self`, `static`, `parent`, or a
/// class name) to its `ClassInfo`.
///
/// Handles the `self`/`static`/`parent` keywords and falls back to
/// `class_loader` then `resolve_target_classes` for named classes.
pub(in crate::completion) fn resolve_static_owner_class(
    class: &str,
    rctx: &ResolutionCtx<'_>,
) -> Option<Arc<ClassInfo>> {
    if is_self_or_static(class) {
        rctx.current_class.map(|cc| Arc::new(cc.clone()))
    } else if let Some(resolved_name) = resolve_class_keyword(class, rctx.current_class) {
        // parent — load via class_loader so we get the full parent ClassInfo
        (rctx.class_loader)(&resolved_name)
    } else {
        find_class_by_name(rctx.all_classes, class)
            .map(Arc::clone)
            .or_else(|| (rctx.class_loader)(class))
            .or_else(|| {
                resolved_to_arcs(resolve_target_classes(
                    class,
                    crate::AccessKind::DoubleColon,
                    rctx,
                ))
                .into_iter()
                .next()
            })
    }
}

/// Apply instanceof / assert narrowing for a property-access path.
///
/// This is the property-level analog of the narrowing that
/// [`super::variable::resolution::walk_statements_for_assignments`]
/// performs for plain variables.  It re-parses the source, locates
/// the enclosing method body, and walks its statements with a
/// [`VarResolutionCtx`] whose `var_name` is the full property path
/// (e.g. `$this->timeline`).  The existing narrowing functions in
/// [`super::types::narrowing`] already support property paths via
/// [`super::types::narrowing::expr_to_subject_key`], so no changes
/// to those functions are required.
pub(crate) fn apply_property_narrowing(
    property_path: &str,
    current_class: &ClassInfo,
    rctx: &ResolutionCtx<'_>,
    results: &mut Vec<Arc<ClassInfo>>, // still operates on Arc<ClassInfo> — called from property chain
) {
    use crate::parser::with_parsed_program;

    // The narrowing walk functions operate on Vec<ClassInfo>, so unwrap
    // the Arcs, run narrowing, then re-wrap.
    let mut plain: Vec<ClassInfo> = results.drain(..).map(Arc::unwrap_or_clone).collect();

    with_parsed_program(
        rctx.content,
        "apply_property_narrowing",
        |program, _content| {
            let ctx = VarResolutionCtx {
                var_name: property_path,
                current_class,
                all_classes: rctx.all_classes,
                content: rctx.content,
                cursor_offset: rctx.cursor_offset,
                class_loader: rctx.class_loader,
                loaders: Loaders::with_function(rctx.function_loader),
                resolved_class_cache: crate::virtual_members::active_resolved_class_cache(),
                enclosing_return_type: None,
                top_level_scope: None,
                branch_aware: false,
                match_arm_narrowing: HashMap::new(),
                scope_var_resolver: None,
            };
            walk_property_narrowing_in_statements(program.statements.iter(), &ctx, &mut plain);
        },
    );

    *results = plain.into_iter().map(Arc::new).collect();
}

/// Walk top-level statements to find the class + method containing the
/// cursor, then apply narrowing to `results` for the given property path.
fn walk_property_narrowing_in_statements<'b>(
    statements: impl Iterator<Item = &'b mago_syntax::ast::Statement<'b>>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) {
    use mago_span::HasSpan;
    use mago_syntax::ast::*;

    for stmt in statements {
        match stmt {
            Statement::Class(class) => {
                let start = class.left_brace.start.offset;
                let end = class.right_brace.end.offset;
                if ctx.cursor_offset >= start && ctx.cursor_offset <= end {
                    walk_property_narrowing_in_members(class.members.iter(), ctx, results);
                    return;
                }
            }
            Statement::Trait(trait_def) => {
                let start = trait_def.left_brace.start.offset;
                let end = trait_def.right_brace.end.offset;
                if ctx.cursor_offset >= start && ctx.cursor_offset <= end {
                    walk_property_narrowing_in_members(trait_def.members.iter(), ctx, results);
                    return;
                }
            }
            Statement::Namespace(ns) => {
                let ns_span = ns.span();
                if ctx.cursor_offset >= ns_span.start.offset
                    && ctx.cursor_offset <= ns_span.end.offset
                {
                    walk_property_narrowing_in_statements(ns.statements().iter(), ctx, results);
                    return;
                }
            }
            Statement::Function(func) => {
                let body_start = func.body.left_brace.start.offset;
                let body_end = func.body.right_brace.end.offset;
                if ctx.cursor_offset >= body_start && ctx.cursor_offset <= body_end {
                    walk_property_narrowing_stmts(func.body.statements.iter(), ctx, results);
                    return;
                }
            }
            // ── Functions inside if-guards / blocks ──
            // The common PHP pattern `if (! function_exists('foo'))
            // { function foo(…) { … } }` nests the function
            // declaration inside an if body.  Recurse into blocks
            // and if-bodies so property narrowing still works.
            Statement::If(if_stmt) => {
                let if_span = stmt.span();
                if ctx.cursor_offset >= if_span.start.offset
                    && ctx.cursor_offset <= if_span.end.offset
                {
                    for inner in if_stmt.body.statements().iter() {
                        walk_property_narrowing_in_statements(std::iter::once(inner), ctx, results);
                    }
                }
            }
            Statement::Block(block) => {
                let blk_span = stmt.span();
                if ctx.cursor_offset >= blk_span.start.offset
                    && ctx.cursor_offset <= blk_span.end.offset
                {
                    walk_property_narrowing_in_statements(block.statements.iter(), ctx, results);
                }
            }
            _ => {}
        }
    }
}

/// Walk class members to find the method containing the cursor, then
/// apply instanceof / guard-clause narrowing for the property path.
fn walk_property_narrowing_in_members<'b>(
    members: impl Iterator<Item = &'b mago_syntax::ast::class_like::member::ClassLikeMember<'b>>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) {
    use mago_syntax::ast::class_like::member::ClassLikeMember;
    use mago_syntax::ast::class_like::method::MethodBody;

    for member in members {
        if let ClassLikeMember::Method(method) = member {
            let body = match &method.body {
                MethodBody::Concrete(block) => block,
                _ => continue,
            };
            let body_start = body.left_brace.start.offset;
            let body_end = body.right_brace.end.offset;
            if ctx.cursor_offset >= body_start && ctx.cursor_offset <= body_end {
                walk_property_narrowing_stmts(body.statements.iter(), ctx, results);
                return;
            }
        }
    }
}

/// Walk statements applying only narrowing (no assignment scanning)
/// for a property path like `$this->prop`.
fn walk_property_narrowing_stmts<'b>(
    statements: impl Iterator<Item = &'b mago_syntax::ast::Statement<'b>>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) {
    use mago_span::HasSpan;
    use mago_syntax::ast::*;

    use super::types::narrowing;

    for stmt in statements {
        let stmt_span = stmt.span();
        // Only consider statements whose start is before the cursor.
        if stmt_span.start.offset >= ctx.cursor_offset {
            continue;
        }

        match stmt {
            Statement::If(if_stmt) => {
                walk_property_narrowing_if(if_stmt, stmt, ctx, results);
            }
            Statement::Block(block) => {
                walk_property_narrowing_stmts(block.statements.iter(), ctx, results);
            }
            Statement::Expression(expr_stmt) => {
                // assert($this->prop instanceof Foo) — unconditional
                narrowing::try_apply_assert_instanceof_narrowing(
                    expr_stmt.expression,
                    ctx,
                    results,
                );
            }
            Statement::Foreach(foreach) => match &foreach.body {
                ForeachBody::Statement(inner) => {
                    walk_property_narrowing_stmt(inner, ctx, results);
                }
                ForeachBody::ColonDelimited(body) => {
                    walk_property_narrowing_stmts(body.statements.iter(), ctx, results);
                }
            },
            Statement::While(while_stmt) => match &while_stmt.body {
                WhileBody::Statement(inner) => {
                    walk_property_narrowing_stmt(inner, ctx, results);
                }
                WhileBody::ColonDelimited(body) => {
                    walk_property_narrowing_stmts(body.statements.iter(), ctx, results);
                }
            },
            Statement::For(for_stmt) => match &for_stmt.body {
                ForBody::Statement(inner) => {
                    walk_property_narrowing_stmt(inner, ctx, results);
                }
                ForBody::ColonDelimited(body) => {
                    walk_property_narrowing_stmts(body.statements.iter(), ctx, results);
                }
            },
            Statement::DoWhile(dw) => {
                walk_property_narrowing_stmt(dw.statement, ctx, results);
            }
            Statement::Try(try_stmt) => {
                walk_property_narrowing_stmts(try_stmt.block.statements.iter(), ctx, results);
                for catch in try_stmt.catch_clauses.iter() {
                    walk_property_narrowing_stmts(catch.block.statements.iter(), ctx, results);
                }
                if let Some(finally) = &try_stmt.finally_clause {
                    walk_property_narrowing_stmts(finally.block.statements.iter(), ctx, results);
                }
            }
            Statement::Switch(switch) => {
                for case in switch.body.cases().iter() {
                    walk_property_narrowing_stmts(case.statements().iter(), ctx, results);
                }
            }
            _ => {}
        }
    }
}

/// Apply property-level narrowing inside an if / elseif / else chain.
fn walk_property_narrowing_if<'b>(
    if_stmt: &'b mago_syntax::ast::If<'b>,
    enclosing_stmt: &'b mago_syntax::ast::Statement<'b>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) {
    use mago_span::HasSpan;
    use mago_syntax::ast::*;

    use super::types::narrowing;

    match &if_stmt.body {
        IfBody::Statement(body) => {
            // ── then-body narrowing ──
            narrowing::try_apply_instanceof_narrowing(
                if_stmt.condition,
                body.statement.span(),
                ctx,
                results,
            );
            walk_property_narrowing_stmt(body.statement, ctx, results);

            // ── elseif narrowing ──
            for else_if in body.else_if_clauses.iter() {
                narrowing::try_apply_instanceof_narrowing(
                    else_if.condition,
                    else_if.statement.span(),
                    ctx,
                    results,
                );
                walk_property_narrowing_stmt(else_if.statement, ctx, results);
            }

            // ── else-body inverse narrowing ──
            if let Some(else_clause) = &body.else_clause {
                let else_span = else_clause.statement.span();
                narrowing::try_apply_instanceof_narrowing_inverse(
                    if_stmt.condition,
                    else_span,
                    ctx,
                    results,
                );
                for else_if in body.else_if_clauses.iter() {
                    narrowing::try_apply_instanceof_narrowing_inverse(
                        else_if.condition,
                        else_span,
                        ctx,
                        results,
                    );
                }
                walk_property_narrowing_stmt(else_clause.statement, ctx, results);
            }
        }
        IfBody::ColonDelimited(body) => {
            let then_end = if !body.else_if_clauses.is_empty() {
                body.else_if_clauses
                    .first()
                    .unwrap()
                    .elseif
                    .span()
                    .start
                    .offset
            } else if let Some(ref ec) = body.else_clause {
                ec.r#else.span().start.offset
            } else {
                body.endif.span().start.offset
            };
            let then_span = mago_span::Span::new(
                body.colon.file_id,
                body.colon.start,
                mago_span::Position::new(then_end),
            );
            narrowing::try_apply_instanceof_narrowing(if_stmt.condition, then_span, ctx, results);
            walk_property_narrowing_stmts(body.statements.iter(), ctx, results);

            for else_if in body.else_if_clauses.iter() {
                let ei_span = mago_span::Span::new(
                    else_if.colon.file_id,
                    else_if.colon.start,
                    mago_span::Position::new(
                        else_if
                            .statements
                            .span(else_if.colon.file_id, else_if.colon.end)
                            .end
                            .offset,
                    ),
                );
                narrowing::try_apply_instanceof_narrowing(else_if.condition, ei_span, ctx, results);
                walk_property_narrowing_stmts(else_if.statements.iter(), ctx, results);
            }

            if let Some(else_clause) = &body.else_clause {
                let else_span = mago_span::Span::new(
                    else_clause.colon.file_id,
                    else_clause.colon.start,
                    mago_span::Position::new(
                        else_clause
                            .statements
                            .span(else_clause.colon.file_id, else_clause.colon.end)
                            .end
                            .offset,
                    ),
                );
                narrowing::try_apply_instanceof_narrowing_inverse(
                    if_stmt.condition,
                    else_span,
                    ctx,
                    results,
                );
                for else_if in body.else_if_clauses.iter() {
                    narrowing::try_apply_instanceof_narrowing_inverse(
                        else_if.condition,
                        else_span,
                        ctx,
                        results,
                    );
                }
                walk_property_narrowing_stmts(else_clause.statements.iter(), ctx, results);
            }
        }
    }

    // ── Guard clause narrowing ──
    // When the then-body unconditionally exits and there are no
    // elseif / else branches, apply inverse narrowing after the if.
    if enclosing_stmt.span().end.offset < ctx.cursor_offset {
        narrowing::apply_guard_clause_narrowing(if_stmt, ctx, results);
    }
}

/// Dispatch a single statement to `walk_property_narrowing_stmts`.
fn walk_property_narrowing_stmt<'b>(
    stmt: &'b mago_syntax::ast::Statement<'b>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) {
    walk_property_narrowing_stmts(std::iter::once(stmt), ctx, results);
}
