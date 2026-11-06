//! Doctrine query calls whose type the DQL decides (see
//! [`crate::doctrine::query`]): `createQuery()` on an entity manager, the
//! query builder `createQueryBuilder()` starts and each call on it, the
//! query its `getQuery()` gives, and the result methods of a query.
//!
//! A builder carries the DQL its calls have built in its type (see
//! [`TypeKind::DqlBuilder`]), so it is followed through a variable, a
//! branch, and a method that returns it, the way PHPStan's Doctrine
//! extension follows it.  The functions here are shared by the resolver
//! that reads a call from the AST and the one that reads it from text; each
//! hands over the call's arguments already read.

use crate::Backend;
use crate::doctrine::query::{
    self, BuilderArg, CallArg, ENTITY_MANAGER, HYDRATION_MODE_PARAMETER, QUERY_BUILDER,
    QueryBuilderState, ResolverModel,
};
use crate::php_type::{PhpType, TypeKind};
use crate::text_scan::split_top_level_commas;
use crate::type_engine::resolver::ResolutionCtx;
use crate::types::{ClassInfo, MethodInfo, ResolvedType};

/// The builders `receiver` may be, when every one of them carries the DQL
/// it has built.
fn dql_builders(receiver: &PhpType) -> Option<Vec<(&PhpType, &QueryBuilderState)>> {
    receiver
        .union_members()
        .into_iter()
        .map(PhpType::as_dql_builder)
        .collect()
}

/// What the call of `method` on `receiver` gives when `receiver` is one or
/// more query builders that carry the DQL they have built: the query
/// `getQuery()` types by that DQL, the builder a replayed call leaves with
/// the DQL it adds, and the bare builder class (DQL no longer known) from a
/// call that changes the builder in a way that is not replayed.  `args`
/// reads the call's arguments.
pub(crate) fn dql_builder_call(
    method: &str,
    receiver: &PhpType,
    args: impl FnOnce() -> Vec<CallArg>,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let builders = dql_builders(receiver)?;
    let unknown = || PhpType::union(builders.iter().map(|(class, _)| (*class).clone()).collect());
    if method.eq_ignore_ascii_case("getQuery") {
        let model = ResolverModel::new(ctx);
        return Some(PhpType::union(
            builders
                .iter()
                .map(|(_, state)| query::builder_query_type(state, &model))
                .collect(),
        ));
    }
    // Filtering or ordering the rows leaves the builder's result as it was,
    // whatever the arguments are.
    if query::is_result_neutral(method) {
        return Some(receiver.clone());
    }
    if query::is_replayed(method) {
        let Some(args) = query::builder_args(method, args()) else {
            return Some(unknown());
        };
        let mut types = Vec::with_capacity(builders.len());
        for (class, state) in &builders {
            let mut state = (*state).clone();
            if state.apply(method, &args).is_none() {
                return Some(unknown());
            }
            types.push(PhpType::dql_builder((*class).clone(), state));
        }
        return Some(PhpType::union(types));
    }
    // Any other call that returns the builder may have changed it.
    let returns_builder = builders.iter().any(|(class, _)| {
        let TypeKind::Named(name) = class.kind() else {
            return false;
        };
        (ctx.file.class_loader)(crate::class_lookup::Fqn::new(name)).is_some_and(|class| {
            class
                .get_method_ci(method)
                .and_then(|m| m.return_type.as_ref())
                .is_some_and(PhpType::is_self_like)
        })
    });
    returns_builder.then(unknown)
}

/// What the call of `method` on `owner` gives when it starts a query
/// builder, or asks a builder whose DQL is not known for its query:
/// `createQueryBuilder()` on an entity manager or a repository gives a
/// builder carrying its DQL, and `getQuery()` a query of unknown result.
/// `generic_args` are the type arguments the receiver carries, and `args`
/// reads the call's arguments.
pub(crate) fn builder_owner_call(
    owner: &ClassInfo,
    generic_args: &[PhpType],
    method: &str,
    args: impl FnOnce() -> Vec<CallArg>,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let loader = ctx.file.class_loader;
    if method.eq_ignore_ascii_case("createQueryBuilder") {
        let state = query::root_state(owner, generic_args, args(), loader)?;
        return Some(PhpType::dql_builder(
            PhpType::named(crate::atom::atom(QUERY_BUILDER)),
            state,
        ));
    }
    if method.eq_ignore_ascii_case("getQuery")
        && crate::class_lookup::is_subtype_of(owner, QUERY_BUILDER, loader)
    {
        return Some(query::query_of(PhpType::mixed(), PhpType::mixed()));
    }
    None
}

/// The query builders a call of `method` on `owner` returns, read from the
/// method's body, when it declares it returns a builder (`declared`) and
/// every `return` gives one that carries its DQL: the builder a helper or a
/// custom repository sets up.
pub(crate) fn builder_returned_by(
    owner: &ClassInfo,
    method: &MethodInfo,
    declared: &PhpType,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let TypeKind::Named(name) = declared.raw_kind() else {
        return None;
    };
    if method.name_offset == 0
        || method.is_virtual
        || method.name.eq_ignore_ascii_case("createQueryBuilder")
    {
        return None;
    }
    let backend = ctx
        .file
        .backend
        .filter(|backend| backend.is_doctrine_project())?;
    let loader = ctx.file.class_loader;
    let class = loader(crate::class_lookup::Fqn::new(name))?;
    // The builder is a class, so any other class returned is one only when
    // it extends it.
    if class.fqn() != QUERY_BUILDER
        && (class.parent_class.is_none()
            || !crate::class_lookup::is_subtype_of(&class, QUERY_BUILDER, loader))
    {
        return None;
    }
    // The builder's own methods are replayed rather than read.
    if crate::class_lookup::is_subtype_of(owner, QUERY_BUILDER, loader) {
        return None;
    }
    let inferred = crate::type_engine::call_resolution::try_infer_body_return_type(
        backend,
        &owner.fqn(),
        method,
        &[],
    )?;
    dql_builders(&inferred).is_some().then_some(inferred)
}

/// What the call of `method_name` on a receiver whose resolved types are
/// `receiver` returns given its argument text, when its DQL decides it.
pub(crate) fn query_at_call(
    method_name: &str,
    text_args: &str,
    receiver: &[ResolvedType],
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    if let Some(index) = query::result_method(method_name) {
        return result_at_call(method_name, index, text_args, receiver, ctx);
    }
    if receiver.iter().any(|rt| rt.type_string.has_dql_builder()) {
        let joined = ResolvedType::types_joined(receiver);
        if let Some(ty) = dql_builder_call(method_name, &joined, || call_args(text_args, ctx), ctx)
        {
            return Some(ty);
        }
    }
    let loader = ctx.file.class_loader;
    let mut owners = receiver.iter().filter_map(|r| r.class_info.as_deref());
    if method_name.eq_ignore_ascii_case("createQuery")
        && owners
            .clone()
            .any(|owner| crate::class_lookup::is_subtype_of(owner, ENTITY_MANAGER, loader))
    {
        let args = split_top_level_commas(text_args);
        let dql =
            crate::call_args::first_param_text_arg(&args, "dql").map(|arg| arg_type(arg, ctx));
        return Some(query::create_query_type(dql, &ResolverModel::new(ctx)));
    }
    let generic_args: &[PhpType] = match receiver {
        [only] => match only.type_string.kind() {
            TypeKind::Generic(generic) => &generic.args,
            _ => &[],
        },
        _ => &[],
    };
    if let (Some(owner), None) = (owners.next(), owners.next()) {
        if let Some(ty) = builder_owner_call(
            owner,
            generic_args,
            method_name,
            || call_args(text_args, ctx),
            ctx,
        ) {
            return Some(ty);
        }
        if let Some(method) = owner.get_method_ci(method_name)
            && let Some(declared) = method.return_type.as_ref()
        {
            return builder_returned_by(owner, method, declared, ctx);
        }
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
/// class string, or an array of strings, or the expression object a `new`
/// makes.
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
    if let Some(rest) = text.strip_prefix("new ")
        && let Some((_, args)) = rest.split_once('(')
        && let Some(args) = args.strip_suffix(')')
        && let Some(ty) = arg_type(text, ctx)
        && let TypeKind::Named(class) = ty.kind()
    {
        return BuilderArg::New {
            class: class.to_string(),
            args: call_args(args, ctx),
        };
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
