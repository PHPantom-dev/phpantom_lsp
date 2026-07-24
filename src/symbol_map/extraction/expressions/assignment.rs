use mago_span::HasSpan;
use mago_syntax::cst::*;

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
            let name = {
                let s = bytes_to_str(dv.name);
                s.strip_prefix('$').unwrap_or(s).to_string()
            };
            let kind = if assign.operator.is_assign() {
                VarDefKind::Assignment
            } else {
                VarDefKind::CompoundAssignment
            };
            ctx.var_defs.push(VarDefSite {
                offset: dv.span.start.offset,
                name,
                kind,
                scope_start,
                effective_from: effective,
                nesting_depth: ctx.cond_nesting_depth,
                block_end: ctx.cond_block_end_stack.last().copied().unwrap_or(u32::MAX),
            });
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
