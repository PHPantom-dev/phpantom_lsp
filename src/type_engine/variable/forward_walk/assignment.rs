use super::*;
use std::collections::HashMap;

use mago_span::HasSpan;
use mago_syntax::cst::argument::Argument;

use crate::atom::{Atom, atom, bytes_to_str};
use crate::docblock::type_strings::split_type_token;
use crate::parser::with_parsed_program;
use crate::php_type::{LiteralValue, PhpType, TypeKind};
use crate::type_engine::call_resolution::{OutParamCallee, effective_out_type};
use crate::type_engine::resolver::VarResolutionCtx;
use crate::type_engine::types::narrowing;
use crate::types::{ClassInfo, ResolvedType};

use super::super::rhs_resolution::{
    ArithmeticOpKind, infer_addition_result_type, infer_arithmetic_result_type,
};

// ─── Statement processing ───────────────────────────────────────────────────

/// Process a single statement, updating `scope` with any variable
/// assignments, narrowing, or control-flow effects.
pub(crate) fn process_statement<'b>(
    stmt: &'b Statement<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // An expression statement runs its own `@var` handling, which has
    // extra rules (the LHS is left alone while the cursor sits in the
    // RHS, a scalar RHS blocks a class override).  Every other statement
    // kind only ever sees standalone annotations.
    if !matches!(stmt, Statement::Expression(_)) {
        let stmt_offset = stmt.span().start.offset;
        if apply_standalone_var_docblocks(stmt_offset, scope, ctx) {
            // The diagnostic scope snapshot at this offset was recorded
            // by the caller *before* this call, so it still holds the
            // pre-docblock scope. Re-record it now so that a lookup for
            // an expression inside this same statement (e.g. `echo
            // $v->method()` right after a standalone `@var $v` block)
            // sees the type the docblock just applied instead of
            // falling through to the stale snapshot.
            record_scope_snapshot(stmt_offset, scope);
        }
    }

    match stmt {
        Statement::Expression(expr_stmt) => {
            process_expression_statement(expr_stmt, scope, ctx);
        }
        Statement::Foreach(foreach) => {
            process_foreach(foreach, scope, ctx);
        }
        Statement::If(if_stmt) => {
            process_if(if_stmt, stmt, scope, ctx);
        }
        Statement::While(while_stmt) => {
            process_while(while_stmt, scope, ctx);
        }
        Statement::For(for_stmt) => {
            process_for(for_stmt, scope, ctx);
        }
        Statement::DoWhile(dw) => {
            process_do_while(dw, scope, ctx);
        }
        Statement::Try(try_stmt) => {
            process_try(try_stmt, scope, ctx);
        }
        Statement::Switch(switch) => {
            process_switch(switch, scope, ctx);
        }
        Statement::Block(block) => {
            walk_body_forward(block.statements.iter(), scope, ctx);
        }
        // A jump out of a loop carries the types it holds *here* to the
        // loop's join, not to the statement that follows it.  The loop
        // that owns the edge folds it back in.
        Statement::Break(brk) => {
            record_exit_edge(exit_level(brk.level), true, scope);
        }
        Statement::Continue(cont) => {
            record_exit_edge(exit_level(cont.level), false, scope);
        }
        Statement::Unset(unset_stmt) => {
            for val in unset_stmt.values.iter() {
                match val {
                    Expression::Variable(Variable::Direct(dv)) => {
                        scope.remove(bytes_to_str(dv.name));
                    }
                    // `unset($arr['key'])` removes one element rather than
                    // the whole variable, so a `non-empty-array` or shape
                    // type must lose whatever emptiness guarantee that
                    // element was supplying — otherwise a later `foreach`
                    // over the same array still assumes its body runs.
                    Expression::ArrayAccess(array_access) => {
                        if let Some((base_name, key_chain)) =
                            super::super::resolution::extract_nested_array_access_chain(
                                array_access,
                            )
                        {
                            let Some(base_type) = scope
                                .get(&base_name)
                                .last()
                                .map(|rt| rt.type_string.clone())
                            else {
                                continue;
                            };
                            let keys: Vec<Option<String>> = key_chain
                                .iter()
                                .map(|idx| {
                                    super::super::resolution::extract_array_key_for_shape(idx)
                                })
                                .collect();
                            let updated = super::super::resolution::apply_nested_array_unset(
                                &base_type, &keys,
                            );
                            scope.set(&base_name, vec![ResolvedType::from_type_string(updated)]);
                        }
                    }
                    _ => {}
                }
            }
        }
        Statement::Namespace(ns) => {
            walk_body_forward(ns.statements().iter(), scope, ctx);
        }
        Statement::Global(global) => {
            for var in global.variables.iter() {
                if let Variable::Direct(dv) = var {
                    let var_name = bytes_to_str(dv.name).to_string();
                    if let Some(top_scope) = &ctx.top_level_scope {
                        if let Some(types) = top_scope.get(&atom(&var_name)) {
                            scope.set(&var_name, types.clone());
                        } else {
                            scope.set_empty(&var_name);
                        }
                    } else {
                        scope.set_empty(&var_name);
                    }
                }
            }
        }
        Statement::Return(ret) => {
            if let Some(val) = ret.value {
                process_assignment_expr(val, scope, ctx);

                // Record `&&` and `||` chain snapshots so that member
                // accesses after an instanceof/null guard see the
                // narrowed type.  E.g. `return $x instanceof Foo && $x->bar()`
                record_short_circuit_snapshots(val, scope, ctx);

                // Record narrowed snapshots inside match(true) arms
                // and ternary instanceof branches.
                if is_diagnostic_scope_active() {
                    record_match_ternary_snapshots(val, scope, ctx);
                }
            }

            // A `return` leaves the body with the types it holds *here*.
            // For a closure walked to see what it writes to its `use (&$x)`
            // captures, that state is part of the exit state even though
            // the branch merge drops the branch it sits in.
            record_return_edge(scope);
        }
        // An echoed expression narrows exactly the way a returned one
        // does: `echo $s ? strtoupper($s) : '';` proves `$s` a string
        // inside the arm that runs it.  Blade compiles every `{{ … }}`
        // to an `echo`, so a template's guards live here.
        Statement::Echo(echo) => {
            for value in echo.values.iter() {
                process_echoed_expression(value, scope, ctx);
            }
        }
        Statement::EchoTag(echo) => {
            for value in echo.values.iter() {
                process_echoed_expression(value, scope, ctx);
            }
        }
        _ => {}
    }
}

/// Apply one echoed expression's effects to the scope: the assignments it
/// makes, and the narrowing its short-circuit chains, ternaries and
/// `match (true)` arms prove for the code inside them.
fn process_echoed_expression<'b>(
    value: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    process_assignment_expr(value, scope, ctx);
    record_short_circuit_snapshots(value, scope, ctx);
    if is_diagnostic_scope_active() {
        record_match_ternary_snapshots(value, scope, ctx);
    }
}

// ─── Expression statement handling ──────────────────────────────────────────

/// Process an expression statement: handle assignments, assert narrowing,
/// pass-by-reference type inference, etc.
pub(crate) fn process_expression_statement<'b>(
    expr_stmt: &'b ExpressionStatement<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // `($a = expr);` is a parenthesized expression statement, written by
    // hand or produced by the Blade preprocessor for `@php($a = expr)`.
    // The statement offset stays on the outer expression so a preceding
    // `@var` docblock is still found, but everything that inspects the
    // expression's shape works on the inner one.
    let outer = expr_stmt.expression;
    let expr = unwrap_parens(outer);

    // Try inline `/** @var Type $x */` override first.
    // A `@var` block is authoritative over the assignment it annotates,
    // so that one pass is skipped.  Only that one: everything else the
    // statement carries — the ternary and short-circuit snapshots, assert
    // narrowing, by-ref captures — still has to run, now against the scope
    // the docblock just established.  Returning outright here left
    // `takesString($m->virtual ? $m->virtual : $m->title)` under a
    // preceding `/** @var Model $m */` with no branch snapshots at all, so
    // the truthy arm read the property's declared nullable type.
    // What the assignment target held before the docblock retyped it. A
    // `@var` above an assignment describes the variable *after* it runs,
    // so the right-hand side still reads the old value:
    // `/** @var Base $b */ $b = $b->inner;` resolves `$b->inner` against
    // whatever `$b` was on the way in, not against `Base`.
    let assigned_var = match expr {
        Expression::Assignment(assignment) => match assignment.lhs {
            Expression::Variable(Variable::Direct(dv)) => {
                let name = bytes_to_str(dv.name).to_string();
                let before = scope.get(&name).to_vec();
                Some((name, before))
            }
            _ => None,
        },
        _ => None,
    };

    let skip_assignment =
        match try_process_inline_var_override(expr, stmt_offset(outer), scope, ctx) {
            VarOverrideResult::NamedVar => {
                // Re-record the scope snapshot at this expression's offset
                // so that variable lookups within the same statement (e.g.
                // `$app` in `$client = $app->make(...)` where a preceding
                // `@var` block declared `$app`) see the updated types.
                // The snapshot recorded by `walk_body_for_diagnostics` at
                // the statement start was taken *before* the `@var`
                // override was applied.  The assignment target is put back
                // to its incoming type for that snapshot alone, so the
                // right-hand side is read the way it was written.
                match assigned_var {
                    Some((ref name, ref before)) if !before.is_empty() => {
                        let mut rhs_scope = scope.clone();
                        rhs_scope.set(name, before.clone());
                        record_scope_snapshot(stmt_offset(outer), &rhs_scope);
                    }
                    _ => record_scope_snapshot(stmt_offset(outer), scope),
                }
                true
            }
            // A `@var Type` (no variable name) was applied to the assignment
            // LHS.  The snapshot is deliberately *not* re-recorded: the LHS
            // variable must not be visible to lookups inside the RHS.
            VarOverrideResult::NoVar => true,
            VarOverrideResult::None => false,
        };

    // Record intermediate scope snapshots within `&&` and `||` chains
    // so that member accesses after an instanceof/null guard see the
    // narrowed type.  E.g. `$x instanceof Foo && $x->bar()` as an
    // expression statement.
    record_short_circuit_snapshots(expr, scope, ctx);

    // Record narrowed snapshots inside match(true) arms and ternary
    // instanceof branches within this expression.
    if is_diagnostic_scope_active() {
        record_match_ternary_snapshots(expr, scope, ctx);
    }

    if !skip_assignment {
        process_assignment_expr(expr, scope, ctx);
    }

    process_by_ref_closure_captures(expr, scope, ctx);

    process_pass_by_ref(expr, scope, ctx);

    // Sits between the passes that *read* the statement's expressions and
    // the passes that record what it *proves*.  The reads above see the
    // state the call itself saw; a proof below describes the value the
    // call handed back, so it must outlive the call's own invalidation
    // (`assertNotNull($holder->find('a'))` proves something about the very
    // call it makes).
    process_receiver_mutation(expr, scope, ctx);

    process_assert_narrowing(expr, scope, ctx);

    process_self_out_narrowing(expr, scope, ctx);

    // Process increment/decrement: $a++, ++$a, $a--, --$a.
    process_increment_decrement(expr, scope, ctx);
}

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

pub(crate) fn process_by_ref_closure_captures<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    match expr {
        Expression::Call(call) => {
            // The receiver runs before the arguments, and a chained call
            // (`$db->connect()->transaction(...)`) hides closure arguments
            // inside its receiver expression, so recurse into it first.
            match call {
                Call::Method(mc) => process_by_ref_closure_captures(mc.object, scope, ctx),
                Call::NullSafeMethod(mc) => process_by_ref_closure_captures(mc.object, scope, ctx),
                _ => {}
            }

            // `(function () use (&$x) { ... })()` runs the closure right
            // here, so its final variable state replaces the outer one.
            if let Call::Function(fc) = call
                && let Expression::Closure(closure) = unwrap_parens(fc.function)
            {
                process_by_ref_closure_capture(closure, scope, ctx, true);
            }

            let args = match call {
                Call::Function(fc) => &fc.argument_list,
                Call::Method(mc) => &mc.argument_list,
                Call::NullSafeMethod(mc) => &mc.argument_list,
                Call::StaticMethod(sc) => &sc.argument_list,
            };
            let mut next_positional = 0usize;
            for arg in args.arguments.iter() {
                let (arg_expr, selector) = arg_expr_and_selector(arg, &mut next_positional);
                if let Expression::Closure(closure) = arg_expr {
                    let certain = call_invokes_arg_immediately(call, &selector, scope, ctx);
                    process_by_ref_closure_capture(closure, scope, ctx, certain);
                } else {
                    process_by_ref_closure_captures(arg_expr, scope, ctx);
                }
            }
        }
        // A closure that is defined but not provably invoked (stored in a
        // variable, passed somewhere opaque) may still run any time later,
        // so the types it assigns are unioned into the captured variables.
        Expression::Closure(closure) => {
            process_by_ref_closure_capture(closure, scope, ctx, false);
        }
        // `new Wrapper(function () use (&$x) { … })` hands the closure to an
        // object that invokes it later (or never), which is the
        // widen-don't-replace case the `Closure` arm handles — the point is
        // that the capture is seen at all instead of the mutation going
        // missing.  Same for a closure inside an array literal.
        Expression::Instantiation(instantiation) => {
            if let Some(ref args) = instantiation.argument_list {
                for arg in args.arguments.iter() {
                    process_by_ref_closure_captures(arg.value(), scope, ctx);
                }
            }
        }
        Expression::Array(arr) => {
            for elem in arr.elements.iter() {
                if let Some(value) = array_element_value(elem) {
                    process_by_ref_closure_captures(value, scope, ctx);
                }
            }
        }
        Expression::LegacyArray(arr) => {
            for elem in arr.elements.iter() {
                if let Some(value) = array_element_value(elem) {
                    process_by_ref_closure_captures(value, scope, ctx);
                }
            }
        }
        Expression::Parenthesized(inner) => {
            process_by_ref_closure_captures(inner.expression, scope, ctx);
        }
        Expression::Assignment(assignment) => {
            process_by_ref_closure_captures(assignment.rhs, scope, ctx);
        }
        _ => {}
    }
}

/// The value expression of an array element, ignoring its key.
fn array_element_value<'b>(elem: &'b ArrayElement<'b>) -> Option<&'b Expression<'b>> {
    match elem {
        ArrayElement::KeyValue(kv) => Some(kv.value),
        ArrayElement::Value(v) => Some(v.value),
        ArrayElement::Variadic(v) => Some(v.value),
        ArrayElement::Missing(_) => None,
    }
}

/// Whether a call provably invokes its callable argument before
/// returning, so a by-ref capture's final state can *replace* the outer
/// variable rather than widen it.
///
/// Follows PHPStan's defaults: function callable parameters are
/// immediate unless tagged `@param-later-invoked-callable`; method
/// callable parameters are later-invoked unless tagged
/// `@param-immediately-invoked-callable`.  An unresolvable callee or
/// receiver is not proof either way, so it answers `false` and the
/// caller falls back to widening.
fn call_invokes_arg_immediately(
    call: &Call<'_>,
    selector: &ArgSelector,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    match call {
        Call::Function(fc) => {
            let Expression::Identifier(ident) = fc.function else {
                return false;
            };
            function_invokes_callable_arg_immediately(bytes_to_str(ident.value()), selector, ctx)
        }
        Call::Method(mc) => {
            let ClassLikeMemberSelector::Identifier(ident) = &mc.method else {
                return false;
            };
            let receiver_names = receiver_class_names(mc.object, scope, ctx);
            !receiver_names.is_empty()
                && method_invokes_callable_arg_immediately(
                    &receiver_names,
                    bytes_to_str(ident.value),
                    selector,
                    ctx,
                )
        }
        Call::NullSafeMethod(mc) => {
            let ClassLikeMemberSelector::Identifier(ident) = &mc.method else {
                return false;
            };
            let receiver_names = receiver_class_names(mc.object, scope, ctx);
            !receiver_names.is_empty()
                && method_invokes_callable_arg_immediately(
                    &receiver_names,
                    bytes_to_str(ident.value),
                    selector,
                    ctx,
                )
        }
        Call::StaticMethod(sc) => {
            let ClassLikeMemberSelector::Identifier(ident) = &sc.method else {
                return false;
            };
            let receiver_names = static_receiver_class_names(sc.class, ctx);
            !receiver_names.is_empty()
                && method_invokes_callable_arg_immediately(
                    &receiver_names,
                    bytes_to_str(ident.value),
                    selector,
                    ctx,
                )
        }
    }
}

/// Identifies which callee parameter a call argument fills.
///
/// Positional arguments bind by their ordinal position; named arguments
/// (`foo(callback: ...)`) bind by the declared parameter name and may
/// appear out of their natural position, so they must be resolved by
/// name rather than by their slot in the argument list.
pub(crate) enum ArgSelector {
    Position(usize),
    Name(String),
}

/// Extract a call argument's value expression and the selector that
/// identifies which parameter it fills. `next_positional` tracks the
/// running position of positional arguments (PHP requires positional
/// arguments to precede named ones, so this stays aligned with the
/// parameter list).
pub(crate) fn arg_expr_and_selector<'b>(
    arg: &'b Argument<'b>,
    next_positional: &mut usize,
) -> (&'b Expression<'b>, ArgSelector) {
    match arg {
        Argument::Positional(pos) => {
            let selector = ArgSelector::Position(*next_positional);
            *next_positional += 1;
            (pos.value, selector)
        }
        Argument::Named(named) => (
            named.value,
            ArgSelector::Name(bytes_to_str(named.name.value).to_string()),
        ),
    }
}

/// Find the callee parameter that a call argument fills, honouring both
/// positional and named binding.
pub(crate) fn select_param<'p>(
    parameters: impl Iterator<Item = &'p FunctionLikeParameter<'p>>,
    selector: &ArgSelector,
) -> Option<&'p FunctionLikeParameter<'p>> {
    match selector {
        ArgSelector::Position(idx) => parameters.into_iter().nth(*idx),
        ArgSelector::Name(name) => parameters
            .into_iter()
            .find(|param| bytes_to_str(param.variable.name).trim_start_matches('$') == name),
    }
}

pub(crate) fn function_invokes_callable_arg_immediately(
    func_name: &str,
    selector: &ArgSelector,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    with_parsed_program(
        ctx.content,
        "function_invokes_callable_arg",
        |program, _| {
            let mut stmts = Vec::new();
            flatten_namespaced_statements(program.statements.iter(), &mut stmts);
            stmts.into_iter().any(|stmt| {
                if let Statement::Function(func) = stmt
                    && bytes_to_str(func.name.value).eq_ignore_ascii_case(func_name)
                {
                    let Some(param) = select_param(func.parameter_list.parameters.iter(), selector)
                    else {
                        return false;
                    };
                    return !function_param_has_invocation_tag(
                        func.name.span.start.offset as usize,
                        ctx.content,
                        bytes_to_str(param.variable.name),
                        "param-later-invoked-callable",
                    );
                }
                false
            })
        },
    )
}

/// Flatten a statement iterator, descending into `namespace Foo;` and
/// `namespace Foo { ... }` blocks so that function and class
/// declarations inside a namespace are visited alongside top-level
/// declarations. Nearly all real-world PHP declares its symbols inside
/// a namespace, so a search that only inspects `program.statements`
/// would never find the callee.
pub(crate) fn flatten_namespaced_statements<'b>(
    statements: impl Iterator<Item = &'b Statement<'b>>,
    out: &mut Vec<&'b Statement<'b>>,
) {
    for stmt in statements {
        if let Statement::Namespace(ns) = stmt {
            flatten_namespaced_statements(ns.statements().iter(), out);
        } else {
            out.push(stmt);
        }
    }
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

pub(crate) fn method_invokes_callable_arg_immediately(
    receiver_names: &[String],
    method_name: &str,
    selector: &ArgSelector,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    with_parsed_program(ctx.content, "method_invokes_callable_arg", |program, _| {
        let mut stmts = Vec::new();
        flatten_namespaced_statements(program.statements.iter(), &mut stmts);
        stmts.into_iter().any(|stmt| {
            let members = match stmt {
                Statement::Class(class)
                    if class_name_matches_receiver(class.name.value, receiver_names) =>
                {
                    Some(class.members.iter())
                }
                _ => None,
            };

            let Some(members) = members else {
                return false;
            };

            members.into_iter().any(|member| {
                if let ClassLikeMember::Method(method) = member
                    && bytes_to_str(method.name.value).eq_ignore_ascii_case(method_name)
                {
                    let Some(param) =
                        select_param(method.parameter_list.parameters.iter(), selector)
                    else {
                        return false;
                    };
                    return node_param_has_invocation_tag(
                        method.name.span.start.offset as usize,
                        ctx.content,
                        bytes_to_str(param.variable.name),
                        "param-immediately-invoked-callable",
                    );
                }
                false
            })
        })
    })
}

pub(crate) fn class_name_matches_receiver(name: &[u8], receiver_names: &[String]) -> bool {
    let class_name = bytes_to_str(name);
    receiver_names.iter().any(|receiver| {
        receiver.eq_ignore_ascii_case(class_name)
            || crate::util::short_name(receiver).eq_ignore_ascii_case(class_name)
    })
}

pub(crate) fn function_param_has_invocation_tag(
    node_start: usize,
    content: &str,
    param_name: &str,
    tag_name: &str,
) -> bool {
    node_param_has_invocation_tag(node_start, content, param_name, tag_name)
}

pub(crate) fn node_param_has_invocation_tag(
    node_start: usize,
    content: &str,
    param_name: &str,
    tag_name: &str,
) -> bool {
    let Some(docblock) = preceding_docblock_text(content, node_start) else {
        return false;
    };
    docblock.lines().any(|line| {
        let line = line
            .trim()
            .trim_start_matches("/**")
            .trim_start_matches('*')
            .trim_end_matches("*/")
            .trim();
        line.starts_with(&format!("@{tag_name}"))
            && line
                .split_whitespace()
                .any(|part| part.trim_matches(',') == param_name)
    })
}

pub(crate) fn preceding_docblock_text(content: &str, node_start: usize) -> Option<&str> {
    let before = content.get(..node_start)?;
    let doc_end = before.rfind("*/")? + 2;
    let between = &before[doc_end..];
    if between.contains(';') || between.contains('{') || between.contains('}') {
        return None;
    }
    let doc_start = before[..doc_end].rfind("/**")?;
    Some(&before[doc_start..doc_end])
}

/// Walk a closure body and propagate the types it assigns to `use (&$x)`
/// captures back into the outer scope.
///
/// `invoked_immediately` decides how: when the closure provably runs
/// before the call returns, the closure's final state *replaces* the
/// outer variable; otherwise the closure may run zero or more times at
/// any later point, so the assigned types are *unioned* with the outer
/// types (mirroring PHPStan, which widens by-ref captures even for
/// closures that are merely defined).
pub(crate) fn process_by_ref_closure_capture<'b>(
    closure: &'b Closure<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    invoked_immediately: bool,
) {
    let captured: Vec<String> = closure
        .use_clause
        .as_ref()
        .map(|use_clause| {
            use_clause
                .variables
                .iter()
                .filter(|use_var| use_var.ampersand.is_some())
                .map(|use_var| bytes_to_str(use_var.variable.name).to_string())
                .collect()
        })
        .unwrap_or_default();
    if captured.is_empty() {
        return;
    }

    let full_ctx = ctx.with_cursor_offset(u32::MAX);
    let mut closure_scope = ScopeState::new();

    seed_closure_captures(&mut closure_scope, scope, closure.use_clause.as_ref());
    seed_closure_this(
        &mut closure_scope,
        Some(scope),
        closure.span().start.offset,
        closure.body.span().start.offset,
        ctx,
    );

    seed_closure_params(
        &mut closure_scope,
        &closure.parameter_list,
        closure.span().start.offset,
        &[],
        &full_ctx,
    );

    push_return_frame();
    walk_body_forward(
        closure.body.statements.iter(),
        &mut closure_scope,
        &full_ctx,
    );
    // Every `return` in the body is an exit of the closure just as much as
    // falling off its end is, and a capture written on a returning path is
    // still written.  `walk_body_forward` leaves only the fall-through
    // state behind, so the returning paths are folded back in here.
    if let Some(returned) = pop_return_frame() {
        closure_scope.merge_branch(&returned);
    }

    for var_name in captured {
        scope.invalidate_dependent_keys(&var_name);
        scope.invalidate_proofs(&var_name);
        let types = closure_scope.get(&var_name).to_vec();
        if !types.is_empty() {
            if invoked_immediately {
                scope.set(&var_name, types);
            } else {
                let mut combined = scope.get(&var_name).to_vec();
                ResolvedType::extend_unique(&mut combined, types);
                scope.set(&var_name, combined);
            }
        } else if closure_scope.contains(&var_name) {
            scope.set_empty(&var_name);
        }
    }
}

/// Process increment/decrement expressions (`$a++`, `++$a`, `$a--`, `--$a`).
///
/// For numeric types (int, float), the base type is preserved.
/// Numeric literals and refined numeric types are widened because the
/// operation changes the value and may invalidate the refinement.
/// For numeric strings, the result becomes `int|float`.
/// For general strings, PHP increments alphabetically (stays string), while
/// decrementing a known non-numeric string is a no-op and stays exact.
/// Incrementing `null` produces `1`, while decrementing it leaves `null`
/// unchanged.
#[derive(Clone, Copy)]
enum IncrementDecrementKind {
    Increment,
    Decrement,
}

fn type_after_increment_decrement(ty: &PhpType, operation: IncrementDecrementKind) -> PhpType {
    match ty.kind() {
        TypeKind::Union(members) => {
            let mut transformed = Vec::with_capacity(members.len());
            for member in members {
                let member = type_after_increment_decrement(member, operation);
                for alternative in member.union_members() {
                    if !transformed.iter().any(|existing| existing == alternative) {
                        transformed.push(alternative.clone());
                    }
                }
            }
            match transformed.len() {
                0 => ty.clone(),
                1 => transformed.into_iter().next().unwrap(),
                _ => PhpType::union(transformed),
            }
        }
        TypeKind::Nullable(inner) => {
            let inner = type_after_increment_decrement(inner, operation);
            match operation {
                IncrementDecrementKind::Increment => {
                    let mut alternatives = vec![PhpType::int()];
                    for alternative in inner.union_members() {
                        if !alternatives.iter().any(|existing| existing == alternative) {
                            alternatives.push(alternative.clone());
                        }
                    }
                    if alternatives.len() == 1 {
                        alternatives.into_iter().next().unwrap()
                    } else {
                        PhpType::union(alternatives)
                    }
                }
                IncrementDecrementKind::Decrement => {
                    if matches!(inner.kind(), TypeKind::Union(_)) {
                        let mut alternatives: Vec<PhpType> =
                            inner.union_members().into_iter().cloned().collect();
                        alternatives.push(PhpType::null());
                        PhpType::union(alternatives)
                    } else {
                        PhpType::nullable(inner)
                    }
                }
            }
        }
        _ if ty.is_null() => match operation {
            IncrementDecrementKind::Increment => PhpType::int(),
            IncrementDecrementKind::Decrement => PhpType::null(),
        },
        _ if ty.is_named_ci("numeric")
            || ty.is_named("number")
            || ty.is_named_ci("numeric-string")
            || ty.is_subtype_of(&PhpType::named(atom("numeric-string"))) =>
        {
            PhpType::union(vec![PhpType::int(), PhpType::float()])
        }
        _ if ty.is_int_subtype() => PhpType::int(),
        _ if ty.is_float_subtype() => PhpType::float(),
        TypeKind::Literal(value) if matches!(&**value, LiteralValue::String(_)) => {
            match operation {
                IncrementDecrementKind::Increment => PhpType::string(),
                IncrementDecrementKind::Decrement => ty.clone(),
            }
        }
        // A broad string may be numeric at runtime. PHP converts numeric
        // strings to int or float for both operators; non-numeric strings
        // remain strings (apart from the deprecated increment behaviour).
        _ if ty.is_string_subtype() => {
            PhpType::union(vec![PhpType::int(), PhpType::float(), PhpType::string()])
        }
        _ => ty.clone(),
    }
}

pub(crate) fn process_increment_decrement<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    _ctx: &ForwardWalkCtx<'_>,
) {
    use mago_syntax::cst::unary::{UnaryPostfixOperator, UnaryPrefixOperator};

    let (var_expr, operation) = match expr {
        Expression::UnaryPostfix(postfix) => match &postfix.operator {
            UnaryPostfixOperator::PostIncrement(_) => {
                (postfix.operand, IncrementDecrementKind::Increment)
            }
            UnaryPostfixOperator::PostDecrement(_) => {
                (postfix.operand, IncrementDecrementKind::Decrement)
            }
        },
        Expression::UnaryPrefix(prefix) => match &prefix.operator {
            UnaryPrefixOperator::PreIncrement(_) => {
                (prefix.operand, IncrementDecrementKind::Increment)
            }
            UnaryPrefixOperator::PreDecrement(_) => {
                (prefix.operand, IncrementDecrementKind::Decrement)
            }
            _ => return,
        },
        _ => return,
    };

    let var_name = match var_expr {
        Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
        _ => return,
    };

    let existing = scope.get(&var_name).to_vec();
    if existing.is_empty() {
        return;
    }

    let current_type = ResolvedType::types_joined(&existing);
    let transformed = type_after_increment_decrement(&current_type, operation);
    if transformed != current_type {
        scope.set(&var_name, vec![ResolvedType::from_type_string(transformed)]);
    }
}

/// Get the byte offset of an expression (used for cursor comparisons).
pub(crate) fn stmt_offset(expr: &Expression<'_>) -> u32 {
    expr.span().start.offset
}

/// Result of [`try_process_inline_var_override`].
pub(crate) enum VarOverrideResult {
    /// No `@var` docblock found.
    None,
    /// A `@var Type $varName` block (with explicit variable name) was
    /// applied.  The caller should re-record the scope snapshot so that
    /// lookups within the same statement see the updated types.
    NamedVar,
    /// A `@var Type` block (without variable name) was applied to the
    /// assignment LHS.  The caller must NOT re-record the snapshot
    /// because the LHS variable should not be visible in the RHS.
    NoVar,
}

/// Try to process an inline `/** @var Type $x */` docblock override.
pub(crate) fn try_process_inline_var_override<'b>(
    expr: &'b Expression<'b>,
    expr_offset: u32,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> VarOverrideResult {
    // Parse the inline @var docblock at this expression's position.
    let offset = expr_offset as usize;
    if offset == 0 {
        return VarOverrideResult::None;
    }

    // Look for `/** @var Type $varName */` before this expression.
    let before = &ctx.content[..offset.min(ctx.content.len())];
    let trimmed = trim_trailing_line_comments(before);

    let Some(doc_text) = trailing_docblock(trimmed) else {
        return VarOverrideResult::None;
    };
    let doc_start = trimmed.len() - doc_text.len();

    // Try multi-@var first: a single docblock may declare several
    // variables (e.g. `/** @var App $app  @var array{…} $params */`).
    let multi = parse_all_inline_var_docblocks(doc_text, ctx);
    if !multi.is_empty() {
        // The blocks further back run first, so a name this docblock also
        // declares ends up carrying the type written closest to the
        // expression.
        apply_preceding_var_docblocks(&trimmed[..doc_start], scope, ctx);
        // When the cursor is inside the RHS of an assignment, skip
        // overriding the LHS variable so that hover/completion on the
        // RHS sees the pre-override type.  E.g.:
        //   /** @var array<string, mixed> $response */
        //   $response = $response->json();
        // Hovering on the RHS `$response` should show `ApiResponse`,
        // not `array<string, mixed>`.
        let skip_var: Option<String> = if let Expression::Assignment(assignment) = expr {
            let rhs_span = assignment.rhs.span();
            let cursor_in_rhs = ctx.cursor_offset >= rhs_span.start.offset
                && ctx.cursor_offset <= rhs_span.end.offset;
            if cursor_in_rhs {
                if let Expression::Variable(Variable::Direct(dv)) = assignment.lhs {
                    Some(bytes_to_str(dv.name).to_string())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        for (var_name, php_type) in &multi {
            if skip_var.as_deref() == Some(var_name.as_str()) {
                continue;
            }
            let resolved = resolve_type_to_resolved_types(php_type, ctx);
            scope.set(var_name, resolved);
        }

        // When the @var variable names all differ from the assignment
        // LHS, return None so the caller continues processing the
        // assignment.  E.g.:
        //   /** @var Foo[] $items */
        //   $item = array_shift($items);
        // The @var sets `$items` in scope (done above), and the caller
        // must also process `$item = array_shift($items)`.
        //
        // When any @var name matches the LHS, return NamedVar so the
        // caller skips the assignment (the @var type is authoritative).
        if let Expression::Assignment(assignment) = expr
            && let Expression::Variable(Variable::Direct(dv)) = assignment.lhs
        {
            let lhs_name = bytes_to_str(dv.name).to_string();
            if !multi.iter().any(|(n, _)| *n == lhs_name) {
                return VarOverrideResult::None;
            }
        }
        return VarOverrideResult::NamedVar;
    }

    // Also check for `/** @var Type */` without variable name — this
    // applies to the immediately following expression if it's a simple
    // variable or assignment.
    if let Some(php_type) = parse_inline_var_docblock_no_var(doc_text) {
        let resolved = resolve_type_to_resolved_types(&php_type, ctx);
        if let Expression::Assignment(assignment) = expr {
            if let Expression::Variable(Variable::Direct(dv)) = assignment.lhs {
                // When the cursor is inside the RHS, skip the override
                // so that the variable retains its pre-assignment type.
                // E.g. `/** @var array<string, mixed> */ $data = $data->toArray()`
                // — the cursor on `$data->` in the RHS should see Data, not array.
                let rhs_span = assignment.rhs.span();
                let cursor_in_rhs = ctx.cursor_offset >= rhs_span.start.offset
                    && ctx.cursor_offset <= rhs_span.end.offset;
                if cursor_in_rhs {
                    return VarOverrideResult::None;
                }

                // Scalar-blocking: when the RHS resolves to a concrete
                // scalar type (string, int, bool, etc.), reject a class
                // `@var` override.  E.g. `/** @var Session */ $s =
                // $this->getName()` where `getName()` returns `string`
                // should NOT override `$s` to `Session`.
                let native_type = resolve_rhs_native_type(assignment.rhs, scope, ctx);
                if let Some(ref native) = native_type
                    && !crate::docblock::should_override_type_typed(&php_type, native)
                {
                    // The override was rejected (scalar blocking).
                    return VarOverrideResult::None;
                }

                let var_name = bytes_to_str(dv.name).to_string();
                // Scan for preceding docblocks first, so this one wins.
                apply_preceding_var_docblocks(&trimmed[..doc_start], scope, ctx);
                scope.set(&var_name, resolved);
                return VarOverrideResult::NoVar;
            }
        } else if let Expression::Variable(Variable::Direct(dv)) = expr {
            let var_name = bytes_to_str(dv.name).to_string();
            apply_preceding_var_docblocks(&trimmed[..doc_start], scope, ctx);
            scope.set(&var_name, resolved);
            return VarOverrideResult::NoVar;
        }
    }

    VarOverrideResult::None
}

/// Extract the native type of an RHS expression using the current scope.
///
/// Used by [`try_process_inline_var_override`] to determine whether a
/// `@var` override should be blocked by a scalar native type.
///
/// This delegates to [`super::super::resolution::extract_native_type_from_rhs`]
/// via a `VarResolutionCtx` that has scope-based variable resolution.
/// That function already handles method calls, function calls, static
/// calls, casts, literals, and other patterns — including extracting
/// scalar return types from method signatures.
pub(crate) fn resolve_rhs_native_type(
    rhs: &Expression<'_>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<PhpType> {
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = move |vn: &str| -> Vec<ResolvedType> {
        scope_snapshot.get(&atom(vn)).cloned().unwrap_or_default()
    };
    let var_ctx =
        ctx.var_ctx_for_with_scope("$__rhs_check", 0, &scope_resolver, Some(scope.proofs()));
    super::super::resolution::extract_native_type_from_rhs(rhs, &var_ctx)
}

/// Scan backwards through `before` (content before a docblock we already
/// processed) for additional standalone `/** @var Type $var */` blocks.
/// Each discovered block's `@var` tags are applied to `scope`.  Stops as
/// soon as the text no longer ends with `*/` (after trimming).
///
/// Returns whether any `@var` annotation was applied, so callers that
/// recorded a diagnostic scope snapshot before this call know to
/// re-record it afterward.
pub(crate) fn apply_preceding_var_docblocks(
    before: &str,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let mut applied = false;
    let mut remaining = trim_trailing_line_comments(before);
    // The blocks are visited nearest first, so a name a nearer one already
    // declared is not overwritten by a further one that repeats it.
    let mut declared: Vec<String> = Vec::new();
    // Keep scanning as long as the preceding text ends with a docblock.
    while let Some(doc_text) = trailing_docblock(remaining) {
        let doc_start = remaining.len() - doc_text.len();
        let vars = parse_all_inline_var_docblocks(doc_text, ctx);
        if vars.is_empty() {
            // Not a @var docblock — stop scanning.
            break;
        }
        for (var_name, php_type) in &vars {
            if declared.iter().any(|seen| seen == var_name) {
                continue;
            }
            let resolved = resolve_type_to_resolved_types(php_type, ctx);
            scope.set(var_name, resolved);
            declared.push(var_name.clone());
        }
        applied = true;
        remaining = trim_trailing_line_comments(&remaining[..doc_start]);
    }
    applied
}

/// The `/** … */` docblock `text` ends with, or `None` when it ends with
/// something else.
///
/// The comment that ends the text opens at the first `/*` no earlier
/// comment closes.  An ordinary `/* … */` block is not a docblock, and
/// searching past it for the nearest `/**` would take everything in
/// between — including whole statements — for the docblock's body, and
/// read out of it a `@var` annotation that the code in between has
/// already superseded.  A preprocessed Blade template is full of such
/// blocks (every `{{-- --}}` comment and every component tag becomes
/// one), but so is any PHP file with a commented-out line between two
/// annotated assignments.
fn trailing_docblock(text: &str) -> Option<&str> {
    let body = text.strip_suffix("*/")?;
    let after_previous = body.rfind("*/").map_or(0, |pos| pos + 2);
    let start = after_previous + body[after_previous..].find("/*")?;
    text.get(start..).filter(|doc| doc.starts_with("/**"))
}

/// Trim trailing whitespace and whole-line `//` / `#` comments from
/// `before`, so that a `/** @var Type $var */` block separated from the
/// code it annotates by ordinary comment lines is still discovered.
///
/// Only comments that occupy a whole line are removed: a `//` following
/// code on the same line may well sit inside a string literal
/// (`$url = 'https://…';`), and `#[` opens an attribute rather than a
/// comment.
///
/// This runs once per statement the forward walker visits, so the search
/// for the start of the trailing line is capped at
/// [`MAX_COMMENT_LINE_LOOKBACK`] bytes.  Beyond that the line is far too
/// long to be a comment worth stepping over, and an unbounded scan would
/// be quadratic on a file written as one very long line.
pub(crate) fn trim_trailing_line_comments(before: &str) -> &str {
    let mut head = before.trim_end();
    loop {
        // A docblock ends the search: the caller wants exactly this.
        if head.ends_with("*/") {
            return head;
        }
        let line_start = match head
            .as_bytes()
            .iter()
            .rev()
            .take(MAX_COMMENT_LINE_LOOKBACK)
            .position(|&b| b == b'\n')
        {
            // `position` counts back from the end, so the byte after the
            // newline sits at `len - offset`.  That is a char boundary.
            Some(offset) => head.len() - offset,
            None if head.len() <= MAX_COMMENT_LINE_LOOKBACK => 0,
            None => return head,
        };
        let line = head[line_start..].trim_start();
        if line.starts_with("//") || (line.starts_with('#') && !line.starts_with("#[")) {
            head = head[..line_start].trim_end();
        } else {
            return head;
        }
    }
}

/// How far back [`trim_trailing_line_comments`] looks for the start of
/// the line it is inspecting.
const MAX_COMMENT_LINE_LOOKBACK: usize = 512;

/// Apply `/** @var Type $var */` docblocks that precede a statement
/// without being attached to an assignment.
///
/// [`try_process_inline_var_override`] covers the assignment case
/// (`/** @var Foo $x */ $x = …`); this covers annotations that stand on
/// their own, such as the `@var` block a Blade template opens with or a
/// docblock written above an `if`.  Without it the variable never enters
/// the walker's scope and every use of it falls back to a backward text
/// scan, which cannot tell a preceding sibling block from a preceding
/// sibling function body.
///
/// Returns whether any `@var` annotation was applied.
pub(crate) fn apply_standalone_var_docblocks(
    stmt_offset: u32,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let offset = (stmt_offset as usize).min(ctx.content.len());
    if offset == 0 {
        return false;
    }
    apply_preceding_var_docblocks(&ctx.content[..offset], scope, ctx)
}

/// Look up a standalone `/** @var Type */` docblock (no variable name)
/// immediately preceding the statement starting at `stmt_start`.
///
/// This casts the type of that statement's expression outright — used for
/// a `return` statement, where PHPStan treats the annotation as
/// authoritative rather than as a refinement.  That is stricter than the
/// same annotation above an assignment ([`try_process_inline_var_override`]'s
/// `NoVar` case), which only overrides when the override is a genuine
/// refinement of the RHS's native type.
pub(crate) fn find_preceding_nameless_var_cast(
    content: &str,
    stmt_start: usize,
) -> Option<PhpType> {
    let offset = stmt_start.min(content.len());
    if offset == 0 {
        return None;
    }
    let before = &content[..offset];
    let trimmed = trim_trailing_line_comments(before);
    let doc_text = trailing_docblock(trimmed)?;
    parse_inline_var_docblock_no_var(doc_text)
}

/// Resolve a [`PhpType`] to a complete `Vec<ResolvedType>` with
/// `class_info` populated when possible.  Falls back to a
/// type-string-only entry for scalars and unresolvable types.
///
/// The type comes from a docblock the walker just read out of the source,
/// so its class names are still spelled the way the author wrote them.
/// They are qualified against the enclosing namespace first, matching how
/// PHP reads the same spelling and how the parser already resolved the
/// `@param`/`@return` tags these types get compared against.
pub(crate) fn resolve_type_to_resolved_types(
    php_type: &PhpType,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<ResolvedType> {
    let php_type = crate::util::resolve_source_php_type_names(
        php_type,
        ctx.current_class.file_namespace.as_deref(),
        ctx.class_loader,
    );
    let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        &php_type,
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    );
    if !classes.is_empty() {
        ResolvedType::from_classes_with_hint(classes, php_type)
    } else {
        vec![ResolvedType::from_type_string(php_type)]
    }
}

/// Strip the `/**`…`*/` wrapper from a docblock and collapse its
/// line-continuation markers into a single space-joined string.
///
/// This flattens type strings that span multiple lines (e.g. a
/// `array{...}` shape written across several ` * ` lines) so they can be
/// parsed as one token sequence instead of retaining the leading `*`
/// markers, which [`PhpType::parse`] cannot interpret.
pub(crate) fn flatten_docblock_inner(doc_text: &str) -> Option<String> {
    let inner = doc_text.strip_prefix("/**")?.strip_suffix("*/")?;
    Some(
        inner
            .lines()
            .map(|l| l.trim().trim_start_matches('*').trim())
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// Parse ALL `@var Type $varName` pairs from a docblock.  Returns an
/// empty vec when none are found.  Handles multi-line docblocks with one
/// annotation per line as well as a single annotation whose type spans
/// several lines:
/// ```text
/// /**
///  * @var App                      $app
///  * @var array{indexName: string} $params
///  */
/// ```
/// ```text
/// /**
///  * @var array{
///  *     Label,
///  *     Stmt,
///  * } $pair
///  */
/// ```
pub(crate) fn parse_var_docblock_pairs(doc_text: &str) -> Vec<(String, PhpType)> {
    let inner = match flatten_docblock_inner(doc_text) {
        Some(s) => s,
        None => return vec![],
    };
    let inner = inner.as_str();

    let mut results = Vec::new();

    // Split on `@var` and process each occurrence.
    let mut search_from = 0;
    while let Some(pos) = inner[search_from..].find("@var") {
        let abs_pos = search_from + pos;
        let tag_start = abs_pos + 4;
        let after = inner[tag_start..].trim_start();
        let leading_ws = inner[tag_start..].len() - after.len();

        // Split off the type token, respecting `()`/`<>`/`{}` nesting, so a
        // closure signature's own `$`-prefixed parameter names (e.g.
        // `\Closure(\App\Models\User $user): string`) are not mistaken for
        // the variable this `@var` annotates.
        let (type_str, remainder) = split_type_token(after);
        if !type_str.is_empty()
            && let Some(var_name) = remainder.split_whitespace().next()
            && var_name.starts_with('$')
        {
            let php_type = PhpType::parse(type_str);
            results.push((var_name.to_string(), php_type));
        }

        search_from = tag_start + leading_ws + type_str.len();
    }

    results
}

/// Parse ALL `@var Type $varName` pairs from a docblock preceding an
/// assignment or expression.
pub(crate) fn parse_all_inline_var_docblocks(
    doc_text: &str,
    _ctx: &ForwardWalkCtx<'_>,
) -> Vec<(String, PhpType)> {
    parse_var_docblock_pairs(doc_text)
}

/// Parse ALL `@var Type $varName` annotations from a docblock.
/// Supports single-line (`/** @var Type $var */`), one-annotation-per-line
/// multi-line docblocks, and annotations whose type spans several lines
/// (e.g. a multi-line `array{...}` shape).
pub(crate) fn parse_all_var_docblock_annotations(doc_text: &str) -> Vec<(String, PhpType)> {
    parse_var_docblock_pairs(doc_text)
}

/// Parse `/** @var Type */` (without variable name) and return the PhpType.
pub(crate) fn parse_inline_var_docblock_no_var(doc_text: &str) -> Option<PhpType> {
    // Flatten line-continuation markers so a `array{...}` shape spread
    // across several lines is parsed as one type string.
    let inner = flatten_docblock_inner(doc_text)?;
    let inner = inner.trim().strip_prefix("@var")?.trim();

    // Stop at the next docblock tag so trailing tags (e.g. `@psalm-suppress`)
    // do not corrupt the type string.
    let type_str = match inner.find(" @") {
        Some(pos) => inner[..pos].trim(),
        None => inner,
    };
    // Strip a trailing `*` that may remain from `* @var Type *` formatting.
    let type_str = type_str.trim_end_matches('*').trim();

    // If there's a `$` it has a variable name — not the no-var form.
    if type_str.contains('$') {
        return None;
    }

    if type_str.is_empty() {
        return None;
    }

    Some(PhpType::parse(type_str))
}

/// Process assignment expressions, updating the scope.
pub(crate) fn process_assignment_expr<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // `($a = expr);` is a parenthesized assignment statement — written by
    // hand, or produced by the Blade preprocessor for `@php($a = expr)`.
    if let Expression::Assignment(assignment) = unwrap_parens(expr) {
        // An assignment buried in the value runs before the target here is
        // written, and the rest of the value reads what it wrote:
        // `$ok = ($x = $map[$key])->truthy();`.  A right-hand side that
        // *is* an assignment is left to the chain handling below, which
        // knows shapes (destructuring, indexed writes) this does not.
        if !matches!(unwrap_parens(assignment.rhs), Expression::Assignment(_)) {
            process_nested_assignments(assignment.rhs, scope, ctx);
        }

        if !assignment.operator.is_assign() {
            // Compound assignment: $x op= expr.
            // The type depends on the operator.
            process_compound_assignment(assignment, scope, ctx);
            return;
        }

        // Chain assignments: `$a = $b = expr` — the RHS is itself an
        // assignment expression.  Process it first so that the inner
        // variable (`$b`) gets its type before we resolve the outer one.
        if matches!(assignment.rhs, Expression::Assignment(_)) {
            process_assignment_expr(assignment.rhs, scope, ctx);
        }

        // Array destructuring: `[$a, $b] = …` / `list($a, $b) = …`
        if matches!(assignment.lhs, Expression::Array(_) | Expression::List(_)) {
            process_destructuring_assignment(assignment, scope, ctx);
            return;
        }

        // Array key assignment: `$var['key'] = expr;`
        if let Expression::ArrayAccess(array_access) = assignment.lhs {
            process_array_key_assignment(array_access, assignment, scope, ctx);
            return;
        }

        // Array push: `$var[] = expr;` and `$var['a'][$i][] = expr;`
        if let Expression::ArrayAppend(array_append) = assignment.lhs {
            process_array_append(array_append, assignment, scope, ctx);
            return;
        }

        // Property assignment: `$var->prop = expr;` (and null-safe
        // `$var?->prop = expr;`).  Record the assigned type under the
        // property-path key (e.g. `$settings->cache`) so that a later
        // read of that path resolves through the assignment rather than
        // the declaring class's declared property hints.  This is what
        // lets nested object property chains resolve, most notably on
        // `stdClass` which has no declared properties:
        //
        //     $s = new stdClass();
        //     $s->cache = new stdClass();
        //     $s->cache->ttl = 1;   // `$s->cache` now resolves to stdClass
        //
        // The key contains `->`, so it is treated as a synthetic
        // narrowing entry and stripped at loop boundaries — matching the
        // conservative behaviour of condition-based property narrowing.
        // A static property (`self::$repo = …`) is recorded the same way:
        // it is a member path with a declared type, and the lazy-init
        // idiom writes it in exactly the shape this branch handles.
        if matches!(
            assignment.lhs,
            Expression::Access(
                Access::Property(_) | Access::NullSafeProperty(_) | Access::StaticProperty(_)
            )
        ) {
            // Skip when the cursor is inside the RHS so that lookups
            // within the RHS see the pre-assignment state.
            let rhs_span = assignment.rhs.span();
            if ctx.cursor_offset >= rhs_span.start.offset
                && ctx.cursor_offset <= rhs_span.end.offset
            {
                return;
            }
            if let Some(key) = narrowing::expr_to_subject_key(assignment.lhs) {
                // A write that dispatches to `__set` is opaque: the
                // magic setter may transform, reroute, or drop the
                // value, and a later read goes through `__get`, which
                // decides what comes back.  Drop whatever was known
                // about the path instead of recording the written type.
                if property_write_dispatches_to_magic_set(assignment.lhs, scope, ctx) {
                    scope.remove(&key);
                    scope.invalidate_dependent_keys(&key);
                    return;
                }
                let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
                if rhs_types.is_empty() {
                    // The right-hand side did not resolve. Unlike a plain
                    // variable (`set_unknown`), a property's correct
                    // fallback is its *declared* type, not "unknown", so
                    // drop the key entirely rather than blanking it — a
                    // blank entry would leave the pre-write `instanceof`
                    // narrowing looking current instead of falling
                    // through to the declared type.
                    scope.remove(&key);
                } else {
                    scope.invalidate_proofs(&key);
                    scope.set(&key, rhs_types);
                }
            }
            return;
        }

        // Simple variable assignment: `$var = expr;`
        let lhs_name = match assignment.lhs {
            Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
            _ => return,
        };

        // When the cursor is inside the RHS of this assignment, skip
        // storing the new type so that variable lookups within the RHS
        // see the pre-assignment type.  E.g. in `$request = new Bar(
        // name: $request->)`, the cursor on `$request->` should see
        // the old `Foo` type, not the new `Bar` type.
        let rhs_span = assignment.rhs.span();
        let cursor_in_rhs =
            ctx.cursor_offset >= rhs_span.start.offset && ctx.cursor_offset <= rhs_span.end.offset;
        if cursor_in_rhs {
            return;
        }

        let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
        // Reassigning the variable replaces its object identity, so any
        // property/array-access key rooted at it (seeded by an earlier
        // assignment or condition narrowing) is now stale.  Drop them
        // after resolving the RHS, so `$x = $x->foo` still reads the old
        // key while resolving.
        scope.invalidate_dependent_keys(&lhs_name);
        scope.invalidate_proofs(&lhs_name);
        if !rhs_types.is_empty() {
            scope.set(&lhs_name, rhs_types);
        } else if !scope.get(&lhs_name).is_empty()
            && rhs_fails_on_resolved_receiver(assignment.rhs, scope)
        {
            // The right-hand side did not resolve, so nothing is known
            // about what the variable now holds — but the value it held
            // before the assignment is gone either way. Keeping the old
            // type is what made `$acc = $acc->merge($x)` (with `$x`
            // unresolved) report a member access on the `null` that
            // `$acc` was initialised with.
            //
            // Flagging it as unresolved is what keeps the loss local: a
            // join with a path that still knows the type takes that
            // path's answer, so `$acc = $acc->missing()` inside a loop
            // reports the missing member rather than turning `$acc`
            // unknown for the rest of the body.
            scope.set_unknown(&lhs_name);
        } else {
            // Nothing was known about the variable beforehand, or the
            // failure came from somewhere the answer was already "could
            // be anything". Neither is a type this walk lost, so the
            // entry is the plain unknown a join treats as top.
            scope.set_untyped(&lhs_name);
        }
        // `$isHtml = $raw instanceof HtmlString` makes `$isHtml` stand
        // for the check, so testing it later narrows `$raw`.
        record_assertion_variable(&lhs_name, assignment.rhs, scope);
        // `$period = $agreement?->latestPeriod()` makes `$period`'s null
        // stand for `$agreement`'s, so ruling out one rules out the other.
        record_nullsafe_origin(&lhs_name, assignment.rhs, scope);
        // `$ok = preg_match('/…/', $s, $m)` makes `$ok` stand for the
        // match's outcome, so testing it later narrows `$m`.
        record_preg_outcome(&lhs_name, assignment.rhs, scope, ctx);
    } else {
        // The expression assigns nothing at its root but may still assign
        // inside itself: `return ($x = $map[$key])->truthy();`.
        process_nested_assignments(expr, scope, ctx);
    }
}

/// Whether a right-hand side that resolved to nothing failed on a member
/// of a class the walker did resolve.
///
/// This is the one shape where the failure is the walker's own and it
/// says so out loud: the receiver is a known class, the member is not on
/// it, and `unknown_member` is reported on this very line. Anything else
/// — a value that had no type to start with, a chain that already lost
/// the thread further up — is a failure inherited from somewhere the
/// answer was already "could be anything", and passing it on is all the
/// assignment does.
///
/// The receiver is read straight out of the scope rather than resolved,
/// so this costs nothing on a path that is already a dead end. It also
/// keeps the answer to the accumulator idiom the flag exists for —
/// `$acc = $acc->…`, whose receiver is the variable being written — and
/// leaves a longer chain alone, which is the right way round: the deeper
/// the chain, the likelier it is that what failed was some link of it
/// rather than the member on the end.
fn rhs_fails_on_resolved_receiver(rhs: &Expression<'_>, scope: &ScopeState) -> bool {
    let receiver = match rhs {
        Expression::Call(Call::Method(call)) => call.object,
        Expression::Call(Call::NullSafeMethod(call)) => call.object,
        Expression::Access(Access::Property(access)) => access.object,
        Expression::Access(Access::NullSafeProperty(access)) => access.object,
        _ => return false,
    };
    let Expression::Variable(Variable::Direct(var)) = receiver else {
        return false;
    };
    scope
        .get(bytes_to_str(var.name))
        .iter()
        .any(|rt| rt.class_info.is_some())
}

/// Whether `$obj->prop = …` writes through the subject class's `__set`
/// magic method instead of storing the value in a real property.
///
/// Returns `false` whenever no subject class resolves: without a class
/// there is no evidence of a magic setter, and the write is recorded as
/// before.
fn property_write_dispatches_to_magic_set(
    lhs: &Expression<'_>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let (object, selector) = match lhs {
        Expression::Access(Access::Property(pa)) => (pa.object, &pa.property),
        Expression::Access(Access::NullSafeProperty(pa)) => (pa.object, &pa.property),
        _ => return false,
    };
    let ClassLikeMemberSelector::Identifier(ident) = selector else {
        return false;
    };
    let prop_name = bytes_to_str(ident.value);
    let is_magic = |cls: &ClassInfo| {
        crate::virtual_members::property_write_is_magic(
            cls,
            prop_name,
            ctx.class_loader,
            ctx.resolved_class_cache,
        )
    };

    // `$this` and plain variables are answered from the walker's own
    // state, so the common write shapes cost no resolution.  A union
    // subject is magic as soon as one member routes the write through
    // `__set`: the recorded type would have no authority over what that
    // member's `__get` returns.
    let object = unwrap_parens(object);
    if let Expression::Variable(Variable::Direct(dv)) = object {
        let var_name = bytes_to_str(dv.name);
        if var_name == "$this" {
            return is_magic(ctx.current_class);
        }
        return scope
            .get(var_name)
            .iter()
            .filter_map(|rt| rt.class_info.as_deref())
            .any(is_magic);
    }
    resolve_rhs_with_scope(object, scope, ctx)
        .iter()
        .filter_map(|rt| rt.class_info.as_deref())
        .any(is_magic)
}

/// What a `??=` leaves behind, given what its target and its fallback
/// resolve to.
///
/// `??=` keeps the target when it is not null and assigns the fallback
/// otherwise, so the value is the target's non-null half unioned with the
/// fallback. The resolved types are combined as they are, rather than
/// joined into one union *type string*, so the `class_info` already
/// attached to each operand survives: a rebuilt string carries none, and
/// a member access on the result would have nothing to resolve against.
///
/// Where both sides name the same type (commonly the target's declared
/// element type, which resolved no class, alongside an argument that did)
/// the class-backed entry speaks for the pair.
fn coalesce_assign_value(
    lhs_types: Vec<ResolvedType>,
    rhs_types: Vec<ResolvedType>,
) -> Vec<ResolvedType> {
    let mut combined: Vec<ResolvedType> = lhs_types
        .into_iter()
        .filter(|rt| !rt.type_string.is_null())
        .map(|mut rt| {
            if let Some(non_null) = rt.type_string.non_null_type() {
                rt.type_string = non_null;
            }
            rt
        })
        .collect();
    ResolvedType::extend_unique(&mut combined, rhs_types);
    let class_backed: Vec<PhpType> = combined
        .iter()
        .filter(|rt| rt.class_info.is_some())
        .map(|rt| rt.type_string.clone())
        .collect();
    combined.retain(|rt| rt.class_info.is_some() || !class_backed.contains(&rt.type_string));
    combined
}

/// Process compound assignment operators (`+=`, `-=`, `/=`, `*=`, etc.).
///
/// The result type depends on the operator kind:
/// - `.=` → string
/// - `%=` → int
/// - `<<=`, `>>=`, `&=`, `|=`, `^=` → int
/// - `+=`, `-=`, `*=`, `/=`, `**=` → int|float
/// - `??=` → union of LHS non-null type and RHS type
pub(crate) fn process_compound_assignment<'b>(
    assignment: &'b Assignment<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    use mago_syntax::cst::assignment::AssignmentOperator;

    let var_name = match assignment.lhs {
        Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
        // `$this->regexp ??= $this->generate();` leaves the property
        // non-null just as surely as the same operator leaves a local
        // non-null, and the scope names a member path the same way it
        // names a local.  Only `??=` is routed this way: the arithmetic
        // operators below read the target's current type, which a member
        // path the scope has never seen does not have.
        _ if matches!(assignment.operator, AssignmentOperator::Coalesce(_)) => {
            match crate::type_engine::types::narrowing::expr_to_subject_key(assignment.lhs) {
                Some(key) => key,
                None => return,
            }
        }
        _ => return,
    };
    if matches!(assignment.operator, AssignmentOperator::Coalesce(_)) {
        // A member path the scope has not narrowed yet still has a
        // declared type, and `??=` only keeps that type's non-null half —
        // reading nothing there would drop every alternative the
        // declaration allows besides the fallback's.
        let lhs_types = match scope.get(&var_name) {
            [] => resolve_rhs_with_scope(assignment.lhs, scope, ctx),
            existing => existing.to_vec(),
        };
        let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
        let combined = coalesce_assign_value(lhs_types, rhs_types);
        if !combined.is_empty() {
            scope.set(&var_name, combined);
        } else if !scope.contains(&var_name) {
            scope.set_empty(&var_name);
        }
        return;
    }

    let result_type = match &assignment.operator {
        AssignmentOperator::Concat(_) => PhpType::string(),
        AssignmentOperator::Modulo(_) => PhpType::int(),
        AssignmentOperator::LeftShift(_)
        | AssignmentOperator::RightShift(_)
        | AssignmentOperator::BitwiseAnd(_)
        | AssignmentOperator::BitwiseOr(_)
        | AssignmentOperator::BitwiseXor(_) => PhpType::int(),
        AssignmentOperator::Addition(_) => {
            let lhs_types = scope.get(&var_name).to_vec();
            let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
            infer_addition_result_type(&lhs_types, &rhs_types)
        }
        AssignmentOperator::Subtraction(_)
        | AssignmentOperator::Multiplication(_)
        | AssignmentOperator::Division(_)
        | AssignmentOperator::Exponentiation(_) => {
            let lhs_types = scope.get(&var_name).to_vec();
            let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
            let op_kind = match assignment.operator {
                AssignmentOperator::Division(_) => ArithmeticOpKind::Division,
                AssignmentOperator::Exponentiation(_) => ArithmeticOpKind::Exponentiation,
                _ => ArithmeticOpKind::Other,
            };
            infer_arithmetic_result_type(&lhs_types, &rhs_types, op_kind)
        }
        AssignmentOperator::Coalesce(_) | AssignmentOperator::Assign(_) => return, // handled above / elsewhere
    };

    scope.set(&var_name, vec![ResolvedType::from_type_string(result_type)]);
}

/// Unwrap parenthesized expressions to their inner expression.
pub(crate) fn unwrap_parens<'a>(expr: &'a Expression<'a>) -> &'a Expression<'a> {
    match expr {
        Expression::Parenthesized(p) => unwrap_parens(p.expression),
        other => other,
    }
}

/// Resolve the type of an RHS expression using the current scope.
///
/// This is the key integration point: instead of calling
/// `resolve_variable_types` (which would recurse), we build a
/// `VarResolutionCtx` that already has the answer for any variable
/// references in the RHS — the forward walker has already resolved
/// them.
///
/// We delegate to `resolve_rhs_expression` with a `VarResolutionCtx`
/// whose `scope_var_resolver` reads directly from the forward walker's
/// in-progress `ScopeState`.  For bare variable references in the RHS,
/// we intercept them and return the scope-based result directly.
pub(crate) fn resolve_rhs_with_scope<'b>(
    rhs: &'b Expression<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<ResolvedType> {
    // Chain assignment: `$a = $b = expr` — the value of an assignment
    // expression is the value of its RHS.  Recurse into the inner RHS
    // so that `$a` resolves to the same type as `$b`.
    if let Expression::Assignment(assignment) = rhs
        && assignment.operator.is_assign()
    {
        return resolve_rhs_with_scope(assignment.rhs, scope, ctx);
    }

    // Compound assignment as RHS: `$a = ($x /= 2)` — the value of the
    // compound assignment is the result after the operation.  Infer the
    // type from the operator kind.
    if let Expression::Assignment(assignment) = rhs
        && !assignment.operator.is_assign()
    {
        use mago_syntax::cst::assignment::AssignmentOperator;
        let result_type = match &assignment.operator {
            AssignmentOperator::Concat(_) => Some(PhpType::string()),
            AssignmentOperator::Modulo(_) => Some(PhpType::int()),
            AssignmentOperator::LeftShift(_)
            | AssignmentOperator::RightShift(_)
            | AssignmentOperator::BitwiseAnd(_)
            | AssignmentOperator::BitwiseOr(_)
            | AssignmentOperator::BitwiseXor(_) => Some(PhpType::int()),
            AssignmentOperator::Addition(_) => {
                let lhs_types = if let Expression::Variable(Variable::Direct(dv)) = assignment.lhs {
                    scope.get(bytes_to_str(dv.name)).to_vec()
                } else {
                    vec![]
                };
                let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
                Some(infer_addition_result_type(&lhs_types, &rhs_types))
            }
            AssignmentOperator::Subtraction(_)
            | AssignmentOperator::Multiplication(_)
            | AssignmentOperator::Division(_)
            | AssignmentOperator::Exponentiation(_) => {
                let lhs_types = if let Expression::Variable(Variable::Direct(dv)) = assignment.lhs {
                    scope.get(bytes_to_str(dv.name)).to_vec()
                } else {
                    vec![]
                };
                let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
                let op_kind = match assignment.operator {
                    AssignmentOperator::Division(_) => ArithmeticOpKind::Division,
                    AssignmentOperator::Exponentiation(_) => ArithmeticOpKind::Exponentiation,
                    _ => ArithmeticOpKind::Other,
                };
                Some(infer_arithmetic_result_type(
                    &lhs_types, &rhs_types, op_kind,
                ))
            }
            // `$x = $cache[$k] ??= expensive();` — the value is whichever
            // side survives: the target's non-null half, or the fallback
            // that replaced it.
            AssignmentOperator::Coalesce(_) => {
                let lhs_types = resolve_rhs_with_scope(assignment.lhs, scope, ctx);
                let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
                let combined = coalesce_assign_value(lhs_types, rhs_types);
                if combined.is_empty() {
                    return vec![ResolvedType::from_type_string(PhpType::mixed())];
                }
                return combined;
            }
            AssignmentOperator::Assign(_) => None,
        };
        if let Some(ty) = result_type {
            return vec![ResolvedType::from_type_string(ty)];
        }
    }

    // For bare variable references, read directly from scope.
    // This is the O(1) path that replaces the recursive backward scan.
    if let Expression::Variable(Variable::Direct(dv)) = rhs {
        let var_name = bytes_to_str(dv.name).to_string();
        let from_scope = scope.get(&var_name);
        if !from_scope.is_empty() {
            return from_scope.to_vec();
        }
        // Variable not in scope — fall through to rhs_resolution which
        // handles some special patterns.
    }

    // ── Foo::class → class-string<Foo> ──────────────────────────
    // `Foo::class` is parsed as `Access::ClassConstant` with the
    // identifier `class`.  resolve_rhs_expression doesn't return a
    // useful type for this (it looks for a constant named "class"
    // on the class and finds nothing).  Handle it here so that
    // subsequent `new $var` can resolve the class-string.
    if let Expression::Access(Access::ClassConstant(cca)) = rhs
        && let ClassLikeConstantSelector::Identifier(ident) = &cca.constant
        && ident.value == b"class"
    {
        let class_name = match cca.class {
            Expression::Identifier(id) => Some(bytes_to_str(id.value()).to_string()),
            Expression::Self_(_) | Expression::Static(_) => {
                if !ctx.current_class.name.is_empty() {
                    Some(ctx.current_class.name.to_string())
                } else {
                    None
                }
            }
            Expression::Parent(_) => ctx.current_class.parent_class.map(|a| a.to_string()),
            _ => None,
        };
        if let Some(name) = class_name {
            let resolved_name = name.strip_prefix('\\').unwrap_or(&name);
            // Resolve the class so we can store a proper ResolvedType
            // with class_info.  This allows `new $var` to work.
            let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
                &PhpType::named(atom(resolved_name)),
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            );
            // The identifier is spelled as the source writes it, which for a
            // name reached through a namespace import (`Support\Pen` behind
            // `use App\Support;`) is neither the FQCN nor resolvable once the
            // class-string is read somewhere else.  Prefer the resolved class.
            let class_string_type = PhpType::class_string(Some(match classes.first() {
                Some(cls) => PhpType::named(cls.fqn()),
                None => PhpType::named(atom(resolved_name)),
            }));
            if !classes.is_empty() {
                return ResolvedType::from_classes_with_hint(classes, class_string_type);
            }
            // Even if we can't resolve the class, return a type-string-only result
            // so the variable is non-empty in scope.
            return vec![ResolvedType::from_type_string(class_string_type)];
        }
    }

    // ── Fast paths for expressions whose type is known structurally ──
    // These avoid the full resolve_rhs_expression round-trip for
    // common patterns where the result type depends only on the
    // expression kind, not on the operand types.

    // Type casts (`(int) $x`), `!`, and `~`.  `-`/`+` are left to the
    // unified resolver below, which preserves signed numeric literals and
    // falls back to `int|float` for non-literal operands.
    if let Expression::UnaryPrefix(prefix) = rhs
        && let Some(ty) =
            super::super::rhs_resolution::unary_prefix_result_type(&prefix.operator, || {
                resolve_rhs_with_scope(prefix.operand, scope, ctx)
            })
    {
        return vec![ResolvedType::from_type_string(ty)];
    }

    // For all other expressions, delegate to the existing RHS resolver
    // with a scope-based variable resolver injected.  When
    // `resolve_rhs_expression` (or its sub-functions like
    // `resolve_rhs_method_call_inner`, `resolve_rhs_property_access`)
    // need to resolve a variable's type, they call `resolve_var_types`
    // which checks `scope_var_resolver` first.  This reads directly
    // from the forward walker's in-progress `ScopeState`, bypassing
    // `resolve_variable_types` entirely.
    let rhs_offset = rhs.span().start.offset;
    let dummy_var = "$__rhs";
    let scope_locals = &scope.locals;
    let scope_resolver = |var_name: &str| -> Vec<ResolvedType> {
        scope_locals
            .get(&atom(var_name))
            .cloned()
            .unwrap_or_default()
    };
    let var_ctx =
        ctx.var_ctx_for_with_scope(dummy_var, rhs_offset, &scope_resolver, Some(scope.proofs()));

    let result = super::super::rhs_resolution::resolve_rhs_expression(rhs, &var_ctx);
    if !result.is_empty() {
        return result;
    }

    // ── Structural fallbacks ────────────────────────────────────
    // When resolve_rhs_expression returns empty, infer the type
    // purely from the expression structure.  These only fire as a
    // last resort so they never override a more precise result.

    // Unwrap parenthesized expressions for structural inference.
    let rhs = unwrap_parens(rhs);

    // Composite strings are not scalar literals and may not be handled by the
    // canonical literal resolver. Exact scalar literals have no fallback here:
    // reintroducing broad int/string/float types would silently undo its
    // precision whenever resolution regressed or hit a recursion guard.
    if matches!(rhs, Expression::CompositeString(_)) {
        return vec![ResolvedType::from_type_string(PhpType::string())];
    }

    // ── Subject pipeline fallback ───────────────────────────────
    // When resolve_rhs_expression and the structural fallbacks both
    // return empty, try the full subject resolution pipeline
    // (resolve_target_classes).  This handles method calls and
    // static calls that resolve_rhs_expression cannot resolve
    // because the receiver or intermediate types are only reachable
    // through the subject pipeline's broader strategies (e.g.
    // docblock @return types, merged inheritance, virtual members).
    //
    // Property access (Expression::Access) is intentionally excluded
    // because resolve_target_classes resolves the *subject* (what
    // you'd complete after `->`) rather than the property's value
    // type.  For Eloquent relations like `$this->model->orderProducts`,
    // the subject pipeline returns the element type instead of the
    // collection, which breaks foreach value binding.  Property
    // access RHS resolution is handled by resolve_rhs_expression's
    // own property resolution path.
    if matches!(rhs, Expression::Call(_) | Expression::Instantiation(_)) {
        let rhs_span = rhs.span();
        let rhs_start = rhs_span.start.offset as usize;
        let rhs_end = rhs_span.end.offset as usize;
        if let Some(rhs_text) = ctx.content.get(rhs_start..rhs_end) {
            let rhs_text = rhs_text.trim();
            if !rhs_text.is_empty() {
                let subject_result = resolve_rhs_via_subject(rhs_text, scope, ctx);
                if !subject_result.is_empty() {
                    return subject_result;
                }
            }
        }
    }

    result
}

/// Resolve an RHS expression through the full subject pipeline.
///
/// This is a last-resort fallback for expressions that
/// `resolve_rhs_expression` can't handle.  It extracts the
/// expression text and passes it to `resolve_target_classes`, which
/// goes through SubjectExpr parsing, property/method chain
/// resolution, and the full type resolution infrastructure.
///
/// Only called for method calls, property access, static calls, and
/// instantiation — expression kinds that typically produce
/// object-typed results resolvable through the subject pipeline.
pub(crate) fn resolve_rhs_via_subject(
    rhs_text: &str,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<ResolvedType> {
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = move |var_name: &str| -> Vec<ResolvedType> {
        scope_snapshot
            .get(&atom(var_name))
            .cloned()
            .unwrap_or_default()
    };
    let var_ctx =
        ctx.var_ctx_for_with_scope("$__rhs_subject", 0, &scope_resolver, Some(scope.proofs()));
    let rctx = var_ctx.as_resolution_ctx();

    // Determine the access kind from the expression text.
    let access_kind = if rhs_text.contains("::") {
        crate::types::AccessKind::DoubleColon
    } else {
        crate::types::AccessKind::Arrow
    };

    crate::type_engine::resolver::resolve_target_classes(rhs_text, access_kind, &rctx)
}

/// Process array destructuring assignments.
///
/// Resolves the RHS type once, then walks the LHS pattern to assign
/// types to each destructured variable.  Handles nested patterns like
/// `[$a, [$b, $c]] = $nested` by recursing into inner array/list
/// expressions.
pub(crate) fn process_destructuring_assignment<'b>(
    assignment: &'b Assignment<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = |var_name: &str| -> Vec<ResolvedType> {
        scope_snapshot
            .get(&atom(var_name))
            .cloned()
            .unwrap_or_default()
    };

    // Build a temporary VarResolutionCtx just to resolve the RHS type.
    // The var_name doesn't matter here since we're resolving the RHS
    // expression, not looking up a specific variable.
    let dummy_name = String::from("$__destructuring_rhs");
    let var_ctx = VarResolutionCtx {
        var_name: &dummy_name,
        current_class: ctx.current_class,
        all_classes: ctx.all_classes,
        content: ctx.content,
        cursor_offset: assignment.span().start.offset,
        class_loader: ctx.class_loader,
        backend: ctx.backend,
        loaders: ctx.loaders,
        resolved_class_cache: ctx.resolved_class_cache,
        enclosing_return_type: ctx.enclosing_return_type.clone(),
        top_level_scope: ctx.top_level_scope.clone(),
        branch_aware: false,
        match_arm_narrowing: HashMap::new(),
        scope_var_resolver: Some(&scope_resolver),
        scope_proofs: Some(scope.proofs()),
    };

    // Try inline @var docblock first, then fall back to RHS expression.
    let stmt_offset = assignment.span().start.offset as usize;
    let raw_type: Option<PhpType> =
        crate::docblock::find_inline_var_docblock(ctx.content, stmt_offset)
            .map(|(vt, _)| crate::util::resolve_php_type_names(&vt, ctx.class_loader))
            .or_else(|| {
                super::super::foreach_resolution::resolve_expression_type(assignment.rhs, &var_ctx)
            });

    // Expand type aliases before shape/generic extraction.
    let raw_type = raw_type.map(|rt| {
        crate::type_engine::type_resolution::resolve_type_alias_typed(
            &rt,
            &ctx.current_class.name,
            ctx.all_classes,
            ctx.class_loader,
        )
        .unwrap_or(rt)
    });

    if let Some(ref rhs_type) = raw_type {
        bind_destructured_pattern(assignment.lhs, rhs_type, scope, ctx);
    }

    // Ensure every destructured variable is present in scope even when the
    // RHS type (or an individual element's type) could not be resolved.  A
    // plain assignment from an unresolvable RHS records the variable with an
    // empty type list via `set_empty`, which lets later assert narrowing seed
    // a type for it.  Without this, list-destructuring from an unresolvable
    // RHS leaves the variables absent from scope entirely, so the assert
    // narrowing loop never visits them and the asserted type is dropped.
    seed_destructured_vars_empty(assignment.lhs, scope);
}

/// Walk a destructuring LHS pattern and record every direct variable in
/// scope with an empty type list, unless it is already present.  Used so
/// that variables destructured from an unresolvable RHS still participate
/// in later narrowing (`set_empty` leaves any already-bound type intact).
pub(crate) fn seed_destructured_vars_empty<'b>(lhs: &'b Expression<'b>, scope: &mut ScopeState) {
    let elements: Vec<&ArrayElement<'b>> = match lhs {
        Expression::Array(arr) => arr.elements.iter().collect(),
        Expression::List(list) => list.elements.iter().collect(),
        _ => return,
    };

    for elem in elements {
        let value_expr = match elem {
            ArrayElement::KeyValue(kv) => kv.value,
            ArrayElement::Value(val) => val.value,
            _ => continue,
        };
        match value_expr {
            Expression::Variable(Variable::Direct(dv)) => {
                scope.set_empty(bytes_to_str(dv.name));
            }
            Expression::Array(_) | Expression::List(_) => {
                seed_destructured_vars_empty(value_expr, scope);
            }
            _ => {}
        }
    }
}

/// Recursively bind types from a destructuring LHS pattern against a
/// resolved RHS type.  For each variable in the pattern, extracts the
/// corresponding type from the RHS type (via shape key or positional
/// index) and sets it in scope.  For nested array/list sub-patterns,
/// recurses with the extracted element type.
pub(crate) fn bind_destructured_pattern<'b>(
    lhs: &'b Expression<'b>,
    rhs_type: &PhpType,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let elements: Vec<&ArrayElement<'b>> = match lhs {
        Expression::Array(arr) => arr.elements.iter().collect(),
        Expression::List(list) => list.elements.iter().collect(),
        _ => return,
    };

    let mut positional_index: usize = 0;
    for elem in elements {
        let (value_expr, shape_key) = match elem {
            ArrayElement::KeyValue(kv) => {
                let key = extract_foreach_destr_key(kv.key);
                (kv.value, key)
            }
            ArrayElement::Value(val) => {
                let key = Some(positional_index.to_string());
                positional_index += 1;
                (val.value, key)
            }
            // A hole (`[, $second]`) names nothing but still consumes the
            // position, so every later element shifts along with it.
            ArrayElement::Missing(_) => {
                positional_index += 1;
                continue;
            }
            _ => continue,
        };

        // Determine the type for this element position.
        let elem_type: Option<PhpType> = shape_key
            .as_ref()
            .and_then(|k| rhs_type.shape_value_type(k).cloned())
            .or_else(|| rhs_type.extract_value_type(false).cloned());

        match value_expr {
            // Direct variable: bind the type.
            Expression::Variable(Variable::Direct(dv)) => {
                if let Some(ref vt) = elem_type {
                    let resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
                        vt,
                        &ctx.current_class.name,
                        ctx.all_classes,
                        ctx.class_loader,
                    );
                    let resolved_types = if !resolved.is_empty() {
                        ResolvedType::from_classes_with_hint(resolved, vt.clone())
                    } else {
                        vec![ResolvedType::from_type_string(vt.clone())]
                    };
                    scope.set(bytes_to_str(dv.name), resolved_types);
                }
            }
            // Nested pattern: recurse with the extracted element type.
            Expression::Array(_) | Expression::List(_) => {
                if let Some(ref vt) = elem_type {
                    bind_destructured_pattern(value_expr, vt, scope, ctx);
                }
            }
            _ => {}
        }
    }
}

/// Process array key assignment: `$var['key'] = expr;`
pub(crate) fn process_array_key_assignment<'b>(
    array_access: &'b ArrayAccess<'b>,
    assignment: &'b Assignment<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    if let Some((base_name, key_chain)) =
        super::super::resolution::extract_nested_array_access_chain(array_access)
    {
        apply_array_write(&base_name, &key_chain, false, assignment, scope, ctx);
    }
}

/// Process array append: `$var[] = expr;` and `$var['a'][$i][] = expr;`
pub(crate) fn process_array_append<'b>(
    array_append: &'b ArrayAppend<'b>,
    assignment: &'b Assignment<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    match array_append.array {
        Expression::Variable(Variable::Direct(dv)) => {
            let base_name = bytes_to_str(dv.name).to_string();
            apply_array_write(&base_name, &[], true, assignment, scope, ctx);
        }
        // `$var['a'][$i][] = …` — the append lands on the innermost level
        // of an array-access chain rather than on the variable itself.
        Expression::ArrayAccess(inner) => {
            if let Some((base_name, key_chain)) =
                super::super::resolution::extract_nested_array_access_chain(inner)
            {
                apply_array_write(&base_name, &key_chain, true, assignment, scope, ctx);
            }
        }
        _ => {}
    }
}

/// Merge the RHS of an element write into the base variable's type.
///
/// `key_chain` holds the array-access keys from outermost to innermost;
/// `append` marks a trailing `[]` past the last key. Literal-string keys
/// become shape entries, dynamic keys become generic `array<K, V>`
/// levels, and missing intermediate levels auto-vivify.
fn apply_array_write<'b>(
    base_name: &str,
    key_chain: &[&Expression<'b>],
    append: bool,
    assignment: &'b Assignment<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
    // An append with no inferable element type leaves the variable alone
    // rather than widening a tracked `list<T>` with `mixed`. A keyed write
    // records `mixed` so the key itself still shows up in the shape.
    if append && rhs_types.is_empty() {
        return;
    }
    let value_php_type = if rhs_types.is_empty() {
        PhpType::mixed()
    } else {
        ResolvedType::types_joined(&rhs_types)
    };
    let base_type = scope
        .get(base_name)
        .last()
        .map(|rt| rt.type_string.clone())
        .unwrap_or_else(PhpType::array);

    // If the base variable is an object (e.g. SplObjectStorage, ArrayAccess),
    // array-access syntax invokes offsetSet, not actual array mutation.
    // Preserve the original object type instead of overwriting it with an array shape.
    if base_type.is_object_like() && !base_type.is_array_like() {
        return;
    }

    // If the base variable is a string, bracket-indexed assignment
    // (`$str[0] = 'z'`) modifies the string in-place — the variable
    // remains a string, it does NOT become an array.
    if base_type.is_string_subtype() {
        scope.set(
            base_name,
            vec![ResolvedType::from_type_string(PhpType::string())],
        );
        return;
    }

    let mut write_keys: Vec<super::super::resolution::ArrayWriteKey> = key_chain
        .iter()
        .map(
            |idx| match super::super::resolution::extract_array_key_for_shape(idx) {
                Some(key) => super::super::resolution::ArrayWriteKey::Shape(key),
                None => {
                    let index_types = resolve_rhs_with_scope(idx, scope, ctx);
                    super::super::resolution::ArrayWriteKey::Keyed {
                        key_type: super::super::resolution::infer_array_key_type(idx, &index_types),
                        slot: super::super::resolution::extract_array_write_index(idx),
                    }
                }
            },
        )
        .collect();
    if append {
        write_keys.push(super::super::resolution::ArrayWriteKey::Append);
    }

    let merged = super::super::resolution::merge_nested_array_write(
        &base_type,
        &write_keys,
        &value_php_type,
    );
    scope.set(base_name, vec![ResolvedType::from_type_string(merged)]);

    // A keyed write is authoritative for the element it targets, so it
    // must overwrite any synthetic scope key (`$tmp[$key]`, `$a["x"]`)
    // narrowing left behind for that same subject. Left stale, a
    // narrowed-to-null entry from an `isset`/`!isset` guard survives past
    // the write that just proved the key present, and resurfaces when
    // this branch's scope merges back with one where the key was proven
    // present a different way — see `apply_null_narrowing_truthy`'s
    // `extract_not_isset_vars` arm, which narrows the synthetic key to
    // null before the guarded body ever runs. An append (`$var[] = …`)
    // has no addressable key to overwrite and is skipped.
    if !append && let Some(key) = array_write_synthetic_key(base_name, key_chain) {
        // `rhs_types`, not `value_php_type`: the latter is a plain
        // `PhpType` string flattened for the shape merge above, which
        // drops the `class_info` a member-access completion on the
        // synthetic key (`$result["user"]->`) needs.
        let synthetic_types = if rhs_types.is_empty() {
            vec![ResolvedType::from_type_string(PhpType::mixed())]
        } else {
            rhs_types
        };
        scope.set(&key, synthetic_types);
    }
}

/// Render the synthetic scope key a keyed write targets, matching the key
/// text [`narrowing::expr_to_subject_key`] builds for a read of the same
/// subject (`$tmp[$key]`, `$a["x"][$i]`), so a write can find and
/// overwrite whatever narrowing recorded under that key.
fn array_write_synthetic_key(base_name: &str, key_chain: &[&Expression<'_>]) -> Option<String> {
    let mut key = base_name.to_string();
    for index in key_chain {
        if let Some(literal) = narrowing::array_index_literal_key(index) {
            key.push_str(&format!("[\"{literal}\"]"));
        } else {
            let index_key = narrowing::array_index_key(index)?;
            // `expr_to_subject_key`'s `array_access_subject_key` only
            // renders a non-literal index that reads a variable
            // (`contains('$')`); an index that writes, concatenates, or
            // compares is not the same subject a read of it renders, so
            // there is no synthetic key to find.
            if !index_key.contains('$') {
                return None;
            }
            key.push_str(&format!("[{index_key}]"));
        }
    }
    Some(key)
}

/// Process pass-by-reference parameter type inference.
pub(crate) fn process_pass_by_ref<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // `$ok = preg_match($p, $s, $m);` makes the same call, and writes the
    // same out-parameter, as the bare `preg_match($p, $s, $m);` statement
    // does; none of the three passes below recognise anything but a call,
    // so the assignment has to come off first.
    let (expr, assigned) = pass_by_ref_call_expr(expr);

    // The assignment lands once the call has returned, so a variable that
    // is both the statement's target and an out-parameter of its call
    // (`$file = end($file);`) ends up holding what was assigned to it, not
    // what the callee wrote through the reference.  Everything the passes
    // below decide about it is put back afterwards.
    let assigned_before: Vec<(&str, Option<Vec<ResolvedType>>)> = assigned
        .iter()
        .map(|name| (*name, scope.locals.get(&atom(name)).cloned()))
        .collect();

    // When a function call passes a variable to a parameter declared
    // as `Type &$param`, the variable acquires that type after the call.
    //
    // We need to check both variables already in scope AND variables
    // that appear as arguments but don't exist in scope yet (e.g.
    // `$matches` in `preg_match($pattern, $subject, $matches)`).
    //
    // Phase 1: use the existing `try_apply_pass_by_reference_type`
    // infrastructure for variables already in scope (works for class
    // types like `Type &$param`).
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = |var_name: &str| -> Vec<ResolvedType> {
        scope_snapshot
            .get(&atom(var_name))
            .cloned()
            .unwrap_or_default()
    };

    // Collect all variable names that appear as arguments in this
    // expression, including ones not yet in scope.
    let mut all_var_names: Vec<String> = scope.locals.keys().map(|k| k.to_string()).collect();
    for arg_var in extract_call_arg_variables(expr) {
        if !all_var_names.contains(&arg_var) {
            all_var_names.push(arg_var);
        }
    }

    for var_name in all_var_names {
        let var_ctx = VarResolutionCtx {
            var_name: &var_name,
            current_class: ctx.current_class,
            all_classes: ctx.all_classes,
            content: ctx.content,
            cursor_offset: ctx.cursor_offset,
            class_loader: ctx.class_loader,
            backend: ctx.backend,
            loaders: ctx.loaders,
            resolved_class_cache: ctx.resolved_class_cache,
            enclosing_return_type: ctx.enclosing_return_type.clone(),
            top_level_scope: ctx.top_level_scope.clone(),
            branch_aware: false,
            match_arm_narrowing: HashMap::new(),
            scope_var_resolver: Some(&scope_resolver),
            scope_proofs: Some(scope.proofs()),
        };
        let before = scope.get(&var_name).to_vec();
        let mut results = before.clone();
        super::super::resolution::try_apply_pass_by_reference_type(
            expr,
            &var_ctx,
            &mut results,
            false,
        );
        if resolved_types_differ(&results, &before) {
            scope.set(&var_name, results);
        }
    }

    // Phase 2: for variables NOT yet in scope that are passed to
    // pass-by-reference parameters with primitive type hints (e.g.
    // `array &$matches` in `preg_match`), store the type hint
    // directly.  `try_apply_pass_by_reference_type` only produces
    // results for class-based type hints; primitive types like
    // `array`, `int`, `string` return empty from
    // `type_hint_to_classes_typed` and are missed.
    seed_pass_by_ref_primitives(expr, scope, ctx);

    for (name, before) in assigned_before {
        let key = atom(name);
        match before {
            Some(types) => scope.locals.insert(key, types),
            None => scope.locals.remove(&key),
        };
    }
}

/// The call an expression statement makes, and the variables it assigns the
/// result to.
///
/// A statement that stores the call's result (`$ok = f($out);`, and the
/// chained `$a = $b = f($out);`) still makes the call and still lets the
/// callee write through `$out`, so the assignment wrapper is looked
/// through before the by-reference passes read the expression.  The
/// assigned names come back with it because the assignment outlives the
/// call: `$file = end($file);` leaves `$file` holding what `end()`
/// returned, not the `array|object` its parameter is declared as.
fn pass_by_ref_call_expr<'b>(expr: &'b Expression<'b>) -> (&'b Expression<'b>, Vec<&'b str>) {
    let mut assigned = Vec::new();
    let mut inner = expr;
    loop {
        match inner {
            Expression::Assignment(assignment) => {
                if let Expression::Variable(Variable::Direct(dv)) = assignment.lhs {
                    assigned.push(bytes_to_str(dv.name));
                }
                inner = assignment.rhs;
            }
            Expression::Parenthesized(paren) => inner = paren.expression,
            _ => break,
        }
    }
    (inner, assigned)
}

/// Seed PHP superglobals (`$_SERVER`, `$_GET`, `$_POST`, etc.) into the
/// scope as `array` so that accesses on them resolve correctly.
/// PHP makes these available in every scope without
/// an explicit `global` declaration.
pub(crate) fn seed_superglobals(scope: &mut ScopeState) {
    let array_type = vec![ResolvedType::from_type_string(PhpType::named(atom(
        "array",
    )))];
    for name in [
        "$_SERVER",
        "$_GET",
        "$_POST",
        "$_COOKIE",
        "$_REQUEST",
        "$_FILES",
        "$_ENV",
        "$_SESSION",
        "$GLOBALS",
    ] {
        scope.set(name, array_type.clone());
    }
}

/// Recursively walk an expression tree to find function call
/// sub-expressions and seed pass-by-reference primitive types for each.
/// This handles patterns like `if (preg_match($pattern, $subject, $matches))`
/// and `if (preg_match(..., $matches) === 1)` where the call is nested
/// inside a comparison or logical expression rather than appearing as a
/// standalone expression statement.
///
/// Only uses [`seed_pass_by_ref_primitives`] (not the full
/// [`process_pass_by_ref`]) to avoid triggering recursive variable
/// resolution through `try_apply_pass_by_reference_type`, which would
/// inflate the fallthrough counter for every variable already in scope.
pub(crate) fn seed_pass_by_ref_in_condition<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    match expr {
        // Direct call expressions — seed primitive pass-by-ref types.
        Expression::Call(_) => {
            seed_pass_by_ref_primitives(expr, scope, ctx);
        }
        // Binary operators (e.g. `preg_match(...) === 1`, `a && b`)
        // — recurse into both sides.
        Expression::Binary(bin) => {
            seed_pass_by_ref_in_condition(bin.lhs, scope, ctx);
            seed_pass_by_ref_in_condition(bin.rhs, scope, ctx);
        }
        // Unary prefix (e.g. `!preg_match(...)`) — recurse into operand.
        Expression::UnaryPrefix(unary) => {
            seed_pass_by_ref_in_condition(unary.operand, scope, ctx);
        }
        // Unary postfix — recurse into operand.
        Expression::UnaryPostfix(unary) => {
            seed_pass_by_ref_in_condition(unary.operand, scope, ctx);
        }
        // Parenthesized — recurse into inner expression.
        Expression::Parenthesized(paren) => {
            seed_pass_by_ref_in_condition(paren.expression, scope, ctx);
        }
        // Assignment in condition (e.g. `if ($x = preg_match(..., $m))`)
        // — recurse into the RHS.
        Expression::Assignment(assignment) => {
            seed_pass_by_ref_in_condition(assignment.rhs, scope, ctx);
        }
        _ => {}
    }
}

/// For each variable argument in a call expression that is passed to a
/// pass-by-reference parameter with a primitive type hint (e.g.
/// `array &$matches`), seed or refresh the variable in scope. Existing exact
/// values must be invalidated because the callee may assign any value allowed
/// by the parameter type. This complements [`process_pass_by_ref`] which
/// handles class-typed parameters via `try_apply_pass_by_reference_type`.
pub(crate) fn seed_pass_by_ref_primitives<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // `preg_match`'s `$matches` has no other by-reference parameter beside
    // it, so nothing below is left to do once the pattern has typed it.
    if seed_preg_matches(expr, scope, ctx) {
        return;
    }

    // Resolve the called function/method's parameters.
    let (arg_list, parameters, template_owner) = match expr {
        Expression::Call(Call::Function(func_call)) => {
            let func_name = match func_call.function {
                Expression::Identifier(ident) => bytes_to_str(ident.value()).to_string(),
                _ => return,
            };
            let func_name_offset = func_call.function.span().start.offset;
            let fl = match ctx.loaders.function_loader {
                Some(fl) => fl,
                None => return,
            };
            let func_info = match fl(&func_name, func_name_offset) {
                Some(fi) => fi,
                None => return,
            };
            let parameters = func_info.parameters.clone();
            (
                &func_call.argument_list,
                parameters,
                OutParamCallee::Function(Box::new(func_info)),
            )
        }
        Expression::Call(Call::Method(mc)) => {
            let method_name = match &mc.method {
                ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value).to_string(),
                _ => return,
            };
            let receiver_class = match mc.object {
                Expression::Variable(Variable::Direct(dv)) if dv.name == b"$this" => {
                    Some(ctx.current_class.name.to_string())
                }
                Expression::Variable(Variable::Direct(dv)) => {
                    let types = scope.get(bytes_to_str(dv.name));
                    types.iter().find_map(|rt| {
                        let name = rt.type_string.base_name()?;
                        if crate::php_type::is_primitive_scalar_name(name) {
                            None
                        } else {
                            Some(name.to_string())
                        }
                    })
                }
                _ => return,
            };
            let class_name = match receiver_class {
                Some(n) => n,
                None => return,
            };
            let cls = match (ctx.class_loader)(&class_name) {
                Some(c) => c,
                None => return,
            };
            let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                &cls,
                ctx.class_loader,
                ctx.resolved_class_cache,
            );
            let method = match merged.get_method(&method_name) {
                Some(m) => m,
                None => return,
            };
            (
                &mc.argument_list,
                method.parameters.clone(),
                OutParamCallee::Method(merged, atom(&method_name)),
            )
        }
        Expression::Call(Call::NullSafeMethod(mc)) => {
            let method_name = match &mc.method {
                ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value).to_string(),
                _ => return,
            };
            let receiver_class = match mc.object {
                Expression::Variable(Variable::Direct(dv)) if dv.name == b"$this" => {
                    Some(ctx.current_class.name.to_string())
                }
                Expression::Variable(Variable::Direct(dv)) => {
                    let types = scope.get(bytes_to_str(dv.name));
                    types.iter().find_map(|rt| {
                        let name = rt.type_string.base_name()?;
                        if crate::php_type::is_primitive_scalar_name(name) {
                            None
                        } else {
                            Some(name.to_string())
                        }
                    })
                }
                _ => return,
            };
            let class_name = match receiver_class {
                Some(n) => n,
                None => return,
            };
            let cls = match (ctx.class_loader)(&class_name) {
                Some(c) => c,
                None => return,
            };
            let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                &cls,
                ctx.class_loader,
                ctx.resolved_class_cache,
            );
            let method = match merged.get_method(&method_name) {
                Some(m) => m,
                None => return,
            };
            (
                &mc.argument_list,
                method.parameters.clone(),
                OutParamCallee::Method(merged, atom(&method_name)),
            )
        }
        Expression::Call(Call::StaticMethod(sc)) => {
            let method_name = match &sc.method {
                ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value).to_string(),
                _ => return,
            };
            let class_name = match sc.class {
                Expression::Self_(_) | Expression::Static(_) => ctx.current_class.name.to_string(),
                Expression::Parent(_) => match ctx.current_class.parent_class {
                    Some(p) => p.to_string(),
                    None => return,
                },
                Expression::Identifier(ident) => bytes_to_str(ident.value()).to_string(),
                _ => return,
            };
            let cls = match (ctx.class_loader)(&class_name) {
                Some(c) => c,
                None => return,
            };
            let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                &cls,
                ctx.class_loader,
                ctx.resolved_class_cache,
            );
            let method = match merged.get_method(&method_name) {
                Some(m) => m,
                None => return,
            };
            (
                &sc.argument_list,
                method.parameters.clone(),
                OutParamCallee::Method(merged, atom(&method_name)),
            )
        }
        _ => return,
    };

    // Bind arguments to parameters following PHP's rules so a named argument
    // seeds the parameter it actually targets, not the one at its ordinal
    // position in the call.
    let bound = crate::call_args::bind_args_to_params(&parameters, arg_list);

    // An out type written in the callee's own `@template` params
    // (`usort`'s `array<TKey, TValue> &$array`) describes the caller's
    // variable only once those params are bound, and this pass has no
    // binding for them. Applied raw it would replace a precise argument
    // type (`TargetClass<ContainerExtension>[]`) with an array of a name
    // nothing can resolve, so the variable is left as it was instead.
    let callee_templates = template_owner.template_params();

    for (param_index, (param, arg_expr)) in parameters.iter().zip(bound.iter()).enumerate() {
        let arg_expr = match arg_expr {
            Some(expr) => *expr,
            None => continue,
        };

        // Only handle direct variable arguments.
        let var_name = match arg_expr {
            Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
            _ => continue,
        };

        // Check if the corresponding parameter is pass-by-reference.
        if !param.is_reference {
            continue;
        }

        let already_in_scope = !scope.get(&var_name).is_empty();
        let mut seeded = false;
        if let Some(out_hint) = effective_out_type(param, param_index, &template_owner, ctx.backend)
        {
            if out_hint.references_any_name(callee_templates) {
                continue;
            }
            // A variadic parameter's stored PHPDoc type may describe the
            // collected argument array (`string[] &$values`), while each
            // call-site variable is one element of that collection. Native
            // element hints such as `string &...$values` are already scalar
            // and therefore pass through unchanged.
            let effective_hint = if param.is_variadic {
                out_hint
                    .iterable_element_type()
                    .unwrap_or_else(|| out_hint.clone())
            } else {
                out_hint
            };
            let primitive_hint = match effective_hint.kind() {
                TypeKind::Union(members) | TypeKind::Intersection(members) => {
                    !members.is_empty() && members.iter().all(PhpType::is_scalar)
                }
                _ => effective_hint.is_scalar(),
            };
            if primitive_hint {
                // The callee may assign any value the parameter type allows,
                // so an exact value observed before the call is stale. What
                // the call cannot invalidate is precision the hint does not
                // contradict: `array_shift(array &$array)` says nothing about
                // the element type of a `Node[]` argument. Keep a value the
                // hint already covers, minus its literal precision, and fall
                // back to the hint only when the two genuinely disagree.
                let existing = scope.get(&var_name);
                let refined = (!existing.is_empty())
                    .then(|| ResolvedType::types_joined(existing))
                    // An empty array shape (`$matches = [];` before
                    // `preg_match_all(…, $matches)`) records no element
                    // precision, only that nothing had been written yet —
                    // which is exactly what the call invalidates. It is a
                    // subtype of every array hint, so without this it would
                    // survive the call and claim the result is still empty.
                    // A bare `null` (`$key = null;` before the call) says the
                    // same thing about a nullable out type, and keeping it
                    // would claim the callee wrote nothing at all.
                    .filter(|existing| !existing.shape_entries().is_some_and(<[_]>::is_empty))
                    .filter(|existing| !existing.is_null())
                    .filter(|existing| existing.is_subtype_of(&effective_hint))
                    .map(|existing| existing.widen_scalar_literals())
                    .unwrap_or(effective_hint);
                scope.set(&var_name, vec![ResolvedType::from_type_string(refined)]);
                seeded = true;
            }
        }
        if !seeded && !already_in_scope && param.type_hint.is_none() {
            // Untyped pass-by-reference parameters (e.g. `&$matches`
            // in `preg_match`, `&$result` in `parse_str`) are most
            // commonly arrays. Seed only new variables as `array`; an
            // existing value has no sounder replacement without a hint,
            // and neither has one the callee's body did not give up.
            scope.set(
                &var_name,
                vec![ResolvedType::from_type_string(PhpType::named(atom(
                    "array",
                )))],
            );
        }
    }
}

/// Type `$matches` from the capture groups of the pattern a
/// `preg_match`/`preg_match_all` call passes.
///
/// The parameter is declared `?array &$matches`, and a bare `array` is all
/// the generic by-reference seeding above can offer. A literal pattern says
/// more: which keys the array has, and which of them a successful match may
/// leave out.
///
/// The call site is not the place that knows whether the match succeeded, so
/// what lands in the scope here is what the call leaves either way. A branch
/// guarded on the outcome narrows it down (see
/// [`apply_preg_match_narrowing`]).
///
/// Returns whether the variable was typed. A pattern the group walk refuses,
/// or a `$flags` argument that does not resolve to a constant, leaves the
/// call to the generic path.
fn seed_preg_matches<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let Some(call) = crate::type_engine::regex_shape::preg_call(expr) else {
        return false;
    };
    let Some(matched) = preg_matched_type(&call, scope, ctx) else {
        return false;
    };
    scope.set(
        call.matches_var,
        vec![ResolvedType::from_type_string(
            crate::type_engine::regex_shape::or_no_match(matched, call.matches_all),
        )],
    );
    true
}

/// The type a *successful* match leaves in the out-parameter of `call`.
///
/// `None` when the analysis refuses the call: a pattern whose group list the
/// walk cannot read, or a `$flags` argument that does not resolve to a
/// constant whose bits the shape analysis models.
pub(crate) fn preg_matched_type<'b>(
    call: &crate::type_engine::regex_shape::PregCall<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<PhpType> {
    let flags = match call.flags {
        None => 0,
        Some(flags) => preg_flag_bits(flags, scope, ctx)?,
    };
    call.pattern
        .as_deref()
        .and_then(|pattern| {
            crate::type_engine::regex_shape::matches_type(pattern, flags, call.matches_all)
        })
        .or_else(|| crate::type_engine::regex_shape::opaque_matches_type(flags, call.matches_all))
}

/// The flag mask a `preg_match` `$flags` argument holds.
///
/// Resolved through the shared pipeline so a named constant, a class
/// constant, and a variable holding one all read the same. Returns `None`
/// when the argument does not resolve to a single integer value, or to one
/// whose bits the shape analysis does not model — the result's shape depends
/// on the flags, so a mask that cannot be read is not one to guess at.
fn preg_flag_bits<'b>(
    expr: &'b Expression<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<i64> {
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = |var_name: &str| -> Vec<ResolvedType> {
        scope_snapshot
            .get(&atom(var_name))
            .cloned()
            .unwrap_or_default()
    };
    let var_ctx = VarResolutionCtx {
        var_name: "",
        current_class: ctx.current_class,
        all_classes: ctx.all_classes,
        content: ctx.content,
        cursor_offset: ctx.cursor_offset,
        class_loader: ctx.class_loader,
        backend: ctx.backend,
        loaders: ctx.loaders,
        resolved_class_cache: ctx.resolved_class_cache,
        enclosing_return_type: ctx.enclosing_return_type.clone(),
        top_level_scope: ctx.top_level_scope.clone(),
        branch_aware: false,
        match_arm_narrowing: HashMap::new(),
        scope_var_resolver: Some(&scope_resolver),
        scope_proofs: Some(scope.proofs()),
    };
    let flags =
        crate::type_engine::variable::foreach_resolution::resolve_expression_type(expr, &var_ctx)
            .as_ref()
            .and_then(crate::type_engine::types::const_fold::literal_int_value)?;
    crate::type_engine::regex_shape::flags_are_modelled(flags).then_some(flags)
}

/// Extract all `$variable` names that appear as direct arguments in a
/// call expression.  Used by [`process_pass_by_ref`] to discover
/// variables that may be introduced by pass-by-reference parameters
/// (e.g. `$matches` in `preg_match($pattern, $subject, $matches)`).
pub(crate) fn extract_call_arg_variables<'b>(expr: &'b Expression<'b>) -> Vec<String> {
    let arg_list = match expr {
        Expression::Call(Call::Function(fc)) => &fc.argument_list,
        Expression::Call(Call::Method(mc)) => &mc.argument_list,
        Expression::Call(Call::NullSafeMethod(mc)) => &mc.argument_list,
        Expression::Call(Call::StaticMethod(sc)) => &sc.argument_list,
        Expression::Instantiation(inst) => match &inst.argument_list {
            Some(al) => al,
            None => return vec![],
        },
        _ => return vec![],
    };
    let mut vars = Vec::new();
    for arg in arg_list.arguments.iter() {
        let arg_expr = match arg {
            Argument::Positional(pos) => pos.value,
            Argument::Named(named) => named.value,
        };
        if let Expression::Variable(Variable::Direct(dv)) = arg_expr {
            vars.push(bytes_to_str(dv.name).to_string());
        }
    }
    vars
}

/// The guard functions whose first argument is a condition the call
/// leaves proven, paired with that parameter's name and with whether the
/// condition still holds once the call returns.
///
/// `assert()` proves its argument true outright.  Laravel's
/// `abort_unless()` / `throw_unless()` prove it true by bailing out when
/// it is false, and `abort_if()` / `throw_if()` prove its *negation* the
/// same way.  Neither the framework nor any stub annotates these four
/// with `@phpstan-assert`, so the name is the only signal there is, and
/// this is the set the Laravel PHPStan extensions special-case too.
const CONDITION_GUARD_FUNCTIONS: [(&str, &str, bool); 5] = [
    ("assert", "assertion", true),
    ("abort_if", "boolean", false),
    ("abort_unless", "boolean", true),
    ("throw_if", "condition", false),
    ("throw_unless", "condition", true),
];

/// The condition a [guard call](CONDITION_GUARD_FUNCTIONS) proves and
/// whether it holds after the call, or `None` when `expr` is not one.
///
/// Matches every spelling PHP accepts: unqualified, fully-qualified
/// (`\assert`), and any letter case.
fn guard_call_condition<'b>(expr: &'b Expression<'b>) -> Option<(&'b Expression<'b>, bool)> {
    let Expression::Call(Call::Function(fc)) = unwrap_parens(expr) else {
        return None;
    };
    let Expression::Identifier(ident) = fc.function else {
        return None;
    };
    let raw = bytes_to_str(ident.value());
    let called = raw.strip_prefix('\\').unwrap_or(raw);
    let (_, param, holds_after) = CONDITION_GUARD_FUNCTIONS
        .iter()
        .find(|(name, _, _)| called.eq_ignore_ascii_case(name))?;

    // A named argument may sit anywhere in the list, so `abort_if(code:
    // 404, boolean: $x === null)` still has to reach the condition.
    let condition = match fc.argument_list.arguments.first()? {
        Argument::Positional(pos) => pos.value,
        Argument::Named(_) => fc
            .argument_list
            .arguments
            .iter()
            .find_map(|arg| match arg {
                Argument::Named(named)
                    if bytes_to_str(named.name.value).eq_ignore_ascii_case(param) =>
                {
                    Some(named.value)
                }
                _ => None,
            })?,
    };
    Some((condition, *holds_after))
}

/// Process assert narrowing (assert($x instanceof Foo), @phpstan-assert, etc.)
pub(crate) fn process_assert_narrowing<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Every narrowing path below only fires for a (possibly parenthesized)
    // call expression, so a non-call statement can never be an assert() /
    // custom type-guard call. Bail out before the scope clone below, which
    // otherwise runs once per in-scope variable for every statement.
    let unwrapped = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    if !matches!(unwrapped, Expression::Call(_)) {
        return;
    }

    let guard = guard_call_condition(expr);

    // ── Handle assert($x instanceof Foo) for variables NOT yet in scope ──
    // When a foreach binds a variable but the iterable element type is
    // unknown, the variable won't be in the scope map.  A subsequent
    // `assert($x instanceof Foo)` (or `abort_unless($x instanceof Foo,
    // 403)`) should add it with the asserted type.
    if let Some((condition, true)) = guard
        && let Expression::Binary(bin) = condition
        && bin.operator.is_instanceof()
        && let Expression::Variable(Variable::Direct(dv)) = bin.lhs
    {
        let var_name = bytes_to_str(dv.name).to_string();
        if scope.get(&var_name).is_empty() {
            // Variable not in scope — seed it with the asserted type.
            let class_name = match bin.rhs {
                Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
                Expression::Self_(_) => Some(ctx.current_class.name.to_string()),
                Expression::Static(_) => Some(ctx.current_class.name.to_string()),
                Expression::Parent(_) => ctx.current_class.parent_class.map(|a| a.to_string()),
                _ => None,
            };
            if let Some(name) = class_name {
                let resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
                    &PhpType::named(atom(&name)),
                    &ctx.current_class.name,
                    ctx.all_classes,
                    ctx.class_loader,
                );
                if !resolved.is_empty() {
                    scope.set(
                        &var_name,
                        ResolvedType::from_classes_with_hint(resolved, PhpType::named(atom(&name))),
                    );
                } else {
                    scope.set(
                        &var_name,
                        vec![ResolvedType::from_type_string(PhpType::named(atom(&name)))],
                    );
                }
            }
        }
    }

    // Seed property/array-access subject keys that appear as arguments
    // to the assert call (e.g. `assertInstanceOf(X::class, $view->component)`
    // or a `@phpstan-assert` helper called on `$arg->value`) so the
    // narrowing loop below can find and narrow them.
    seed_assert_arg_subject_keys(expr, scope, ctx);

    // Re-export narrowing: PHPUnit's `assertTrue()` / `assertFalse()` carry
    // `@psalm-assert true/false $condition`.  When the argument is a boolean
    // condition expression (e.g. `property_exists($x, 'p')`), proving it
    // true/false is equivalent to a guard on that condition, so run the
    // standard condition-narrowing pipeline on the argument.
    let reexport_conditions = {
        let reexport_snapshot = scope.locals.clone();
        let reexport_resolver = |vn: &str| -> Vec<ResolvedType> {
            reexport_snapshot
                .get(&atom(vn))
                .cloned()
                .unwrap_or_default()
        };
        let reexport_ctx = build_var_ctx("", ctx, &reexport_resolver);
        narrowing::collect_assert_reexport_conditions(expr, &reexport_ctx)
    };
    for (condition, asserts_true) in reexport_conditions {
        if asserts_true {
            apply_condition_narrowing(condition, scope, ctx);
        } else {
            apply_condition_narrowing_inverse(condition, scope, ctx);
        }
    }

    // A conditional return type whose `never` branch some argument value
    // would have selected proves that value never reached the call.
    apply_never_branch_narrowing(unwrapped, scope, ctx);

    // `assert(<condition>)` proves its argument true for everything that
    // follows in the same scope, exactly the way entering `if (<condition>)`
    // proves it for the block body.  `abort_if(<condition>, 404)` and its
    // siblings prove the same thing about the branch that survives them,
    // just in the polarity their name picks.  Feeding the argument into the
    // same pipeline the `if` takes means every guard form is honoured in
    // both places: `$x !== null`, `$x !== false`, `is_string($x)`, `&&`
    // chains, and so on.
    if let Some((condition, holds_after)) = guard {
        if holds_after {
            apply_condition_narrowing(condition, scope, ctx);
        } else {
            apply_condition_narrowing_inverse(condition, scope, ctx);
        }
    }

    // Apply assert narrowing to each variable in scope.
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = |var_name: &str| -> Vec<ResolvedType> {
        scope_snapshot
            .get(&atom(var_name))
            .cloned()
            .unwrap_or_default()
    };
    let var_names: Vec<Atom> = scope.locals.keys().copied().collect();
    for var_name in var_names {
        let var_ctx = VarResolutionCtx {
            var_name: &var_name,
            current_class: ctx.current_class,
            all_classes: ctx.all_classes,
            content: ctx.content,
            cursor_offset: ctx.cursor_offset,
            class_loader: ctx.class_loader,
            backend: ctx.backend,
            loaders: ctx.loaders,
            resolved_class_cache: ctx.resolved_class_cache,
            enclosing_return_type: ctx.enclosing_return_type.clone(),
            top_level_scope: ctx.top_level_scope.clone(),
            branch_aware: false,
            match_arm_narrowing: HashMap::new(),
            scope_var_resolver: Some(&scope_resolver),
            scope_proofs: Some(scope.proofs()),
        };
        let before = scope.get(&var_name).to_vec();
        let mut results = before.clone();

        // @phpstan-assert / @psalm-assert
        let mut type_guard: Option<(narrowing::TypeGuardKind, bool)> = None;
        let mut intersected = false;
        ResolvedType::apply_narrowing(&mut results, |classes| {
            narrowing::try_apply_custom_assert_narrowing(
                expr,
                &var_ctx,
                classes,
                &mut type_guard,
                &mut intersected,
            )
        });
        // The assertion proved a class the subject does not nominally
        // implement, so the entries describe one value that is all of them
        // rather than a choice between them.  Untagged they join as a
        // union, which satisfies neither half's declared type.
        if intersected {
            ResolvedType::tag_as_intersection(&mut results);
        }

        // A scalar / pseudo-type assertion (`assertIsString`, `assertIsObject`,
        // `assertIsArray`, their `assertIsNot*` negations, or the `object`
        // fallback for an unresolvable `assertInstanceOf` class argument) is a
        // type guard, not a class narrowing.  Apply it on the full resolved
        // types so union members are kept or dropped by category — e.g.
        // `assertIsObject` drops null/scalar members while keeping the class,
        // and `assertIsNotObject` drops the class.
        if let Some((kind, exclude)) = type_guard {
            if exclude {
                narrowing::apply_type_guard_exclusion(kind, &mut results, Some(ctx.class_loader));
            } else {
                narrowing::apply_type_guard_inclusion(kind, &mut results, Some(ctx.class_loader));
            }
        }

        // A not-null assertion (`@phpstan-assert !null $x`, e.g. PHPUnit's
        // `assertNotNull`) removes the `null` pseudo-type, which the
        // class-based exclusion above cannot express.  Strip null from the
        // subject's resolved types directly so a value that was tracked as
        // exactly `null` (e.g. after `$obj->prop = null;`) no longer reads
        // as null after the assertion.
        if narrowing::call_asserts_not_null(expr, &var_ctx) {
            results.retain_mut(|rt| match rt.type_string.non_null_type() {
                Some(non_null) => {
                    rt.type_string = non_null;
                    true
                }
                None => rt.type_string != PhpType::null(),
            });
        }

        if resolved_types_differ(&results, &before) {
            if results.is_empty() {
                // Narrowing removed all types (e.g. assert($x instanceof
                // UnresolvableClass)).  Explicitly clear the variable so
                // that diagnostics see "unknown type" and suppress false
                // positives.  `scope.set()` is a no-op for empty vecs.
                scope.set_untyped(&var_name);
            } else {
                scope.set(&var_name, results);
            }
        }
    }
}

/// `@psalm-this-out` / `@phpstan-self-out`: a call to a method carrying
/// this annotation changes the type the walker tracks for its receiver,
/// the way an assignment changes a variable's type.  Method-level
/// template parameters bound from the call's arguments are substituted
/// into the annotation's type before it replaces the receiver's tracked
/// type: `$box->replace('x')` on a `MutableBox<int> $box`, where
/// `replace(U $value)` declares `@psalm-this-out self<U>`, re-binds
/// `$box` to `MutableBox<string>` for the rest of the block.
///
/// Only fires for a receiver that is a plain variable already in scope
/// with a resolved class — `$this` is excluded because there is no
/// receiver variable to re-bind.
pub(crate) fn process_self_out_narrowing<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let unwrapped = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    let (object, method, argument_list) = match unwrapped {
        Expression::Call(Call::Method(mc)) => (mc.object, &mc.method, &mc.argument_list),
        Expression::Call(Call::NullSafeMethod(mc)) => (mc.object, &mc.method, &mc.argument_list),
        _ => return,
    };
    let ClassLikeMemberSelector::Identifier(ident) = method else {
        return;
    };
    let Expression::Variable(Variable::Direct(dv)) = object else {
        return;
    };
    let var_name = bytes_to_str(dv.name);
    if var_name == "$this" {
        return;
    }
    let method_name = bytes_to_str(ident.value).to_string();

    let before = scope.get(var_name).to_vec();
    if before.is_empty() {
        return;
    }
    // Cheap check before the template substitution machinery below runs:
    // bail out unless at least one branch's class actually declares a
    // self-out type for this method.
    if !before.iter().any(|rt| {
        rt.class_info
            .as_ref()
            .and_then(|c| c.get_method_ci(&method_name))
            .is_some_and(|m| m.self_out.is_some())
    }) {
        return;
    }

    let arg_texts = crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast(
        argument_list,
        ctx.content,
    );
    let arg_refs: Vec<&str> = arg_texts.iter().map(|s| s.as_str()).collect();

    let scope_snapshot = scope.locals.clone();
    let scope_resolver = |vn: &str| -> Vec<ResolvedType> {
        scope_snapshot.get(&atom(vn)).cloned().unwrap_or_default()
    };
    let var_ctx = VarResolutionCtx {
        var_name,
        current_class: ctx.current_class,
        all_classes: ctx.all_classes,
        content: ctx.content,
        cursor_offset: ctx.cursor_offset,
        class_loader: ctx.class_loader,
        backend: ctx.backend,
        loaders: ctx.loaders,
        resolved_class_cache: ctx.resolved_class_cache,
        enclosing_return_type: ctx.enclosing_return_type.clone(),
        top_level_scope: ctx.top_level_scope.clone(),
        branch_aware: false,
        match_arm_narrowing: HashMap::new(),
        scope_var_resolver: Some(&scope_resolver),
        scope_proofs: Some(scope.proofs()),
    };
    let rctx = var_ctx.as_resolution_ctx();

    let mut changed = false;
    let mut results: Vec<ResolvedType> = Vec::with_capacity(before.len());
    for rt in &before {
        let mutated = rt.class_info.as_ref().and_then(|owner| {
            let self_out = owner.get_method_ci(&method_name)?.self_out.clone()?;
            let template_subs = crate::type_engine::call_resolution::build_call_template_subs(
                owner,
                &method_name,
                &arg_refs,
                Some(&rt.type_string),
                &rctx,
            );
            let substituted = self_out.substitute(&template_subs).simplified();
            let final_ty = if substituted.contains_self_ref() {
                substituted.replace_self_with_type(&rt.type_string)
            } else {
                substituted
            };
            // Re-resolve the class for the new type rather than keeping the
            // receiver's existing `class_info`: that one still carries the
            // *old* template substitution, so members typed by a template
            // parameter would keep resolving to the pre-call binding.
            let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
                &final_ty,
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            );
            Some(if classes.is_empty() {
                vec![ResolvedType::from_type_string(final_ty)]
            } else {
                ResolvedType::from_classes_with_hint(classes, final_ty)
            })
        });
        match mutated {
            Some(new_rts) => {
                changed = true;
                results.extend(new_rts);
            }
            None => results.push(rt.clone()),
        }
    }

    if changed {
        scope.set(var_name, results);
    }
}

/// Compare two `ResolvedType` slices by their observable identity
/// (type string + class FQN).  `ResolvedType` intentionally does not
/// implement `PartialEq` because `ClassInfo` is a large struct where
/// field-by-field equality is too expensive and semantically wrong.
/// This lightweight comparison detects when narrowing changed the
/// resolved type (e.g. replaced `BaseCatalogFeature` with `self`).
pub(crate) fn resolved_types_differ(a: &[ResolvedType], b: &[ResolvedType]) -> bool {
    if a.len() != b.len() {
        return true;
    }
    for (ra, rb) in a.iter().zip(b.iter()) {
        if ra.type_string != rb.type_string {
            return true;
        }
        match (&ra.class_info, &rb.class_info) {
            (Some(ca), Some(cb)) => {
                if ca.fqn() != cb.fqn() {
                    return true;
                }
            }
            (None, None) => {}
            _ => return true,
        }
    }
    false
}

/// Subtract from each argument the values a `never` branch of the callee's
/// conditional return type rules out.
///
/// `throw_unless()`, `throw_if()`, `abort_unless()` and their family carry
/// no `@phpstan-assert` tag; they declare their effect in the return type
/// itself:
///
/// ```text
/// @return ($condition is false ? never : ($condition is non-empty-mixed ? TValue : never))
/// ```
///
/// A `null` argument lands on a `never` branch there, so a run that gets
/// past the call did not pass one, and the following scope can drop it.
/// This is the same subtraction the `if (!$x) { throw …; }` form already
/// gets, derived from the declaration instead of from a body.
fn apply_never_branch_narrowing<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let Expression::Call(call) = expr else {
        return;
    };

    let snapshot = scope.locals.clone();
    let resolver =
        |vn: &str| -> Vec<ResolvedType> { snapshot.get(&atom(vn)).cloned().unwrap_or_default() };
    let var_ctx = build_var_ctx("", ctx, &resolver);
    let Some(info) = narrowing::extract_conditional_return_call(call, &var_ctx) else {
        return;
    };

    // A subject named by an argument is keyed the same way a condition's
    // subject is, so a property path or a call argument narrows too.
    seed_assert_arg_subject_keys(expr, scope, ctx);

    for (index, parameter) in info.parameters.iter().enumerate() {
        let Some(argument) = info.argument_list.arguments.iter().nth(index) else {
            continue;
        };
        let arg_expr = narrowing::argument_value(argument);
        let Some(key) = narrowing::expr_to_subject_key(arg_expr) else {
            continue;
        };
        let current = scope.get(&key).to_vec();
        if current.is_empty() {
            continue;
        }
        let joined = ResolvedType::types_joined(&current);
        let ruled_out = narrowing::never_ruled_out_members(
            &info.return_type,
            &parameter.name,
            &joined,
            ctx.class_loader,
        );
        if ruled_out.is_empty() {
            continue;
        }
        let kept: Vec<ResolvedType> = current
            .into_iter()
            .filter(|rt| !ruled_out.iter().any(|out| out.equivalent(&rt.type_string)))
            .map(|mut rt| {
                if let Some(narrowed) = subtract_members(&rt.type_string, &ruled_out) {
                    rt.type_string = narrowed;
                }
                rt
            })
            .collect();
        if !kept.is_empty() {
            scope.set(&key, kept);
        }
    }
}

/// `ty` with every alternative `ruled_out` names removed, or `None` when
/// it names none of them.
///
/// A type with a single alternative is left alone: removing its only
/// member would leave nothing to describe the value with.
fn subtract_members(ty: &PhpType, ruled_out: &[PhpType]) -> Option<PhpType> {
    let members = narrowing::split_into_runtime_members(ty);
    if members.len() < 2 {
        return None;
    }
    let kept: Vec<PhpType> = members
        .iter()
        .filter(|member| !ruled_out.iter().any(|out| out.equivalent(member)))
        .cloned()
        .collect();
    if kept.is_empty() || kept.len() == members.len() {
        return None;
    }
    Some(PhpType::union(kept))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_line_comments_are_stepped_over_to_reach_a_docblock() {
        // Whole-line `//` and `#` comments separate the block from the
        // code but do not detach it.
        assert!(trim_trailing_line_comments("/** @var Foo $x */\n// short\n").ends_with("*/"));
        assert!(trim_trailing_line_comments("/** @var Foo $x */\n\n# short\n\n").ends_with("*/"));
        assert!(trim_trailing_line_comments("/** @var Foo $x */\n// a\n  // b\n").ends_with("*/"));

        // A `//` after code on the same line may be inside a string.
        assert_eq!(
            trim_trailing_line_comments("/** @var Foo $x */\n$url = 'https://example.test';"),
            "/** @var Foo $x */\n$url = 'https://example.test';"
        );

        // `#[` opens an attribute, not a comment.
        assert_eq!(
            trim_trailing_line_comments("/** @var Foo $x */\n#[Attr]"),
            "/** @var Foo $x */\n#[Attr]"
        );

        // A comment line longer than the look-back window is left alone
        // rather than triggering an unbounded backward scan.
        let long = format!("/** @var Foo $x */\n// {}\n", "x".repeat(600));
        assert!(!trim_trailing_line_comments(&long).ends_with("*/"));

        // Nothing but comments: the scan bottoms out at an empty string.
        assert_eq!(trim_trailing_line_comments("// only\n"), "");
        assert_eq!(trim_trailing_line_comments(""), "");
    }

    #[test]
    fn only_a_docblock_ends_the_backward_search_for_one() {
        assert_eq!(
            trailing_docblock("$a = 1; /** @var Foo $x */"),
            Some("/** @var Foo $x */")
        );
        // An ordinary block comment is where the search stops: reading
        // past it would take the statements in between for the body of
        // the docblock beyond them.
        assert_eq!(
            trailing_docblock("/** @var Foo $x */ $x = 1; /* note */"),
            None
        );
        assert_eq!(trailing_docblock("$a = 1;"), None);
        assert_eq!(trailing_docblock(""), None);
        // PHP block comments do not nest, so a `/*` written inside a
        // docblock is part of its body rather than a comment of its own.
        assert_eq!(
            trailing_docblock("/** @var Foo $x  see /* the note */"),
            Some("/** @var Foo $x  see /* the note */")
        );
    }

    #[test]
    fn var_docblock_with_closure_signature_binds_the_trailing_variable() {
        // A closure type's own `$`-prefixed parameter name must not be
        // mistaken for the variable the `@var` tag annotates.
        let pairs = parse_var_docblock_pairs(
            "/** @var \\Closure(\\App\\Models\\User $user): string $callback */",
        );
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, "$callback");
        assert_eq!(
            pairs[0].1.to_string(),
            "\\Closure(\\App\\Models\\User): string"
        );
    }

    #[test]
    fn increment_decrement_preserves_only_values_that_cannot_change() {
        let text = PhpType::literal_string_raw("'abc'");

        assert_eq!(
            type_after_increment_decrement(&text, IncrementDecrementKind::Increment),
            PhpType::string()
        );
        assert_eq!(
            type_after_increment_decrement(&text, IncrementDecrementKind::Decrement),
            text
        );
        assert_eq!(
            type_after_increment_decrement(
                &PhpType::named(atom("number")),
                IncrementDecrementKind::Increment
            ),
            PhpType::union(vec![PhpType::int(), PhpType::float()])
        );
        assert_eq!(
            type_after_increment_decrement(
                &PhpType::named(atom("Number")),
                IncrementDecrementKind::Increment
            ),
            PhpType::named(atom("Number"))
        );
    }
}
