//! Fluent method chains, folded outward from their base instead of
//! recursing once per link.

use mago_syntax::cst::*;

use crate::type_engine::resolver::VarResolutionCtx;
use crate::types::ResolvedType;

use super::method_calls::{
    MethodReceiver, resolve_method_call_on_receiver, resolve_method_receiver,
};
use super::nullsafe::{ReceiverNullability, apply_nullsafe_short_circuit};
use super::scope_reads::narrowed_call;
use super::{calls, peel_type_transparent};

/// One link of a fluent chain: `->method(args)` applied to `object`.
struct ChainLink<'b> {
    /// The call expression the link was peeled from, which is what a
    /// check written on the call is keyed under.
    call: &'b Expression<'b>,
    object: &'b Expression<'b>,
    method: &'b ClassLikeMemberSelector<'b>,
    argument_list: &'b ArgumentList<'b>,
    nullsafe: bool,
}

/// Resolve a method call, walking its receiver spine iteratively.
///
/// `$x->a()->b()->c()` nests one method call per link, and each link's
/// receiver is the link before it — a chain, not a tree.  Resolving the
/// outermost call and recursing into its receiver therefore costs a stack
/// frame per link, so a long enough chain overflows the stack whatever
/// size it is given.  Hand-written code stays short, but generated query
/// builders and generated API clients do not.
///
/// So the spine is peeled into a `Vec` and folded outward from the base
/// instead: only the innermost link resolves an object expression (which
/// is where `$this`, a bare variable, or `(new Foo)` is read), and every
/// link after it is handed the previous link's result as its receiver.
///
/// A chain that mixes in property reads (`$x->a()->prop->b()`) breaks the
/// spine at the property, which resolves through the normal recursive
/// path; only the method links either side of it are flattened.
pub(super) fn resolve_method_chain<'b>(
    expr: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let mut links: Vec<ChainLink<'b>> = Vec::new();
    let mut current = expr;
    loop {
        let peeled = peel_type_transparent(current);
        let link = match peeled {
            Expression::Call(Call::Method(call)) => ChainLink {
                call: peeled,
                object: call.object,
                method: &call.method,
                argument_list: &call.argument_list,
                nullsafe: false,
            },
            Expression::Call(Call::NullSafeMethod(call)) => ChainLink {
                call: peeled,
                object: call.object,
                method: &call.method,
                argument_list: &call.argument_list,
                nullsafe: true,
            },
            // The base of the spine.  It is left to the innermost link,
            // which knows how to read it as a receiver.
            _ => break,
        };
        current = link.object;
        links.push(link);
    }

    let mut receiver: Option<MethodReceiver> = None;
    for link in links.iter().rev() {
        let (owners, receiver_resolved) =
            receiver.unwrap_or_else(|| resolve_method_receiver(link.object, ctx));
        let nullability = ReceiverNullability::of(&receiver_resolved);
        let mut resolved = resolve_method_call_on_receiver(
            link.object,
            link.method,
            link.argument_list,
            Some((owners, receiver_resolved)),
            ctx,
        );
        calls::finish_return_constant_operands(&mut resolved, ctx);
        apply_nullsafe_short_circuit(&mut resolved, nullability, link.nullsafe, link.object);
        // A check written on the call itself (`if ($h->get() instanceof
        // Foo)`, `if ($this->option('from') !== null)`) is keyed under the
        // call's own text, so a later occurrence of that text reads the
        // narrowed type instead of the method's declared return type.
        if let Some(narrowed) = narrowed_call(link.call, ctx) {
            resolved = narrowed;
        }
        receiver = Some((ResolvedType::into_arced_classes(resolved.clone()), resolved));
    }
    // `links` always holds at least the call this was entered on.
    receiver.map(|(_, resolved)| resolved).unwrap_or_default()
}
