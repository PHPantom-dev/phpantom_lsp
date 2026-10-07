//! Context-sensitive Eloquent relation callback inference shared by both walks.

use super::*;
use crate::atom::atom;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, MAX_INHERITANCE_DEPTH, ResolvedType};
use std::sync::Arc;

/// Refine only the closure bound to the relation constraint parameter.
/// Binding against the installed declaration handles reordered named arguments
/// and callbacks after optional parameters without duplicating PHP's call rules.
pub(crate) fn infer_relation_callback_params(
    receivers: &[ResolvedType],
    method_name: &str,
    arg_idx: usize,
    argument_list: &ArgumentList<'_>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<Vec<PhpType>> {
    let method_name = *crate::virtual_members::laravel::RELATION_QUERY_METHODS
        .iter()
        .find(|name| name.eq_ignore_ascii_case(method_name))?;
    let callback_name = if method_name.ends_with("Relation") {
        "column"
    } else {
        "callback"
    };
    let relation_name = if method_name == "with" {
        "relations"
    } else {
        "relation"
    };
    let argument = argument_list.arguments.iter().nth(arg_idx)?.value();
    let mut alternatives = Vec::new();
    let mut refined = false;
    for receiver in receivers {
        let Some(class) = receiver.class_info.as_ref() else {
            continue;
        };
        let method = class.get_method_arc(method_name).or_else(|| {
            crate::virtual_members::resolve_class_fully_maybe_cached(
                class,
                ctx.class_loader,
                ctx.resolved_class_cache,
            )
            .get_method_arc(method_name)
        });
        let Some(method) = method else { continue };
        let declared = extract_callable_params_at_fw(&method.parameters, argument_list, arg_idx);
        if !is_framework_relation_method(class, method_name, ctx) {
            alternatives.push(declared);
            continue;
        }
        let param_index = |name| {
            method
                .parameters
                .iter()
                .position(|p| p.name.trim_start_matches('$') == name)
        };
        let (Some(relation_idx), Some(callback_idx)) =
            (param_index(relation_name), param_index(callback_name))
        else {
            continue;
        };
        let bound = crate::call_args::bind_args_to_params(&method.parameters, argument_list);
        if bound[callback_idx].is_none_or(|callback| callback.span() != argument.span()) {
            continue;
        }
        let relation = bound[relation_idx]?;
        let scope_resolver =
            |name: &str| scope.locals.get(&atom(name)).cloned().unwrap_or_default();
        let var_ctx = ctx.var_ctx_for_with_scope(
            "$__infer",
            relation.span().start.offset,
            &scope_resolver,
            Some(scope.proofs()),
        );
        let relation_type = super::super::resolution::resolve_arg_raw_type(relation, &var_ctx)?;
        let candidates = if method_name.contains("Morph") {
            let types = bound[param_index("types")?]?;
            Some(
                super::super::resolution::resolve_arg_raw_type(types, &var_ctx)
                    .unwrap_or_else(PhpType::mixed),
            )
        } else {
            None
        };
        let Some(mut inferred) = super::super::closure_resolution::try_relation_query_override_pub(
            std::slice::from_ref(receiver),
            method_name,
            &relation_type,
            candidates.as_ref(),
            &var_ctx.as_resolution_ctx(),
        ) else {
            alternatives.push(declared);
            continue;
        };
        // Refine the query parameter without losing Laravel's second morph
        // callback parameter (the candidate's class-string).
        inferred.extend(declared.into_iter().skip(1));
        alternatives.push(inferred);
        refined = true;
    }
    refined.then(|| merge_callback_parameters(alternatives))
}

fn merge_callback_parameters(mut alternatives: Vec<Vec<PhpType>>) -> Vec<PhpType> {
    if alternatives.len() == 1 {
        return alternatives.pop().unwrap_or_default();
    }
    let count = alternatives.iter().map(Vec::len).max().unwrap_or(0);
    (0..count)
        .map(|index| {
            PhpType::union(
                alternatives
                    .iter()
                    .filter_map(|params| params.get(index).cloned())
                    .collect(),
            )
            .simplified()
        })
        .collect()
}

fn is_framework_relation_method(class: &ClassInfo, method: &str, ctx: &ForwardWalkCtx<'_>) -> bool {
    let Some(raw) = (ctx.class_loader)(&class.fqn()) else {
        return false;
    };
    if let Some(framework) = declared_relation_method(&raw, method, ctx.class_loader, 0) {
        return framework;
    }
    if !crate::virtual_members::laravel::extends_eloquent_model(&raw, ctx.class_loader) {
        return false;
    }
    let builder = crate::virtual_members::laravel::model_builder_type(&raw, ctx.class_loader);
    builder
        .base_name()
        .and_then(ctx.class_loader)
        .is_some_and(|builder| {
            declared_relation_method(&builder, method, ctx.class_loader, 0) == Some(true)
        })
}

// Only framework declarations carry Eloquent's relation-argument semantics.
// Raw members and trait aliases take precedence over forwarded builder methods.
fn declared_relation_method(
    class: &ClassInfo,
    method: &str,
    loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    depth: u32,
) -> Option<bool> {
    if depth >= MAX_INHERITANCE_DEPTH {
        return Some(false);
    }
    if class.get_method(method).is_some()
        || class.doc_members.as_ref().is_some_and(|doc| {
            doc.methods
                .iter()
                .any(|m| m.name.eq_ignore_ascii_case(method))
        })
        || class.trait_aliases.iter().any(|alias| {
            alias
                .alias
                .is_some_and(|name| name.eq_ignore_ascii_case(method))
        })
    {
        return Some(
            matches!(
                class.fqn().as_str(),
                "Illuminate\\Database\\Eloquent\\Builder"
                    | "Illuminate\\Database\\Eloquent\\Concerns\\QueriesRelationships"
            ) || (method == "with" && class.fqn() == "Illuminate\\Database\\Eloquent\\Model"),
        );
    }
    for name in &class.used_traits {
        if class.trait_precedences.iter().any(|precedence| {
            precedence.method_name.eq_ignore_ascii_case(method)
                && precedence.insteadof.contains(name)
        }) {
            continue;
        }
        if let Some(class) = loader(name)
            && let Some(result) = declared_relation_method(&class, method, loader, depth + 1)
        {
            return Some(result);
        }
    }
    class
        .parent_class
        .as_ref()
        .and_then(|name| loader(name))
        .and_then(|parent| declared_relation_method(&parent, method, loader, depth + 1))
}

/// An expression inside an eager-loading array and its contextual parameters.
/// Non-callback expressions are retained so both walkers still visit them.
pub(crate) struct EagerCallbackArgument<'a> {
    /// The array key, value, or closure to visit.
    pub expression: &'a Expression<'a>,
    /// Parameters inferred for a direct constraint closure, if any.
    pub parameters: Vec<PhpType>,
}

/// Resolve eager-array callbacks once for both cursor and diagnostic walks.
/// Keys describe relation paths; nested arrays extend the parent key's path.
pub(crate) fn eager_callback_arguments<'a>(
    call: &Call<'a>,
    arg_idx: usize,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<Vec<EagerCallbackArgument<'a>>> {
    let (selector, arguments, receiver, is_static) = match call {
        Call::Method(call) => (&call.method, &call.argument_list, call.object, false),
        Call::NullSafeMethod(call) => (&call.method, &call.argument_list, call.object, false),
        Call::StaticMethod(call) => (&call.method, &call.argument_list, call.class, true),
        _ => return None,
    };
    let ClassLikeMemberSelector::Identifier(name) = selector else {
        return None;
    };
    if !crate::atom::bytes_to_str(name.value).eq_ignore_ascii_case("with") {
        return None;
    }
    let argument = arguments.arguments.iter().nth(arg_idx)?.value();
    let mut array = argument;
    while let Expression::Parenthesized(inner) = array {
        array = inner.expression;
    }
    crate::parser::array_literal_elements(array)?;
    if !contains_eager_callback(argument) {
        return None;
    }
    let scope_resolver = |name: &str| scope.locals.get(&atom(name)).cloned().unwrap_or_default();
    let var_ctx = ctx.var_ctx_for_with_scope(
        "$__infer",
        argument.span().start.offset,
        &scope_resolver,
        Some(scope.proofs()),
    );
    let rctx = var_ctx.as_resolution_ctx();
    let receivers = if is_static {
        let name = super::super::closure_resolution::static_receiver_class_name(
            receiver,
            Some(ctx.current_class),
        )?;
        let owner = super::super::closure_resolution::find_owner_by_name(
            &name,
            ctx.all_classes,
            ctx.class_loader,
        )?;
        vec![ResolvedType::from_class(owner)]
    } else {
        let span = receiver.span();
        super::super::closure_resolution::resolve_receiver_types(
            span.start.offset,
            span.end.offset,
            &rctx,
        )
    };
    let receivers: Vec<_> = receivers
        .into_iter()
        .filter(|receiver| {
            let Some(class) = receiver.class_info.as_ref() else {
                return false;
            };
            if !is_framework_relation_method(class, "with", ctx) {
                return false;
            }
            let Some(method) = class.get_method_arc("with").or_else(|| {
                crate::virtual_members::resolve_class_fully_maybe_cached(
                    class,
                    ctx.class_loader,
                    ctx.resolved_class_cache,
                )
                .get_method_arc("with")
            }) else {
                return false;
            };
            let Some(index) = method
                .parameters
                .iter()
                .position(|p| p.name.trim_start_matches('$') == "relations")
            else {
                return false;
            };
            crate::call_args::bind_args_to_params(&method.parameters, arguments)[index]
                .is_some_and(|bound| bound.span() == argument.span())
        })
        .collect();
    if receivers.is_empty() {
        return None;
    }
    let mut entries = Vec::new();
    collect_eager_arguments(argument, None, &receivers, &var_ctx, &mut entries);
    Some(entries)
}

// Most eager-load lists contain only names. Avoid resolving their receiver
// or constructing contextual entries when there is no callback to type.
fn contains_eager_callback(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::Closure(_) | Expression::ArrowFunction(_) => true,
        Expression::Parenthesized(inner) => contains_eager_callback(inner.expression),
        _ => crate::parser::array_literal_elements(expression).is_some_and(|elements| {
            elements
                .iter()
                .filter_map(crate::parser::array_element_value)
                .any(contains_eager_callback)
        }),
    }
}

fn collect_eager_arguments<'a>(
    expression: &'a Expression<'a>,
    prefix: Option<&PhpType>,
    receivers: &[ResolvedType],
    ctx: &crate::type_engine::resolver::VarResolutionCtx<'_>,
    entries: &mut Vec<EagerCallbackArgument<'a>>,
) {
    if let Expression::Parenthesized(inner) = expression {
        collect_eager_arguments(inner.expression, prefix, receivers, ctx, entries);
        return;
    }
    if let Some(elements) = crate::parser::array_literal_elements(expression) {
        for element in elements.iter() {
            let key = crate::parser::array_element_key(element);
            let path = key
                .and_then(|key| super::super::resolution::resolve_arg_raw_type(key, ctx))
                .map(|key| join_relation_path(prefix, &key));
            if let Some(key) = key {
                entries.push(EagerCallbackArgument {
                    expression: key,
                    parameters: Vec::new(),
                });
            }
            if let Some(value) = crate::parser::array_element_value(element) {
                collect_eager_arguments(value, path.as_ref(), receivers, ctx, entries);
            }
        }
        return;
    }
    let parameters = if matches!(
        expression,
        Expression::Closure(_) | Expression::ArrowFunction(_)
    ) {
        prefix
            .map(|path| {
                super::super::closure_resolution::try_relation_query_override_pub(
                    receivers,
                    "with",
                    path,
                    None,
                    &ctx.as_resolution_ctx(),
                )
                .unwrap_or_else(|| {
                    vec![PhpType::generic(
                        "Illuminate\\Database\\Eloquent\\Relations\\Relation",
                        vec![PhpType::mixed(); 3],
                    )]
                })
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    entries.push(EagerCallbackArgument {
        expression,
        parameters,
    });
}

fn join_relation_path(prefix: Option<&PhpType>, key: &PhpType) -> PhpType {
    let Some(prefix) = prefix else {
        return key.clone();
    };
    if let crate::php_type::TypeKind::Union(parts) = prefix.kind() {
        return PhpType::union(
            parts
                .iter()
                .map(|part| join_relation_path(Some(part), key))
                .collect(),
        )
        .simplified();
    }
    if let crate::php_type::TypeKind::Union(parts) = key.kind() {
        return PhpType::union(
            parts
                .iter()
                .map(|part| join_relation_path(Some(prefix), part))
                .collect(),
        )
        .simplified();
    }
    let parent = prefix.as_literal().and_then(|value| value.string_content());
    let child = key.as_literal().and_then(|value| value.string_content());
    match (parent, child) {
        (Some(parent), Some(child)) => PhpType::literal_string_value(format!("{parent}.{child}")),
        _ => PhpType::named(atom("string")),
    }
}

#[cfg(test)]
#[path = "relation_callbacks_tests.rs"]
mod tests;
