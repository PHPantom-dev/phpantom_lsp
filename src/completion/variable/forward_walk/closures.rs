use super::*;

use mago_span::HasSpan;
use mago_syntax::cst::argument::Argument;

use crate::atom::bytes_to_str;
use crate::php_type::PhpType;

// ─── Closure handling ───────────────────────────────────────────────────────

/// Try to enter a closure or arrow function if the cursor is inside one.
///
/// Returns `true` if the cursor was inside a closure and the scope was
/// updated accordingly.
pub(crate) fn try_enter_closure<'b>(
    stmt: &'b Statement<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    // Walk the statement's expression tree looking for closures/arrow
    // functions that contain the cursor.
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
        // Also check elseif conditions and bodies for closures.
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

                // PHP closures implicitly capture `$this` from the
                // enclosing class method.
                let this_types = scope.get("$this");
                if !this_types.is_empty() {
                    closure_scope.set("$this", this_types.to_vec());
                }

                // Seed with `use(...)` variables from the outer scope.
                if let Some(ref use_clause) = closure.use_clause {
                    for use_var in use_clause.variables.iter() {
                        let var_name = bytes_to_str(use_var.variable.name).to_string();
                        let from_outer = scope.get(&var_name);
                        if !from_outer.is_empty() {
                            closure_scope.set(&var_name, from_outer.to_vec());
                        }
                    }
                }

                // Seed with parameter types, using callable inference
                // when available (mirroring the diagnostic path's
                // seed_closure_params logic).
                let inferred = inferred_params.unwrap_or(&[]);
                let filtered_inferred = filter_resolvable_inferred_params(inferred, ctx);
                seed_closure_params(
                    &mut closure_scope,
                    &closure.parameter_list,
                    closure.span().start.offset,
                    &filtered_inferred,
                    ctx,
                );

                // Walk the closure body.
                walk_body_forward(closure.body.statements.iter(), &mut closure_scope, ctx);

                // Replace the outer scope with the closure scope.
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
                // The body is a single expression.  Recurse into it
                // to find nested closures/arrow functions that may
                // contain the cursor (e.g. a closure passed as an
                // argument inside the arrow body).
                try_enter_closure_expr(arrow.expression, scope, ctx, None);
                return true;
            }
        }
        // Recurse into sub-expressions that might contain closures.
        Expression::Parenthesized(inner) => {
            return try_enter_closure_expr(inner.expression, scope, ctx, None);
        }
        Expression::Assignment(assignment) => {
            // Process the assignment first so the LHS var is in scope.
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
                // Only infer callable params when the argument is a
                // closure or arrow function (or wraps one).
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
        _ => {}
    }
    false
}

/// Widen a literal type to its base type (e.g. `1` → `int`, `'foo'` → `string`).
/// Non-literal types are returned unchanged.
pub(crate) fn widen_literal(ty: &PhpType) -> PhpType {
    match ty {
        PhpType::Literal(crate::php_type::LiteralValue::Int(_)) => PhpType::int(),
        PhpType::Literal(crate::php_type::LiteralValue::String(_)) => PhpType::string(),
        PhpType::Literal(crate::php_type::LiteralValue::Float(_)) => PhpType::float(),
        _ => ty.clone(),
    }
}
