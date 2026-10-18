use mago_span::HasSpan;

use super::*;

// ─── Closures ────────────────────────────────────────────────────────────────

pub(super) fn extract_closure_expr<'a>(closure: &'a Closure<'a>, ctx: &mut ExtractionCtx<'a>) {
    let closure_scope_start = closure.body.left_brace.start.offset;
    let closure_scope_end = closure.body.right_brace.end.offset;
    ctx.scopes.push((closure_scope_start, closure_scope_end));
    ctx.body_scopes
        .push((closure_scope_start, closure_scope_end));

    for param in closure.parameter_list.parameters.iter() {
        extract_from_parameter(param, ctx, closure_scope_start);
    }
    if let Some(ref use_clause) = closure.use_clause {
        for var in use_clause.variables.iter() {
            // The capture is defined in the closure body's scope and is
            // visible at once, so go-to-definition inside the body finds it.
            ctx.push_variable_definition(
                (var.variable.span.start.offset, var.variable.span.end.offset),
                bare_variable_name(var.variable.name),
                VarDefKind::ClosureCapture,
                closure_scope_start,
            );
        }
    }
    if let Some(ref return_type) = closure.return_type_hint {
        extract_from_hint_ctx(&return_type.hint, &mut ctx.spans, ClassRefContext::TypeHint);
    }
    for s in closure.body.statements.iter() {
        extract_from_statement(s, ctx, closure_scope_start);
    }
}

// ─── Arrow functions ─────────────────────────────────────────────────────────

pub(super) fn extract_arrow_function_expr<'a>(
    arrow: &'a ArrowFunction<'a>,
    ctx: &mut ExtractionCtx<'a>,
) {
    // Arrow functions introduce a new scope for their parameters.
    // They don't have braces, so use the span of the arrow function itself.
    let arrow_scope_start = arrow.span().start.offset;
    let arrow_scope_end = arrow.span().end.offset;
    ctx.scopes.push((arrow_scope_start, arrow_scope_end));
    ctx.arrow_fn_scopes.push(arrow_scope_start);
    // Body scope starts at `=>` for signature help suppression.
    ctx.body_scopes
        .push((arrow.arrow.start.offset, arrow_scope_end));

    for param in arrow.parameter_list.parameters.iter() {
        extract_from_parameter(param, ctx, arrow_scope_start);
    }
    if let Some(ref return_type) = arrow.return_type_hint {
        extract_from_hint_ctx(&return_type.hint, &mut ctx.spans, ClassRefContext::TypeHint);
    }
    extract_from_expression(arrow.expression, ctx, arrow_scope_start);
}
