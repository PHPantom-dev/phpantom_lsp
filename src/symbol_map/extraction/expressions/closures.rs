use mago_span::HasSpan;
use mago_syntax::cst::*;

use super::*;

// ─── Closures ────────────────────────────────────────────────────────────────

pub(super) fn extract_closure_expr<'a>(closure: &'a Closure<'a>, ctx: &mut ExtractionCtx<'a>) {
    // Closure introduces a new scope.
    let closure_scope_start = closure.body.left_brace.start.offset;
    let closure_scope_end = closure.body.right_brace.end.offset;
    ctx.scopes.push((closure_scope_start, closure_scope_end));
    ctx.body_scopes
        .push((closure_scope_start, closure_scope_end));

    for param in closure.parameter_list.parameters.iter() {
        // Attributes (PHP 8) on the parameter.
        extract_from_attribute_lists(&param.attribute_lists, ctx, 0);
        if let Some(ref hint) = param.hint {
            extract_from_hint_ctx(hint, &mut ctx.spans, ClassRefContext::TypeHint);
        }
        let name = {
            let s = bytes_to_str(param.variable.name);
            s.strip_prefix('$').unwrap_or(s).to_string()
        };
        ctx.spans.push(SymbolSpan {
            start: param.variable.span.start.offset,
            end: param.variable.span.end.offset,
            kind: SymbolKind::Variable { name: name.clone() },
        });
        // Emit VarDefSite for closure parameter.
        let cp_offset = param.variable.span.start.offset;
        ctx.var_defs.push(VarDefSite {
            offset: cp_offset,
            name,
            kind: VarDefKind::Parameter,
            scope_start: closure_scope_start,
            effective_from: cp_offset,
            nesting_depth: ctx.cond_nesting_depth,
            block_end: ctx.cond_block_end_stack.last().copied().unwrap_or(u32::MAX),
        });
        if let Some(ref default) = param.default_value {
            extract_from_expression(default.value, ctx, closure_scope_start);
        }
    }
    if let Some(ref use_clause) = closure.use_clause {
        for var in use_clause.variables.iter() {
            let name = {
                let s = bytes_to_str(var.variable.name);
                s.strip_prefix('$').unwrap_or(s).to_string()
            };
            ctx.spans.push(SymbolSpan {
                start: var.variable.span.start.offset,
                end: var.variable.span.end.offset,
                kind: SymbolKind::Variable { name: name.clone() },
            });
            // Emit VarDefSite so that GTD inside the closure body
            // can find the captured variable.  The definition is
            // scoped to the closure body and immediately visible.
            let use_var_offset = var.variable.span.start.offset;
            ctx.var_defs.push(VarDefSite {
                offset: use_var_offset,
                name,
                kind: VarDefKind::ClosureCapture,
                scope_start: closure_scope_start,
                effective_from: use_var_offset,
                nesting_depth: ctx.cond_nesting_depth,
                block_end: ctx.cond_block_end_stack.last().copied().unwrap_or(u32::MAX),
            });
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
        // Attributes (PHP 8) on the parameter.
        extract_from_attribute_lists(&param.attribute_lists, ctx, 0);
        if let Some(ref hint) = param.hint {
            extract_from_hint_ctx(hint, &mut ctx.spans, ClassRefContext::TypeHint);
        }
        let name = {
            let s = bytes_to_str(param.variable.name);
            s.strip_prefix('$').unwrap_or(s).to_string()
        };
        ctx.spans.push(SymbolSpan {
            start: param.variable.span.start.offset,
            end: param.variable.span.end.offset,
            kind: SymbolKind::Variable { name: name.clone() },
        });
        // Emit VarDefSite for arrow function parameter.
        let ap_offset = param.variable.span.start.offset;
        ctx.var_defs.push(VarDefSite {
            offset: ap_offset,
            name,
            kind: VarDefKind::Parameter,
            scope_start: arrow_scope_start,
            effective_from: ap_offset,
            nesting_depth: ctx.cond_nesting_depth,
            block_end: ctx.cond_block_end_stack.last().copied().unwrap_or(u32::MAX),
        });
        if let Some(ref default) = param.default_value {
            extract_from_expression(default.value, ctx, arrow_scope_start);
        }
    }
    if let Some(ref return_type) = arrow.return_type_hint {
        extract_from_hint_ctx(&return_type.hint, &mut ctx.spans, ClassRefContext::TypeHint);
    }
    extract_from_expression(arrow.expression, ctx, arrow_scope_start);
}
