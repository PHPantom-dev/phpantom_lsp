use super::*;
use std::collections::HashMap;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::argument::Argument;
use mago_syntax::cst::sequence::TokenSeparatedSequence;

use crate::atom::{atom, bytes_to_str};
use crate::php_type::PhpType;
use crate::types::{AccessKind, ClassInfo, ResolvedType};

// ─── Callable parameter inference for the forward walker ────────────────────
//
// These functions mirror the inference logic in `closure_resolution.rs`
// but operate with a `ForwardWalkCtx` + `ScopeState` instead of a
// `VarResolutionCtx`.  They build a temporary `VarResolutionCtx` with
// a scope-based variable resolver injected so that variable lookups
// during receiver resolution read from the forward walker's scope.

/// Infer callable parameter types for a closure passed at position
/// `arg_idx` to a standalone function call.
pub(crate) fn infer_callable_params_from_function_fw(
    func_name: &str,
    arg_idx: usize,
    arguments: &TokenSeparatedSequence<'_, Argument<'_>>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<PhpType> {
    let scope_locals = &scope.locals;
    let scope_resolver = |var_name: &str| -> Vec<ResolvedType> {
        scope_locals
            .get(&atom(var_name))
            .cloned()
            .unwrap_or_default()
    };
    let var_ctx = ctx.var_ctx_for_with_scope("$__infer", ctx.cursor_offset, &scope_resolver);
    let rctx = var_ctx.as_resolution_ctx();
    let func_info = if let Some(fl) = rctx.function_loader {
        fl(func_name)
    } else {
        None
    };
    if let Some(fi) = func_info {
        let mut params = extract_callable_params_at_fw(&fi.parameters, arg_idx);

        if !params.is_empty() && !fi.template_params.is_empty() && !fi.template_bindings.is_empty()
        {
            let arg_texts = extract_argument_texts_fw(arguments, ctx.content);
            let subs =
                super::super::rhs_resolution::build_function_template_subs(&fi, &arg_texts, &rctx);
            if !subs.is_empty() {
                params = params.into_iter().map(|p| p.substitute(&subs)).collect();
            }
        }

        params
    } else {
        vec![]
    }
}

/// Infer callable parameter types for a closure passed at position
/// `arg_idx` to an instance method call.
pub(crate) fn infer_callable_params_from_receiver_fw(
    obj_start: u32,
    obj_end: u32,
    method_name: &str,
    arg_idx: usize,
    first_arg_text: Option<&str>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<PhpType> {
    let start = obj_start as usize;
    let end = obj_end as usize;
    if end > ctx.content.len() {
        return vec![];
    }
    let obj_text = ctx.content[start..end].trim();
    let scope_locals = &scope.locals;
    let scope_resolver = |var_name: &str| -> Vec<ResolvedType> {
        scope_locals
            .get(&atom(var_name))
            .cloned()
            .unwrap_or_default()
    };
    let var_ctx = ctx.var_ctx_for_with_scope("$__infer", obj_start, &scope_resolver);
    let rctx = var_ctx.as_resolution_ctx();
    // Keep the raw ResolvedTypes so we can extract generic args from
    // the receiver's type_string (e.g. `Builder<Product>` carries the
    // concrete `Product` arg that must substitute `TModel`).
    let resolved_types =
        crate::completion::resolver::resolve_target_classes(obj_text, AccessKind::Arrow, &rctx);
    let receiver_classes = ResolvedType::into_arced_classes(resolved_types.clone());

    // For relation-query methods (whereHas, etc.), override the closure
    // parameter type with Builder<RelatedModel>.
    if let Some(override_params) = super::super::closure_resolution::try_relation_query_override_pub(
        &receiver_classes,
        method_name,
        first_arg_text,
        ctx.class_loader,
    ) {
        return override_params;
    }

    let params = find_callable_params_on_classes_fw(&receiver_classes, method_name, arg_idx, ctx);

    // Build a template substitution map from the receiver's generic
    // args.  When the receiver resolves to e.g. `Builder<Product>`,
    // the type_string is `Generic("Builder", [Named("Product")])`.
    // We extract those args, pair them with the class's @template
    // params (e.g. `TModel`), and substitute so that callable params
    // like `Closure(Builder<TModel>)` become `Closure(Builder<Product>)`.
    let template_subs = build_receiver_template_subs(&resolved_types, &receiver_classes, ctx);

    // Apply template substitution, then replace `$this`/`static`
    // tokens with the receiver's full type.
    let params = if !template_subs.is_empty() {
        params
            .into_iter()
            .map(|p| p.substitute(&template_subs))
            .collect()
    } else {
        params
    };

    if let Some(receiver) = receiver_classes.first() {
        let receiver_type = super::super::closure_resolution::build_receiver_self_type_pub(
            receiver,
            ctx.class_loader,
        );
        params
            .into_iter()
            .map(|p| p.replace_self_with_type(&receiver_type))
            .collect()
    } else {
        params
    }
}

/// Filter inferred callable param types, replacing any param whose type
/// has an unresolvable base (e.g. PHPStan pseudo-types like
/// `collection-of<T>`) with `PhpType::mixed()`.  `mixed` is not
/// considered informative by `seed_closure_params`, so the param simply
/// won't be seeded — much better than skipping the entire closure body.
pub(crate) fn filter_resolvable_inferred_params(
    inferred: &[PhpType],
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<PhpType> {
    inferred
        .iter()
        .map(|ty| {
            if has_unresolvable_base(ty, ctx) {
                PhpType::mixed()
            } else {
                ty.clone()
            }
        })
        .collect()
}

/// Check whether a type has a base name that looks class-like but
/// doesn't resolve to any known class in the project or stubs.
pub(crate) fn has_unresolvable_base(ty: &PhpType, ctx: &ForwardWalkCtx<'_>) -> bool {
    match ty {
        PhpType::Named(name) => is_unresolvable_class_name(name, ctx),
        PhpType::Generic(base, args) => {
            is_unresolvable_class_name(base, ctx)
                || args.iter().any(|a| has_unresolvable_base(a, ctx))
        }
        PhpType::Union(parts) | PhpType::Intersection(parts) => {
            parts.iter().any(|p| has_unresolvable_base(p, ctx))
        }
        PhpType::Nullable(inner) => has_unresolvable_base(inner, ctx),
        PhpType::Callable {
            params,
            return_type,
            ..
        } => {
            if let Some(ret) = return_type
                && has_unresolvable_base(ret, ctx)
            {
                return true;
            }
            params
                .iter()
                .any(|p| has_unresolvable_base(&p.type_hint, ctx))
        }
        _ => false,
    }
}

/// A class name is "unresolvable" if it:
/// 1. Contains a hyphen (e.g. `collection-of`, `non-empty-list`) — these
///    are PHPStan pseudo-types that aren't real PHP classes.
/// 2. Is not a scalar/builtin/special type.
/// 3. Doesn't resolve to a class in the project or stubs.
///
/// We only flag hyphenated names because they are guaranteed to not be
/// valid PHP class names.  Non-hyphenated names that fail resolution
/// might just be missing from the index (vendor code, etc.) and
/// shouldn't trigger the guard.
pub(crate) fn is_unresolvable_class_name(name: &str, _ctx: &ForwardWalkCtx<'_>) -> bool {
    // Hyphenated names are never valid PHP class names.  PHPStan uses
    // them for pseudo-types like `collection-of`, `non-empty-list`,
    // `non-empty-array`, `non-empty-string`, `class-string`, etc.
    // `class-string` is handled elsewhere, but the rest are not
    // resolvable as classes.
    if name.contains('-') {
        // Allow well-known pseudo-types that we DO handle elsewhere.
        let lower = name.to_ascii_lowercase();
        if lower == "class-string"
            || lower == "array-key"
            || lower == "non-empty-string"
            || lower == "non-empty-array"
            || lower == "non-empty-list"
            || lower == "non-falsy-string"
            || lower == "numeric-string"
            || lower == "literal-string"
            || lower == "callable-string"
        {
            return false;
        }
        return true;
    }
    false
}

/// Build a template substitution map from the receiver's resolved types.
///
/// When the receiver resolves to a generic type like `Builder<Product>`,
/// this extracts the generic args from the `type_string` and pairs them
/// with the class's `@template` parameters to produce a substitution map
/// (e.g. `{TModel => Product}`).  This enables callable parameter types
/// that reference template params to be fully substituted.
///
/// When the `type_string` is self-like (`static`, `self`, `$this`) —
/// which happens when a method returns `static` on a generic class —
/// the function reconstructs the generic args from the class_info's
/// method return types via `build_receiver_self_type_pub`.  This
/// preserves generic context through method chains like
/// `Model::where(…)->orderBy(…)->each(fn)` where intermediate steps
/// return `static`.
pub(crate) fn build_receiver_template_subs(
    resolved_types: &[ResolvedType],
    receiver_classes: &[Arc<ClassInfo>],
    ctx: &ForwardWalkCtx<'_>,
) -> HashMap<String, PhpType> {
    // Use the first resolved type that has generic args and a matching
    // class with template params.
    for rt in resolved_types {
        let generic_args = match &rt.type_string {
            PhpType::Generic(_, args) if !args.is_empty() => args,
            _ => continue,
        };
        // Find the matching class info (by FQN or short name).
        let base_name = rt.type_string.base_name().unwrap_or_default();
        let class = receiver_classes.iter().find(|c| {
            c.fqn() == base_name
                || c.name == base_name
                || crate::util::short_name(&c.fqn()) == crate::util::short_name(base_name)
        });
        if let Some(cls) = class
            && !cls.template_params.is_empty()
        {
            return crate::inheritance::build_generic_subs(cls, generic_args);
        }
    }

    // Fallback: when the type_string is self-like (e.g. `Named("static")`)
    // but the class has template params, reconstruct the generic args from
    // the class_info's method return types.  This handles method chains
    // where `static` returns lose generic context in the type_string.
    for rt in resolved_types {
        if !rt.type_string.is_self_like() {
            continue;
        }
        let cls = match &rt.class_info {
            Some(c) if !c.template_params.is_empty() => c,
            _ => continue,
        };
        let reconstructed =
            super::super::closure_resolution::build_receiver_self_type_pub(cls, ctx.class_loader);
        if let PhpType::Generic(_, ref args) = reconstructed
            && !args.is_empty()
        {
            return crate::inheritance::build_generic_subs(cls, args);
        }
    }

    HashMap::new()
}

/// Infer callable parameter types for a closure passed at position
/// `arg_idx` to a static method call.
pub(crate) fn infer_callable_params_from_static_receiver_fw(
    class_expr: &Expression<'_>,
    method_name: &str,
    arg_idx: usize,
    first_arg_text: Option<&str>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<PhpType> {
    let _ = scope; // scope not needed for static receiver resolution

    let class_name = match class_expr {
        Expression::Self_(_) => Some(ctx.current_class.name.to_string()),
        Expression::Static(_) => Some(ctx.current_class.name.to_string()),
        Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
        Expression::Parent(_) => ctx.current_class.parent_class.map(|a| a.to_string()),
        _ => None,
    };
    let owner = class_name.and_then(|name| {
        ctx.all_classes
            .iter()
            .find(|c| c.name == name)
            .map(|c| ClassInfo::clone(c))
            .or_else(|| (ctx.class_loader)(&name).map(Arc::unwrap_or_clone))
    });
    if let Some(ref cls) = owner {
        // For relation-query methods, override with Builder<RelatedModel>.
        if let Some(override_params) =
            super::super::closure_resolution::try_relation_query_override_pub(
                &[Arc::new(cls.clone())],
                method_name,
                first_arg_text,
                ctx.class_loader,
            )
        {
            return override_params;
        }

        let resolved = crate::virtual_members::resolve_class_fully_maybe_cached(
            cls,
            ctx.class_loader,
            ctx.resolved_class_cache,
        );
        let params = find_callable_params_on_method_fw(&resolved, method_name, arg_idx);

        // Build a template substitution map from the owner class.
        // When the owner is a generic class (e.g. `Builder<Customer>`
        // via `@extends`), reconstruct its full generic type and pair
        // the args with the class's @template params so that callable
        // params like `Closure(Collection<int, TModel>)` become
        // `Closure(Collection<int, Customer>)`.
        let receiver_type =
            super::super::closure_resolution::build_receiver_self_type_pub(cls, ctx.class_loader);
        let template_subs = if let PhpType::Generic(_, ref args) = receiver_type
            && !args.is_empty()
            && !cls.template_params.is_empty()
        {
            crate::inheritance::build_generic_subs(cls, args)
        } else {
            HashMap::new()
        };

        let params = if !template_subs.is_empty() {
            params
                .into_iter()
                .map(|p| p.substitute(&template_subs))
                .collect()
        } else {
            params
        };

        params
            .into_iter()
            .map(|p| p.replace_self_with_type(&receiver_type))
            .collect()
    } else {
        vec![]
    }
}

/// Search for method `method_name` on each of `classes` and extract
/// callable parameter types at `arg_idx`.
///
/// Tries the input class first — when the receiver came from a generic
/// instantiation (e.g. `Stream<int, Product>`), its `class_info` already
/// has template substitutions applied by `type_hint_to_classes_typed` →
/// `resolve_class_fully_with_generics`.  Extracting callable params from
/// that class preserves the concrete types (e.g. `callable(Product)`
/// instead of `callable(TVal)`).
///
/// Falls back to `resolve_class_fully_maybe_cached` only when the method
/// isn't found on the input class — this handles methods that come
/// exclusively from virtual member providers or late-merged traits.
pub(crate) fn find_callable_params_on_classes_fw(
    classes: &[Arc<ClassInfo>],
    method_name: &str,
    arg_idx: usize,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<PhpType> {
    for cls in classes {
        // First: try the class as-is.  When it came from a generic
        // instantiation, template params are already substituted in
        // all method signatures.  Re-resolving via
        // `resolve_class_fully_maybe_cached` would load the base
        // class definition (keyed by FQN with empty generic args),
        // discarding those substitutions.
        let result = find_callable_params_on_method_fw(cls, method_name, arg_idx);
        if !result.is_empty() {
            return result;
        }

        // Fallback: the method wasn't found on the input class.
        // This can happen when the method comes from a virtual member
        // provider, a late-merged trait, or a mixin that wasn't
        // included in the original resolution.  Re-resolve fully
        // and try again.
        let resolved = crate::virtual_members::resolve_class_fully_maybe_cached(
            cls,
            ctx.class_loader,
            ctx.resolved_class_cache,
        );
        let result = find_callable_params_on_method_fw(&resolved, method_name, arg_idx);
        if !result.is_empty() {
            return result;
        }
    }
    vec![]
}

/// Look up method `method_name` on `class` and extract callable
/// parameter types from the parameter at position `arg_idx`.
pub(crate) fn find_callable_params_on_method_fw(
    class: &ClassInfo,
    method_name: &str,
    arg_idx: usize,
) -> Vec<PhpType> {
    let method = class.get_method(method_name);
    if let Some(m) = method {
        extract_callable_params_at_fw(&m.parameters, arg_idx)
    } else {
        vec![]
    }
}

/// Given a list of parameters, look at `arg_idx` and extract callable
/// parameter types if the type hint is `callable(...)` or `Closure(...)`.
pub(crate) fn extract_callable_params_at_fw(
    params: &[crate::types::ParameterInfo],
    arg_idx: usize,
) -> Vec<PhpType> {
    let param = params.get(arg_idx);
    if let Some(p) = param
        && let Some(ref hint) = p.type_hint
        && let Some(callable_params) = hint.callable_param_types()
    {
        return callable_params
            .iter()
            .map(|cp| cp.type_hint.clone())
            .collect();
    }
    vec![]
}

/// Extract the text of each argument from a call's argument list.
pub(crate) fn extract_argument_texts_fw(
    arguments: &TokenSeparatedSequence<'_, Argument<'_>>,
    content: &str,
) -> Vec<String> {
    arguments
        .iter()
        .map(|arg| {
            let span = match arg {
                Argument::Positional(pos) => pos.value.span(),
                Argument::Named(named) => named.value.span(),
            };
            let start = span.start.offset as usize;
            let end = span.end.offset as usize;
            if end <= content.len() {
                content[start..end].to_string()
            } else {
                String::new()
            }
        })
        .collect()
}

/// Extract the text of the first positional argument, stripping quotes.
pub(crate) fn extract_first_arg_string_fw(
    arguments: &TokenSeparatedSequence<'_, Argument<'_>>,
    content: &str,
) -> Option<String> {
    let first = arguments.iter().next()?;
    let expr = match first {
        Argument::Positional(pos) => pos.value,
        Argument::Named(named) => named.value,
    };
    let span = expr.span();
    let start = span.start.offset as usize;
    let end = span.end.offset as usize;
    let raw = content.get(start..end)?.trim();

    if raw.len() >= 2
        && ((raw.starts_with('\'') && raw.ends_with('\''))
            || (raw.starts_with('"') && raw.ends_with('"')))
    {
        Some(raw[1..raw.len() - 1].to_string())
    } else {
        None
    }
}

/// Recursively walk the AST to find function and method bodies, running
/// the forward walker on each.
/// Seed `$this` in the scope when inside a non-static class method.
///
/// This creates a `ResolvedType` from the enclosing `ClassInfo` and
/// stores it under `"$this"`.  The scope-based variable resolver then
/// returns this entry for any `$this` lookup, eliminating the need to
/// remain unresolved.
pub(crate) fn seed_this(scope: &mut ScopeState, current_class: &ClassInfo) {
    if current_class.name.is_empty() {
        return;
    }
    scope.set(
        "$this",
        vec![ResolvedType::from_class(current_class.clone())],
    );
}

/// Infer callable parameter types for a specific argument index of a
/// call expression.  This reuses the same inference functions as the
/// diagnostic path (`infer_callable_params_from_function_fw`,
/// `infer_callable_params_from_receiver_fw`,
/// `infer_callable_params_from_static_receiver_fw`) so that closure
/// parameters on the completion/hover path receive the same
/// generic-substituted types.
pub(crate) fn infer_callable_params_for_call(
    call: &Call<'_>,
    arg_idx: usize,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<PhpType> {
    match call {
        Call::Function(fc) => {
            let func_name = match fc.function {
                Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
                _ => None,
            };
            if let Some(ref name) = func_name {
                infer_callable_params_from_function_fw(
                    name,
                    arg_idx,
                    &fc.argument_list.arguments,
                    scope,
                    ctx,
                )
            } else {
                vec![]
            }
        }
        Call::Method(mc) => {
            let method_name = if let ClassLikeMemberSelector::Identifier(ident) = &mc.method {
                Some(bytes_to_str(ident.value).to_string())
            } else {
                None
            };
            if let Some(ref name) = method_name {
                let obj_span = mc.object.span();
                let first_arg =
                    extract_first_arg_string_fw(&mc.argument_list.arguments, ctx.content);
                infer_callable_params_from_receiver_fw(
                    obj_span.start.offset,
                    obj_span.end.offset,
                    name,
                    arg_idx,
                    first_arg.as_deref(),
                    scope,
                    ctx,
                )
            } else {
                vec![]
            }
        }
        Call::NullSafeMethod(mc) => {
            let method_name = if let ClassLikeMemberSelector::Identifier(ident) = &mc.method {
                Some(bytes_to_str(ident.value).to_string())
            } else {
                None
            };
            if let Some(ref name) = method_name {
                let obj_span = mc.object.span();
                let first_arg =
                    extract_first_arg_string_fw(&mc.argument_list.arguments, ctx.content);
                infer_callable_params_from_receiver_fw(
                    obj_span.start.offset,
                    obj_span.end.offset,
                    name,
                    arg_idx,
                    first_arg.as_deref(),
                    scope,
                    ctx,
                )
            } else {
                vec![]
            }
        }
        Call::StaticMethod(sc) => {
            let method_name = if let ClassLikeMemberSelector::Identifier(ident) = &sc.method {
                Some(bytes_to_str(ident.value).to_string())
            } else {
                None
            };
            if let Some(ref name) = method_name {
                let first_arg =
                    extract_first_arg_string_fw(&sc.argument_list.arguments, ctx.content);
                infer_callable_params_from_static_receiver_fw(
                    sc.class,
                    name,
                    arg_idx,
                    first_arg.as_deref(),
                    scope,
                    ctx,
                )
            } else {
                vec![]
            }
        }
    }
}
