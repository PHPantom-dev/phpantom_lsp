use super::*;

use mago_span::HasSpan;
use mago_syntax::cst::argument::Argument;
use mago_syntax::cst::function_like::closure::ClosureUseClause;

use crate::atom::bytes_to_str;
use crate::php_type::PhpType;

// ─── Closure return edges ───────────────────────────────────────────────────

/// What one entry of the [`RETURN_EDGES`] stack is doing.
enum ReturnFrame {
    /// Accumulates the states that `return` out of the body being walked.
    /// `None` until the first `return` is seen.
    ///
    /// Boxed because a `ScopeState` is far larger than the other variant,
    /// and the stack holds one frame per body being walked rather than one
    /// per statement, so the indirection is paid once per closure.
    Open(Option<Box<ScopeState>>),
    /// A nested body's returns say nothing about the body outside it.
    Barrier,
}

thread_local! {
    /// One frame per body being walked for the types it writes to its
    /// `use (&$x)` captures, innermost last.
    ///
    /// The branch merge drops a branch that returns, because such a branch
    /// does not reach the statement after the `if`.  The end of a closure
    /// body is not that statement: a by-reference capture written on a
    /// path that returns early is visible to the caller all the same.  So
    /// a `return` records the state it leaves with here, and the walk that
    /// opened the frame folds those states into the body's exit state.
    ///
    /// Set and cleared within a single synchronous walk, like the loop
    /// exit edges in `loop_control`.
    static RETURN_EDGES: RefCell<Vec<ReturnFrame>> = const { RefCell::new(Vec::new()) };
}

/// Open a frame for a closure body about to be walked for its by-reference
/// captures.
pub(crate) fn push_return_frame() {
    RETURN_EDGES.with(|frames| frames.borrow_mut().push(ReturnFrame::Open(None)));
}

/// Close the innermost frame and return the state its `return`s carried
/// out, or `None` when the body has no reachable `return`.
pub(crate) fn pop_return_frame() -> Option<ScopeState> {
    RETURN_EDGES.with(|frames| match frames.borrow_mut().pop() {
        Some(ReturnFrame::Open(state)) => state.map(|s| *s),
        _ => None,
    })
}

/// Lifts the barrier [`suspend_return_edges`] put in place.
pub(crate) struct ReturnEdgeBarrierGuard {
    pushed: bool,
}

impl Drop for ReturnEdgeBarrierGuard {
    fn drop(&mut self) {
        if self.pushed {
            RETURN_EDGES.with(|frames| {
                frames.borrow_mut().pop();
            });
        }
    }
}

/// Stop `return`s from reaching the enclosing closure's frame for the
/// lifetime of the returned guard.
///
/// A nested body — another closure, an anonymous class method, or a callee
/// whose return type is being inferred — is walked by the same
/// [`walk_body_forward`] machinery, and its `return`s belong to it rather
/// than to whatever closure is being walked further out.
pub(crate) fn suspend_return_edges() -> ReturnEdgeBarrierGuard {
    let pushed = RETURN_EDGES.with(|frames| {
        let mut frames = frames.borrow_mut();
        // Nothing to shield when no frame is collecting.
        if frames.is_empty() {
            return false;
        }
        frames.push(ReturnFrame::Barrier);
        true
    });
    ReturnEdgeBarrierGuard { pushed }
}

/// Record the state a `return` carries out of the body being walked.
pub(crate) fn record_return_edge(scope: &ScopeState) {
    if scope.unreachable {
        return;
    }
    RETURN_EDGES.with(|frames| {
        if let Some(ReturnFrame::Open(state)) = frames.borrow_mut().last_mut() {
            match state {
                Some(accumulated) => accumulated.merge_branch(scope),
                None => *state = Some(Box::new(scope.clone())),
            }
        }
    });
}

// ─── Closure handling ───────────────────────────────────────────────────────

/// Seed a closure's own scope with what it captures from the scope it is
/// written in: `$this`, the `use (…)` variables, and the state recorded
/// against paths read through either of them.
///
/// The paths matter as much as the variables themselves.  A guard above
/// the closure records what it proved under the spelling it tested —
///
/// ```php
/// if ($param->type === null) { continue; }
/// $errors[] = static fn () => new Pair($param->type);   // still non-null
/// ```
///
/// — so a closure scope that carries `$param` but not `$param->type` makes
/// the body fall back to the declaration and report the `null` the guard
/// has already ruled out.
pub(crate) fn seed_closure_captures(
    closure_scope: &mut ScopeState,
    outer: &ScopeState,
    use_clause: Option<&ClosureUseClause<'_>>,
) {
    let carry_paths_through = |root: &str, closure_scope: &mut ScopeState| {
        for (key, types) in outer.locals.iter() {
            if crate::type_engine::types::narrowing::key_reads_variable(key.as_str(), root) {
                closure_scope.set(key.as_str(), types.clone());
            }
        }
    };

    // PHP closures implicitly capture `$this` from the enclosing class
    // method.
    let this_types = outer.get("$this");
    if !this_types.is_empty() {
        closure_scope.set("$this", this_types.to_vec());
        carry_paths_through("$this", closure_scope);
    }

    let Some(use_clause) = use_clause else {
        return;
    };
    for use_var in use_clause.variables.iter() {
        let var_name = bytes_to_str(use_var.variable.name).to_string();
        let from_outer = outer.get(&var_name);
        if !from_outer.is_empty() {
            closure_scope.set(&var_name, from_outer.to_vec());
        } else if outer.contains(&var_name) {
            closure_scope.set_empty(&var_name);
        }
        carry_paths_through(&var_name, closure_scope);
    }
}

/// Try to enter a closure or arrow function if the cursor is inside one.
///
/// Returns `true` if the cursor was inside a closure and the scope was
/// updated accordingly.
pub(crate) fn try_enter_closure<'b>(
    stmt: &'b Statement<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    if let Statement::Expression(expr_stmt) = stmt {
        return try_enter_closure_expr(expr_stmt.expression, scope, ctx, None);
    }
    if let Statement::Return(ret) = stmt
        && let Some(val) = ret.value
    {
        return try_enter_closure_expr(val, scope, ctx, None);
    }
    // Closures/arrow functions can appear inside if/while/for/switch
    // conditions (e.g. `if (array_any($items, fn($x) => $x->...))`).
    // Recurse into these condition expressions so the forward walker
    // can enter the closure scope.
    if let Statement::If(if_stmt) = stmt {
        if try_enter_closure_expr(if_stmt.condition, scope, ctx, None) {
            return true;
        }
        // Also check elseif conditions for closures.
        match &if_stmt.body {
            IfBody::Statement(body) => {
                for ei in body.else_if_clauses.iter() {
                    if try_enter_closure_expr(ei.condition, scope, ctx, None) {
                        return true;
                    }
                }
            }
            IfBody::ColonDelimited(body) => {
                for ei in body.else_if_clauses.iter() {
                    if try_enter_closure_expr(ei.condition, scope, ctx, None) {
                        return true;
                    }
                }
            }
        }
    }
    if let Statement::While(while_stmt) = stmt
        && try_enter_closure_expr(while_stmt.condition, scope, ctx, None)
    {
        return true;
    }
    if let Statement::For(for_stmt) = stmt {
        for cond in for_stmt.conditions.iter() {
            if try_enter_closure_expr(cond, scope, ctx, None) {
                return true;
            }
        }
    }
    if let Statement::Switch(switch) = stmt {
        if try_enter_closure_expr(switch.expression, scope, ctx, None) {
            return true;
        }
        for case in switch.body.cases().iter() {
            if let Some(cond) = case.expression()
                && try_enter_closure_expr(cond, scope, ctx, None)
            {
                return true;
            }
        }
    }
    false
}

/// Recursively search an expression for a closure/arrow function
/// containing the cursor.
pub(crate) fn try_enter_closure_expr<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    inferred_params: Option<&[PhpType]>,
) -> bool {
    match expr {
        Expression::Closure(closure) => {
            let body_span = closure.body.span();
            if ctx.cursor_offset >= body_span.start.offset
                && ctx.cursor_offset <= body_span.end.offset
            {
                // Create a fresh scope for the closure (closures have
                // isolated scope in PHP).
                let mut closure_scope = ScopeState::new();

                seed_closure_captures(&mut closure_scope, scope, closure.use_clause.as_ref());

                // Seed with parameter types, using callable inference
                // when available.
                let inferred = inferred_params.unwrap_or(&[]);
                let filtered_inferred = filter_resolvable_inferred_params(inferred, ctx);
                seed_closure_params(
                    &mut closure_scope,
                    &closure.parameter_list,
                    closure.span().start.offset,
                    &filtered_inferred,
                    ctx,
                );

                {
                    let _barrier = suspend_return_edges();
                    walk_body_forward(closure.body.statements.iter(), &mut closure_scope, ctx);
                }

                *scope = closure_scope;
                return true;
            }
        }
        Expression::ArrowFunction(arrow) => {
            let body_span = arrow.expression.span();
            if ctx.cursor_offset >= body_span.start.offset
                && ctx.cursor_offset <= body_span.end.offset
            {
                // Arrow functions inherit the enclosing scope.
                // Seed with parameter types, using callable inference
                // when available.
                let inferred = inferred_params.unwrap_or(&[]);
                let filtered_inferred = filter_resolvable_inferred_params(inferred, ctx);
                seed_closure_params(
                    scope,
                    &arrow.parameter_list,
                    arrow.span().start.offset,
                    &filtered_inferred,
                    ctx,
                );
                // The arrow body is a single return-value expression, so
                // apply the same cursor narrowing that `walk_body_forward`
                // applies to a statement body.  This narrows a parameter
                // referenced after an earlier `&&` conjunct (e.g.
                // `fn($x) => $x instanceof Foo && $x->bar()`).
                apply_cursor_ternary_narrowing(arrow.expression, scope, ctx);
                // Recurse into the body to find nested closures/arrow
                // functions that may contain the cursor (e.g. a closure
                // passed as an argument inside the arrow body).
                try_enter_closure_expr(arrow.expression, scope, ctx, None);
                return true;
            }
        }
        // Recurse into sub-expressions that might contain closures.
        Expression::Parenthesized(inner) => {
            return try_enter_closure_expr(inner.expression, scope, ctx, None);
        }
        Expression::Assignment(assignment) => {
            // A closure the cursor sits in has to be written on the
            // right-hand side, and only then is processing the assignment
            // first (so the left-hand side is in scope for it) worth
            // anything.  Every other cursor position would apply the
            // assignment a second time — `process_statement` walks it too
            // — and `$x = $x->format()` would resolve against the string
            // the first pass already stored rather than the object.
            let rhs_span = assignment.rhs.span();
            if ctx.cursor_offset < rhs_span.start.offset || ctx.cursor_offset > rhs_span.end.offset
            {
                return false;
            }
            process_assignment_expr(expr, scope, ctx);
            return try_enter_closure_expr(assignment.rhs, scope, ctx, None);
        }
        Expression::Call(call) => {
            // Check if any argument is a closure containing the cursor.
            // Infer callable parameter types from the function/method
            // signature so closure params get generic-substituted types
            // (mirroring the diagnostic path's walk_closures_in_call).
            let args = match call {
                Call::Function(fc) => &fc.argument_list,
                Call::Method(mc) => &mc.argument_list,
                Call::NullSafeMethod(mc) => &mc.argument_list,
                Call::StaticMethod(sc) => &sc.argument_list,
            };
            for (arg_idx, arg) in args.arguments.iter().enumerate() {
                let arg_expr = match arg {
                    Argument::Positional(a) => a.value,
                    Argument::Named(a) => a.value,
                };
                let inferred = infer_callable_params_for_call(call, arg_idx, scope, ctx);
                let inferred_opt = if inferred.is_empty() {
                    None
                } else {
                    Some(inferred.as_slice())
                };
                if try_enter_closure_expr(arg_expr, scope, ctx, inferred_opt) {
                    return true;
                }
            }
        }
        Expression::Access(access) => match access {
            Access::Property(pa) => {
                return try_enter_closure_expr(pa.object, scope, ctx, None);
            }
            Access::NullSafeProperty(pa) => {
                return try_enter_closure_expr(pa.object, scope, ctx, None);
            }
            _ => {}
        },
        Expression::Array(arr) => {
            for elem in arr.elements.iter() {
                let elem_expr = match elem {
                    ArrayElement::KeyValue(kv) => kv.value,
                    ArrayElement::Value(val) => val.value,
                    ArrayElement::Variadic(v) => v.value,
                    ArrayElement::Missing(_) => continue,
                };
                if try_enter_closure_expr(elem_expr, scope, ctx, None) {
                    return true;
                }
            }
        }
        // The immediately indexed dispatch table
        // (`['a' => fn (X $x) => …][$name]`) writes its closures inside
        // the subscripted expression, so both halves are searched.
        Expression::ArrayAccess(aa) => {
            if try_enter_closure_expr(aa.array, scope, ctx, None) {
                return true;
            }
            return try_enter_closure_expr(aa.index, scope, ctx, None);
        }
        _ => {}
    }
    false
}
