use super::*;

/// Apply the assignments an expression performs, in evaluation order.
///
/// PHP assignments are expressions, so one can sit anywhere a value can:
/// a condition (`if ($x = expr())`), a call receiver
/// (`($x = $map[$key])->truthy()`), a call argument
/// (`is_object($token = $tokenizer->next())`).  Whatever follows it in
/// the same expression reads the target it just wrote, so each one is
/// applied to the scope and a snapshot is recorded at its end offset —
/// the nearest snapshot otherwise predates the whole expression, which is
/// the scope from before the write.
///
/// The outermost assignment of a *statement* is not this function's job:
/// `process_assignment_expr` owns that one, and knows about destructuring,
/// `@var` overrides, and indexed writes that this descent does not.
pub(crate) fn process_nested_assignments<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    if let Expression::Assignment(assignment) = expr {
        // The assigned value can itself assign: `if ($a = $b = expr())`,
        // `if ($a = ($b = f())->m())`.  It runs first, so it is applied
        // before the outer target is written.
        process_nested_assignments(assignment.rhs, scope, ctx);
        if assignment.operator.is_assign() {
            if let Expression::Variable(Variable::Direct(dv)) = assignment.lhs {
                let var_name = bytes_to_str(dv.name).to_string();
                let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
                if !rhs_types.is_empty() {
                    scope.set(&var_name, rhs_types);
                }
            }
        } else {
            // `??=`, `.=`, `+=`, … write their target here exactly as they
            // do from a statement, so the statement handler decides what
            // the target ends up holding.  Reading `??=` as "no assignment
            // happened" left the target on the type it had going in, which
            // for the `$x ??= …` idiom is the `null` the fallback exists
            // to replace.
            process_compound_assignment(assignment, scope, ctx);
        }
        record_scope_snapshot(assignment.span().end.offset, scope);
        return;
    }
    // Parenthesized: `if (($x = expr()))`.
    if let Expression::Parenthesized(inner) = expr {
        process_nested_assignments(inner.expression, scope, ctx);
        return;
    }
    // Negated (or otherwise unary-prefixed):
    //   `if (!$x = expr()) { return; }` — PHP parses this as
    //   `!($x = expr())`.  Recurse into the operand.
    if let Expression::UnaryPrefix(prefix) = expr {
        process_nested_assignments(prefix.operand, scope, ctx);
        return;
    }
    // Assignment inside a binary comparison or logical chain:
    //   `if (($x = expr()) !== null)`, `if (null !== ($x = expr()))`,
    //   `while (($x = next()) && $x->valid())`.  Recurse into both
    //   operands so the assignment on either side is seen.
    if let Expression::Binary(bin) = expr {
        process_nested_assignments(bin.lhs, scope, ctx);
        process_nested_assignments(bin.rhs, scope, ctx);
        return;
    }
    // Assignment in the receiver of a member access or an offset read:
    //   `($x = $map[$key])->truthy()`, `($x = f())->prop`.  The receiver
    //   is evaluated before the access, so the write it makes is in force
    //   by the time the member is reached.
    match expr {
        Expression::Access(Access::Property(pa)) => {
            process_nested_assignments(pa.object, scope, ctx);
            return;
        }
        Expression::Access(Access::NullSafeProperty(pa)) => {
            process_nested_assignments(pa.object, scope, ctx);
            return;
        }
        Expression::ArrayAccess(aa) => {
            process_nested_assignments(aa.array, scope, ctx);
            process_nested_assignments(aa.index, scope, ctx);
            return;
        }
        _ => {}
    }
    // Assignment wrapped in a call argument:
    //   `while (is_object($token = $tokenizer->next()))`.  Recurse into
    //   each argument value so the assignment is registered — and into the
    //   receiver, which runs before the arguments do.
    if let Expression::Call(call) = expr {
        let arg_list = match call {
            Call::Function(fc) => {
                process_nested_assignments(fc.function, scope, ctx);
                &fc.argument_list
            }
            Call::Method(mc) => {
                process_nested_assignments(mc.object, scope, ctx);
                &mc.argument_list
            }
            Call::NullSafeMethod(mc) => {
                process_nested_assignments(mc.object, scope, ctx);
                &mc.argument_list
            }
            Call::StaticMethod(sc) => &sc.argument_list,
        };
        for arg in arg_list.arguments.iter() {
            let arg_expr = match arg {
                Argument::Positional(a) => a.value,
                Argument::Named(a) => a.value,
            };
            process_nested_assignments(arg_expr, scope, ctx);
        }
        return;
    }
    // Assignment in a constructor argument, or in an array literal:
    //   `new Foo([$x = 1])`.
    match expr {
        Expression::Instantiation(inst) => {
            if let Some(arg_list) = &inst.argument_list {
                for arg in arg_list.arguments.iter() {
                    process_nested_assignments(arg.value(), scope, ctx);
                }
            }
        }
        Expression::Array(array) => {
            for elem in array.elements.iter() {
                process_nested_assignments_in_element(elem, scope, ctx);
            }
        }
        Expression::LegacyArray(array) => {
            for elem in array.elements.iter() {
                process_nested_assignments_in_element(elem, scope, ctx);
            }
        }
        _ => {}
    }
}

fn process_nested_assignments_in_element<'b>(
    elem: &'b ArrayElement<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    match elem {
        ArrayElement::KeyValue(kv) => {
            process_nested_assignments(kv.key, scope, ctx);
            process_nested_assignments(kv.value, scope, ctx);
        }
        ArrayElement::Value(v) => process_nested_assignments(v.value, scope, ctx),
        ArrayElement::Variadic(v) => process_nested_assignments(v.value, scope, ctx),
        ArrayElement::Missing(_) => {}
    }
}
