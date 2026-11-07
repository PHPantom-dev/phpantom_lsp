//! A Doctrine query builder held in a variable: a call on it changes the
//! builder the variable still holds, so the variable takes the DQL the call
//! adds.

use super::*;

use crate::atom::bytes_to_str;
use crate::doctrine::query;

/// Re-bind the variable a query builder call is made on to the builder the
/// call leaves, when the variable holds a builder that carries the DQL it
/// has built.
///
/// `$qb->andWhere(…)` written as a statement changes `$qb` without
/// assigning it, the way a `@phpstan-self-out` method changes its
/// receiver.  The call may sit at the end of a chain of builder calls on
/// the variable (`$qb->select('u')->from(User::class, 'u')`): the walk
/// reaches each call in the chain in turn, so each adds its part to what
/// the one before it left.  A call whose effect on the DQL is not known
/// leaves the variable a builder of unknown DQL.
pub(crate) fn process_dql_builder_call<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let (object, method, argument_list) = match expr {
        Expression::Call(Call::Method(mc)) => (mc.object, &mc.method, &mc.argument_list),
        Expression::Call(Call::NullSafeMethod(mc)) => (mc.object, &mc.method, &mc.argument_list),
        _ => return,
    };
    let ClassLikeMemberSelector::Identifier(ident) = method else {
        return;
    };
    let method_name = bytes_to_str(ident.value);
    // `getQuery()` and the other readers leave the builder as it was.
    if method_name.eq_ignore_ascii_case("getQuery") {
        return;
    }
    let Some(var_name) = chain_root(object) else {
        return;
    };
    let before = scope.get(var_name);
    // Most chains are on something else, so the names in between are only
    // read once the variable is known to hold a builder.
    if !before.iter().any(|rt| rt.type_string.has_dql_builder()) || !is_builder_chain(object) {
        return;
    }
    let before = before.to_vec();

    let scope_resolver = scope.snapshot_resolver();
    let var_ctx = ctx.var_ctx_for_with_scope(
        var_name,
        ctx.cursor_offset,
        &scope_resolver,
        Some(scope.proofs()),
    );
    let Some(after) = crate::type_engine::variable::rhs_resolution::try_resolve_dql_builder_call(
        method_name,
        &before,
        argument_list,
        &var_ctx,
    ) else {
        return;
    };
    scope.set(var_name, after);
}

/// The variable a chain of method calls ending at `object` is made on:
/// `$qb` for `$qb` itself and for `$qb->select(…)->from(…)`.  `$this` holds
/// no builder.
fn chain_root<'b>(mut object: &'b Expression<'b>) -> Option<&'b str> {
    loop {
        match object {
            Expression::Variable(Variable::Direct(dv)) => {
                let name = bytes_to_str(dv.name);
                return (name != "$this").then_some(name);
            }
            Expression::Parenthesized(parenthesized) => object = parenthesized.expression,
            Expression::Call(Call::Method(mc)) => object = mc.object,
            _ => return None,
        }
    }
}

/// Whether every call in the chain ending at `object` is a builder call
/// that returns the builder it was made on, so the chain's result is the
/// builder its root variable holds.
fn is_builder_chain(mut object: &Expression<'_>) -> bool {
    loop {
        match object {
            Expression::Parenthesized(parenthesized) => object = parenthesized.expression,
            Expression::Call(Call::Method(mc)) => {
                let ClassLikeMemberSelector::Identifier(ident) = &mc.method else {
                    return false;
                };
                if !query::is_replayed(bytes_to_str(ident.value)) {
                    return false;
                }
                object = mc.object;
            }
            _ => return true,
        }
    }
}
