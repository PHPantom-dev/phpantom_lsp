//! Receiver mutation: a call that changes the state of the object it is
//! called on invalidates what the walker knows about that object.

use super::*;

use crate::atom::bytes_to_str;
use crate::type_engine::types::narrowing;

/// Drop what a state-changing call could have altered behind its
/// receiver.
///
/// A check is only worth remembering while the thing it was made about
/// still holds. `if ($stmt->fetch('id') !== false)` proves something about
/// `$stmt`'s current row; `$stmt->execute()` moves to another one, so the
/// proof describes a state the program has left. Every synthetic key read
/// through the receiver goes with it.
///
/// Which calls count is [`callee_changes_state`]'s decision, and getting it
/// wrong the other way is what made guard-then-read fail: a second getter
/// on the same object (`$r->getFileName() !== false` proved, then
/// `$r->getDocComment()` read) is not an event that unproves the first.
pub(crate) fn process_receiver_mutation<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let mut receivers: Vec<(String, Option<String>)> = Vec::new();
    collect_impure_call_receivers(expr, scope, ctx, &mut receivers);
    for (receiver, made) in receivers {
        scope.invalidate_receiver_state(&receiver, made.as_deref());
    }
}

/// Walk `expr` for method calls whose receiver has state worth
/// invalidating, collecting each receiver key at most once.
///
/// Each entry pairs the receiver with the key of the call made on it, so
/// the invalidation can keep the proof about that very call.
fn collect_impure_call_receivers<'b>(
    expr: &'b Expression<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    out: &mut Vec<(String, Option<String>)>,
) {
    match expr {
        Expression::Parenthesized(inner) => {
            collect_impure_call_receivers(inner.expression, scope, ctx, out)
        }
        Expression::Assignment(assignment) => {
            collect_impure_call_receivers(assignment.rhs, scope, ctx, out)
        }
        Expression::Binary(bin) => {
            collect_impure_call_receivers(bin.lhs, scope, ctx, out);
            collect_impure_call_receivers(bin.rhs, scope, ctx, out);
        }
        Expression::UnaryPrefix(unary) => {
            collect_impure_call_receivers(unary.operand, scope, ctx, out)
        }
        Expression::Call(call) => {
            let (object, method, args) = match call {
                Call::Method(mc) => (Some(mc.object), Some(&mc.method), &mc.argument_list),
                Call::NullSafeMethod(mc) => (Some(mc.object), Some(&mc.method), &mc.argument_list),
                Call::Function(fc) => (None, None, &fc.argument_list),
                Call::StaticMethod(sc) => (None, None, &sc.argument_list),
            };
            // A chained call's receiver is itself a call, and an argument
            // may hold one too, so both are searched.
            if let Some(object) = object {
                collect_impure_call_receivers(object, scope, ctx, out);
            }
            for arg in args.arguments.iter() {
                collect_impure_call_receivers(arg.value(), scope, ctx, out);
            }

            let (Some(object), Some(ClassLikeMemberSelector::Identifier(ident))) = (object, method)
            else {
                return;
            };
            let Some(receiver) = narrowing::expr_to_subject_key(object) else {
                return;
            };
            // Nothing is recorded through this receiver, so there is
            // nothing a call on it could invalidate.  Checked before the
            // class lookup below, which is the expensive half: the great
            // majority of calls reach this and stop.
            if !scope_reads_receiver(scope, &receiver) {
                return;
            }
            if !callee_changes_state(object, bytes_to_str(ident.value), scope, ctx) {
                return;
            }
            let made = narrowing::expr_to_subject_key(expr);
            if !out.iter().any(|(r, m)| *r == receiver && *m == made) {
                out.push((receiver, made));
            }
        }
        _ => {}
    }
}

/// Whether the scope holds any synthetic key read through `receiver`.
fn scope_reads_receiver(scope: &ScopeState, receiver: &str) -> bool {
    let reads = |key: &str| {
        key != receiver && crate::type_engine::types::narrowing::key_reads_variable(key, receiver)
    };
    scope.locals.keys().any(|k| reads(k))
        || scope
            .assertions
            .values()
            .any(|checks| checks.iter().any(|c| reads(&c.subject)))
}

/// Whether calling `object->method_name()` should be read as changing
/// state behind the receiver.
///
/// Three signals, in order of authority. `@pure` / `@phpstan-pure` /
/// `@psalm-pure` promises nothing changed; `@impure` / `@phpstan-impure` /
/// `@psalm-impure` promises something did. With neither, the return type
/// decides: a method that hands back nothing was called for its effect,
/// while one that computes a value is read as computing it. That is the
/// same rule PHPStan applies (`MethodReflection::hasSideEffects()`), and
/// the reason it matters is that guard-then-read on two getters of the same
/// object is ordinary code — treating the second getter as a write would
/// unprove the guard on the first for no reason.
///
/// An unresolvable receiver or method counts as changing state: dropping a
/// check costs precision, keeping a stale one costs correctness.
fn callee_changes_state(
    object: &Expression<'_>,
    method_name: &str,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let class_names: Vec<String> = match object {
        Expression::Variable(Variable::Direct(dv)) if dv.name == b"$this" => {
            vec![ctx.current_class.name.to_string()]
        }
        _ => {
            let Some(key) = narrowing::expr_to_subject_key(object) else {
                return true;
            };
            scope
                .get(&key)
                .iter()
                .filter_map(|rt| rt.type_string.base_name().map(str::to_owned))
                .collect()
        }
    };
    if class_names.is_empty() {
        return true;
    }
    class_names.iter().any(|name| {
        let Some(cls) = (ctx.class_loader)(name) else {
            return true;
        };
        let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
            &cls,
            ctx.class_loader,
            ctx.resolved_class_cache,
        );
        let Some(method) = merged.get_method(method_name) else {
            return true;
        };
        if method.is_pure {
            return false;
        }
        method.is_impure
            || method
                .return_type
                .as_ref()
                .is_some_and(|rt| rt.is_void() || rt.is_never())
    })
}

pub(crate) fn receiver_class_names(
    expr: &Expression<'_>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<String> {
    match expr {
        Expression::Variable(Variable::Direct(dv)) => {
            let var_name = bytes_to_str(dv.name);
            if var_name == "$this" && !ctx.current_class.name.is_empty() {
                return vec![
                    ctx.current_class.name.to_string(),
                    ctx.current_class.fqn().to_string(),
                ];
            }
            scope
                .get(var_name)
                .iter()
                .filter_map(|rt| rt.class_info.as_ref())
                .flat_map(|cls| [cls.name.to_string(), cls.fqn().to_string()])
                .collect()
        }
        Expression::Parenthesized(inner) => receiver_class_names(inner.expression, scope, ctx),
        _ => Vec::new(),
    }
}

pub(crate) fn static_receiver_class_names(
    expr: &Expression<'_>,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<String> {
    match expr {
        Expression::Self_(_) | Expression::Static(_) if !ctx.current_class.name.is_empty() => {
            vec![
                ctx.current_class.name.to_string(),
                ctx.current_class.fqn().to_string(),
            ]
        }
        Expression::Parent(_) => ctx
            .current_class
            .parent_class
            .map(|name| vec![name.to_string()])
            .unwrap_or_default(),
        Expression::Identifier(ident) => vec![bytes_to_str(ident.value()).to_string()],
        Expression::Parenthesized(inner) => static_receiver_class_names(inner.expression, ctx),
        _ => Vec::new(),
    }
}

pub(crate) fn class_name_matches_receiver(name: &[u8], receiver_names: &[String]) -> bool {
    let class_name = bytes_to_str(name);
    receiver_names.iter().any(|receiver| {
        receiver.eq_ignore_ascii_case(class_name)
            || crate::util::short_name(receiver).eq_ignore_ascii_case(class_name)
    })
}
