//! Context-sensitive Eloquent relation callback inference shared by both walks:
//! the query a relation constraint callback receives, read off the relation
//! the call names.

use std::sync::Arc;

use crate::atom::atom;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::resolver::ResolutionCtx;
use crate::types::{ClassInfo, MAX_INHERITANCE_DEPTH, ResolvedType};
use crate::virtual_members::laravel::{
    ELOQUENT_BUILDER_FQN, ELOQUENT_MODEL_FQN, RELATION_QUERY_METHODS, extends_eloquent_model,
    model_builder_type, related_model_type, resolve_relation_chain_details,
};

use super::super::closure_resolution::extract_model_from_builder;
use super::*;

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
    let method_name = crate::virtual_members::laravel::relation_query_method(method_name)?;
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
                ctx.file.class_loader,
                ctx.resolved_class_cache,
            )
            .get_method_arc(method_name)
        });
        let Some(method) = method else { continue };
        let declared = extract_callable_params_at_fw(&method.parameters, argument_list, arg_idx);
        if !is_framework_relation_method(receiver, class, method_name, ctx) {
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
        let Some(mut inferred) = try_relation_query_override(
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

fn is_framework_relation_method(
    receiver: &ResolvedType,
    class: &ClassInfo,
    method: &str,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let Some(raw) = (ctx.file.class_loader)(Fqn::new(&class.fqn())) else {
        return false;
    };
    if let Some(framework) = declared_relation_method(&raw, method, ctx.file.class_loader, 0) {
        return framework;
    }
    // Models forward to their builder statically; relations forward to the
    // related model's builder through `__call`.
    let model =
        if crate::virtual_members::laravel::extends_eloquent_model(&raw, ctx.file.class_loader) {
            raw
        } else if let Some(related) = crate::virtual_members::laravel::related_model_type(
            &receiver.type_string,
            ctx.file.class_loader,
        )
        .and_then(|related| {
            related
                .base_name()
                .and_then(|name| (ctx.file.class_loader)(Fqn::new(name)))
        }) {
            related
        } else {
            return false;
        };
    let builder =
        crate::virtual_members::laravel::model_builder_type(&model, ctx.file.class_loader);
    builder
        .base_name()
        .and_then(|name| (ctx.file.class_loader)(Fqn::new(name)))
        .is_some_and(|builder| {
            declared_relation_method(&builder, method, ctx.file.class_loader, 0) == Some(true)
        })
}

// Only framework declarations carry Eloquent's relation-argument semantics.
// Raw members and trait aliases take precedence over forwarded builder methods.
fn declared_relation_method(
    class: &ClassInfo,
    method: &str,
    loader: &ResolvedClassLoader<'_>,
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
        if let Some(class) = loader(Fqn::new(name))
            && let Some(result) = declared_relation_method(&class, method, loader, depth + 1)
        {
            return Some(result);
        }
    }
    class
        .parent_class
        .as_ref()
        .and_then(|name| loader(Fqn::new(name)))
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
    let receivers = if is_static {
        super::super::closure_resolution::resolve_static_receiver_classes(
            receiver,
            &var_ctx.as_resolution_ctx(),
        )
        .into_iter()
        .map(ResolvedType::from_arc)
        .collect()
    } else {
        super::super::rhs_resolution::resolve_rhs_expression(receiver, &var_ctx)
    };
    let receivers: Vec<_> = receivers
        .into_iter()
        .filter(|receiver| {
            let Some(class) = receiver.class_info.as_ref() else {
                return false;
            };
            if !is_framework_relation_method(receiver, class, "with", ctx) {
                return false;
            }
            let Some(method) = class.get_method_arc("with").or_else(|| {
                crate::virtual_members::resolve_class_fully_maybe_cached(
                    class,
                    ctx.file.class_loader,
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
                try_relation_query_override(receivers, "with", path, None, &ctx.as_resolution_ctx())
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

/// Infer an Eloquent constraint's query from the bound relation argument.
/// Names retain their literal alternatives through the shared expression
/// resolver; relation objects supply their own related-model binding.
fn try_relation_query_override(
    receivers: &[ResolvedType],
    method_name: &str,
    relation: &PhpType,
    candidates: Option<&PhpType>,
    rctx: &ResolutionCtx<'_>,
) -> Option<Vec<PhpType>> {
    if !RELATION_QUERY_METHODS.contains(&method_name) {
        return None;
    }
    let mut queries = Vec::new();
    for model in find_models_from_receivers(receivers, rctx) {
        let query = relation_callback_type(&model, method_name, relation, rctx);
        if let Some(candidates) = candidates {
            let fallback = query.unwrap_or_else(|| {
                PhpType::generic(
                    ELOQUENT_BUILDER_FQN,
                    vec![PhpType::named(atom(ELOQUENT_MODEL_FQN))],
                )
            });
            queries.push(morph_candidate_builders(
                candidates,
                &fallback,
                rctx.file.class_loader,
            ));
        } else if let Some(query) = query {
            queries.push(query);
        }
    }
    (!queries.is_empty()).then(|| vec![PhpType::union(queries).simplified()])
}

/// The callback parameter for one relation argument type. Every alternative
/// of a union must resolve: when any of them is unknown (a plain `string`, a
/// name the relation resolver cannot follow), the declared callback type is
/// the fallback rather than a narrower union of the known alternatives.
fn relation_callback_type(
    model: &ClassInfo,
    method_name: &str,
    relation: &PhpType,
    rctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    if let TypeKind::Union(parts) = relation.kind() {
        let queries = parts
            .iter()
            .map(|part| relation_callback_type(model, method_name, part, rctx))
            .collect::<Option<Vec<_>>>()?;
        return Some(PhpType::union(queries).simplified());
    }
    let eager = matches!(method_name, "with" | "withWhereHas" | "withWhereRelation");
    if let Some(name) = relation
        .as_literal()
        .and_then(|value| value.string_content())
    {
        // Only withWhereHas strips a column selection before its existence
        // query. `with($name, $callback)` keys the eager load by the name
        // verbatim, so `with('rel:id', fn)` names no relation at runtime.
        let chain = if method_name == "withWhereHas" {
            name.split_once(':').map_or(name.as_ref(), |(name, _)| name)
        } else {
            name.as_ref()
        };
        let related = resolve_relation_chain_details(
            model,
            chain,
            rctx.file.class_loader,
            rctx.resolved_class_cache,
        )?;
        if method_name == "with" {
            return Some(related.relation_type);
        }
        let builder = PhpType::union(
            related
                .models
                .iter()
                .map(|model| model_builder_type(model, rctx.file.class_loader))
                .collect(),
        )
        .simplified();
        return Some(if eager {
            PhpType::union(vec![builder, related.relation_type])
        } else {
            builder
        });
    }
    if eager {
        return None;
    }

    // A relation object hands its related model to the `Relation` ancestor;
    // projecting through the ancestor chain (which applies the object's own
    // type arguments) copes with subclasses that reorder or fix them.
    let related = related_model_type(relation, rctx.file.class_loader)?;
    let fallback = PhpType::generic(
        ELOQUENT_BUILDER_FQN,
        vec![PhpType::named(atom(ELOQUENT_MODEL_FQN))],
    );
    Some(morph_model_builders(
        &related,
        &fallback,
        rctx.file.class_loader,
    ))
}

/// Map candidate class-strings through the same builder selector as model
/// queries. Unknown candidates contribute the declared relation's fallback;
/// they must not erase known candidates from a mixed array.
fn morph_candidate_builders(
    candidates: &PhpType,
    fallback: &PhpType,
    class_loader: &ResolvedClassLoader<'_>,
) -> PhpType {
    // Laravel casts `$types` to an array, so a `null` candidate adds no
    // model: the callback only runs for the others.
    match candidates.kind() {
        TypeKind::Nullable(inner) => morph_candidate_builders(inner, fallback, class_loader),
        TypeKind::Union(parts) if parts.iter().any(|part| !part.is_null()) => PhpType::union(
            parts
                .iter()
                .filter(|part| !part.is_null())
                .map(|part| morph_candidate_builders(part, fallback, class_loader))
                .collect(),
        ),
        TypeKind::ClassString(Some(model)) => morph_model_builders(model, fallback, class_loader),
        TypeKind::Literal(value) => value
            .string_content()
            .and_then(|name| {
                class_loader(Fqn::new(&name)).filter(|model| {
                    model
                        .fqn()
                        .eq_ignore_ascii_case(name.trim_start_matches('\\'))
                })
            })
            .filter(|model| {
                model.fqn() == ELOQUENT_MODEL_FQN || extends_eloquent_model(model, class_loader)
            })
            .map(|model| model_builder_type(&model, class_loader))
            .unwrap_or_else(|| fallback.clone()),
        _ => candidates
            .iterable_element_type()
            .map(|element| morph_candidate_builders(&element, fallback, class_loader))
            .unwrap_or_else(|| fallback.clone()),
    }
}

fn morph_model_builders(
    model: &PhpType,
    fallback: &PhpType,
    class_loader: &ResolvedClassLoader<'_>,
) -> PhpType {
    if let TypeKind::Union(parts) = model.kind() {
        return PhpType::union(
            parts
                .iter()
                .map(|part| morph_model_builders(part, fallback, class_loader))
                .collect(),
        );
    }
    model
        .base_name()
        .and_then(|name| class_loader(Fqn::new(name)))
        .filter(|model| {
            model.fqn() == ELOQUENT_MODEL_FQN || extends_eloquent_model(model, class_loader)
        })
        .map(|model| model_builder_type(&model, class_loader))
        .unwrap_or_else(|| fallback.clone())
}

/// Given resolved receivers, find the underlying Eloquent model:
/// the receiver itself when it is a model class, or the model extracted
/// from a `Builder<Model>` receiver.
fn find_models_from_receivers(
    receivers: &[ResolvedType],
    rctx: &ResolutionCtx<'_>,
) -> Vec<Arc<ClassInfo>> {
    let mut models = Vec::new();
    for receiver in receivers {
        let Some(cls) = receiver.class_info.as_ref() else {
            continue;
        };
        if cls.fqn() == ELOQUENT_MODEL_FQN || extends_eloquent_model(cls, rctx.file.class_loader) {
            models.push(Arc::clone(cls));
            continue;
        }
        let current_class = rctx
            .current_class
            .map_or_else(|| atom(""), |class| class.name);
        let as_models = |ty: &PhpType| -> Vec<Arc<ClassInfo>> {
            crate::type_engine::type_resolution::type_hint_to_classes_typed(
                ty,
                &current_class,
                rctx.file.all_classes,
                rctx.file.class_loader,
            )
            .into_iter()
            .filter(|model| {
                model.fqn() == ELOQUENT_MODEL_FQN
                    || extends_eloquent_model(model, rctx.file.class_loader)
            })
            .collect()
        };
        if cls.fqn() != ELOQUENT_BUILDER_FQN
            && !crate::virtual_members::laravel::helpers::extends_eloquent_builder(
                cls,
                rctx.file.class_loader,
            )
        {
            // A relation forwards its query methods to a builder over its
            // related model, so that model is the one the callback sees.
            if let Some(related) = related_model_type(&receiver.type_string, rctx.file.class_loader)
            {
                models.extend(as_models(&related));
            }
            continue;
        }
        // Read the receiver's retained generic context before recovering it
        // from declarations; custom builders need not declare TModel at all.
        // Each source is tried in turn, so an unsubstituted `TModel` in one
        // does not hide a concrete model in the next.  A source may name
        // several models (`Builder<Post|Comment>`), and each is kept.
        let non_empty = |found: Vec<Arc<ClassInfo>>| (!found.is_empty()).then_some(found);
        let found = crate::inheritance::extract_generic_arg_from_ancestor(
            &receiver.type_string,
            ELOQUENT_BUILDER_FQN,
            0,
            rctx.file.class_loader,
        )
        .and_then(|ty| non_empty(as_models(&ty)))
        .or_else(|| {
            cls.get_method("getModel")
                .and_then(|method| method.return_type.as_ref())
                .and_then(|ty| non_empty(as_models(ty)))
        })
        .or_else(|| extract_model_from_builder(cls).and_then(|ty| non_empty(as_models(&ty))));
        models.extend(found.into_iter().flatten());
    }
    models
}

#[cfg(test)]
#[path = "relation_callbacks_tests.rs"]
mod tests;
