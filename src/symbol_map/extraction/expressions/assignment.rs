use mago_span::HasSpan;

use super::*;

// ─── Assignment ──────────────────────────────────────────────────────────────

pub(super) fn extract_assignment_expr<'a>(
    assign: &'a Assignment<'a>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
    extract_from_expression(assign.lhs, ctx, scope_start);
    extract_from_expression(assign.rhs, ctx, scope_start);

    // The definition only becomes visible *after* the entire
    // assignment expression — the RHS still sees the previous
    // definition of the variable.
    let effective = assign.span().end.offset;

    // Emit VarDefSite for simple variable assignments: `$var = ...`
    match assign.lhs {
        Expression::Variable(Variable::Direct(dv)) => {
            let name = bare_variable_name(dv.name);
            let kind = if assign.operator.is_assign() {
                VarDefKind::Assignment
            } else {
                VarDefKind::CompoundAssignment
            };
            ctx.push_var_def(dv.span.start.offset, name, kind, scope_start, effective);
        }
        // Array destructuring: `[$a, $b] = ...`
        Expression::Array(arr) => {
            collect_destructuring_var_defs(
                &arr.elements,
                &mut ctx.var_defs,
                scope_start,
                VarDefKind::ArrayDestructuring,
                effective,
            );
        }
        // List destructuring: `list($a, $b) = ...`
        Expression::List(list) => {
            collect_destructuring_var_defs(
                &list.elements,
                &mut ctx.var_defs,
                scope_start,
                VarDefKind::ListDestructuring,
                effective,
            );
        }
        _ => {}
    }
}
