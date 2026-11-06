//! Doctrine query calls whose type the DQL decides, for a call read from
//! its source text (see [`crate::doctrine::query`]): `createQuery()` on an
//! entity manager, `getQuery()` at the end of a query builder chain, and the
//! result methods of the query they give.

use crate::Backend;
use crate::doctrine::query::{
    self, BuilderArg, CallArg, ENTITY_MANAGER, HYDRATION_MODE_PARAMETER, QUERY_BUILDER,
    ResolverModel,
};
use crate::php_type::{PhpType, TypeKind};
use crate::text_scan::split_top_level_commas;
use crate::type_engine::resolver::ResolutionCtx;
use crate::type_engine::subject_expr::SubjectExpr;
use crate::types::{AccessKind, ResolvedType};

/// What the call of `method_name` on `base`, whose resolved types are
/// `receiver`, returns given its argument text, when its DQL decides it.
pub(crate) fn query_at_call(
    base: &SubjectExpr,
    method_name: &str,
    text_args: &str,
    receiver: &[ResolvedType],
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    if let Some(index) = query::result_method(method_name) {
        return result_at_call(method_name, index, text_args, receiver, ctx);
    }
    let loader = ctx.file.class_loader;
    let owned_by = |base: &str| {
        receiver.iter().any(|r| {
            r.class_info
                .as_deref()
                .is_some_and(|owner| crate::class_lookup::is_subtype_of(owner, base, loader))
        })
    };
    if method_name.eq_ignore_ascii_case("createQuery") && owned_by(ENTITY_MANAGER) {
        let args = split_top_level_commas(text_args);
        let dql =
            crate::call_args::first_param_text_arg(&args, "dql").map(|arg| arg_type(arg, ctx));
        return Some(query::create_query_type(dql, &ResolverModel::new(ctx)));
    }
    if method_name.eq_ignore_ascii_case("getQuery") && owned_by(QUERY_BUILDER) {
        return builder_query_at_call(base, ctx);
    }
    None
}

fn result_at_call(
    method_name: &str,
    index: usize,
    text_args: &str,
    receiver: &[ResolvedType],
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    if receiver.is_empty() {
        return None;
    }
    let receiver = ResolvedType::types_joined(receiver);
    if !query::carries_result(&receiver) {
        return None;
    }
    let mut position = 0;
    let mut mode = None;
    for arg in split_top_level_commas(text_args) {
        let (name, value) = crate::call_args::split_named_text_arg(arg);
        let at = match name {
            Some(name) => name == HYDRATION_MODE_PARAMETER,
            None => {
                position += 1;
                position - 1 == index
            }
        };
        if at {
            mode = Some(arg_type(value, ctx).and_then(|ty| query::hydration_mode(&ty)));
            break;
        }
    }
    let loader = ctx.file.class_loader;
    let is_class = |name: &str| loader(crate::class_lookup::Fqn::new(name)).is_some();
    query::hydrated_result(method_name, &receiver, mode, &is_class)
}

/// The type of an argument's text: a literal as its value, which the text
/// resolver widens to its base type.
fn arg_type(text: &str, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    let text = text.trim();
    if let Some(value) = crate::util::unescape_php_string_literal(text) {
        return Some(PhpType::literal_string_value(value));
    }
    if let Ok(value) = text.parse::<i64>() {
        return Some(PhpType::literal_int(value.to_string()));
    }
    Backend::resolve_arg_text_to_type(text, ctx)
}

/// What `getQuery()` returns at the end of a query builder chain that
/// starts at `createQueryBuilder()`; see the AST counterpart in the
/// variable resolver.
fn builder_query_at_call(base: &SubjectExpr, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    let mut links: Vec<(&str, &str)> = Vec::new();
    let mut current = base;
    let (root, root_args) = loop {
        let SubjectExpr::CallExpr { callee, args_text } = current else {
            return None;
        };
        let SubjectExpr::MethodCall { base, method } = callee.as_ref() else {
            return None;
        };
        if method.eq_ignore_ascii_case("createQueryBuilder") {
            break (base.as_ref(), args_text.as_str());
        }
        links.push((method, args_text));
        current = base;
    };

    let receiver =
        crate::type_engine::resolver::resolve_target_classes_expr(root, AccessKind::Arrow, ctx);
    let owners: Vec<_> = receiver
        .iter()
        .filter_map(|r| r.class_info.clone())
        .collect();
    let generic_args: &[PhpType] = match receiver.as_slice() {
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
    for (method, args_text) in links.iter().rev() {
        if query::is_result_neutral(method) {
            continue;
        }
        let args = query::builder_args(method, call_args(args_text, ctx))?;
        state.apply(method, &args)?;
    }
    Some(query::builder_query_type(&state, &ResolverModel::new(ctx)))
}

fn call_args(text: &str, ctx: &ResolutionCtx<'_>) -> Vec<CallArg> {
    split_top_level_commas(text)
        .into_iter()
        .filter(|arg| !arg.trim().is_empty())
        .map(|arg| {
            let (name, value) = crate::call_args::split_named_text_arg(arg);
            let spread = value.starts_with("...");
            CallArg {
                name: name.map(str::to_string),
                spread,
                value: builder_arg(value, ctx),
            }
        })
        .collect()
}

/// What a builder method's argument text holds: the strings of a string, a
/// class string, or an array of strings.
fn builder_arg(text: &str, ctx: &ResolutionCtx<'_>) -> BuilderArg {
    let text = text.trim();
    if let Some(inner) = text.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
        let mut strings = Vec::new();
        for element in split_top_level_commas(inner) {
            if element.trim().is_empty() {
                continue;
            }
            match builder_arg(element, ctx) {
                BuilderArg::Strings(more) if !element.contains("=>") => strings.extend(more),
                _ => return BuilderArg::Unknown,
            }
        }
        return BuilderArg::Strings(strings);
    }
    let Some(ty) = arg_type(text, ctx) else {
        return BuilderArg::Unknown;
    };
    // The text resolver gives `Foo::class` the class itself.
    if text.ends_with("::class")
        && let TypeKind::Named(class) = ty.kind()
    {
        return BuilderArg::Strings(vec![class.to_string()]);
    }
    BuilderArg::of_type(&ty)
}
