//! Seeding a scope with a function's parameters: the native hint, the
//! `@param` docblock, the merged class info, and the Eloquent scope
//! enrichment, plus the property-hook scope and the trait prototype lookup
//! the parameter types resolve through.

use super::*;

use mago_span::HasSpan;

use crate::atom::bytes_to_str;
use crate::parser::extract_hint_type;
use crate::php_type::PhpType;
use crate::types::{MethodInfo, ResolvedType};

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
    // While a body is being read for its return type, the call site that
    // asked has already resolved the arguments; a parameter they decide
    // more precisely than the signature seeds from them instead.
    let call_args = method_name.and_then(|name| {
        crate::type_engine::call_resolution::call_site_param_types(&ctx.current_class.fqn(), name)
    });

    let trait_prototype = trait_prototype_method(method_name, ctx);

    for (index, param) in parameters.enumerate() {
        let pname = bytes_to_str(param.variable.name).to_string();
        let is_variadic = param.ellipsis.is_some();
        // Qualify the hint against this file's imports, as `@param` tags are.
        let native_type = param.hint.as_ref().map(|h| {
            crate::util::resolve_source_php_type_names(
                &extract_hint_type(h),
                ctx.current_class.file_namespace.as_deref(),
                ctx.class_loader,
            )
        });

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

                let results = ctx.resolved_types_for(effective);

                scope.seed(&pname, results);
                continue;
            }
        }

        let param_results = resolve_param_type(
            &pname,
            native_type.as_ref(),
            is_variadic,
            &EnclosingMethod {
                span_start: method_span_start,
                name: method_name,
                has_scope_attr,
                trait_prototype: trait_prototype.as_ref(),
            },
            ctx,
        );

        // A variadic parameter collects the arguments into an array
        // rather than taking the one at its index, so the call site's
        // types don't line up with it.
        if !is_variadic
            && let Some(arg_type) = call_args.as_ref().and_then(|args| args.get(index))
            && let Some(seeded) = seed_from_call_site(arg_type, &param_results, ctx)
        {
            scope.seed(&pname, seeded);
            continue;
        }

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

/// The scope entry a parameter gets from the argument the call site
/// passed it, or `None` when the declaration already says as much.
///
/// The call site only wins where it is strictly more specific than the
/// declaration: an untyped parameter, or one whose declared type the
/// argument is a proper subtype of (`string` handed `'shell'`,
/// `\ReflectionClass` handed a `ReflectionObject<Configuration>`).  A
/// wider or unrelated argument is a call the declaration already rejects,
/// and reading the body as if it were valid would only spread the error.
fn seed_from_call_site(
    arg_type: &PhpType,
    declared: &[ResolvedType],
    ctx: &ForwardWalkCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    if !arg_type.is_informative() {
        return None;
    }
    if !declared.is_empty() {
        let declared_type = ResolvedType::types_joined(declared);
        if declared_type.equivalent(arg_type)
            || !crate::class_lookup::is_subtype_of_typed(arg_type, &declared_type, ctx.class_loader)
        {
            return None;
        }
    }

    let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        arg_type,
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    );
    Some(if classes.is_empty() {
        vec![ResolvedType::from_type_string(arg_type.clone())]
    } else {
        ResolvedType::from_classes_with_hint(classes, arg_type.clone())
    })
}

/// Seed a fresh scope for a property hook body.
///
/// A hook body is a method body in every way the walker cares about:
/// `$this` is the enclosing instance (a hook can never be static), and a
/// `set` hook receives the assigned value as a parameter.  When a `set`
/// hook writes no parameter list of its own, PHP still gives it a `$value`
/// typed as the property, so seed that from `property_hint`.
pub(crate) fn seed_property_hook_scope(
    property_hint: Option<&Hint<'_>>,
    hook: &PropertyHook<'_>,
    ctx: &ForwardWalkCtx<'_>,
) -> ScopeState {
    let mut scope = ScopeState::new();
    seed_this(&mut scope, ctx);

    if let Some(params) = &hook.parameter_list {
        seed_params(
            &mut scope,
            params.parameters.iter(),
            hook.span().start.offset,
            None,
            false,
            ctx,
        );
    } else if hook.name.value.eq_ignore_ascii_case(b"set") {
        seed_implicit_set_value(&mut scope, property_hint, ctx);
    }

    seed_superglobals(&mut scope);
    scope
}

/// Seed the `$value` a `set` hook receives when it declares no parameter
/// list.  Its type is the property's own declared type.
fn seed_implicit_set_value(
    scope: &mut ScopeState,
    property_hint: Option<&Hint<'_>>,
    ctx: &ForwardWalkCtx<'_>,
) {
    let Some(hint) = property_hint else {
        scope.set_empty("$value");
        return;
    };

    let hint_type = extract_hint_type(hint);

    scope.seed("$value", ctx.resolved_types_for(hint_type));
}

/// Finish the type operators a declared type reads through a constant, or
/// `None` when it has none to finish.
///
/// `key-of<ID_TABLE>` names a set of values as concrete as any written-out
/// union, but the docblock parser only ever saw the constant's name.  Every
/// place a declared parameter type is read has to read the constant behind
/// it too, or the operator widens to whatever a key could be in general and
/// the parameter constrains nothing.
fn finish_constant_operands(ty: &PhpType, ctx: &ForwardWalkCtx<'_>) -> Option<PhpType> {
    if !ty.contains_unevaluated_operator() {
        return None;
    }
    crate::type_engine::call_resolution::evaluate_constant_operands(ty, &ctx.as_resolution_ctx())
}

/// Finish a `@param` type the docblock parser could only read as text:
/// qualify the class names in it, bind `self` to the enclosing class, then
/// evaluate the type operators it reads through a constant.
///
/// Reading the constant here means the body sees the keys the table
/// actually has, and the declaration is judged a refinement of the native
/// `string` hint rather than an operator nothing can compare.
pub(crate) fn resolve_docblock_param_type(raw: &PhpType, ctx: &ForwardWalkCtx<'_>) -> PhpType {
    let resolved = crate::util::resolve_php_type_names(raw, ctx.class_loader);
    let resolved = bind_enclosing_self(&resolved, ctx).unwrap_or(resolved);
    finish_constant_operands(&resolved, ctx).unwrap_or(resolved)
}

/// A declared parameter type with `self` bound to the enclosing class, or
/// `None` when there is nothing to bind.
///
/// `self` is lexical, so it names the enclosing class wherever the value
/// travels afterwards (`$o->foo` on an `object{foo: self}`).  A trait is
/// left alone: there `self` is whichever class uses it.
fn bind_enclosing_self(ty: &PhpType, ctx: &ForwardWalkCtx<'_>) -> Option<PhpType> {
    let class = ctx.current_class;
    (!class.name.is_empty()
        && class.kind != crate::types::ClassLikeKind::Trait
        && ty.contains_bare_self())
    .then(|| ty.replace_bare_self(&class.fqn()))
}

/// The declaration a parameter belongs to, as far as resolving its type
/// needs to know it.
#[derive(Clone, Copy)]
pub(crate) struct EnclosingMethod<'a> {
    /// Byte offset the declaration starts at, which the `@param` scan
    /// reads backward from.
    pub span_start: u32,
    /// `None` for a top-level function, where no method-shaped enrichment
    /// applies.
    pub name: Option<&'a str>,
    /// The declaration carries `#[Scope]`, for Eloquent query scopes.
    pub has_scope_attr: bool,
    /// The declaration a trait method implements, which the trait itself
    /// cannot reach — see [`trait_prototype_method`].
    pub trait_prototype: Option<&'a MethodInfo>,
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
    enclosing: &EnclosingMethod<'_>,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<ResolvedType> {
    let EnclosingMethod {
        span_start: method_span_start,
        name: method_name,
        has_scope_attr,
        trait_prototype,
    } = *enclosing;
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

    // Check the `@param` docblock annotation.
    let raw_docblock_type = super::super::resolution::declared_param_docblock_type(
        ctx.content,
        method_span_start as usize,
        pname,
    )
    .map(|t| resolve_docblock_param_type(&t, ctx));

    // With no `@param` of its own, an override inherits the ancestor's,
    // which `@extends`/`@implements` template substitution may have
    // narrowed below the native hint PHP forced the override to restate.
    let inherited_refinement = if raw_docblock_type.is_none() && enriched_type.is_none() {
        inherited_param_refinement(pname, method_name, native_type, ctx)
    } else {
        None
    };

    // A trait's own merged declaration is the un-refined one, so unlike an
    // override's it cannot be read back below — the prototype's `@param`
    // has to be carried through as the effective type instead.
    let trait_refinement =
        if inherited_refinement.is_none() && raw_docblock_type.is_none() && enriched_type.is_none()
        {
            trait_prototype.and_then(|proto| prototype_param_refinement(proto, pname, native_type))
        } else {
            None
        };
    let inherited_refinement = inherited_refinement.or_else(|| trait_refinement.clone());

    let type_for_resolution: Option<&PhpType> = inherited_refinement
        .as_ref()
        .or(enriched_type.as_ref())
        .or(native_type);

    // Pick the effective type: docblock overrides native when it is
    // a compatible refinement.  Use the enriched type (e.g.
    // `Builder<User>`) rather than the bare native type so that
    // the generic args survive into the resolved ClassInfo.
    let native_for_effective = type_for_resolution.cloned();
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
            crate::type_engine::type_resolution::type_hint_to_classes_typed(
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
        let inner_resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
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
        && (trait_refinement.is_some()
            || raw_docblock_type.as_ref().is_some_and(|rdt| *rdt != *eff))
    {
        // The effective type differs from the raw docblock type, meaning
        // template substitution produced a concrete type (e.g. `K` →
        // `array-key`).  Use the substituted type so that downstream
        // narrowing (type guards, instanceof) operates on the concrete
        // type rather than the bare template parameter name.
        vec![ResolvedType::from_type_string(eff.clone())]
    } else if let Some(ref rdt) = raw_docblock_type {
        let parsed_docblock = rdt.clone();
        let resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
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
                let inner = PhpType::named(ci.fqn());
                rt.type_string = PhpType::class_string(Some(inner));
            }
        }
    }

    // A variadic parameter collects its arguments into an array.  Named
    // arguments land in it under their names, so its keys are
    // `int|string` rather than a list's (PHPStan and Psalm agree).  The
    // array exists even when the elements are untyped.
    if is_variadic {
        let variadic_array = |elem: PhpType| {
            PhpType::generic(
                "array",
                vec![
                    PhpType::union(vec![PhpType::int(), PhpType::string()]),
                    elem,
                ],
            )
        };
        if param_results.is_empty() {
            param_results.push(ResolvedType::from_type_string(PhpType::mixed()));
        }
        for rt in &mut param_results {
            rt.type_string = variadic_array(rt.type_string.clone());
            rt.class_info = None;
        }
    }

    param_results
}

/// The declaration a trait's own method implements.
///
/// A trait has no parent class and no interface list, so
/// [`inherited_param_refinement`] has nothing to read. PHP flattens the
/// trait into each using class, and the interface method it implements is
/// declared there, so the bounds every host is guaranteed to satisfy (see
/// [`crate::type_engine::trait_context`]) are where the prototype lives.
///
/// Resolved once per body rather than per parameter: finding a trait's
/// hosts means reading the reverse-inheritance index and loading each one.
pub(crate) fn trait_prototype_method(
    method_name: Option<&str>,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<MethodInfo> {
    let method_name = method_name?;
    let class = ctx.current_class;
    if class.kind != crate::types::ClassLikeKind::Trait {
        return None;
    }
    crate::type_engine::trait_context::trait_this_bounds(
        class,
        ctx.all_classes,
        ctx.class_loader,
        ctx.backend,
    )
    .iter()
    .find_map(|bound| {
        crate::virtual_members::resolve_class_fully_maybe_cached(
            bound,
            ctx.class_loader,
            ctx.resolved_class_cache,
        )
        .get_method(method_name)
        .cloned()
    })
}

/// The `@param` type `prototype` declares for `pname`, when the current
/// declaration only restates the native hint.
///
/// Same test as [`inherited_param_refinement`]: the prototype parameter
/// must be the same declaration (identical native hint) carrying a
/// docblock type that differs from it, which is the refinement PHP's own
/// signature rules could not express.
fn prototype_param_refinement(
    prototype: &MethodInfo,
    pname: &str,
    native_type: Option<&PhpType>,
) -> Option<PhpType> {
    let native = native_type?;
    let param = prototype.parameters.iter().find(|p| p.name == pname)?;
    let hint = param.type_hint.as_ref()?;
    (param.native_type_hint.as_ref() == Some(native) && hint != native).then(|| hint.clone())
}

/// The narrower parameter type an override inherits from its ancestor's
/// `@param` docblock.
///
/// PHP requires an override to restate every native type hint, so
/// `processNode(Node $node)` implementing `@param TNodeType $node` on
/// `@implements Rule<CallLike>` still receives a `CallLike`.  The merged
/// class carries that substituted type (see
/// `inheritance::enrichment::child_native_hint_overrides`); this reads it
/// back out so the walker seeds the body with the refined type instead of
/// the restated hint.
///
/// Only consulted for parameters whose native hint names a class and that
/// carry no `@param` of their own, so the merged-class lookup stays off
/// the common path.
fn inherited_param_refinement(
    pname: &str,
    method_name: Option<&str>,
    native_type: Option<&PhpType>,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<PhpType> {
    let method_name = method_name?;
    let native = native_type?;
    // Only a class-named hint can be refined by an inherited docblock.
    native.base_name()?;
    let class = ctx.current_class;
    if class.name.is_empty()
        || (class.parent_class.is_none() && class.interfaces.is_empty() && class.mixins.is_empty())
    {
        return None;
    }

    let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
        class,
        ctx.class_loader,
        ctx.resolved_class_cache,
    );
    let param = merged
        .get_method(method_name)?
        .parameters
        .iter()
        .find(|p| p.name == pname)?;
    let hint = param.type_hint.as_ref()?;

    // The merged parameter must be the same declaration (same native
    // hint) carrying a docblock type that differs from it.  Enrichment
    // only copies an ancestor type when it is a genuine refinement, so
    // the difference is the inherited narrowing.
    (param.native_type_hint.as_ref() == Some(native) && hint != native).then(|| hint.clone())
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
    let declared = merged_param.type_hint.as_ref()?;
    // The merged declaration is as much a place a `key-of<CONSTANT>` is read
    // as the source docblock is, and for a method it is the one that wins.
    let bound = bind_enclosing_self(declared, ctx);
    let declared = bound.as_ref().unwrap_or(declared);
    let finished = finish_constant_operands(declared, ctx);
    let hint = finished.as_ref().unwrap_or(declared);

    let resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
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
