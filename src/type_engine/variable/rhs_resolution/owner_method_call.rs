//! Resolving a method call's return type against one fully determined
//! owner class, shared by instance and static calls: template
//! substitution, conditional return types, and reading the method body
//! when its declaration says too little.

use std::collections::HashMap;
use std::sync::Arc;

use mago_syntax::cst::*;

use crate::Backend;
use crate::atom::atom;
use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, MethodInfo, ResolvedType};

use crate::type_engine::call_resolution::MethodReturnCtx;
use crate::type_engine::resolver::VarResolutionCtx;
use crate::type_engine::variable::resolution::build_var_resolver_from_ctx;

use super::calls::resolve_return_type;
use super::{resolve_rhs_expression, resolved_type_with_lookup};

/// Resolve a method's PHPStan conditional return type against the call-site
/// arguments, returning the winning branch's type when it is definite and
/// informative.
///
/// The returned type has template substitutions applied, `self`/`static`/
/// `$this` replaced (via the `replace_self` closure, which differs between the
/// instance and static call paths), and any conditionals nested inside the
/// winning branch collapsed.  Returns `None` when the method has no
/// conditional return type, the condition cannot be decided from the
/// arguments, or the winning branch is uninformative (a bare `mixed`/`array`
/// else-branch) — in which case the caller falls back to the native return
/// type so the full union (including scalar/`array` members) is preserved.
#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_conditional_return_for_call(
    method_ref: Option<&crate::types::MethodInfo>,
    text_args: &str,
    var_resolver: crate::type_engine::conditional_resolution::VarClassStringResolver<'_>,
    call_site: crate::type_engine::conditional_resolution::ConditionalClassContext<'_>,
    owner: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    template_subs: &HashMap<String, PhpType>,
    arg_type_resolver: crate::type_engine::conditional_resolution::ArgTypeResolver<'_>,
    replace_self: impl Fn(&PhpType) -> PhpType,
) -> Option<PhpType> {
    let method = method_ref?;
    let cond = method.conditional_return()?;
    let bound_cond = crate::type_engine::conditional_resolution::bind_callable_condition_templates(
        cond,
        template_subs,
    );
    let cond = bound_cond.as_ref().unwrap_or(cond);
    let params = method.parameters.as_slice();
    let declaring_fqn = owner.fqn();
    let class_ctx = crate::type_engine::conditional_resolution::ConditionalClassContext {
        declaring: Some(declaring_fqn.as_str()),
        ..call_site
    };
    let class_values =
        crate::inheritance::class_scoped_template_values(template_subs, method.template_params());
    let this_type = crate::type_engine::conditional_resolution::receiver_type_for_condition(
        declaring_fqn.as_str(),
        &owner.template_params,
        template_subs,
    );
    let tpl = crate::type_engine::conditional_resolution::TemplateContext {
        defaults: Some(class_values.as_ref()),
        params: method.template_params(),
        bindings: method.template_bindings(),
        arg_type_resolver,
        this_type: Some(&this_type),
    };
    let resolved =
        crate::type_engine::conditional_resolution::resolve_conditional_with_text_args_and_defaults(
            cond,
            params,
            text_args,
            var_resolver,
            class_ctx,
            class_loader,
            &tpl,
        )?;
    let substituted = if template_subs.is_empty() {
        resolved
    } else {
        resolved.substitute(template_subs)
    };
    let substituted = if substituted.contains_self_ref() {
        replace_self(&substituted)
    } else {
        substituted
    };
    // Collapse any conditionals nested inside the winning branch.
    let collapsed = if substituted.contains_conditional() {
        let tpl2 = crate::type_engine::conditional_resolution::TemplateContext {
            defaults: Some(class_values.as_ref()),
            params: method.template_params(),
            bindings: method.template_bindings(),
            arg_type_resolver,
            this_type: Some(&this_type),
        };
        crate::type_engine::conditional_resolution::evaluate_nested_conditionals_text(
            &substituted,
            params,
            text_args,
            var_resolver,
            class_ctx,
            class_loader,
            &tpl2,
        )
    } else {
        substituted
    };
    if collapsed.is_void() {
        None
    } else {
        Some(collapsed)
    }
}

/// The types the call site's arguments resolve to, indexed by the
/// parameter each one binds to.
///
/// A parameter no argument supplies, or whose argument resolves to
/// nothing, reads back as `mixed`: uninformative, so the body falls back
/// to what its own signature says about it.
///
/// Resolving an argument costs as much as resolving any other
/// expression, and the great majority of calls never need this, so the
/// result is cached in `cell` for the several fallbacks that may each ask
/// for it while resolving one call.
fn resolve_call_site_arg_types<'c>(
    method: Option<&MethodInfo>,
    argument_list: Option<&ArgumentList<'_>>,
    ctx: &VarResolutionCtx<'_>,
    cell: &'c std::cell::OnceCell<Vec<PhpType>>,
) -> &'c [PhpType] {
    cell.get_or_init(|| {
        let (Some(method), Some(argument_list)) = (method, argument_list) else {
            return Vec::new();
        };
        if argument_list.arguments.is_empty() || method.parameters.is_empty() {
            return Vec::new();
        }
        let types: Vec<PhpType> =
            crate::call_args::bind_args_to_params(&method.parameters, argument_list)
                .into_iter()
                .map(|bound| {
                    let Some(expr) = bound else {
                        return PhpType::mixed();
                    };
                    let resolved = resolve_rhs_expression(expr, ctx);
                    if resolved.is_empty() {
                        PhpType::mixed()
                    } else {
                        ResolvedType::types_joined(&resolved)
                    }
                })
                .collect();
        // A call that decided nothing asks the same question as no call
        // site at all, so answer it under the same memo key rather than
        // walking the body again for every such site.
        if types.iter().any(PhpType::is_informative) {
            types
        } else {
            Vec::new()
        }
    })
}

/// A declared return type replaced by the one the arguments decide, when
/// they decide something strictly more specific.
///
/// `getProperty(\ReflectionClass $r, string $name): \ReflectionProperty`
/// really returns a `ReflectionProperty<Configuration, 'shell'>` once the
/// call site has fixed both arguments, but the declaration erases that
/// and every caller further along the chain loses it. Reading the body
/// recovers it, and the result is only accepted when it is the declared
/// class with type arguments added — a strict subtype, so it can narrow
/// the declaration but never contradict it.
///
/// Reading a body where a return type is already declared is the
/// expensive case, so this only runs while an outer body-return
/// inference is already walking a body: that is the only place a
/// refinement still has a caller to reach.
fn refine_declared_return(
    declared: PhpType,
    method: Option<&MethodInfo>,
    owner: &ClassInfo,
    ctx: &VarResolutionCtx<'_>,
    call_args: &dyn Fn() -> Vec<PhpType>,
) -> PhpType {
    if !crate::type_engine::call_resolution::body_inference_in_progress() {
        return declared;
    }
    let (Some(method), Some(backend)) = (method, ctx.backend) else {
        return declared;
    };
    // Only a bare class name can gain type arguments, and only a real
    // method has a body to read them out of.
    if method.name_offset == 0
        || method.is_virtual
        || method.parameters.is_empty()
        || !matches!(declared.kind(), TypeKind::Named(_))
    {
        return declared;
    }
    let Some(declared_name) = declared.base_name() else {
        return declared;
    };
    // Nothing the call decided means nothing the declaration doesn't
    // already say, so there is no refinement to go looking for.
    let args = call_args();
    if args.is_empty() {
        return declared;
    }
    let Some(inferred) = crate::type_engine::call_resolution::try_infer_body_return_type(
        backend,
        &owner.fqn(),
        method,
        &args,
    ) else {
        return declared;
    };
    let refines = matches!(inferred.kind(), TypeKind::Generic(g) if !g.args.is_empty())
        && inferred
            .base_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(declared_name));
    if refines { inferred } else { declared }
}

/// Resolve a method call's return type against a single, fully determined
/// owner class: template substitution, `@psalm-if-this-is` narrowing (via
/// the caller-supplied `template_subs`), PHPStan conditional return types,
/// and body-return-type inference, in that order.
///
/// Shared by instance method calls (called once per union-receiver branch)
/// and static method calls (a single owner, no receiver-derived generics).
/// `self_replace` maps `static`/`self`/`$this` in a resolved return type to
/// the owner's concrete type: generic-aware (via
/// [`receiver_type_for_owner`](super::method_calls::receiver_type_for_owner))
/// for instance calls, a plain FQN swap for static calls, which have no
/// receiver expression to carry generics.
///
/// `argument_list` is `None` for a first-class callable (`$obj->m(...)`),
/// whose return type is the method's own, decided by no arguments.
pub(super) fn resolve_owner_method_call(
    owner: &ClassInfo,
    method_name: &str,
    argument_list: Option<&ArgumentList<'_>>,
    ctx: &VarResolutionCtx<'_>,
    is_static: bool,
    template_subs: &HashMap<String, PhpType>,
    self_replace: &dyn Fn(&PhpType) -> PhpType,
) -> Vec<ResolvedType> {
    let current_class_fqn = ctx.current_class.fqn();
    let current_class_name: &str = &current_class_fqn;
    let arg_texts = argument_list
        .map(|list| {
            crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast(
                list,
                ctx.content,
            )
        })
        .unwrap_or_default();
    let text_args = arg_texts.join(", ");
    let rctx = ctx.as_resolution_ctx();
    let var_resolver = build_var_resolver_from_ctx(ctx);
    let written = |name: &str| ctx.written_names().resolve_class(name);

    // Try the owner directly first — it may already be fully resolved with
    // generic substitutions applied.  The cache is keyed by bare FQN and
    // returns the un-substituted base class, so prefer the owner's own
    // method to preserve template substitutions.
    let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
        owner,
        ctx.class_loader,
        ctx.resolved_class_cache,
    );
    if let Some((date_class, date_return_type)) =
        Backend::configured_laravel_date_return(&merged, method_name, ctx.class_loader)
    {
        return ResolvedType::from_classes_with_hint(vec![date_class], date_return_type);
    }

    let owner_method = owner.get_method_ci(method_name);
    let merged_method = merged.get_method_ci(method_name);
    // Prefer the merged method's return type when the owner's method has no
    // docblock override (return_type == native_return_type).  The merged
    // method carries inherited types from interfaces/parents with template
    // substitutions already applied (e.g. `V|null` → `User|null` from
    // `@implements Collection<string, User>`).  The effective type is
    // stored name-resolved while the native hint keeps the spelling from
    // the source, so the two are compared as types, not verbatim.
    let echoes_native = |m: &MethodInfo| match (&m.return_type, &m.native_return_type) {
        (Some(ret), Some(native)) => ret.equivalent(native),
        (ret, native) => ret == native,
    };
    // A conditional return type inherited the same way (an override with
    // no docblock of its own) likewise decides the call.
    let method_ref = match (owner_method, merged_method) {
        (Some(om), Some(mm))
            if (echoes_native(om) && !echoes_native(mm))
                || (om.conditional_return().is_none() && mm.conditional_return().is_some()) =>
        {
            Some(mm)
        }
        (Some(om), _) => Some(om),
        (None, Some(mm)) => Some(mm),
        // Method not found — fall back to the magic method's return type.
        (None, None) => merged.get_method_ci(if is_static { "__callStatic" } else { "__call" }),
    };
    // Recover the effective return type string from the method and replace
    // `static`/`self`/`$this` with the owner's concrete type so that e.g.
    // `static[]` becomes `Country[]`.
    let native_ret_type_string = method_ref.and_then(|m| m.return_type.as_ref()).map(|ret| {
        let substituted = if !template_subs.is_empty() {
            ret.substitute(template_subs).simplified()
        } else {
            ret.clone()
        };
        // Resolve `parent` to the concrete parent class name before any
        // self/static replacement so that downstream consumers see a real
        // FQN instead of the keyword.
        let substituted = if substituted.is_parent_ref() {
            owner
                .parent_class
                .as_ref()
                .map(|p| PhpType::named(atom(p.as_ref())))
                .unwrap_or(substituted)
        } else {
            substituted
        };
        let resolved = if substituted.contains_self_ref() {
            self_replace(&substituted)
        } else {
            substituted
        };
        // Eloquent's own `get()`/`all()`/relation accessors are annotated
        // with the base `Collection<int, TModel>`; once TModel is concrete
        // we know which collection subclass the model really builds.
        crate::virtual_members::laravel::replace_eloquent_collections_in_type(
            &resolved,
            ctx.class_loader,
        )
        .unwrap_or(resolved)
    });

    // Resolver from an argument's source text to its type, used to evaluate
    // `is <Type>` conditions whose argument is an expression (a method-call
    // chain, property access, …) rather than a literal.
    let arg_ty_resolver = |t: &str| Backend::resolve_arg_text_to_type(t, &rctx);

    // Resolve the PHPStan conditional return type against the call-site
    // arguments, if the method declares one.  When it yields an informative
    // type it is *authoritative*: the branch it selects (e.g.
    // `list<\stdClass>` from `PDOStatement::fetchAll`, or `array<TKey,
    // static>` for a literal-array argument) supersedes the method's broad
    // native union return type.  Resolving classes from the native union
    // instead would both ignore the call-site narrowing and silently drop
    // scalar or `array` members the union carries.
    let conditional_ret = resolve_conditional_return_for_call(
        method_ref,
        &text_args,
        Some(&var_resolver),
        crate::type_engine::conditional_resolution::ConditionalClassContext {
            calling: Some(current_class_name),
            declaring: None,
            written_class: Some(&written),
        },
        owner,
        ctx.class_loader,
        template_subs,
        Some(&arg_ty_resolver),
        self_replace,
    );

    // Collapse any conditionals nested inside the (template-substituted)
    // native return type against the call arguments, so a generic wrapper
    // like `Collection<($groupBy is array|string ? array-key : …), …>`
    // yields a concrete key type instead of carrying a raw conditional that
    // later gets compared against — and printed in — an argument-type
    // diagnostic.
    let native_ret_type_string = native_ret_type_string.map(|ty| {
        if ty.contains_conditional() {
            let params = method_ref.map(|m| m.parameters.as_slice()).unwrap_or(&[]);
            let tpl = crate::type_engine::conditional_resolution::TemplateContext {
                defaults: Some(template_subs),
                params: method_ref.map(|m| m.template_params()).unwrap_or(&[]),
                bindings: method_ref.map(|m| m.template_bindings()).unwrap_or(&[]),
                arg_type_resolver: Some(&arg_ty_resolver),
                this_type: None,
            };
            crate::type_engine::conditional_resolution::evaluate_nested_conditionals_text(
                &ty,
                params,
                &text_args,
                Some(&var_resolver),
                crate::type_engine::conditional_resolution::ConditionalClassContext {
                    calling: Some(current_class_name),
                    declaring: Some(owner.fqn().as_str()),
                    written_class: Some(&written),
                },
                ctx.class_loader,
                &tpl,
            )
        } else {
            ty
        }
    });

    // When the conditional resolved to a definite, informative type, it
    // wins — resolve the result classes from it directly.
    if let Some(cond_ty) = conditional_ret {
        return resolve_return_type(cond_ty, ctx);
    }

    // The argument types, resolved at most once however many of the
    // fallbacks below end up asking for them.
    let resolved_args = std::cell::OnceCell::new();
    let call_args =
        || resolve_call_site_arg_types(method_ref, argument_list, ctx, &resolved_args).to_vec();

    let ret_type_string = native_ret_type_string
        .map(|d| refine_declared_return(d, method_ref, owner, ctx, &call_args));

    let mr_ctx = MethodReturnCtx {
        all_classes: ctx.all_classes,
        class_loader: ctx.class_loader,
        backend: ctx.backend,
        template_subs,
        var_resolver: Some(&var_resolver),
        cache: ctx.resolved_class_cache,
        calling_class_name: Some(ctx.current_class.fqn().as_str()),
        written_class: Some(&written),
        is_static,
        call_args: Some(&call_args),
    };

    let results =
        Backend::resolve_method_return_types_with_args(owner, method_name, &text_args, &mr_ctx);
    if !results.is_empty() {
        // A `mixed` hint carries no information. `results` being non-empty
        // despite it means `resolve_method_return_types_with_args` reached
        // its own body-inference fallback and resolved real classes —
        // attaching the stale `mixed` hint on top would make the resolved
        // type display as `mixed` instead of the inferred class.
        let hint = ret_type_string.filter(|t| !t.is_mixed());
        return match hint {
            Some(hint) => ResolvedType::from_classes_with_hint(results, hint),
            None => ResolvedType::from_classes(results),
        };
    }

    // Body return type inference fallback: when the method has no declared
    // return type, or its only declared type is `mixed` (native or
    // docblock — carries no information, so reading the body can only
    // narrow it, never contradict it), try to infer the return type from
    // the method body.  This handles non-class types (list<Foo>, int,
    // array shapes) that resolve_method_return_types_with_args cannot
    // represent.  Tried before the type-string-only fallback below so
    // that a `mixed` hint doesn't win by default when inference has
    // something better to offer.
    if method_ref.is_some_and(|m| {
        m.return_type.as_ref().is_none_or(|t| t.is_mixed()) && m.name_offset != 0 && !m.is_virtual
    }) && let Some(backend) = ctx.backend
        && let Some(inferred) = crate::type_engine::call_resolution::try_infer_body_return_type(
            backend,
            &owner.fqn(),
            method_ref.unwrap(),
            &call_args(),
        )
        && !inferred.is_void()
        && !inferred.is_mixed()
    {
        return vec![resolved_type_with_lookup(
            inferred,
            ctx.all_classes,
            ctx.class_loader,
        )];
    }

    // The method has a return type string but `type_hint_to_classes_typed`
    // found no matching class (e.g. `list<Widget>`, `int`, `array{name:
    // string}`), or inference above produced nothing useful.  Return a
    // type-string-only entry so that consumers reading `.type_string`
    // (hover, foreach resolution, null-coalesce stripping) still get the
    // information.
    //
    // Return the type string even for non-informative types like `array` or
    // `mixed` — a correct-but-vague type is better than keeping the
    // previous (wrong) type after reassignment.  Also expand type aliases
    // before returning so that `@phpstan-type UserList array<int, User>`
    // with `@return UserList` is expanded to its concrete type.
    if let Some(hint) = ret_type_string {
        let expanded = crate::type_engine::types::aliases::resolve_type_alias_typed(
            &hint,
            &owner.fqn(),
            ctx.all_classes,
            ctx.class_loader,
        );
        let parsed_effective = expanded.unwrap_or(hint);
        return vec![resolved_type_with_lookup(
            parsed_effective,
            ctx.all_classes,
            ctx.class_loader,
        )];
    }

    vec![]
}
