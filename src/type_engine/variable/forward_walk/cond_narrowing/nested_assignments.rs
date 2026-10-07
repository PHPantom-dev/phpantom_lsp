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
///
/// Returns whether `expr` performed (or contains, anywhere this descent
/// reaches) an assignment.  The only consumer of the return value is the
/// `&&`/`||` case below, which uses it to decide whether forking the scope
/// for the right operand is worth its cost — every other call site
/// discards it exactly as it discarded the previous `()` return.
pub(crate) fn process_nested_assignments<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
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
        return true;
    }
    // Parenthesized: `if (($x = expr()))`.
    if let Expression::Parenthesized(inner) = expr {
        return process_nested_assignments(inner.expression, scope, ctx);
    }
    // Negated (or otherwise unary-prefixed):
    //   `if (!$x = expr()) { return; }` — PHP parses this as
    //   `!($x = expr())`.  Recurse into the operand.
    if let Expression::UnaryPrefix(prefix) = expr {
        return process_nested_assignments(prefix.operand, scope, ctx);
    }
    // Assignment inside a binary comparison or logical chain:
    //   `if (($x = expr()) !== null)`, `if (null !== ($x = expr()))`,
    //   `while (($x = next()) && $x->valid())`.  Recurse into both
    //   operands so the assignment on either side is seen.
    //
    // `&&`/`||` only run their right operand when the left one allows it,
    // so when the right operand actually holds an assignment it is walked
    // on a copy of the scope narrowed by what the left operand proved (or
    // its inverse, for `||`), and the result is joined with the path where
    // the right operand never ran — the same reasoning
    // `process_nullsafe_argument_assignments` uses for `?->` arguments.
    // The fork is skipped when the right operand holds no assignment: it
    // would otherwise merge a scope narrowed only by this pass's own
    // `apply_condition_narrowing` call into `scope`, ahead of (and
    // inconsistent with) the real chain-aware narrowing the condition's
    // caller applies once this whole descent returns.
    if let Expression::Binary(bin) = expr {
        if matches!(
            bin.operator,
            BinaryOperator::And(_) | BinaryOperator::LowAnd(_)
        ) {
            let lhs_assigned = process_nested_assignments(bin.lhs, scope, ctx);
            let mut ran = scope.clone();
            apply_condition_narrowing(bin.lhs, &mut ran, ctx);
            let rhs_assigned = process_nested_assignments(bin.rhs, &mut ran, ctx);
            if rhs_assigned {
                scope.merge_branch(&ran);
            }
            return lhs_assigned || rhs_assigned;
        }
        if matches!(
            bin.operator,
            BinaryOperator::Or(_) | BinaryOperator::LowOr(_)
        ) {
            let lhs_assigned = process_nested_assignments(bin.lhs, scope, ctx);
            let mut ran = scope.clone();
            apply_condition_narrowing_inverse(bin.lhs, &mut ran, ctx);
            let rhs_assigned = process_nested_assignments(bin.rhs, &mut ran, ctx);
            if rhs_assigned {
                scope.merge_branch(&ran);
            }
            return lhs_assigned || rhs_assigned;
        }
        let lhs_assigned = process_nested_assignments(bin.lhs, scope, ctx);
        let rhs_assigned = process_nested_assignments(bin.rhs, scope, ctx);
        return lhs_assigned || rhs_assigned;
    }
    // Assignment in the receiver of a member access or an offset read:
    //   `($x = $map[$key])->truthy()`, `($x = f())->prop`.  The receiver
    //   is evaluated before the access, so the write it makes is in force
    //   by the time the member is reached.
    match expr {
        Expression::Access(Access::Property(pa)) => {
            return process_nested_assignments(pa.object, scope, ctx);
        }
        Expression::Access(Access::NullSafeProperty(pa)) => {
            return process_nested_assignments(pa.object, scope, ctx);
        }
        Expression::ArrayAccess(aa) => {
            let array_assigned = process_nested_assignments(aa.array, scope, ctx);
            let index_assigned = process_nested_assignments(aa.index, scope, ctx);
            return array_assigned || index_assigned;
        }
        _ => {}
    }
    // Assignment wrapped in a call argument:
    //   `while (is_object($token = $tokenizer->next()))`.  Recurse into
    //   each argument value so the assignment is registered — and into the
    //   receiver, which runs before the arguments do.
    if let Expression::Call(call) = expr {
        let (receiver_assigned, arg_list) = match call {
            Call::Function(fc) => (
                process_nested_assignments(fc.function, scope, ctx),
                &fc.argument_list,
            ),
            Call::Method(mc) => (
                process_nested_assignments(mc.object, scope, ctx),
                &mc.argument_list,
            ),
            Call::NullSafeMethod(mc) => {
                let receiver_assigned = process_nested_assignments(mc.object, scope, ctx);
                let args_assigned = if mc.argument_list.arguments.is_empty() {
                    false
                } else {
                    process_nullsafe_argument_assignments(mc, scope, ctx)
                };
                return receiver_assigned || args_assigned;
            }
            Call::StaticMethod(sc) => (false, &sc.argument_list),
        };
        let mut args_assigned = false;
        for arg in arg_list.arguments.iter() {
            let arg_expr = match arg {
                Argument::Positional(a) => a.value,
                Argument::Named(a) => a.value,
            };
            args_assigned |= process_nested_assignments(arg_expr, scope, ctx);
        }
        return receiver_assigned || args_assigned;
    }
    // Assignment inside a match arm: `$r = match ($k) { 1 => $x = $n,
    // default => null };`.  Only one arm runs, so each arm is walked
    // against its own copy of the scope and the copies are joined — an
    // arm that does not run cannot leak a definite assignment.  A match
    // with no matching arm throws `UnhandledMatchError` rather than
    // falling through, so unlike a `switch` without `default` there is
    // no "no arm ran" scope to fold into the join.
    //
    // Split into its own function (rather than inlined here) so its
    // per-arm scope clones don't inflate this function's own frame: a
    // long `->method()` chain recurses through the `Call` case below
    // thousands of levels deep, and every extra byte in *this* frame is
    // paid at every one of those levels.
    if let Expression::Match(match_expr) = expr {
        return process_match_nested_assignments(match_expr, scope, ctx);
    }
    // Assignment inside a ternary branch: `$r = $cond ? $x = $a : $x =
    // $b;`.  Same reasoning as `match` above — only one branch runs.
    if let Expression::Conditional(conditional) = expr {
        return process_conditional_nested_assignments(conditional, scope, ctx);
    }
    // Assignment in a constructor argument, or in an array literal:
    //   `new Foo([$x = 1])`.
    match expr {
        Expression::Instantiation(inst) => {
            let Some(arg_list) = &inst.argument_list else {
                return false;
            };
            let mut assigned = false;
            for arg in arg_list.arguments.iter() {
                assigned |= process_nested_assignments(arg.value(), scope, ctx);
            }
            assigned
        }
        Expression::Array(array) => {
            let mut assigned = false;
            for elem in array.elements.iter() {
                assigned |= process_nested_assignments_in_element(elem, scope, ctx);
            }
            assigned
        }
        Expression::LegacyArray(array) => {
            let mut assigned = false;
            for elem in array.elements.iter() {
                assigned |= process_nested_assignments_in_element(elem, scope, ctx);
            }
            assigned
        }
        _ => false,
    }
}

/// Apply the assignments in the arguments of a `?->` call.
///
/// The arguments run only when the receiver is not null, so they see it
/// narrowed, and what they write is joined with the path where the call
/// short-circuited and wrote nothing.
///
/// Returns whether any argument performed an assignment.
fn process_nullsafe_argument_assignments<'b>(
    call: &'b NullSafeMethodCall<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let mut ran = scope.clone();
    narrow_nullsafe_call_receiver(call.object, &mut ran, ctx);
    let mut assigned = false;
    for arg in call.argument_list.arguments.iter() {
        assigned |= process_nested_assignments(arg.value(), &mut ran, ctx);
    }
    scope.merge_branch(&ran);
    assigned
}

fn process_nested_assignments_in_element<'b>(
    elem: &'b ArrayElement<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    match elem {
        ArrayElement::KeyValue(kv) => {
            let key_assigned = process_nested_assignments(kv.key, scope, ctx);
            let value_assigned = process_nested_assignments(kv.value, scope, ctx);
            key_assigned || value_assigned
        }
        ArrayElement::Value(v) => process_nested_assignments(v.value, scope, ctx),
        ArrayElement::Variadic(v) => process_nested_assignments(v.value, scope, ctx),
        ArrayElement::Missing(_) => false,
    }
}

fn process_match_nested_assignments<'b>(
    match_expr: &'b Match<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let mut assigned = process_nested_assignments(match_expr.expression, scope, ctx);

    let mut arms = match_expr.arms.iter().map(|arm| {
        let mut arm_scope = scope.clone();
        assigned |= process_nested_assignments(arm.expression(), &mut arm_scope, ctx);
        arm_scope
    });
    if let Some(mut merged) = arms.next() {
        for arm_scope in arms {
            merged.merge_branch(&arm_scope);
        }
        *scope = merged;
    }
    assigned
}

fn process_conditional_nested_assignments<'b>(
    conditional: &'b Conditional<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let mut assigned = process_nested_assignments(conditional.condition, scope, ctx);

    let mut then_scope = scope.clone();
    if let Some(then_expr) = conditional.then {
        assigned |= process_nested_assignments(then_expr, &mut then_scope, ctx);
    }
    let mut else_scope = scope.clone();
    assigned |= process_nested_assignments(conditional.r#else, &mut else_scope, ctx);

    then_scope.merge_branch(&else_scope);
    *scope = then_scope;
    assigned
}
