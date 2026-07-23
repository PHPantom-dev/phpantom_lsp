/// Property-path narrowing: applies instanceof / assert / guard-clause
/// narrowing to a `$this->prop` (or `$obj->prop`) resolution result by
/// walking the enclosing method body from its start down to the cursor.
use std::collections::HashMap;
use std::sync::Arc;

use crate::types::ClassInfo;

use super::{Loaders, ResolutionCtx, VarResolutionCtx};

pub(crate) fn apply_property_narrowing(
    property_path: &str,
    current_class: &ClassInfo,
    rctx: &ResolutionCtx<'_>,
    results: &mut Vec<Arc<ClassInfo>>, // still operates on Arc<ClassInfo> — called from property chain
) {
    use crate::parser::with_parsed_program;

    // The narrowing walk functions operate on Vec<ClassInfo>, so unwrap
    // the Arcs, run narrowing, then re-wrap.
    let mut plain: Vec<ClassInfo> = results.drain(..).map(Arc::unwrap_or_clone).collect();

    with_parsed_program(
        rctx.content,
        "apply_property_narrowing",
        |program, _content| {
            let ctx = VarResolutionCtx {
                var_name: property_path,
                current_class,
                all_classes: rctx.all_classes,
                content: rctx.content,
                cursor_offset: rctx.cursor_offset,
                class_loader: rctx.class_loader,
                loaders: Loaders::with_function(rctx.function_loader),
                resolved_class_cache: crate::virtual_members::active_resolved_class_cache(),
                enclosing_return_type: None,
                top_level_scope: None,
                branch_aware: false,
                match_arm_narrowing: HashMap::new(),
                scope_var_resolver: None,
            };
            walk_property_narrowing_in_statements(program.statements.iter(), &ctx, &mut plain);
        },
    );

    *results = plain.into_iter().map(Arc::new).collect();
}

/// Walk top-level statements to find the class + method containing the
/// cursor, then apply narrowing to `results` for the given property path.
fn walk_property_narrowing_in_statements<'b>(
    statements: impl Iterator<Item = &'b mago_syntax::cst::Statement<'b>>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) {
    use mago_span::HasSpan;
    use mago_syntax::cst::*;

    for stmt in statements {
        match stmt {
            Statement::Class(class) => {
                let start = class.left_brace.start.offset;
                let end = class.right_brace.end.offset;
                if ctx.cursor_offset >= start && ctx.cursor_offset <= end {
                    walk_property_narrowing_in_members(class.members.iter(), ctx, results);
                    return;
                }
            }
            Statement::Trait(trait_def) => {
                let start = trait_def.left_brace.start.offset;
                let end = trait_def.right_brace.end.offset;
                if ctx.cursor_offset >= start && ctx.cursor_offset <= end {
                    walk_property_narrowing_in_members(trait_def.members.iter(), ctx, results);
                    return;
                }
            }
            Statement::Namespace(ns) => {
                let ns_span = ns.span();
                if ctx.cursor_offset >= ns_span.start.offset
                    && ctx.cursor_offset <= ns_span.end.offset
                {
                    walk_property_narrowing_in_statements(ns.statements().iter(), ctx, results);
                    return;
                }
            }
            Statement::Function(func) => {
                let body_start = func.body.left_brace.start.offset;
                let body_end = func.body.right_brace.end.offset;
                if ctx.cursor_offset >= body_start && ctx.cursor_offset <= body_end {
                    walk_property_narrowing_stmts(func.body.statements.iter(), ctx, results);
                    return;
                }
            }
            // ── Functions inside if-guards / blocks ──
            // The common PHP pattern `if (! function_exists('foo'))
            // { function foo(…) { … } }` nests the function
            // declaration inside an if body.  Recurse into blocks
            // and if-bodies so property narrowing still works.
            Statement::If(if_stmt) => {
                let if_span = stmt.span();
                if ctx.cursor_offset >= if_span.start.offset
                    && ctx.cursor_offset <= if_span.end.offset
                {
                    for inner in if_stmt.body.statements().iter() {
                        walk_property_narrowing_in_statements(std::iter::once(inner), ctx, results);
                    }
                }
            }
            Statement::Block(block) => {
                let blk_span = stmt.span();
                if ctx.cursor_offset >= blk_span.start.offset
                    && ctx.cursor_offset <= blk_span.end.offset
                {
                    walk_property_narrowing_in_statements(block.statements.iter(), ctx, results);
                }
            }
            _ => {}
        }
    }
}

/// Walk class members to find the method containing the cursor, then
/// apply instanceof / guard-clause narrowing for the property path.
fn walk_property_narrowing_in_members<'b>(
    members: impl Iterator<Item = &'b mago_syntax::cst::class_like::member::ClassLikeMember<'b>>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) {
    use mago_syntax::cst::class_like::member::ClassLikeMember;
    use mago_syntax::cst::class_like::method::MethodBody;

    for member in members {
        if let ClassLikeMember::Method(method) = member {
            let body = match &method.body {
                MethodBody::Concrete(block) => block,
                _ => continue,
            };
            let body_start = body.left_brace.start.offset;
            let body_end = body.right_brace.end.offset;
            if ctx.cursor_offset >= body_start && ctx.cursor_offset <= body_end {
                walk_property_narrowing_stmts(body.statements.iter(), ctx, results);
                return;
            }
        }
    }
}

/// Walk statements applying only narrowing (no assignment scanning)
/// for a property path like `$this->prop`.
fn walk_property_narrowing_stmts<'b>(
    statements: impl Iterator<Item = &'b mago_syntax::cst::Statement<'b>>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) {
    use mago_span::HasSpan;
    use mago_syntax::cst::*;

    use crate::completion::types::narrowing;

    for stmt in statements {
        let stmt_span = stmt.span();
        // Only consider statements whose start is before the cursor.
        if stmt_span.start.offset >= ctx.cursor_offset {
            continue;
        }

        match stmt {
            Statement::If(if_stmt) => {
                walk_property_narrowing_if(if_stmt, stmt, ctx, results);
            }
            Statement::Block(block) => {
                walk_property_narrowing_stmts(block.statements.iter(), ctx, results);
            }
            Statement::Expression(expr_stmt) => {
                // assert($this->prop instanceof Foo) — unconditional
                narrowing::try_apply_assert_instanceof_narrowing(
                    expr_stmt.expression,
                    ctx,
                    results,
                );
                // `$x = $this->prop instanceof Foo ? … : …` and other
                // ternaries nested in the expression narrow the property
                // path inside the branch containing the cursor.
                walk_property_narrowing_expr(expr_stmt.expression, ctx, results);
            }
            Statement::Return(ret) => {
                // `return $this->prop instanceof Foo ? … : …` — narrow the
                // property path inside the ternary branch at the cursor.
                if let Some(value) = ret.value {
                    walk_property_narrowing_expr(value, ctx, results);
                }
            }
            Statement::Foreach(foreach) => match &foreach.body {
                ForeachBody::Statement(inner) => {
                    walk_property_narrowing_stmt(inner, ctx, results);
                }
                ForeachBody::ColonDelimited(body) => {
                    walk_property_narrowing_stmts(body.statements.iter(), ctx, results);
                }
            },
            Statement::While(while_stmt) => match &while_stmt.body {
                WhileBody::Statement(inner) => {
                    walk_property_narrowing_stmt(inner, ctx, results);
                }
                WhileBody::ColonDelimited(body) => {
                    walk_property_narrowing_stmts(body.statements.iter(), ctx, results);
                }
            },
            Statement::For(for_stmt) => match &for_stmt.body {
                ForBody::Statement(inner) => {
                    walk_property_narrowing_stmt(inner, ctx, results);
                }
                ForBody::ColonDelimited(body) => {
                    walk_property_narrowing_stmts(body.statements.iter(), ctx, results);
                }
            },
            Statement::DoWhile(dw) => {
                walk_property_narrowing_stmt(dw.statement, ctx, results);
            }
            Statement::Try(try_stmt) => {
                walk_property_narrowing_stmts(try_stmt.block.statements.iter(), ctx, results);
                for catch in try_stmt.catch_clauses.iter() {
                    walk_property_narrowing_stmts(catch.block.statements.iter(), ctx, results);
                }
                if let Some(finally) = &try_stmt.finally_clause {
                    walk_property_narrowing_stmts(finally.block.statements.iter(), ctx, results);
                }
            }
            Statement::Switch(switch) => {
                for case in switch.body.cases().iter() {
                    walk_property_narrowing_stmts(case.statements().iter(), ctx, results);
                }
            }
            _ => {}
        }
    }
}

/// Apply property-level narrowing inside an if / elseif / else chain.
fn walk_property_narrowing_if<'b>(
    if_stmt: &'b mago_syntax::cst::If<'b>,
    enclosing_stmt: &'b mago_syntax::cst::Statement<'b>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) {
    use mago_span::HasSpan;
    use mago_syntax::cst::*;

    use crate::completion::types::narrowing;

    match &if_stmt.body {
        IfBody::Statement(body) => {
            // ── then-body narrowing ──
            narrowing::try_apply_instanceof_narrowing(
                if_stmt.condition,
                body.statement.span(),
                ctx,
                results,
            );
            walk_property_narrowing_stmt(body.statement, ctx, results);

            // ── elseif narrowing ──
            for else_if in body.else_if_clauses.iter() {
                narrowing::try_apply_instanceof_narrowing(
                    else_if.condition,
                    else_if.statement.span(),
                    ctx,
                    results,
                );
                walk_property_narrowing_stmt(else_if.statement, ctx, results);
            }

            // ── else-body inverse narrowing ──
            if let Some(else_clause) = &body.else_clause {
                let else_span = else_clause.statement.span();
                narrowing::try_apply_instanceof_narrowing_inverse(
                    if_stmt.condition,
                    else_span,
                    ctx,
                    results,
                );
                for else_if in body.else_if_clauses.iter() {
                    narrowing::try_apply_instanceof_narrowing_inverse(
                        else_if.condition,
                        else_span,
                        ctx,
                        results,
                    );
                }
                walk_property_narrowing_stmt(else_clause.statement, ctx, results);
            }
        }
        IfBody::ColonDelimited(body) => {
            let then_end = if !body.else_if_clauses.is_empty() {
                body.else_if_clauses
                    .first()
                    .unwrap()
                    .elseif
                    .span()
                    .start
                    .offset
            } else if let Some(ref ec) = body.else_clause {
                ec.r#else.span().start.offset
            } else {
                body.endif.span().start.offset
            };
            let then_span = mago_span::Span::new(
                body.colon.file_id,
                body.colon.start,
                mago_span::Position::new(then_end),
            );
            narrowing::try_apply_instanceof_narrowing(if_stmt.condition, then_span, ctx, results);
            walk_property_narrowing_stmts(body.statements.iter(), ctx, results);

            for else_if in body.else_if_clauses.iter() {
                let ei_span = mago_span::Span::new(
                    else_if.colon.file_id,
                    else_if.colon.start,
                    mago_span::Position::new(
                        else_if
                            .statements
                            .span(else_if.colon.file_id, else_if.colon.end)
                            .end
                            .offset,
                    ),
                );
                narrowing::try_apply_instanceof_narrowing(else_if.condition, ei_span, ctx, results);
                walk_property_narrowing_stmts(else_if.statements.iter(), ctx, results);
            }

            if let Some(else_clause) = &body.else_clause {
                let else_span = mago_span::Span::new(
                    else_clause.colon.file_id,
                    else_clause.colon.start,
                    mago_span::Position::new(
                        else_clause
                            .statements
                            .span(else_clause.colon.file_id, else_clause.colon.end)
                            .end
                            .offset,
                    ),
                );
                narrowing::try_apply_instanceof_narrowing_inverse(
                    if_stmt.condition,
                    else_span,
                    ctx,
                    results,
                );
                for else_if in body.else_if_clauses.iter() {
                    narrowing::try_apply_instanceof_narrowing_inverse(
                        else_if.condition,
                        else_span,
                        ctx,
                        results,
                    );
                }
                walk_property_narrowing_stmts(else_clause.statements.iter(), ctx, results);
            }
        }
    }

    // ── Guard clause narrowing ──
    // When the then-body unconditionally exits and there are no
    // elseif / else branches, apply inverse narrowing after the if.
    if enclosing_stmt.span().end.offset < ctx.cursor_offset {
        narrowing::apply_guard_clause_narrowing(if_stmt, ctx, results);
    }
}

/// Dispatch a single statement to `walk_property_narrowing_stmts`.
fn walk_property_narrowing_stmt<'b>(
    stmt: &'b mago_syntax::cst::Statement<'b>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) {
    walk_property_narrowing_stmts(std::iter::once(stmt), ctx, results);
}

/// Apply property-level narrowing inside ternary (conditional) expressions.
///
/// When the cursor falls inside the then-branch of
/// `$this->prop instanceof Foo ? <then> : <else>`, the property path is
/// narrowed to `Foo`; inside the else-branch the inverse applies. This
/// mirrors the if-statement narrowing in [`walk_property_narrowing_if`]
/// but for ternaries, which can appear anywhere an expression is expected
/// (return values, assignment RHS, call arguments, …). The walk recurses
/// through those containers so a ternary nested inside them is still
/// reached.
fn walk_property_narrowing_expr<'b>(
    expr: &'b mago_syntax::cst::Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) {
    use mago_span::HasSpan;
    use mago_syntax::cst::*;

    use crate::completion::types::narrowing;

    // Only descend into the sub-expression that contains the cursor.
    let span = expr.span();
    if ctx.cursor_offset < span.start.offset || ctx.cursor_offset > span.end.offset {
        return;
    }

    match expr {
        Expression::Conditional(cond) => {
            // Full ternary `cond ? then : else`. Narrow the property path
            // in whichever branch holds the cursor. The short form
            // `$x ?: $y` has no `then` branch, so nothing to narrow there.
            if let Some(then_expr) = cond.then {
                let then_span = then_expr.span();
                if ctx.cursor_offset >= then_span.start.offset
                    && ctx.cursor_offset <= then_span.end.offset
                {
                    narrowing::try_apply_instanceof_narrowing(
                        cond.condition,
                        then_span,
                        ctx,
                        results,
                    );
                    walk_property_narrowing_expr(then_expr, ctx, results);
                    return;
                }
            }
            let else_span = cond.r#else.span();
            if ctx.cursor_offset >= else_span.start.offset
                && ctx.cursor_offset <= else_span.end.offset
            {
                narrowing::try_apply_instanceof_narrowing_inverse(
                    cond.condition,
                    else_span,
                    ctx,
                    results,
                );
                walk_property_narrowing_expr(cond.r#else, ctx, results);
            }
        }
        Expression::Assignment(assign) => {
            walk_property_narrowing_expr(assign.rhs, ctx, results);
        }
        Expression::Binary(bin) => {
            walk_property_narrowing_expr(bin.lhs, ctx, results);
            walk_property_narrowing_expr(bin.rhs, ctx, results);
        }
        Expression::Parenthesized(inner) => {
            walk_property_narrowing_expr(inner.expression, ctx, results);
        }
        Expression::Call(call) => {
            let args = match call {
                Call::Function(fc) => &fc.argument_list,
                Call::Method(mc) => &mc.argument_list,
                Call::NullSafeMethod(mc) => &mc.argument_list,
                Call::StaticMethod(sc) => &sc.argument_list,
            };
            for arg in args.arguments.iter() {
                let arg_expr = match arg {
                    Argument::Positional(a) => a.value,
                    Argument::Named(a) => a.value,
                };
                walk_property_narrowing_expr(arg_expr, ctx, results);
            }
        }
        _ => {}
    }
}
