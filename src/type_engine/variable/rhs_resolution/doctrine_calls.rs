//! Doctrine query calls whose type the DQL decides (see
//! [`crate::doctrine::query`]): `createQuery()` on an entity manager,
//! `getQuery()` at the end of a query builder chain, and the result methods
//! of the query they give.

use mago_syntax::cst::*;

use crate::atom::bytes_to_str;
use crate::doctrine::query::{
    self, BuilderArg, CallArg, ENTITY_MANAGER, HYDRATION_MODE_PARAMETER, QUERY_BUILDER,
    ResolverModel,
};
use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, ResolvedType};

use crate::type_engine::resolver::VarResolutionCtx;

use super::method_calls::resolve_method_receiver;
use super::{peel_type_transparent, resolve_rhs_expression};

fn resolved(ty: PhpType, ctx: &VarResolutionCtx<'_>) -> Vec<ResolvedType> {
    crate::type_engine::type_ops::resolution::resolved_types_for_hint(
        ty,
        "",
        ctx.file.all_classes,
        ctx.file.class_loader,
    )
}

fn expression_type(expr: &Expression<'_>, ctx: &VarResolutionCtx<'_>) -> Option<PhpType> {
    let types = resolve_rhs_expression(expr, ctx);
    (!types.is_empty()).then(|| ResolvedType::types_joined(&types))
}

/// What `createQuery($dql)` on an entity manager returns: the query typed
/// by its DQL, or by each DQL the argument may be.
pub(super) fn try_resolve_create_query_type(
    owner: &ClassInfo,
    method_name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    if !method_name.eq_ignore_ascii_case("createQuery")
        || !crate::class_lookup::is_subtype_of(owner, ENTITY_MANAGER, ctx.file.class_loader)
    {
        return None;
    }
    let rctx = ctx.as_resolution_ctx();
    let dql = crate::call_args::first_param_arg(argument_list, "dql")
        .map(|argument| expression_type(argument.value(), ctx));
    let ty = query::create_query_type(dql, &ResolverModel::new(&rctx));
    Some(resolved(ty, ctx))
}

/// What a result method (`getResult()`, `getSingleResult()`, …) returns
/// on a query that carries its `TKey` and `TResult`.
pub(super) fn try_resolve_query_result_type(
    method_name: &str,
    receiver: &[ResolvedType],
    argument_list: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let index = query::result_method(method_name)?;
    if receiver.is_empty() {
        return None;
    }
    let receiver = ResolvedType::types_joined(receiver);
    if !query::carries_result(&receiver) {
        return None;
    }
    let mode = argument_at(argument_list, index, HYDRATION_MODE_PARAMETER)
        .map(|value| expression_type(value, ctx).and_then(|ty| query::hydration_mode(&ty)));
    let loader = ctx.file.class_loader;
    let is_class = |name: &str| loader(crate::class_lookup::Fqn::new(name)).is_some();
    let ty = query::hydrated_result(method_name, &receiver, mode, &is_class)?;
    Some(resolved(ty, ctx))
}

/// The argument bound to the parameter at `index`, named `name`.
fn argument_at<'b>(
    argument_list: &ArgumentList<'b>,
    index: usize,
    name: &str,
) -> Option<&'b Expression<'b>> {
    let mut position = 0;
    for argument in argument_list.arguments.iter() {
        match argument {
            Argument::Positional(positional) => {
                if position == index {
                    return Some(positional.value);
                }
                position += 1;
            }
            Argument::Named(named) if bytes_to_str(named.name.value) == name => {
                return Some(named.value);
            }
            Argument::Named(_) => {}
        }
    }
    None
}

/// What `getQuery()` returns at the end of a query builder chain that
/// starts at `createQueryBuilder()`, written in the one expression: the
/// query typed by the DQL the chain builds.  `object` is the receiver of
/// `getQuery()`.
///
/// A builder held in a variable is not followed, since a call on the
/// variable may change the builder without changing what the variable is
/// assigned.
pub(super) fn try_resolve_builder_query_type(
    owner: &ClassInfo,
    method_name: &str,
    object: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    if !method_name.eq_ignore_ascii_case("getQuery")
        || !crate::class_lookup::is_subtype_of(owner, QUERY_BUILDER, ctx.file.class_loader)
    {
        return None;
    }
    let mut links: Vec<(&str, &ArgumentList<'_>)> = Vec::new();
    let mut current = object;
    let (root, root_args) = loop {
        let (inner, method, argument_list) = match peel_type_transparent(current) {
            Expression::Call(Call::Method(call)) => {
                (call.object, &call.method, &call.argument_list)
            }
            Expression::Call(Call::NullSafeMethod(call)) => {
                (call.object, &call.method, &call.argument_list)
            }
            _ => return None,
        };
        let ClassLikeMemberSelector::Identifier(name) = method else {
            return None;
        };
        let name = bytes_to_str(name.value);
        if name.eq_ignore_ascii_case("createQueryBuilder") {
            break (inner, argument_list);
        }
        links.push((name, argument_list));
        current = inner;
    };

    let (owners, receiver_types) = resolve_method_receiver(root, ctx);
    let generic_args: &[PhpType] = match receiver_types.as_slice() {
        [only] => match only.type_string.kind() {
            TypeKind::Generic(generic) => &generic.args,
            _ => &[],
        },
        _ => &[],
    };
    let mut state = query::root_state(
        &owners,
        generic_args,
        call_args(root_args, ctx),
        ctx.file.class_loader,
    )?;
    for (method, argument_list) in links.iter().rev() {
        if query::is_result_neutral(method) {
            continue;
        }
        let args = query::builder_args(method, call_args(argument_list, ctx))?;
        state.apply(method, &args)?;
    }
    let rctx = ctx.as_resolution_ctx();
    let ty = query::builder_query_type(&state, &ResolverModel::new(&rctx));
    Some(resolved(ty, ctx))
}

fn call_args(argument_list: &ArgumentList<'_>, ctx: &VarResolutionCtx<'_>) -> Vec<CallArg> {
    argument_list
        .arguments
        .iter()
        .map(|argument| match argument {
            Argument::Positional(positional) => CallArg {
                name: None,
                spread: positional.ellipsis.is_some(),
                value: builder_arg(positional.value, ctx),
            },
            Argument::Named(named) => CallArg {
                name: Some(bytes_to_str(named.name.value).to_string()),
                spread: false,
                value: builder_arg(named.value, ctx),
            },
        })
        .collect()
}

/// What a builder method's argument holds: the strings of a string, a
/// class string, or an array of strings.
fn builder_arg(value: &Expression<'_>, ctx: &VarResolutionCtx<'_>) -> BuilderArg {
    let elements = match peel_type_transparent(value) {
        Expression::Array(array) => Some(&array.elements),
        Expression::LegacyArray(array) => Some(&array.elements),
        _ => None,
    };
    if let Some(elements) = elements {
        let mut strings = Vec::new();
        for element in elements.iter() {
            let array::ArrayElement::Value(element) = element else {
                return BuilderArg::Unknown;
            };
            match builder_arg(element.value, ctx) {
                BuilderArg::Strings(more) => strings.extend(more),
                _ => return BuilderArg::Unknown,
            }
        }
        return BuilderArg::Strings(strings);
    }
    match expression_type(value, ctx) {
        Some(ty) => BuilderArg::of_type(&ty),
        None => BuilderArg::Unknown,
    }
}
