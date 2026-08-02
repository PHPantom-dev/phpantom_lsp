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
//! branch the call actually produces, mirroring Larastan's conditional
//! return type extensions.  Only the syntactic chain is inspected: when a
//! factory travels through a variable (`$factory = User::factory(); …`)
//! the count state is unknown and the single-model branch is used, which
//! is the common case.

use std::sync::Arc;

use crate::atom::atom;
use crate::php_type::PhpType;
use crate::type_engine::conditional_resolution::split_text_args;
use crate::type_engine::resolver::ResolutionCtx;
use crate::type_engine::subject_expr::SubjectExpr;
use crate::types::{ClassInfo, ELOQUENT_COLLECTION_FQN, ResolvedType};

use super::factory::{extends_eloquent_factory, factory_model_type};

/// How many models a factory chain builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FactoryCount {
    /// No count was set, or a previously set count was cleared with
    /// `count(null)`.
    One,
    /// `count()`, `times()`, or an integer `factory($count)` argument set
    /// a count, so the chain builds a collection.
    Many,
}

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
/// over, and anything that is not a call expression (a variable, a
/// property) ends the walk with [`FactoryCount::One`].
pub(crate) fn chain_count(receiver: &SubjectExpr) -> FactoryCount {
    let mut current = receiver;
    let mut depth = 0u32;

    // Chains are short in practice; the bound only guards against a
    // pathological expression.
    while depth < 64 {
        depth += 1;
        let SubjectExpr::CallExpr { callee, args_text } = current else {
            return FactoryCount::One;
        };
        match callee.as_ref() {
            SubjectExpr::MethodCall { base, method } => {
                if let Some(state) = instance_count_state(method, args_text) {
                    return state;
                }
                current = base;
            }
            // A static call is the head of the chain: `Model::factory()`,
            // `UserFactory::new()`, `UserFactory::times(3)`.
            SubjectExpr::StaticMethodCall { method, .. } => {
                return static_count_state(method, args_text);
            }
            _ => return FactoryCount::One,
        }
    }

    FactoryCount::One
}

/// Count state contributed by an instance call in the chain, or `None`
/// when the call does not touch the count.
fn instance_count_state(method: &str, args_text: &str) -> Option<FactoryCount> {
    match method {
        // `count(?int $count)` — the only way to clear a count is to pass
        // a literal `null`.  A non-literal argument is assumed to be the
        // integer the parameter asks for.
        "count" => Some(match split_text_args(args_text).first() {
            None => FactoryCount::One,
            Some(arg) => {
                if arg.trim().eq_ignore_ascii_case("null") {
                    FactoryCount::One
                } else {
                    FactoryCount::Many
                }
            }
        }),
        // `times(int $count)` cannot be given null.
        "times" => Some(FactoryCount::Many),
        _ => None,
    }
}

/// Count state contributed by the static call that opens the chain.
fn static_count_state(method: &str, args_text: &str) -> FactoryCount {
    match method {
        // `Model::factory($count)` forwards `$count` to `count()` only
        // when it is numeric; an array or callable is state, not a count.
        "factory" if first_arg_is_numeric_literal(args_text) => FactoryCount::Many,
        "times" => FactoryCount::Many,
        _ => FactoryCount::One,
    }
}

/// Whether the first argument is a literal Laravel would consider numeric.
///
/// Only literals count: a variable could hold an array of state just as
/// easily as an integer, and guessing wrong would turn a single model
/// into a collection.
fn first_arg_is_numeric_literal(args_text: &str) -> bool {
    let Some(first) = split_text_args(args_text).first().map(|a| a.trim()) else {
        return false;
    };
    let unquoted = crate::text_scan::unquote_php_string(first).unwrap_or(first);
    !unquoted.is_empty() && unquoted.parse::<f64>().is_ok()
}

/// Resolve `create()` / `createQuietly()` / `make()` on an Eloquent
/// factory to the type the call-site chain actually builds.
///
/// Returns `None` — leaving the declared return type alone — when the
/// method is not count-conditional, the receiver is not a factory, the
/// factory declares the method itself, or the model type cannot be
/// determined.
pub(crate) fn resolve_factory_count_return(
    receiver: &SubjectExpr,
    method_name: &str,
    owners: &[ResolvedType],
    ctx: &ResolutionCtx<'_>,
) -> Option<(Vec<Arc<ClassInfo>>, PhpType)> {
    if !is_count_conditional_method(method_name) {
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

    let resolved = match chain_count(receiver) {
        FactoryCount::One => model,
        FactoryCount::Many => {
            let collection = PhpType::generic(
                ELOQUENT_COLLECTION_FQN,
                vec![PhpType::named(atom("int")), model],
            );
            super::replace_eloquent_collections_in_type(&collection, ctx.class_loader)
                .unwrap_or(collection)
        }
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
