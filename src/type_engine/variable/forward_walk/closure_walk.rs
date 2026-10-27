//! The diagnostic walk's statement loop and the closures it meets.
//!
//! Every closure and arrow function a statement holds is walked with its
//! captures and parameters seeded, the parameters from the signature of
//! the call it is passed to when it declares none, so the snapshots
//! recorded inside its body see the types the cursor walk would.

use super::*;
use std::borrow::Cow;

use mago_span::HasSpan;
use mago_syntax::cst::argument::Argument;
use mago_syntax::cst::sequence::TokenSeparatedSequence;
use mago_syntax::cst::variable::Variable;

use crate::atom::bytes_to_str;
use crate::php_type::PhpType;

/// Walk a sequence of statements for diagnostic scope building.
///
/// Unlike [`walk_body_forward`] (which stops at the cursor), this walks
/// the **entire** body and records a scope snapshot at every statement
/// boundary.  The snapshots are stored in the thread-local
/// [`DIAGNOSTIC_SCOPE`] cache.
///
/// For each statement, this also discovers closure and arrow function
/// expressions and walks their bodies with properly seeded scopes so
/// that variables inside closures are fully resolved.
pub(crate) fn walk_body_for_diagnostics<'b>(
    statements: impl Iterator<Item = &'b Statement<'b>>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    for stmt in statements {
        // Record the scope at this statement's start offset so that
        // any member-access span inside this statement can look up
        // variable types that were established by prior statements.
        record_scope_snapshot(stmt.span().start.offset, scope);

        // Snapshot the pre-statement scope for the closure walk below.
        // Closures and other references inside this statement's own
        // expression evaluate *before* the statement's assignment takes
        // effect, so they must see the pre-assignment types.  E.g. in
        // `$x = f(fn() => ..., $x)` the trailing `$x` argument must
        // resolve to `$x`'s type before the reassignment, not `f()`'s
        // result.
        let pre_stmt_scope = scope.clone();

        process_statement(stmt, scope, ctx);

        // Walk closure and arrow function bodies found in this
        // statement.  Each closure gets a fresh scope seeded with
        // `use()` variables from the outer scope and its own parameter
        // types (with callable inference from the enclosing call's
        // signature).  Arrow functions inherit the outer scope with
        // their parameter types added on top.  The body is fully
        // walked so that scope snapshots are recorded for every
        // statement inside the closure/arrow function.
        walk_closures_in_statement(stmt, &pre_stmt_scope, scope, ctx);

        // Also record at the statement's end offset, which covers
        // member accesses that appear after the last statement in
        // a block (e.g. the closing `}` region).
        record_scope_snapshot(stmt.span().end.offset, scope);
    }
}

/// Scan a **single** statement's direct expressions for closure/arrow
/// function literals and walk their bodies with properly seeded scopes.
///
/// This function intentionally does **not** recurse into nested block
/// bodies (if/while/foreach/try/switch).  Those bodies are walked by
/// [`walk_body_forward`], which calls this function for each statement
/// it processes — at that point the scope already reflects narrowing,
/// foreach bindings, and other context from the enclosing block.
///
/// Only the expressions that are directly part of this statement (the
/// condition expression, the iteration expression, echo values, etc.)
/// are scanned for closures.  Closures inside nested block bodies will
/// be picked up when `walk_body_forward` processes the inner statements.
pub(crate) fn walk_closures_in_statement<'b>(
    stmt: &'b Statement<'b>,
    outer_scope: &ScopeState,
    post_stmt_scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    match stmt {
        Statement::Expression(expr_stmt) => {
            let scope_for_walk =
                self_ref_capture_scope(expr_stmt.expression, outer_scope, post_stmt_scope);
            walk_closures_in_expr(expr_stmt.expression, &scope_for_walk, ctx, None);
        }
        Statement::Return(ret) => {
            if let Some(val) = ret.value {
                walk_closures_in_expr(val, outer_scope, ctx, None);
            }
        }
        Statement::Echo(echo) => {
            for val in echo.values.iter() {
                walk_closures_in_expr(val, outer_scope, ctx, None);
            }
        }
        Statement::EchoTag(echo) => {
            for val in echo.values.iter() {
                walk_closures_in_expr(val, outer_scope, ctx, None);
            }
        }
        // For compound statements (if, while, foreach, etc.) only scan
        // the condition/iteration expression — not the block bodies.
        // Block bodies are walked by walk_body_forward which calls us
        // per-statement with the correct inner scope.
        Statement::If(if_stmt) => {
            walk_closures_in_expr(if_stmt.condition, outer_scope, ctx, None);
        }
        Statement::While(while_stmt) => {
            walk_closures_in_expr(while_stmt.condition, outer_scope, ctx, None);
        }
        Statement::DoWhile(dw) => {
            walk_closures_in_expr(dw.condition, outer_scope, ctx, None);
        }
        Statement::Foreach(foreach) => {
            walk_closures_in_expr(foreach.expression, outer_scope, ctx, None);
        }
        Statement::For(for_stmt) => {
            for init in for_stmt.initializations.iter() {
                walk_closures_in_expr(init, outer_scope, ctx, None);
            }
            for cond in for_stmt.conditions.iter() {
                walk_closures_in_expr(cond, outer_scope, ctx, None);
            }
            for update in for_stmt.increments.iter() {
                walk_closures_in_expr(update, outer_scope, ctx, None);
            }
        }
        Statement::Switch(switch) => {
            walk_closures_in_expr(switch.expression, outer_scope, ctx, None);
        }
        _ => {}
    }
}

/// `outer_scope` is a pre-assignment snapshot, which is right for a
/// closure's `use` variables in general (a plain, non-reference capture of
/// `$x` in `$x = f($x, function () use ($x) { ... })` must see `$x`'s old
/// value). But a closure that captures *by reference* the very variable
/// its literal is being assigned to —
/// `$callback = function () use (&$callback) { ...$callback... };` —
/// is different: PHP creates the closure and only then stores it into
/// `$callback`, so by the time the body ever runs, the reference sees the
/// closure itself, not whatever `$callback` held (or didn't) before this
/// statement. `post_stmt_scope` already has the correct, fully-resolved
/// type for that case (computed by the same assignment pipeline that
/// would have handled a distinct variable), so borrow it from there
/// instead of falling into `seed_closure_captures`'s undefined-by-ref-
/// capture-is-`null` default.
fn self_ref_capture_scope<'a>(
    expr: &Expression<'_>,
    outer_scope: &'a ScopeState,
    post_stmt_scope: &ScopeState,
) -> Cow<'a, ScopeState> {
    let Expression::Assignment(assignment) = expr else {
        return Cow::Borrowed(outer_scope);
    };
    let (Expression::Variable(Variable::Direct(var)), Expression::Closure(closure)) =
        (assignment.lhs, assignment.rhs)
    else {
        return Cow::Borrowed(outer_scope);
    };
    let var_name = bytes_to_str(var.name);
    if !outer_scope.get(var_name).is_empty() {
        // Already has a type in the pre-assignment scope (e.g. a loop
        // reassigning the same closure); nothing to fix up.
        return Cow::Borrowed(outer_scope);
    }
    let captures_self_by_ref = closure.use_clause.as_ref().is_some_and(|use_clause| {
        use_clause.variables.iter().any(|use_var| {
            use_var.ampersand.is_some() && bytes_to_str(use_var.variable.name) == var_name
        })
    });
    if !captures_self_by_ref {
        return Cow::Borrowed(outer_scope);
    }
    let resolved = post_stmt_scope.get(var_name);
    if resolved.is_empty() {
        return Cow::Borrowed(outer_scope);
    }
    let mut seeded = outer_scope.clone();
    seeded.set(var_name, resolved.to_vec());
    Cow::Owned(seeded)
}

/// Recursively scan an expression tree for closures/arrow functions
/// and walk their bodies with properly seeded scopes.
///
/// When a closure/arrow function is found:
/// 1. Build a scope for the closure body (fresh for closures, cloned
///    from outer for arrow functions).
/// 2. Seed the scope with parameter types (using callable inference
///    from the enclosing call's signature when parameters are untyped).
/// 3. Walk the body using [`walk_body_for_diagnostics`] so that scope
///    snapshots are recorded at every statement boundary.
///
/// The `inferred_params` argument carries callable parameter types
/// inferred from the enclosing call's signature.  When a closure is
/// found as a direct argument to a function/method call, the caller
/// passes the inferred types so untyped parameters get the correct
/// types.
pub(crate) fn walk_closures_in_expr<'b>(
    expr: &'b Expression<'b>,
    outer_scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    inferred_params: Option<&[PhpType]>,
) {
    match expr {
        Expression::Closure(closure) => {
            // Build a fresh scope for the closure.
            let mut closure_scope = ScopeState::new();

            // Seed `$this`, the `use (…)` variables, and the paths read
            // through them from the outer scope, so that `$this->prop`
            // inside the closure resolves without calling
            // `resolve_variable_types` and a captured path keeps whatever
            // the code above the closure proved about it.
            seed_closure_captures(&mut closure_scope, outer_scope, closure.use_clause.as_ref());
            widen_by_ref_captures_by_later_writes(closure, &mut closure_scope, outer_scope, ctx);

            // Seed with parameter types, using callable inference when
            // available.  Filter out any inferred params whose base
            // type is unresolvable (e.g. PHPStan pseudo-types like
            // `collection-of<T>`) so they don't poison the scope —
            // the param simply won't be seeded, which is better than
            // skipping the entire closure body.
            let inferred = inferred_params.unwrap_or(&[]);
            let filtered_inferred = filter_resolvable_inferred_params(inferred, ctx);
            seed_closure_params(
                &mut closure_scope,
                &closure.parameter_list,
                closure.span().start.offset,
                &filtered_inferred,
                ctx,
            );

            // Record the scope at the body start.
            let body_span = closure.body.span();
            record_scope_snapshot(body_span.start.offset, &closure_scope);

            // Walk the closure body.
            {
                let _barrier = suspend_return_edges(ctx.flow);
                walk_body_for_diagnostics(closure.body.statements.iter(), &mut closure_scope, ctx);
            }

            // Record at body end (closure scope).
            record_scope_snapshot(body_span.end.offset, &closure_scope);

            // Restore the outer scope immediately after the closure body
            // so that code following the closure in the same expression
            // (e.g. `->where('product_id', $product->id)` after a
            // `whereHas(function (Builder $q) { ... })`) sees the outer
            // scope's variables, not the closure's.
            record_scope_snapshot(body_span.end.offset + 1, outer_scope);
        }
        Expression::ArrowFunction(arrow) => {
            // Arrow functions inherit the enclosing scope.
            let mut arrow_scope = outer_scope.clone();

            // Seed with parameter types, using callable inference when
            // available.
            let inferred = inferred_params.unwrap_or(&[]);
            let filtered_inferred = filter_resolvable_inferred_params(inferred, ctx);
            seed_closure_params(
                &mut arrow_scope,
                &arrow.parameter_list,
                arrow.span().start.offset,
                &filtered_inferred,
                ctx,
            );

            // Record the scope at the body expression.
            let body_span = arrow.expression.span();
            record_scope_snapshot(body_span.start.offset, &arrow_scope);
            // The body is `return <expr>;`, and writes and narrows what
            // that statement would: a snapshot at the end of each
            // assignment, at each `&&` / `||` operand the narrowing the
            // operands before it prove (`fn($x) => $x instanceof Foo &&
            // $x->bar()`), and in each ternary or `match` arm the narrowing
            // that arm runs under.
            process_assignment_expr(arrow.expression, &mut arrow_scope, ctx);
            record_scope_snapshot(body_span.end.offset, &arrow_scope);

            // Restore the outer scope after the arrow body (same
            // reasoning as for closures above).
            record_scope_snapshot(body_span.end.offset + 1, outer_scope);

            // Recurse into the body expression for nested closures.
            walk_closures_in_expr(arrow.expression, &arrow_scope, ctx, None);
        }
        // For call expressions, try to infer callable parameter types
        // from the function/method signature before recursing into
        // the arguments.
        Expression::Call(call) => {
            walk_closures_in_call(call, outer_scope, ctx);
        }
        // Recurse into sub-expressions that may contain closures.
        Expression::Parenthesized(inner) => {
            walk_closures_in_expr(inner.expression, outer_scope, ctx, None);
        }
        Expression::Assignment(assignment) => {
            walk_closures_in_expr(assignment.rhs, outer_scope, ctx, None);
        }
        Expression::Access(access) => match access {
            Access::Property(pa) => {
                walk_closures_in_expr(pa.object, outer_scope, ctx, None);
            }
            Access::NullSafeProperty(pa) => {
                walk_closures_in_expr(pa.object, outer_scope, ctx, None);
            }
            _ => {}
        },
        Expression::Array(_) | Expression::LegacyArray(_) => {
            let elements =
                crate::parser::array_literal_elements(expr).expect("an array literal has elements");
            for elem in elements.iter() {
                if let Some(key) = crate::parser::array_element_key(elem) {
                    walk_closures_in_expr(key, outer_scope, ctx, None);
                }
                if let Some(value) = crate::parser::array_element_value(elem) {
                    walk_closures_in_expr(value, outer_scope, ctx, None);
                }
            }
        }
        Expression::Binary(bin) => {
            walk_closures_in_expr(bin.lhs, outer_scope, ctx, None);
            // The right operand of `&&` runs only when the left one was
            // truthy, and that of `||` only when it was falsy.
            let polarity = match bin.operator {
                BinaryOperator::And(_) | BinaryOperator::LowAnd(_) => Some(true),
                BinaryOperator::Or(_) | BinaryOperator::LowOr(_) => Some(false),
                _ => None,
            };
            match polarity {
                Some(truthy) => walk_closures_in_branch(bin.rhs, outer_scope, ctx, |scope| {
                    narrow_by_condition(bin.lhs, truthy, scope, ctx);
                }),
                None => walk_closures_in_expr(bin.rhs, outer_scope, ctx, None),
            }
        }
        Expression::UnaryPrefix(prefix) => {
            walk_closures_in_expr(prefix.operand, outer_scope, ctx, None);
        }
        // A subscript evaluates both halves, so a closure written in
        // either one still needs its parameter scope — the immediately
        // indexed dispatch table (`['a' => fn (X $x) => …][$name]`)
        // writes it in the subscripted expression.
        Expression::ArrayAccess(aa) => {
            walk_closures_in_expr(aa.array, outer_scope, ctx, None);
            walk_closures_in_expr(aa.index, outer_scope, ctx, None);
        }
        // Each branch of a ternary and each arm of a `match` runs under
        // what its condition proved, the way an `if` body does.
        Expression::Conditional(cond) => {
            walk_closures_in_expr(cond.condition, outer_scope, ctx, None);
            if let Some(then_expr) = cond.then {
                walk_closures_in_branch(then_expr, outer_scope, ctx, |scope| {
                    narrow_by_condition(cond.condition, true, scope, ctx);
                });
            }
            walk_closures_in_branch(cond.r#else, outer_scope, ctx, |scope| {
                narrow_by_condition(cond.condition, false, scope, ctx);
            });
        }
        Expression::Match(m) => {
            walk_closures_in_expr(m.expression, outer_scope, ctx, None);
            let class_subject =
                crate::type_engine::type_ops::condition_facts::match_class_subject_var(
                    m.expression,
                );
            for arm in m.arms.iter() {
                let MatchArm::Expression(expr_arm) = arm else {
                    walk_closures_in_expr(arm.expression(), outer_scope, ctx, None);
                    continue;
                };
                if m.expression.is_true() {
                    walk_closures_in_branch(expr_arm.expression, outer_scope, ctx, |scope| {
                        for condition in expr_arm.conditions.iter() {
                            apply_condition_narrowing(condition, scope, ctx);
                        }
                    });
                } else if let Some(var) = class_subject {
                    walk_closures_in_branch(expr_arm.expression, outer_scope, ctx, |scope| {
                        apply_class_match_arm_narrowing(var, expr_arm, scope, ctx);
                    });
                } else {
                    walk_closures_in_expr(expr_arm.expression, outer_scope, ctx, None);
                }
            }
        }
        Expression::Instantiation(inst) => {
            if let Some(ref args) = inst.argument_list {
                walk_closures_in_call_args(&args.arguments, None, outer_scope, ctx);
            }
        }
        Expression::AnonymousClass(anon) => {
            // Constructor arguments evaluate in the outer scope (with the
            // outer `$this`), so scan them for closures there.
            if let Some(ref args) = anon.argument_list {
                walk_closure_in_partial_call_args(&args.arguments, outer_scope, ctx);
            }
            // The anonymous class's own method bodies have their own
            // `$this` (the anonymous class), so walk them separately.
            {
                let _barrier = suspend_return_edges(ctx.flow);
                walk_anonymous_class_member_bodies(anon, ctx);
            }

            // Restore the outer scope immediately after the anonymous
            // class body so that code following it in the same expression
            // (e.g. a sibling call argument `f(new class {...}, $this->x)`)
            // sees the outer `$this`, not the anonymous class's.
            record_scope_snapshot(anon.right_brace.end.offset + 1, outer_scope);
        }
        Expression::Yield(y) => match y {
            Yield::Value(yv) => {
                if let Some(val) = &yv.value {
                    walk_closures_in_expr(val, outer_scope, ctx, None);
                }
            }
            Yield::Pair(yp) => {
                walk_closures_in_expr(yp.key, outer_scope, ctx, None);
                walk_closures_in_expr(yp.value, outer_scope, ctx, None);
            }
            Yield::From(yf) => {
                walk_closures_in_expr(yf.iterator, outer_scope, ctx, None);
            }
        },
        Expression::Throw(t) => {
            walk_closures_in_expr(t.exception, outer_scope, ctx, None);
        }
        Expression::Clone(c) => {
            walk_closures_in_expr(c.object, outer_scope, ctx, None);
        }
        Expression::Pipe(p) => {
            walk_closures_in_expr(p.input, outer_scope, ctx, None);
            walk_closures_in_expr(p.callable, outer_scope, ctx, None);
        }
        _ => {}
    }
}

/// Walk the closures in a branch that runs only under some narrowing of
/// `outer_scope`, which `narrow` applies.
///
/// A closure captures the variables it uses when it is created, so one
/// written in the right operand of `$c && …`, in a ternary branch or in a
/// `match` arm sees them as that branch's condition left them, exactly as
/// a closure written inside an `if ($c)` block does.  The scope is only
/// copied and narrowed when the branch holds something
/// [`ClosureFinder`] reports.
fn walk_closures_in_branch<'b>(
    branch: &'b Expression<'b>,
    outer_scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    narrow: impl FnOnce(&mut ScopeState),
) {
    let mut last_closure_end = None;
    mago_syntax::walker::Walker::walk_expression(&ClosureFinder, branch, &mut last_closure_end);
    let Some(last_closure_end) = last_closure_end else {
        return;
    };
    let mut branch_scope = outer_scope.clone();
    narrow(&mut branch_scope);
    walk_closures_in_expr(branch, &branch_scope, ctx, None);

    // A closure or anonymous class puts the scope it was walked in back
    // just past its end.  When it is the last thing in the branch, that
    // lands outside the branch, where the narrowing no longer holds: the
    // `$c` in `f($c && fn () => 1, $c)` is not proven truthy.
    let restore = last_closure_end + 1;
    if restore >= branch.span().end.offset {
        record_scope_snapshot(restore, outer_scope);
    }
}

/// Narrow `scope` by what `condition` proves when it is truthy, or by
/// what it proves when it is falsy.
fn narrow_by_condition<'b>(
    condition: &'b Expression<'b>,
    truthy: bool,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    if truthy {
        apply_condition_narrowing(condition, scope, ctx);
    } else {
        apply_condition_narrowing_inverse(condition, scope, ctx);
    }
}

/// Records where the last closure, arrow function or anonymous class it
/// reaches ends, without descending into one.  These are everything
/// [`walk_closures_in_expr`] walks a body of; a branch holding none of
/// them has nothing to walk.
struct ClosureFinder;

impl ClosureFinder {
    fn found(end: u32, last_end: &mut Option<u32>) {
        *last_end = Some(last_end.map_or(end, |last| last.max(end)));
    }
}

impl<'ast, 'arena> mago_syntax::walker::Walker<'ast, 'arena, Option<u32>> for ClosureFinder {
    fn walk_closure(&self, closure: &'ast Closure<'arena>, last_end: &mut Option<u32>) {
        Self::found(closure.span().end.offset, last_end);
    }

    fn walk_arrow_function(&self, arrow: &'ast ArrowFunction<'arena>, last_end: &mut Option<u32>) {
        Self::found(arrow.span().end.offset, last_end);
    }

    fn walk_anonymous_class(&self, anon: &'ast AnonymousClass<'arena>, last_end: &mut Option<u32>) {
        Self::found(anon.right_brace.end.offset, last_end);
    }
}

/// Handle a call expression: infer callable parameter types from the
/// function/method signature and pass them when walking closure arguments.
pub(crate) fn walk_closures_in_call<'b>(
    call: &'b Call<'b>,
    outer_scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let arguments = match call {
        Call::Function(fc) => {
            // Recurse into the function expression (for closures in
            // chained calls like `$fn()($anotherClosure)`).
            walk_closures_in_expr(fc.function, outer_scope, ctx, None);
            &fc.argument_list.arguments
        }
        Call::Method(mc) => {
            match closure_call_scope(mc, outer_scope, ctx) {
                Some(call_scope) => walk_closures_in_expr(mc.object, &call_scope, ctx, None),
                None => walk_closures_in_expr(mc.object, outer_scope, ctx, None),
            }
            &mc.argument_list.arguments
        }
        Call::NullSafeMethod(mc) => {
            walk_closures_in_expr(mc.object, outer_scope, ctx, None);
            &mc.argument_list.arguments
        }
        Call::StaticMethod(sc) => {
            walk_closures_in_expr(sc.class, outer_scope, ctx, None);
            &sc.argument_list.arguments
        }
    };
    walk_closures_in_call_args(arguments, Some(call), outer_scope, ctx);
}

/// Walk the arguments of a call expression.  A closure or arrow function
/// passed to `call` gets the parameter types the callee's signature gives
/// it, so untyped parameters get the correct types, and a
/// `@param-closure-this` on the parameter receiving it rebinds its `$this`.
pub(crate) fn walk_closures_in_call_args<'b>(
    arguments: &'b TokenSeparatedSequence<'b, Argument<'b>>,
    call: Option<&Call<'b>>,
    outer_scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    for (arg_idx, arg) in arguments.iter().enumerate() {
        let arg_expr = match arg {
            Argument::Positional(a) => a.value,
            Argument::Named(a) => a.value,
        };
        if let Some(entries) =
            call.and_then(|call| eager_callback_arguments(call, arg_idx, outer_scope, ctx))
        {
            for entry in entries {
                walk_closures_in_expr(entry.expression, outer_scope, ctx, Some(&entry.parameters));
            }
            continue;
        }
        match arg_expr {
            Expression::Closure(_) | Expression::ArrowFunction(_) => {
                let inferred = call.map_or_else(Vec::new, |call| {
                    infer_callable_params_for_call(call, arg_idx, outer_scope, ctx)
                });
                let rebound = call
                    .and_then(|call| closure_this_argument_scope(call, arg_idx, outer_scope, ctx));
                walk_closures_in_expr(
                    arg_expr,
                    rebound.as_ref().unwrap_or(outer_scope),
                    ctx,
                    if inferred.is_empty() {
                        None
                    } else {
                        Some(&inferred)
                    },
                );
            }
            _ => {
                walk_closures_in_expr(arg_expr, outer_scope, ctx, None);
            }
        }
    }
}

/// Walk the closures among the partial arguments of an anonymous class's
/// constructor call.
pub(crate) fn walk_closure_in_partial_call_args<'b>(
    arguments: &'b TokenSeparatedSequence<'b, PartialArgument<'b>>,
    outer_scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    for arg_expr in arguments.iter().filter_map(PartialArgument::value) {
        walk_closures_in_expr(arg_expr, outer_scope, ctx, None);
    }
}
