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
///
/// Context types ([`ResolutionCtx`], [`VarResolutionCtx`], [`Loaders`]) and
/// the thread-local chain resolution cache live in [`context`].
/// Property-path (`$this->prop`) narrowing lives in [`property_narrowing`].
mod context;
mod property_narrowing;

pub(crate) use context::{
    FunctionLoaderFn, Loaders, ResolutionCtx, VarResolutionCtx, with_chain_resolution_cache,
};
pub(crate) use property_narrowing::apply_property_narrowing;

use std::sync::Arc;

use crate::Backend;
use crate::docblock;
use crate::inheritance::resolve_property_type_hint;
use crate::php_type::PhpType;
use crate::subject_expr::BracketSegment;
use crate::subject_expr::SubjectExpr;
use crate::types::*;
use crate::util::{find_class_by_name, is_self_or_static, resolve_class_keyword};
use crate::virtual_members::resolve_class_fully_maybe_cached;

use context::{CHAIN_CACHE, resolved_to_arcs};

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
    // `$obj->prop`, `$args[0]->value`, `$this->a->b`) are also excluded
    // because instanceof narrowing can change the resolved type at
    // different positions within the same method body.  For example,
    // `$this->pet` may resolve to `Dog` inside `if ($this->pet
    // instanceof Dog)` but to `Cat` after `if (!$this->pet instanceof
    // Cat) { return; }`.  The root test is transitive: a chain rooted in
    // a variable through array accesses or nested property chains
    // (`$args[0]->value`) is just as narrowable as a direct one.
    //
    // Call expressions and static accesses are safe to cache because
    // their return types are deterministic (method signatures don't
    // change based on narrowing context).
    let is_cacheable_chain = match expr {
        SubjectExpr::CallExpr { .. }
        | SubjectExpr::MethodCall { .. }
        | SubjectExpr::StaticMethodCall { .. }
        | SubjectExpr::StaticAccess { .. } => true,
        // PropertyChain is only cacheable when its base does NOT root
        // in a variable — e.g. `$this->method()->prop` (rooted in a
        // call) is safe, but `$this->pet`, `$args[0]->value`, and
        // `$this->a->b` (rooted in `$this`/a variable) are subject to
        // narrowing.
        SubjectExpr::PropertyChain { base, .. } => !base_roots_in_variable(base),
        _ => false,
    };
    if is_cacheable_chain {
        // A chain that references a local variable (as receiver or as a call
        // argument) can resolve to different types at call sites where the
        // variable holds a different type — e.g. `$this->parse($stmt)` where
        // `$stmt` is a different subtype in two methods, or a `@template T`
        // method binding `@return T` from a variable argument.  Keying by the
        // subject text alone would leak the result across sites, so mix in a
        // discriminator built from those variables' resolved types: sites
        // where the variables share a type still share the cache entry (so
        // the common case stays fast), while differently-typed sites get
        // distinct entries.  When the variables can't be resolved cheaply
        // (no active scope), fall back to a per-site key so nothing leaks.
        // Chains with no local variables keep the shared text-only key.
        let cache_key = {
            let mut vars = Vec::new();
            expr.collect_local_variables(&mut vars);
            if vars.is_empty() {
                expr.to_subject_text()
            } else if let Some(disc) = scope_type_discriminator(&vars, ctx) {
                format!("{}{}", expr.to_subject_text(), disc)
            } else {
                format!("{}@{}", expr.to_subject_text(), ctx.cursor_offset)
            }
        };
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

            // Consult the forward-walk scope for a narrowed or seeded
            // `$this` type.  This covers two cases the lexical
            // `current_class` fallback cannot:
            //   - `assert($this instanceof X)` inside a top-level closure
            //     (e.g. a Pest test) where there is no enclosing class,
            //     so `current_class` is `None`.
            //   - `instanceof` narrowing of `$this` to a subclass inside
            //     a regular method body.
            // When the scope yields nothing, fall back to the lexical
            // `current_class` below.
            let mut this_types = if let Some(scope_types) = resolve_this_from_scope(ctx) {
                scope_types
            } else {
                current_class
                    .map(|cc| ResolvedType::from_class(cc.clone()))
                    .into_iter()
                    .collect()
            };

            // A trait annotated `@phpstan-require-extends Base` or
            // `@phpstan-require-implements Contract` guarantees that every
            // class using the trait satisfies that bound, so inside the
            // trait's own methods `$this` can access those members. PHPStan
            // only ever analyzes traits in the context of a using class, but
            // we analyze them standalone, so resolve the required types
            // alongside the trait itself.
            if let Some(cc) = current_class
                && cc.kind == ClassLikeKind::Trait
            {
                if let Some(ref required) = cc.require_extends {
                    let resolved = find_class_by_name(all_classes, required)
                        .map(|cls| ResolvedType::from_arc(Arc::clone(cls)))
                        .or_else(|| class_loader(required).map(ResolvedType::from_arc));
                    if let Some(rt) = resolved {
                        ResolvedType::extend_unique(&mut this_types, vec![rt]);
                    }
                }
                for required in &cc.require_implements {
                    let resolved = find_class_by_name(all_classes, required)
                        .map(|cls| ResolvedType::from_arc(Arc::clone(cls)))
                        .or_else(|| class_loader(required).map(ResolvedType::from_arc));
                    if let Some(rt) = resolved {
                        ResolvedType::extend_unique(&mut this_types, vec![rt]);
                    }
                }
            }

            this_types
        }
        SubjectExpr::SelfKw | SubjectExpr::StaticKw => resolve_self_static_class(ctx)
            .map(ResolvedType::from_class)
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
                resolve_self_static_class(ctx)
                    .map(Arc::new)
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
            // `new X` is a source-level reference: PHP resolves an
            // unqualified name against the current namespace before the
            // global scope, so a same-namespace class must win over a
            // global stub of the same short name.
            let ns = current_class.and_then(|c| c.file_namespace.as_deref());
            let fqn = crate::util::resolve_source_class_name(class_name, ns, class_loader);
            class_loader(&fqn)
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
            // ── Forward-walker scope narrowing ──────────────────
            // The forward walker computes narrowing for compound
            // conditions that the property-narrowing re-walk below
            // cannot express: inline `&&` where a later conjunct uses
            // an earlier one's narrowing, `||` guard clauses whose
            // De Morgan expansion narrows several distinct subjects,
            // and array-indexed subjects.  When it has already
            // narrowed this exact property path, trust it.
            let full_path = subject_scope_key(expr);
            if let Some(narrowed) = lookup_scope_for_subject(&full_path, ctx) {
                return narrowed;
            }

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
                apply_property_narrowing(&full_path, effective_class, ctx, &mut arc_results);
            }

            arc_results
                .into_iter()
                .map(ResolvedType::from_arc)
                .collect()
        }

        // ── Array access on variable or call expression ─────────
        SubjectExpr::ArrayAccess { base, segments } => {
            // Build the scope key using the canonical double-quote
            // format that the forward walker's `expr_to_subject_key`
            // produces (e.g. `$row["page"]`, `$stmts["0"]`).  Integer
            // indices are stringified because PHP normalises them, so
            // `$a[0]` and `$a["0"]` narrow the same subject.
            let scope_key = subject_scope_key(expr);

            // Check if the forward-walker scope has a narrowed type for
            // this array access (e.g. `$row['page']` narrowed via
            // `instanceof`, or `$stmts[0]` after a guard clause).
            if let Some(narrowed) = lookup_scope_for_subject(&scope_key, ctx) {
                return narrowed;
            }

            // When no scope resolver is available (top-level completion),
            // try resolving the full array access key through the forward
            // walker.  This picks up instanceof narrowing on array elements
            // (e.g. `$row['page'] instanceof Page` narrows `$row["page"]`).
            if ctx.scope_var_resolver.is_none() && matches!(base.as_ref(), SubjectExpr::Variable(_))
            {
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
                // Resolve the call's return type with template and generic
                // substitution applied, so that a method declared
                // `@return T[]` with a `class-string<T>` parameter resolves
                // its element type from the call-site argument (e.g.
                // `$a->findChildrenOfType(Foo::class)[0]` → `Foo`).  The
                // un-substituted raw return type is kept as a fallback for
                // callees the hint path doesn't cover.
                let mut hint: Option<PhpType> = None;
                let _ = Backend::resolve_call_return_types_expr_with_hint(
                    callee,
                    args_text,
                    ctx,
                    Some(&mut hint),
                );
                let raw = resolve_call_raw_return_type(callee, args_text, ctx);
                let candidates = hint.into_iter().chain(raw);
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
                // Neither the substituted hint nor the raw return type had
                // array-shape / generic / iterable annotations covering the
                // bracket access.  Return empty: `call()[i]` is never the
                // same type as `call()`.
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
        // A call returning `object` (or `?object`) yields no concrete
        // class, but `object` is the "any object" escape hatch: member
        // access is always valid at runtime.  Resolve it to a synthetic
        // `stdClass` so downstream verification treats it like the plain
        // `object` property/parameter case, instead of reporting the
        // subject type as unresolved.  `is_object()` unwraps nullability,
        // so `?object` is handled here too.
        if let Some(raw_type) = resolve_call_raw_return_type(callee, "", ctx)
            && raw_type.is_object()
        {
            let synthetic = Arc::new(ClassInfo {
                name: crate::atom::atom("stdClass"),
                ..ClassInfo::default()
            });
            return SubjectOutcome::Resolved(vec![synthetic]);
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

/// Resolve `$this` from the forward-walk scope when it carries a
/// narrowed or seeded type.
///
/// `$this` is normally resolved from the lexical `current_class`, but a
/// closure body may have a `$this` type that the enclosing class cannot
/// supply:
///
///   - `assert($this instanceof X)` inside a top-level closure (Pest
///     tests) seeds `$this` in the forward-walk scope even though there
///     is no lexical class.
///   - `instanceof` narrowing refines `$this` to a subclass.
///
/// This consults the injected `scope_var_resolver` first (used while the
/// forward walker resolves an assignment RHS), then the diagnostic scope
/// snapshot cache (used by the member-verification path after the scope
/// has been built).  Returns `None` when neither yields a type so the
/// caller falls back to `current_class`.
fn resolve_this_from_scope(ctx: &ResolutionCtx<'_>) -> Option<Vec<ResolvedType>> {
    use crate::completion::variable::forward_walk;

    if let Some(scope_resolver) = ctx.scope_var_resolver {
        let from_scope = scope_resolver("$this");
        return (!from_scope.is_empty()).then_some(from_scope);
    }

    if forward_walk::is_diagnostic_scope_active()
        && !forward_walk::is_building_scopes()
        && let Some(from_scope) = forward_walk::lookup_diagnostic_scope("$this", ctx.cursor_offset)
        && !from_scope.is_empty()
    {
        return Some(from_scope);
    }

    None
}

/// Builds a cache-key discriminator from the resolved types of the local
/// variables an expression references, so that two textually identical
/// chains resolve from separate cache entries when their variables hold
/// different types.
///
/// Returns `None` when variable types cannot be resolved cheaply (no
/// forward-walk scope resolver and no active diagnostic scope snapshot);
/// the caller then falls back to a per-site key.  Variables that resolve
/// to nothing contribute an empty type: an unresolvable receiver yields an
/// empty chain result regardless, so sharing those entries is safe.
fn scope_type_discriminator(vars: &[String], ctx: &ResolutionCtx<'_>) -> Option<String> {
    use crate::completion::variable::forward_walk;

    let scope_active =
        forward_walk::is_diagnostic_scope_active() && !forward_walk::is_building_scopes();
    if ctx.scope_var_resolver.is_none() && !scope_active {
        return None;
    }

    let mut names: Vec<&String> = vars.iter().collect();
    names.sort();
    names.dedup();

    let mut disc = String::new();
    for name in names {
        let resolved: Vec<ResolvedType> = if let Some(scope_resolver) = ctx.scope_var_resolver {
            scope_resolver(name)
        } else {
            forward_walk::lookup_diagnostic_scope(name, ctx.cursor_offset).unwrap_or_default()
        };

        let mut parts: Vec<String> = resolved
            .iter()
            .map(|rt| match &rt.class_info {
                Some(ci) => ci.fqn().to_string(),
                None => rt.type_string.to_string(),
            })
            .collect();
        parts.sort();
        parts.dedup();

        disc.push('|');
        disc.push_str(name);
        disc.push('=');
        disc.push_str(&parts.join("&"));
    }
    Some(disc)
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

/// Resolve the class that a bare `self`/`static` keyword refers to at
/// the cursor position.
///
/// Normally this is the lexically enclosing class, but inside a
/// closure whose enclosing call site declares `@param-closure-this`
/// (e.g. a Laravel `Macroable` or Carbon `macro()` registration), the
/// runtime binds the closure with the target class as its scope
/// (`Closure::bind`), so `self::` and `static::` refer to the bound
/// target rather than the class that lexically encloses the closure.
fn resolve_self_static_class(ctx: &ResolutionCtx<'_>) -> Option<ClassInfo> {
    super::variable::closure_resolution::find_closure_this_override(ctx)
        .or_else(|| ctx.current_class.cloned())
}

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
        resolve_self_static_class(rctx).map(Arc::new)
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
/// Consult the forward-walker scope for a narrowed type for a compound
/// subject key (property path like `$a->b->c` or array access like
/// `$a["k"]`).
///
/// The forward walker seeds and narrows these keys while walking the
/// enclosing method, capturing narrowing shapes the property-narrowing
/// re-walk in [`apply_property_narrowing`] cannot express (compound
/// `&&`/`||` conditions with mixed subjects, guard clauses whose De
/// Morgan expansion narrows several distinct subjects, etc.).
///
/// Returns `Some(types)` only when the scope holds a non-empty narrowed
/// type for `key`; the caller then trusts it and skips the re-walk.
/// Returns `None` when no scope is active or the key was never seeded,
/// so the caller falls back to normal resolution.
/// Whether a subject expression transitively roots in a variable
/// (`$var`, `$this`, `self`, `static`, or `parent`), possibly through
/// array accesses or nested property chains.
///
/// Such expressions are subject to `instanceof`/`assert` narrowing that
/// changes their resolved type at different positions in the same method
/// body (e.g. `$args[0]->value instanceof Foo` in one `if` branch vs.
/// `instanceof Bar` in the following `elseif`).  Their resolution must
/// therefore never be cached by subject text alone.  Expressions rooted
/// in a call (`$this->make()->prop`) resolve deterministically and stay
/// cacheable.
fn base_roots_in_variable(expr: &SubjectExpr) -> bool {
    match expr {
        SubjectExpr::This
        | SubjectExpr::SelfKw
        | SubjectExpr::StaticKw
        | SubjectExpr::Parent
        | SubjectExpr::Variable(_) => true,
        SubjectExpr::PropertyChain { base, .. } => base_roots_in_variable(base),
        SubjectExpr::ArrayAccess { base, .. } => base_roots_in_variable(base),
        _ => false,
    }
}

/// Build the canonical forward-walker scope key for a subject
/// expression (e.g. `$row["page"]`, `$stmts["0"]`, `$args["0"]->value`).
///
/// Mirrors the format that `expr_to_subject_key` produces on the AST
/// side: property paths join with `->`, array keys use double quotes,
/// and integer indices are stringified so `$a[0]` and `$a["0"]` map to
/// the same key (matching PHP's integer/string key coercion).  Any
/// subject shape the forward walker does not key on falls back to
/// `to_subject_text`.
fn subject_scope_key(expr: &SubjectExpr) -> String {
    match expr {
        SubjectExpr::PropertyChain { base, property } => {
            format!("{}->{}", subject_scope_key(base), property)
        }
        SubjectExpr::ArrayAccess { base, segments } => {
            let mut k = subject_scope_key(base);
            for seg in segments {
                match seg {
                    BracketSegment::StringKey(s) => k.push_str(&format!("[\"{}\"]", s)),
                    BracketSegment::IntKey(n) => k.push_str(&format!("[\"{}\"]", n)),
                    BracketSegment::ElementAccess => k.push_str("[]"),
                }
            }
            k
        }
        _ => expr.to_subject_text(),
    }
}

fn lookup_scope_for_subject(key: &str, ctx: &ResolutionCtx<'_>) -> Option<Vec<ResolvedType>> {
    use crate::completion::variable::forward_walk;

    // During diagnostic passes the forward walker records scope
    // snapshots for the whole method; these are the authority.  Skip
    // while the snapshots are still being built (the walker is the
    // authority then and re-entry would be incomplete).
    // A snapshot exists but this key was never seeded → fall through to
    // normal resolution rather than short-circuiting to empty.
    if forward_walk::is_diagnostic_scope_active()
        && !forward_walk::is_building_scopes()
        && let Some(types) = forward_walk::lookup_diagnostic_scope(key, ctx.cursor_offset)
        && !types.is_empty()
    {
        return Some(types);
    }

    // Interactive (completion / hover) forward walk carries a live
    // scope resolver.
    if let Some(resolver) = ctx.scope_var_resolver {
        let types = resolver(key);
        if !types.is_empty() {
            return Some(types);
        }
    }

    None
}
