use super::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use mago_span::HasSpan;

use crate::atom::{Atom, AtomMap, atom, bytes_to_str};
use crate::completion::resolver::{Loaders, VarResolutionCtx};
use crate::parser::extract_hint_type;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, ResolvedType};

// ─── Core data structures ───────────────────────────────────────────────────

/// The type-state of all variables at a single program point.
///
/// This is the equivalent of PHPStan's `expressionTypes` map and Mago's
/// `BlockContext.locals`.  It is created once at the start of a function
/// body analysis, seeded with parameter types, and passed as `&mut` through
/// the forward walk.
#[derive(Clone, Debug)]
pub(crate) struct ScopeState {
    /// Variable name (with `$` prefix, e.g. `"$foo"`) → resolved types.
    ///
    /// This is the single source of truth for all variable types at the
    /// current program point.  Every variable that has been assigned,
    /// declared as a parameter, or bound by a foreach/catch before the
    /// current statement has an entry here.
    pub locals: AtomMap<Vec<ResolvedType>>,
}

impl ScopeState {
    /// Create an empty scope.
    pub fn new() -> Self {
        Self {
            locals: AtomMap::default(),
        }
    }

    /// Look up a variable's types.  Returns an empty slice when the
    /// variable has not been assigned.
    pub fn get(&self, var_name: &str) -> &[ResolvedType] {
        self.locals
            .get(&atom(var_name))
            .map_or(&[], |v| v.as_slice())
    }

    /// Check whether a variable exists in scope (even if its type list is empty).
    pub fn contains(&self, var_name: &str) -> bool {
        self.locals.contains_key(&atom(var_name))
    }

    /// Insert or overwrite a variable's types.
    pub fn set(&mut self, var_name: &str, types: Vec<ResolvedType>) {
        if types.is_empty() {
            return;
        }
        self.locals.insert(atom(var_name), types);
    }

    /// Record that a variable exists in scope with an empty type list.
    /// This prevents the variable from appearing unseen by the forward
    /// walker.
    pub fn set_empty(&mut self, var_name: &str) {
        self.locals.entry(atom(var_name)).or_default();
    }

    /// Insert a variable's types from parameter seeding.
    pub fn seed(&mut self, var_name: &str, types: Vec<ResolvedType>) {
        if types.is_empty() {
            return;
        }
        self.locals.insert(atom(var_name), types);
    }

    /// Remove a variable (e.g. after `unset($x)`).
    pub fn remove(&mut self, var_name: &str) {
        self.locals.remove(&atom(var_name));
    }

    /// Remove synthetic property/array-access keys rooted at `var_name`
    /// (e.g. `$s->cache`, `$s["k"]`).  Called when the base variable is
    /// reassigned: the previous object identity no longer holds, so any
    /// type tracked for one of its properties is stale.
    pub fn invalidate_dependent_keys(&mut self, var_name: &str) {
        let prop_prefix = format!("{var_name}->");
        let arr_prefix = format!("{var_name}[");
        self.locals
            .retain(|key, _| !key.starts_with(&prop_prefix) && !key.starts_with(&arr_prefix));
    }

    /// Merge another scope into `self`.
    ///
    /// For each variable:
    /// - Present in both: union the type sets (variable was assigned
    ///   in both branches).
    /// - Present in only one: keep it with the existing types (variable
    ///   was assigned in only one branch — it *might* have those types).
    ///
    /// After merging, subsumed entries are removed.  When one entry's
    /// type is a subset of another (e.g. `string|null` ⊆
    /// `int|string|null`, or `Foo` ⊆ `mixed`), the subset entry is
    /// dropped because the superset already covers it.  Without this,
    /// narrowed types from non-exiting if-branches leak into the
    /// post-merge scope and pollute subsequent narrowing operations.
    pub fn merge_branch(&mut self, other: &ScopeState) {
        for (name, other_types) in &other.locals {
            let entry = self.locals.entry(*name).or_default();

            // Merge other_types into entry.  When an incoming entry
            // shares a class name with an existing entry but has a
            // broader type_string (e.g. `?A` vs `A`), widen the
            // existing entry's type_string instead of discarding
            // the incoming one.  This prevents post-loop merges from
            // losing nullable information.
            for rt in other_types.iter() {
                let mut merged_into_existing = false;
                if let Some(ref rt_cls) = rt.class_info {
                    for existing in entry.iter_mut() {
                        if let Some(ref ex_cls) = existing.class_info
                            && ex_cls.name == rt_cls.name
                        {
                            // Same class.  If the incoming type is
                            // broader, adopt it.
                            if existing.type_string != rt.type_string
                                && existing.type_string.is_subset_of(&rt.type_string)
                            {
                                existing.type_string = rt.type_string.clone();
                            }
                            // A virtual member that only one branch's
                            // class_info carries (e.g. a member injected by
                            // `property_exists` / `method_exists` narrowing
                            // inside a guarded branch) must not survive the
                            // merge: the member is only proven where the
                            // guard held.  Drop any virtual member missing
                            // from the incoming branch.
                            drop_branch_local_virtual_members(existing, rt);
                            merged_into_existing = true;
                            break;
                        }
                    }
                } else if rt.type_string.is_array_shape() {
                    // Fold an incoming array-shape variant into an
                    // existing array-shape entry instead of accumulating
                    // one variant per branch (`array{a: int}` merged with
                    // `array{a: int, b: string}` becomes
                    // `array{a: int, b?: string}`).  A variable written
                    // key-by-key across hundreds of conditionals would
                    // otherwise collect hundreds of near-identical shape
                    // variants, and the pairwise subsumption pass below
                    // makes every subsequent merge quadratic in that
                    // variant count.
                    for existing in entry.iter_mut() {
                        if existing.class_info.is_none()
                            && let Some(joined) = existing.type_string.join_shapes(&rt.type_string)
                        {
                            existing.type_string = joined;
                            merged_into_existing = true;
                            break;
                        }
                    }
                }
                if !merged_into_existing {
                    ResolvedType::push_unique(entry, rt.clone());
                }
            }

            // Remove entries whose type is subsumed by a broader entry.
            // E.g. `string|null` ⊆ `int|string|null` → drop the former.
            if entry.len() > 1 {
                let types: Vec<crate::php_type::PhpType> =
                    entry.iter().map(|rt| rt.type_string.clone()).collect();
                let mut keep = vec![true; types.len()];
                for i in 0..types.len() {
                    if !keep[i] {
                        continue;
                    }
                    for j in 0..types.len() {
                        if i == j || !keep[j] {
                            continue;
                        }
                        // If j is a strict subset of i, drop j.
                        if types[j] != types[i] && types[j].is_subset_of(&types[i]) {
                            keep[j] = false;
                        }
                    }
                }
                let mut idx = 0;
                entry.retain(|_| {
                    let k = keep[idx];
                    idx += 1;
                    k
                });
            }
        }
    }
}

/// Drop virtual members from `existing`'s class_info that the `incoming`
/// branch's same-class class_info does not carry.
///
/// Branch-local narrowing (notably `property_exists` / `method_exists`)
/// injects a virtual member into a *clone* of the variable's class_info
/// for the guarded branch only.  When that branch merges with a sibling
/// that never proved the member, the union no longer guarantees it, so
/// the injected member must not leak into the merged scope.
///
/// Only virtual members are reconciled — real declared members are
/// identical across branches (same class source) and never removed.  A
/// virtual member present in *both* branches (e.g. an `@property` tag or
/// a Laravel model column baked into the base class_info) is kept,
/// because both branches derive from the same pre-branch class_info, so
/// any base virtual member appears on both sides and only narrowing-added
/// members appear on one.
pub(crate) fn drop_branch_local_virtual_members(
    existing: &mut ResolvedType,
    incoming: &ResolvedType,
) {
    let (Some(ex_cls), Some(in_cls)) = (&existing.class_info, &incoming.class_info) else {
        return;
    };
    // Same Arc → identical member sets, nothing to reconcile.  This is
    // the common case (no branch narrowed the type), so the merge stays
    // cheap.
    if Arc::ptr_eq(ex_cls, in_cls) {
        return;
    }

    let incoming_virtual_props: HashSet<&str> = in_cls
        .properties
        .iter()
        .filter(|p| p.is_virtual)
        .map(|p| p.name.as_str())
        .collect();
    let incoming_virtual_methods: HashSet<String> = in_cls
        .methods
        .iter()
        .filter(|m| m.is_virtual)
        .map(|m| m.name.to_ascii_lowercase())
        .collect();

    let drop_prop = ex_cls
        .properties
        .iter()
        .any(|p| p.is_virtual && !incoming_virtual_props.contains(p.name.as_str()));
    let drop_method = ex_cls
        .methods
        .iter()
        .any(|m| m.is_virtual && !incoming_virtual_methods.contains(&m.name.to_ascii_lowercase()));
    if !drop_prop && !drop_method {
        return;
    }

    let mut narrowed = (**ex_cls).clone();
    if drop_prop {
        narrowed
            .properties
            .make_mut()
            .retain(|p| !p.is_virtual || incoming_virtual_props.contains(p.name.as_str()));
    }
    if drop_method {
        narrowed.methods.make_mut().retain(|m| {
            !m.is_virtual || incoming_virtual_methods.contains(&m.name.to_ascii_lowercase())
        });
    }
    existing.class_info = Some(Arc::new(narrowed));
}

/// Simplify unions in a scope by collapsing child/parent class pairs.
///
/// When merging branches produces a union like `Child | Parent` where
/// `Child extends Parent`, the union is redundant — every value of
/// type `Child` is also a `Parent`.  This collapses such unions to
/// the broadest (parent) type.
///
/// Only operates on variables that have exactly two `ResolvedType`
/// entries with named class types.  More complex unions (3+ members,
/// scalars, generics) are left unchanged.
pub(crate) fn simplify_class_hierarchy_unions(
    scope: &mut ScopeState,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) {
    let keys: Vec<Atom> = scope.locals.keys().copied().collect();
    for key in keys {
        let types = match scope.locals.get(&key) {
            Some(t) if t.len() == 2 => t,
            _ => continue,
        };

        // Extract class names from the two ResolvedType entries.
        let name_a = match types[0].type_string.class_name() {
            Some(n) => n,
            None => continue,
        };
        let name_b = match types[1].type_string.class_name() {
            Some(n) => n,
            None => continue,
        };

        // Check if one is a subclass of the other.
        if is_subclass_of(name_a, name_b, class_loader) {
            // A extends B → keep B (the parent).
            scope.locals.get_mut(&key).unwrap().remove(0);
        } else if is_subclass_of(name_b, name_a, class_loader) {
            // B extends A → keep A (the parent).
            scope.locals.get_mut(&key).unwrap().remove(1);
        }
    }
}

/// Check whether `child` is a subclass (direct or transitive) of
/// `parent` by walking the inheritance chain via the class loader.
///
/// Returns `false` if either class cannot be loaded or if there is
/// no inheritance relationship.  Limits the chain walk to 20 steps
/// to avoid infinite loops on cyclic hierarchies.
pub(crate) fn is_subclass_of(
    child: &str,
    parent: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    if child.eq_ignore_ascii_case(parent) {
        return false; // same class, not a subclass
    }
    let mut current = child.to_string();
    for _ in 0..20 {
        let cls = match class_loader(&current) {
            Some(c) => c,
            None => return false,
        };
        // Check implemented interfaces at every level.
        for iface in &cls.interfaces {
            if iface.as_str().eq_ignore_ascii_case(parent) {
                return true;
            }
        }
        if let Some(ref p) = cls.parent_class {
            if p.as_str().eq_ignore_ascii_case(parent) {
                return true;
            }
            current = p.to_string();
        } else {
            return false;
        }
    }
    false
}

/// Context for the forward walk.
///
/// Bundles the immutable context that every statement/expression handler
/// needs — the class loader, function loader, current class info, source
/// text, etc.  The mutable `ScopeState` is passed separately as `&mut`.
pub(crate) struct ForwardWalkCtx<'a> {
    /// The class containing the method being analyzed (or a dummy for
    /// top-level functions).
    pub current_class: &'a ClassInfo,
    /// All classes known in the current file.
    pub all_classes: &'a [Arc<ClassInfo>],
    /// Full source text of the current file.
    pub content: &'a str,
    /// Byte offset of the cursor.  The walk stops when a statement's
    /// start offset reaches or exceeds this value.
    pub cursor_offset: u32,
    /// Cross-file class resolution callback.
    pub class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    /// Cross-file loader callbacks (function loader, constant loader).
    pub loaders: Loaders<'a>,
    /// Shared cache of fully-resolved classes.
    pub resolved_class_cache: Option<&'a crate::virtual_members::ResolvedClassCache>,
    /// The `@return` type of the enclosing function/method, if known.
    /// Used for generator yield inference.
    pub enclosing_return_type: Option<PhpType>,
    /// Pre-computed top-level scope for resolving `global` variable imports.
    /// When a function body contains `global $x;`, the walker looks up
    /// `$x` in this map to seed the local scope with the top-level type.
    pub top_level_scope: Option<AtomMap<Vec<ResolvedType>>>,
}

impl<'a> ForwardWalkCtx<'a> {
    /// Return a copy of this context with a different `cursor_offset`.
    ///
    /// Used by the two-pass loop strategy: pass 1 runs with
    /// `cursor_offset = u32::MAX` so the entire loop body is walked
    /// and all assignments are discovered, even those after the real
    /// cursor position.
    pub(crate) fn with_cursor_offset(&self, cursor_offset: u32) -> ForwardWalkCtx<'a> {
        ForwardWalkCtx {
            current_class: self.current_class,
            all_classes: self.all_classes,
            content: self.content,
            cursor_offset,
            class_loader: self.class_loader,
            loaders: self.loaders,
            resolved_class_cache: self.resolved_class_cache,
            enclosing_return_type: self.enclosing_return_type.clone(),
            top_level_scope: self.top_level_scope.clone(),
        }
    }

    /// Build a [`VarResolutionCtx`] with a scope-based variable
    /// resolver.  Used by [`resolve_rhs_with_scope`] so that
    /// `resolve_rhs_expression` and its sub-functions read variable
    /// types from the forward walker's in-progress `ScopeState`
    /// instead of re-entering `resolve_variable_types`.
    pub(crate) fn var_ctx_for_with_scope<'b>(
        &'b self,
        var_name: &'b str,
        cursor_offset: u32,
        scope_resolver: &'b dyn Fn(&str) -> Vec<ResolvedType>,
    ) -> VarResolutionCtx<'b>
    where
        'a: 'b,
    {
        VarResolutionCtx {
            var_name,
            current_class: self.current_class,
            all_classes: self.all_classes,
            content: self.content,
            cursor_offset,
            class_loader: self.class_loader,
            loaders: self.loaders,
            resolved_class_cache: self.resolved_class_cache,
            enclosing_return_type: self.enclosing_return_type.clone(),
            top_level_scope: self.top_level_scope.clone(),
            branch_aware: false,
            match_arm_narrowing: HashMap::new(),
            scope_var_resolver: Some(scope_resolver),
        }
    }
}

// ─── Parameter seeding ──────────────────────────────────────────────────────

/// Seed the scope with types from function/method parameters.
///
/// For each parameter, resolves its type from:
/// 1. The native type hint
/// 2. The `@param` docblock annotation (which may be more specific)
/// 3. The merged class info (from parent/interface inheritance)
/// 4. Eloquent scope Builder enrichment
pub(crate) fn seed_params<'b>(
    scope: &mut ScopeState,
    parameters: impl Iterator<Item = &'b FunctionLikeParameter<'b>>,
    method_span_start: u32,
    method_name: Option<&str>,
    has_scope_attr: bool,
    ctx: &ForwardWalkCtx<'_>,
) {
    for param in parameters {
        let pname = bytes_to_str(param.variable.name).to_string();
        let is_variadic = param.ellipsis.is_some();
        let native_type = param.hint.as_ref().map(|h| extract_hint_type(h));

        // For promoted constructor properties, check for an inline
        // `/** @var Type */` docblock on the parameter itself.  The
        // property parser already uses this for the property's type_hint,
        // but the forward walker resolves parameter variables via
        // `resolve_param_type` which only checks `@param` tags on the
        // method docblock.  When an inline `@var` is present, resolve it
        // directly and seed the scope, bypassing `resolve_param_type`
        // (which would otherwise fall back to the merged class's native
        // parameter type, losing the docblock refinement).
        if param.is_promoted_property() {
            let param_offset = param.span().start.offset as usize;
            if let Some((var_type, _name)) =
                crate::docblock::find_inline_var_docblock(ctx.content, param_offset)
            {
                let var_type = crate::util::resolve_php_type_names(&var_type, ctx.class_loader);
                let effective = crate::docblock::resolve_effective_type_typed(
                    native_type.as_ref(),
                    Some(&var_type),
                )
                .unwrap_or(var_type);

                let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                    &effective,
                    &ctx.current_class.name,
                    ctx.all_classes,
                    ctx.class_loader,
                );

                let results = if !resolved.is_empty() {
                    ResolvedType::from_classes_with_hint(resolved, effective)
                } else {
                    vec![ResolvedType::from_type_string(effective)]
                };

                scope.seed(&pname, results);
                continue;
            }
        }

        let param_results = resolve_param_type(
            &pname,
            native_type.as_ref(),
            is_variadic,
            method_span_start,
            method_name,
            has_scope_attr,
            ctx,
        );

        if !param_results.is_empty() {
            scope.seed(&pname, param_results);
        } else {
            // Seed untyped parameters with empty types so they exist
            // in scope.  This allows instanceof narrowing to find them
            // (apply_condition_narrowing iterates scope.locals.keys()).
            scope.set_empty(&pname);
        }
    }
}

/// Resolve a single parameter's type through the full resolution
/// pipeline: native hint → Eloquent Builder enrichment → docblock
/// `@param` → template substitution → merged class fallback →
/// type-string-only fallback.
///
/// Used by [`seed_params`] (forward walker) and
/// [`super::super::resolution::resolve_abstract_method_param`] (abstract
/// methods with no body).
pub(crate) fn resolve_param_type(
    pname: &str,
    native_type: Option<&PhpType>,
    is_variadic: bool,
    method_span_start: u32,
    method_name: Option<&str>,
    has_scope_attr: bool,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<ResolvedType> {
    // Eloquent scope Builder enrichment: when the enclosing class
    // extends Eloquent Model and this is a scope method (convention
    // or #[Scope] attribute), enrich bare `Builder` to
    // `Builder<EnclosingModel>`.
    let enriched_type = native_type.and_then(|nt| {
        if let Some(mname) = method_name {
            super::super::resolution::enrich_builder_type_in_scope(
                nt,
                mname,
                has_scope_attr,
                ctx.current_class,
                ctx.class_loader,
            )
        } else {
            None
        }
    });

    let type_for_resolution: Option<&PhpType> = enriched_type.as_ref().or(native_type);

    // Check the `@param` docblock annotation.
    let raw_docblock_type = crate::docblock::find_iterable_raw_type_in_source(
        ctx.content,
        method_span_start as usize,
        pname,
    )
    .map(|t| crate::util::resolve_php_type_names(&t, ctx.class_loader));

    // Pick the effective type: docblock overrides native when it is
    // a compatible refinement.  Use the enriched type (e.g.
    // `Builder<User>`) rather than the bare native type so that
    // the generic args survive into the resolved ClassInfo.
    let native_for_effective = enriched_type.as_ref().or(native_type).cloned();
    let doc_parsed = raw_docblock_type.clone();
    let effective_type = crate::docblock::resolve_effective_type_typed(
        native_for_effective.as_ref(),
        doc_parsed.as_ref(),
    );

    // Substitute method-level template params with their bounds.
    let effective_type = effective_type.map(|ty| {
        let ty = super::super::resolution::substitute_template_param_bounds(
            ty,
            ctx.content,
            method_span_start as usize,
        );
        // Also substitute inside class-string<T> so that
        // `class-string<T>` with `@template T of Foo` becomes
        // `class-string<Foo>`.
        super::super::resolution::substitute_class_string_template_bounds(
            ty,
            ctx.content,
            method_span_start as usize,
        )
    });

    let mut resolved_from_effective = effective_type
        .as_ref()
        .map(|ty| {
            crate::completion::type_resolution::type_hint_to_classes_typed(
                ty,
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            )
        })
        .unwrap_or_default();

    // When the effective type is `class-string<Foo>`, the base
    // type `class-string` doesn't resolve to a class.  Unwrap the
    // inner type and resolve it so that `$class::KEY` finds
    // static members on `Foo`.
    let mut resolved_from_class_string_inner = false;
    if resolved_from_effective.is_empty()
        && let Some(ref eff) = effective_type
        && let Some(inner) = eff.unwrap_class_string_inner()
    {
        let inner_resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
            inner,
            &ctx.current_class.name,
            ctx.all_classes,
            ctx.class_loader,
        );
        if !inner_resolved.is_empty() {
            resolved_from_effective = inner_resolved;
            resolved_from_class_string_inner = true;
        }
    }

    let mut param_results = if !resolved_from_effective.is_empty() {
        ResolvedType::from_classes_with_hint(
            resolved_from_effective,
            effective_type.unwrap_or_else(|| {
                type_for_resolution
                    .cloned()
                    .unwrap_or_else(PhpType::untyped)
            }),
        )
    } else if let Some(ref eff) = effective_type
        && raw_docblock_type.as_ref().is_some_and(|rdt| *rdt != *eff)
    {
        // The effective type differs from the raw docblock type, meaning
        // template substitution produced a concrete type (e.g. `K` →
        // `array-key`).  Use the substituted type so that downstream
        // narrowing (type guards, instanceof) operates on the concrete
        // type rather than the bare template parameter name.
        vec![ResolvedType::from_type_string(eff.clone())]
    } else if let Some(ref rdt) = raw_docblock_type {
        let parsed_docblock = rdt.clone();
        let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
            &parsed_docblock,
            &ctx.current_class.name,
            ctx.all_classes,
            ctx.class_loader,
        );
        if !resolved.is_empty() {
            ResolvedType::from_classes_with_hint(resolved, parsed_docblock)
        } else {
            // Try the merged class for a richer type.
            try_resolve_from_merged_class(pname, method_name, ctx).unwrap_or_else(|| {
                build_type_string_only_result(
                    raw_docblock_type.as_ref(),
                    type_for_resolution,
                    ctx.content,
                    method_span_start as usize,
                )
            })
        }
    } else {
        // Try the merged class.
        try_resolve_from_merged_class(pname, method_name, ctx).unwrap_or_else(|| {
            build_type_string_only_result(
                raw_docblock_type.as_ref(),
                type_for_resolution,
                ctx.content,
                method_span_start as usize,
            )
        })
    };

    // Preserve the `class-string<...>` wrapper on the resolved value
    // type.  When `class-string<A|B>` unwraps to multiple classes,
    // `from_classes_with_hint` rebuilds the union from bare class names,
    // which drops the wrapper and makes the value look like an instance
    // of the class rather than a class-string naming it.  Re-wrap each
    // class member so the value keeps its class-string type (matching the
    // single-class case, which already carries `class-string<Foo>`).
    if resolved_from_class_string_inner && param_results.len() > 1 {
        for rt in &mut param_results {
            if let Some(ci) = rt.class_info.as_ref() {
                let inner = PhpType::Named(ci.fqn().to_string());
                rt.type_string = PhpType::ClassString(Some(Box::new(inner)));
            }
        }
    }

    // Variadic parameter wrapping.
    if is_variadic && !param_results.is_empty() {
        for rt in &mut param_results {
            rt.type_string = PhpType::list(rt.type_string.clone());
            rt.class_info = None;
        }
    }

    param_results
}

/// Try to resolve a parameter type from the fully-merged class info
/// (with interface members merged and `@implements` generics applied).
///
/// When a class declares `@implements CastsAttributes<Decimal, Decimal>`
/// and the interface method `set()` has a generic parameter `TSet $value`,
/// the merged class will have `set($value: Decimal)`.  This function
/// looks up the merged method and returns the substituted parameter type.
pub(crate) fn try_resolve_from_merged_class(
    pname: &str,
    method_name: Option<&str>,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let method_name = method_name?;

    // Only attempt this for real classes (not the default/dummy class
    // used for top-level functions).
    if ctx.current_class.name.is_empty() {
        return None;
    }

    let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
        ctx.current_class,
        ctx.class_loader,
        ctx.resolved_class_cache,
    );

    let merged_method = merged.get_method(method_name)?;

    // Find the matching parameter by name.
    // ParameterInfo.name includes the `$` prefix.
    let merged_param = merged_method.parameters.iter().find(|p| p.name == pname)?;
    let hint = merged_param.type_hint.as_ref()?;

    let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
        hint,
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    );

    if !resolved.is_empty() {
        Some(ResolvedType::from_classes_with_hint(resolved, hint.clone()))
    } else {
        // The merged type doesn't resolve to a class (e.g. `list<Pen>`,
        // `array<string, int>`).  Return a type-string-only result so
        // the merged hint (which may be richer than the native type
        // from the child's signature, e.g. `list<Pen>` vs bare `array`)
        // is preserved in the scope.  This allows array-access
        // resolution to extract the element type from `list<Pen>`.
        Some(vec![ResolvedType::from_type_string(hint.clone())])
    }
}

/// Build a type-string-only `ResolvedType` result for a parameter whose
/// type does not resolve to any class.
pub(crate) fn build_type_string_only_result(
    raw_docblock_type: Option<&PhpType>,
    type_for_resolution: Option<&PhpType>,
    content: &str,
    method_span_start: usize,
) -> Vec<ResolvedType> {
    let best_type = if let Some(rdt) = raw_docblock_type {
        Some(rdt.clone())
    } else {
        type_for_resolution.cloned()
    };
    if let Some(mut parsed) = best_type {
        parsed = super::super::resolution::substitute_class_string_template_bounds(
            parsed,
            content,
            method_span_start,
        );
        vec![ResolvedType::from_type_string(parsed)]
    } else {
        vec![]
    }
}
