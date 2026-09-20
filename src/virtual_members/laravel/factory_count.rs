//! Count-conditional return types for Eloquent factory chains.
//!
//! `User::factory()->create()` builds a single `User`, but
//! `User::factory(3)->create()`, `User::factory()->count(3)->create()` and
//! `UserFactory::times(3)->create()` all build a
//! `Collection<int, User>`.  Laravel expresses both outcomes with one
//! `@return Collection<int, TModel>|TModel` annotation on
//! `Factory::create()`/`make()`, which is ambiguous at every call site.
//!
//! This module reads the count state off the receiver chain and picks the
//! branch the call actually produces, the way a PHPStan conditional return
//! type extension does.  The state reaches the `create()` two ways:
//! from the syntax of the chain it is written on, and from the value the
//! receiver resolved to — every factory-returning call tags its result
//! with a [`FactoryCount`], so a chain that travels through a variable
//! (`$factory = User::factory(); … $factory->state([…])->create()`) still
//! knows what it builds.  A factory whose count never became visible
//! (a parameter, a property, a branch join that disagreed) is left alone
//! rather than guessed at: narrowing it to one model would make
//! `create()->first()` a false positive whenever it did hold a count.

use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::{Argument, ArgumentList, Call, ClassLikeMemberSelector, Expression};

use crate::atom::{atom, bytes_to_str};
use crate::php_type::PhpType;
use crate::type_engine::conditional_resolution::split_text_args;
use crate::type_engine::resolver::ResolutionCtx;
use crate::type_engine::subject_expr::SubjectExpr;
use crate::types::{ClassInfo, ELOQUENT_COLLECTION_FQN, FactoryCount, ResolvedType};

use super::factory::{extends_eloquent_factory, factory_model_type};

/// Whether `name` is one of the `Factory` methods whose return type
/// depends on the chain's count state.
///
/// `createOne()`/`makeOne()` always build one model and
/// `createMany()`/`makeMany()` always build a collection, so neither is
/// count-conditional.
pub(crate) fn is_count_conditional_method(name: &str) -> bool {
    matches!(name, "create" | "createQuietly" | "make")
}

/// Read the count state off a factory receiver chain.
///
/// The chain is walked outermost-first, so the *last* count-setting call
/// wins — `User::factory(3)->count(null)` builds one model and
/// `User::factory()->count(2)` builds two.  Calls that are not
/// count-setting (`state()`, `hasPosts()`, `trashed()`, …) are stepped
/// over.  A static call settles the count either way, since it is the
/// head of the chain; anything else the walk reaches (a variable, a
/// property, `new UserFactory(…)` whose arguments the subject parser
/// does not keep) leaves it [`FactoryCount::Unknown`].
pub(crate) fn chain_count(receiver: &SubjectExpr) -> FactoryCount {
    // Every step descends into a strictly smaller sub-expression, so the
    // walk terminates on any chain the subject parser can produce.
    let mut current = receiver;
    loop {
        let SubjectExpr::CallExpr { callee, args_text } = current else {
            return FactoryCount::Unknown;
        };
        match callee.as_ref() {
            SubjectExpr::MethodCall { base, method } => {
                let args = if method == "count" {
                    Some(split_text_args(args_text))
                } else {
                    None
                };
                let first_arg = args.as_ref().and_then(|args| {
                    args.first()
                        .map(|arg| crate::call_args::text_arg_value(arg))
                });
                if let Some(state) = instance_count_state(method, first_arg, &|| None) {
                    return state;
                }
                current = base;
            }
            // A static call is the head of the chain: `Model::factory()`,
            // `UserFactory::new()`, `UserFactory::times(3)`.
            SubjectExpr::StaticMethodCall { method, .. } => {
                let args = if method == "factory" {
                    Some(split_text_args(args_text))
                } else {
                    None
                };
                let first_arg = args.as_ref().and_then(|args| {
                    args.first()
                        .map(|arg| crate::call_args::text_arg_value(arg))
                });
                return static_count_state(method, first_arg, &|| None);
            }
            _ => return FactoryCount::Unknown,
        }
    }
}

/// Read the count state off a factory receiver chain in its AST form.
///
/// The AST-walking resolution path (assignments, arguments, property and
/// return types) hands over an [`Expression`] rather than a parsed
/// [`SubjectExpr`], so the same outermost-first walk runs over the call
/// nodes directly.  Both walks share the per-call count rules below, so
/// the two paths cannot drift apart on what a chain builds.
pub(crate) fn chain_count_ast(receiver: &Expression<'_>, content: &str) -> FactoryCount {
    // Every step descends into a strictly smaller sub-expression, so the
    // walk terminates on any chain the parser can produce.
    let mut current = receiver;
    loop {
        let call = match current {
            Expression::Call(call) => call,
            Expression::Parenthesized(inner) => {
                current = inner.expression;
                continue;
            }
            _ => return FactoryCount::Unknown,
        };
        let (object, selector, argument_list) = match call {
            Call::Method(mc) => (Some(mc.object), &mc.method, &mc.argument_list),
            Call::NullSafeMethod(mc) => (Some(mc.object), &mc.method, &mc.argument_list),
            // A static call is the head of the chain: `Model::factory()`,
            // `UserFactory::new()`, `UserFactory::times(3)`.
            Call::StaticMethod(sc) => (None, &sc.method, &sc.argument_list),
            Call::Function(_) => return FactoryCount::Unknown,
        };
        // A computed call target (`$factory->$method()`) says nothing
        // about the count.
        let ClassLikeMemberSelector::Identifier(ident) = selector else {
            return FactoryCount::Unknown;
        };
        let method = bytes_to_str(ident.value);
        let reads_first_arg = match object {
            Some(_) => method == "count",
            None => method == "factory",
        };
        let first_arg = if reads_first_arg {
            first_argument_text(argument_list, content)
        } else {
            None
        };
        match object {
            Some(base) => {
                if let Some(state) = instance_count_state(method, first_arg, &|| None) {
                    return state;
                }
                current = base;
            }
            None => return static_count_state(method, first_arg, &|| None),
        }
    }
}

/// The source text of a call's first argument, for the count rules that
/// gate on how the argument was written.
fn first_argument_text<'c>(argument_list: &ArgumentList<'_>, content: &'c str) -> Option<&'c str> {
    let span = match argument_list.arguments.first()? {
        Argument::Positional(pos) => pos.value.span(),
        Argument::Named(named) => named.value.span(),
    };
    content
        .get(span.start.offset as usize..span.end.offset as usize)
        .map(str::trim)
}

/// Count state contributed by an instance call in the chain, or `None`
/// when the call does not touch the count.
fn instance_count_state(
    method: &str,
    first_arg: Option<&str>,
    first_arg_type: &dyn Fn() -> Option<PhpType>,
) -> Option<FactoryCount> {
    match method {
        // `count(?int $count)` clears the count for null and sets one for an
        // integer. A non-literal can only pick a branch once its type is
        // known; `?int` keeps both outcomes possible.
        "count" => Some(count_argument_count(first_arg, first_arg_type)),
        // `times(int $count)` cannot be given null.
        "times" => Some(FactoryCount::Many),
        _ => None,
    }
}

/// Count state set by `Factory::count(?int $count)`.
///
/// Literal integers and `null` settle the branch without type resolution.
/// Anything else asks for the argument's resolved type, which callers supply
/// lazily because almost every method call in a project is not `count()`.
fn count_argument_count(
    first_arg: Option<&str>,
    first_arg_type: &dyn Fn() -> Option<PhpType>,
) -> FactoryCount {
    let Some(first) = first_arg else {
        // The call is invalid because Laravel requires the argument, but
        // preserving the old single-model result avoids inventing a count.
        return FactoryCount::One;
    };
    let first = first.trim();
    if first.eq_ignore_ascii_case("null") {
        return FactoryCount::One;
    }
    if is_int_literal_syntax(first) {
        return FactoryCount::Many;
    }

    first_arg_type().map_or(FactoryCount::Unknown, |ty| count_argument_type(&ty))
}

/// Whether source text is a PHP integer literal.
///
/// The ordinary decimal path avoids the allocation used to normalize the
/// less-common underscored and radix-prefixed spellings.
fn is_int_literal_syntax(raw: &str) -> bool {
    let trimmed = raw.trim();
    let unsigned = trimmed
        .strip_prefix('+')
        .or_else(|| trimmed.strip_prefix('-'))
        .unwrap_or(trimmed);
    if !unsigned.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        return false;
    }

    trimmed.parse::<i64>().is_ok() || crate::php_type::parse_php_int_literal(trimmed).is_some()
}

/// Count state settled by a resolved `count()` argument type.
fn count_argument_type(ty: &PhpType) -> FactoryCount {
    if ty.is_null() {
        FactoryCount::One
    } else if ty.accepts_null() {
        FactoryCount::Unknown
    } else if ty.is_int_subtype() {
        FactoryCount::Many
    } else {
        FactoryCount::Unknown
    }
}

/// Count state contributed by the static call that opens the chain.
///
/// `first_arg_type` answers for an argument whose spelling does not
/// settle what `is_numeric()` would say about it, and is only asked when
/// that happens.
fn static_count_state(
    method: &str,
    first_arg: Option<&str>,
    first_arg_type: &dyn Fn() -> Option<PhpType>,
) -> FactoryCount {
    match method {
        // `Model::factory(…)` forwards its first argument to `count()`
        // only when it is numeric; an array or callable is state.
        "factory" => factory_argument_count(first_arg, first_arg_type),
        "times" => FactoryCount::Many,
        // `Factory::new(array $attributes)` takes state, never a count.
        "new" => FactoryCount::One,
        // Some other static call handed back a factory, and whatever
        // count it was built with is not visible from here.
        _ => FactoryCount::Unknown,
    }
}

/// Count state set by `Model::factory(…)`'s first argument.
///
/// Laravel gates on `is_numeric($parameters[0])`, so a numeric literal
/// (`3`, and `'3'`, which `is_numeric()` also accepts) sets a count and a
/// literal it rejects (an array, a closure, `null`) is state.  An
/// argument written as anything else — `factory($count)` — is settled by
/// its type instead.
fn factory_argument_count(
    first_arg: Option<&str>,
    first_arg_type: &dyn Fn() -> Option<PhpType>,
) -> FactoryCount {
    let Some(first) = first_arg else {
        return FactoryCount::One;
    };
    if !is_decidable_literal(first) {
        return first_arg_type().map_or(FactoryCount::Unknown, |ty| numeric_argument_count(&ty));
    }
    let unquoted = crate::text_scan::unquote_php_string(first).unwrap_or(first);
    if !unquoted.is_empty() && unquoted.parse::<f64>().is_ok() {
        FactoryCount::Many
    } else {
        FactoryCount::One
    }
}

/// Count state a resolved argument type settles.
///
/// A number is what `is_numeric()` accepts, and the alternative the
/// parameter is written for — an array or a callable of state — is what
/// it rejects.  Anything else (a `mixed`, a union spanning both, a
/// `string` that may or may not hold digits) is left undecided.
fn numeric_argument_count(ty: &PhpType) -> FactoryCount {
    if ty.is_int_subtype() || ty.is_float_subtype() || is_numeric_string_literal(ty) {
        return FactoryCount::Many;
    }
    if ty.is_array_like() || ty.is_callable() || ty.is_closure() || ty.is_null() {
        return FactoryCount::One;
    }
    FactoryCount::Unknown
}

/// Whether `ty` is a string literal holding digits, which `is_numeric()`
/// accepts just as it accepts the number itself.
fn is_numeric_string_literal(ty: &PhpType) -> bool {
    match ty.kind() {
        crate::php_type::TypeKind::Literal(value) => value.is_numeric_string(),
        _ => false,
    }
}

/// Whether an argument's spelling settles what `is_numeric()` would say
/// about it.
///
/// A literal does — a number, a quoted string, an array, a closure,
/// `null` — while a variable, a constant, or another call does not.
fn is_decidable_literal(arg: &str) -> bool {
    match arg.chars().next() {
        Some(c) if c.is_ascii_digit() || matches!(c, '\'' | '"' | '[' | '-' | '+' | '.') => true,
        Some(_) => {
            ["null", "true", "false"]
                .iter()
                .any(|k| arg.eq_ignore_ascii_case(k))
                || [
                    "array(",
                    "fn(",
                    "fn ",
                    "function(",
                    "function ",
                    "static ",
                    "new ",
                ]
                .iter()
                .any(|p| {
                    arg.get(..p.len())
                        .is_some_and(|head| head.eq_ignore_ascii_case(p))
                })
        }
        None => false,
    }
}

/// The count state a resolved receiver carries.
///
/// Only entries that name a class have a say: the `null` half of a
/// `?UserFactory` is not a factory whose count went missing, so it does
/// not veto the factory's own state.  Class entries that disagree do,
/// since the value is then a factory of unknown count.
fn carried_count(receiver: &[ResolvedType]) -> FactoryCount {
    let mut carried = None;
    for count in receiver
        .iter()
        .filter(|rt| rt.class_info.is_some())
        .map(|rt| rt.factory_count)
    {
        if count == FactoryCount::Unknown || carried.is_some_and(|previous| previous != count) {
            return FactoryCount::Unknown;
        }
        carried = Some(count);
    }
    carried.unwrap_or(FactoryCount::Unknown)
}

/// The count state an instance call hands to its result, or `None` when
/// the call has none to hand on.
///
/// A fluent factory method (`state()`, `for()`, `hasPosts()`, …) returns
/// `static`, so its result is the same factory and carries the same
/// count.  `count()` and `times()` set one instead.  Everything else —
/// which is every method call in a codebase that is not on a factory —
/// answers `None` after one look at the receiver's own state.
pub(crate) fn fluent_factory_count(
    receiver: &[ResolvedType],
    method: &str,
    first_arg: Option<&str>,
    first_arg_type: &dyn Fn() -> Option<PhpType>,
    ctx: &ResolutionCtx<'_>,
) -> Option<FactoryCount> {
    let inherited = carried_count(receiver);
    if !matches!(method, "count" | "times") {
        return (inherited != FactoryCount::Unknown).then_some(inherited);
    }
    // `count()` is a common method name, so a receiver that never
    // carried a factory state has to prove it is a factory before its argument
    // is resolved or the call is read as a factory count setter.
    if inherited == FactoryCount::Unknown && !receiver_is_factory(receiver, ctx) {
        return None;
    }
    instance_count_state(method, first_arg, first_arg_type)
}

/// Whether any of the receiver's classes is an Eloquent factory.
fn receiver_is_factory(receiver: &[ResolvedType], ctx: &ResolutionCtx<'_>) -> bool {
    receiver.iter().any(|rt| {
        rt.class_info
            .as_ref()
            .is_some_and(|ci| extends_eloquent_factory(ci, ctx.class_loader))
    })
}

/// Give every result that is the receiver's own class the count state
/// `count` describes.
///
/// A fluent factory method returns `static`, so the result naming the
/// same class is the same factory travelling on.  A method that returned
/// something else (the model out of `create()`, a builder) is not, and
/// keeps whatever state it resolved with.
pub(crate) fn carry_factory_count(
    results: &mut [ResolvedType],
    receiver: &[ResolvedType],
    count: FactoryCount,
) {
    for result in results.iter_mut() {
        let Some(result_class) = result.class_info.as_ref() else {
            continue;
        };
        let same_class = receiver.iter().any(|rt| {
            rt.class_info
                .as_ref()
                .is_some_and(|ci| ci.fqn() == result_class.fqn())
        });
        if same_class {
            result.factory_count = count;
        }
    }
}

/// Tag the factory a static call opened with the count it was opened
/// with: `Model::factory(3)`, `UserFactory::times(3)`,
/// `UserFactory::new()`.
///
/// The method name is checked first, so the class-hierarchy walk that
/// confirms the result really is a factory only runs for the three names
/// that could have opened one.
pub(crate) fn tag_static_factory_call(
    results: &mut [ResolvedType],
    method: &str,
    first_arg: Option<&str>,
    first_arg_type: &dyn Fn() -> Option<PhpType>,
    ctx: &ResolutionCtx<'_>,
) {
    if !matches!(method, "factory" | "times" | "new") {
        return;
    }
    let mut count = None;
    for result in results.iter_mut() {
        if result
            .class_info
            .as_ref()
            .is_some_and(|ci| extends_eloquent_factory(ci, ctx.class_loader))
        {
            let count =
                *count.get_or_insert_with(|| static_count_state(method, first_arg, first_arg_type));
            if count == FactoryCount::Unknown {
                return;
            }
            result.factory_count = count;
        }
    }
}

/// Resolve `create()` / `createQuietly()` / `make()` on an Eloquent
/// factory to the type the call-site chain actually builds.
///
/// Returns `None` — leaving the declared return type alone — when the
/// method is not count-conditional, the chain's count state is unknown,
/// the receiver is not a factory, the factory declares the method itself,
/// or the model type cannot be determined.
pub(crate) fn resolve_factory_count_return(
    receiver: &SubjectExpr,
    method_name: &str,
    owners: &[ResolvedType],
    ctx: &ResolutionCtx<'_>,
) -> Option<(Vec<Arc<ClassInfo>>, PhpType)> {
    if !is_count_conditional_method(method_name) {
        return None;
    }
    // Read the count off the chain before touching the class loader.  The
    // walk is pure syntax, and it rules out the great majority of the
    // `create()`/`make()` calls in a codebase — every builder that is not
    // a factory — without a hierarchy walk per owner.
    resolve_for_count(
        effective_count(chain_count(receiver), owners),
        method_name,
        owners,
        ctx,
    )
}

/// The count state to resolve a `create()`/`make()` call with.
///
/// The chain's own syntax answers first, since it is the cheaper of the
/// two and settles every chain written out in one expression.  What it
/// cannot see — the head of a chain that reaches back through a variable,
/// a parameter, or a property — the receiver's own resolved state can.
fn effective_count(from_syntax: FactoryCount, owners: &[ResolvedType]) -> FactoryCount {
    match from_syntax {
        FactoryCount::Unknown => carried_count(owners),
        settled => settled,
    }
}

/// [`resolve_factory_count_return`] for the AST-walking resolution path.
///
/// Assignments, arguments, property writes and returns reach method calls
/// as [`Expression`] nodes rather than parsed subject strings, so they
/// read the chain's count with [`chain_count_ast`] and share everything
/// downstream of it.
pub(crate) fn resolve_factory_count_return_ast(
    receiver: &Expression<'_>,
    method_name: &str,
    owners: &[ResolvedType],
    content: &str,
    ctx: &ResolutionCtx<'_>,
) -> Option<(Vec<Arc<ClassInfo>>, PhpType)> {
    if !is_count_conditional_method(method_name) {
        return None;
    }
    resolve_for_count(
        effective_count(chain_count_ast(receiver, content), owners),
        method_name,
        owners,
        ctx,
    )
}

/// Pick the type a count-conditional factory call builds, given the count
/// state its receiver chain established.
fn resolve_for_count(
    count: FactoryCount,
    method_name: &str,
    owners: &[ResolvedType],
    ctx: &ResolutionCtx<'_>,
) -> Option<(Vec<Arc<ClassInfo>>, PhpType)> {
    if count == FactoryCount::Unknown {
        return None;
    }

    let factory = owners.iter().find_map(|rt| {
        rt.class_info
            .as_ref()
            .filter(|ci| extends_eloquent_factory(ci, ctx.class_loader))
    })?;

    // A factory that writes its own `create()`/`make()` keeps whatever it
    // declared — only the signature inherited from Laravel's `Factory`
    // (and the single-model stand-in PHPantom synthesizes for
    // convention-based factories) is ours to reinterpret.  The receiver
    // may already be a merged class, so the own-member check goes through
    // the loader, which hands back the class as parsed.
    let fqn = factory.fqn();
    if (ctx.class_loader)(fqn.as_str()).is_some_and(|raw| raw.get_method_ci(method_name).is_some())
    {
        return None;
    }

    let model = factory_model_type(factory, ctx.class_loader)?;

    // The call has to resolve to *some* inherited or synthesized method;
    // a factory with no `create()` at all gets no return type from us.
    let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
        factory,
        ctx.class_loader,
        ctx.resolved_class_cache,
    );
    merged.get_method_ci(method_name)?;

    let resolved = match count {
        FactoryCount::Many => {
            let collection = PhpType::generic(
                ELOQUENT_COLLECTION_FQN,
                vec![PhpType::named(atom("int")), model],
            );
            super::replace_eloquent_collections_in_type(&collection, ctx.class_loader)
                .unwrap_or(collection)
        }
        FactoryCount::One | FactoryCount::Unknown => model,
    };

    let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        &resolved,
        fqn.as_str(),
        ctx.all_classes,
        ctx.class_loader,
    );
    if classes.is_empty() {
        return None;
    }

    Some((classes, resolved))
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "factory_count_tests.rs"]
mod tests;
