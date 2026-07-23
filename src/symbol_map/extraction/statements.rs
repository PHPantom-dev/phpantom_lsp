use mago_span::HasSpan;
use mago_syntax::cst::*;

use super::*;

// ─── Statement extractor ────────────────────────────────────────────────────

pub(super) fn extract_from_statement<'a>(
    stmt: &'a Statement<'a>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
    match stmt {
        Statement::Namespace(ns) => {
            // Emit a span for the namespace name itself so rename can target it.
            if let Some(ref ident) = ns.name {
                let name = bytes_to_str(ident.value()).to_string();
                if !name.is_empty() {
                    ctx.spans.push(SymbolSpan {
                        start: ident.span().start.offset,
                        end: ident.span().end.offset,
                        kind: SymbolKind::NamespaceDeclaration { name },
                    });
                }
            }
            for inner in ns.statements().iter() {
                extract_from_statement(inner, ctx, scope_start);
            }
        }
        Statement::Class(class) => {
            extract_from_class(class, ctx);
        }
        Statement::Interface(iface) => {
            extract_from_interface(iface, ctx);
        }
        Statement::Trait(trait_def) => {
            extract_from_trait(trait_def, ctx);
        }
        Statement::Enum(enum_def) => {
            extract_from_enum(enum_def, ctx);
        }
        Statement::Function(func) => {
            extract_from_function(func, ctx);
        }
        Statement::Use(use_stmt) => {
            emit_keyword(&use_stmt.r#use, ctx);
            extract_from_use_statement(use_stmt, &mut ctx.spans);
        }
        Statement::Expression(expr_stmt) => {
            extract_inline_docblock(expr_stmt, ctx, scope_start);
            // Detect `assert($var instanceof ...)` and record its offset
            // as a sequential narrowing boundary for the diagnostic cache.
            if is_assert_instanceof(expr_stmt.expression) {
                ctx.assert_narrowing_offsets
                    .push(expr_stmt.expression.span().start.offset);
            }
            extract_from_expression(expr_stmt.expression, ctx, scope_start);
        }
        Statement::Return(ret) => {
            emit_keyword(&ret.r#return, ctx);
            extract_inline_docblock(ret, ctx, scope_start);
            if let Some(val) = ret.value {
                extract_from_expression(val, ctx, scope_start);
            }
        }
        Statement::Echo(echo) => {
            emit_keyword(&echo.echo, ctx);
            extract_inline_docblock(echo, ctx, scope_start);
            for expr in echo.values.iter() {
                extract_from_expression(expr, ctx, scope_start);
            }
        }
        Statement::If(if_stmt) => {
            emit_keyword(&if_stmt.r#if, ctx);
            extract_from_expression(if_stmt.condition, ctx, scope_start);
            extract_from_if_body(&if_stmt.body, ctx, scope_start);
        }
        Statement::While(while_stmt) => {
            emit_keyword(&while_stmt.r#while, ctx);
            extract_from_expression(while_stmt.condition, ctx, scope_start);
            let body_span = while_stmt.body.span();
            record_breakable_scope(body_span.start.offset, body_span.end.offset, ctx);
            record_loop_scope(body_span.start.offset, body_span.end.offset, ctx);
            extract_from_while_body(&while_stmt.body, ctx, scope_start);
        }
        Statement::DoWhile(do_while) => {
            emit_keyword(&do_while.r#do, ctx);
            emit_keyword(&do_while.r#while, ctx);
            let body_span = do_while.statement.span();
            record_breakable_scope(body_span.start.offset, body_span.end.offset, ctx);
            record_loop_scope(body_span.start.offset, body_span.end.offset, ctx);
            extract_from_statement(do_while.statement, ctx, scope_start);
            extract_from_expression(do_while.condition, ctx, scope_start);
        }
        Statement::For(for_stmt) => {
            emit_keyword(&for_stmt.r#for, ctx);
            for expr in for_stmt.initializations.iter() {
                extract_from_expression(expr, ctx, scope_start);
            }
            for expr in for_stmt.conditions.iter() {
                extract_from_expression(expr, ctx, scope_start);
            }
            for expr in for_stmt.increments.iter() {
                extract_from_expression(expr, ctx, scope_start);
            }
            let body_span = for_stmt.body.span();
            record_breakable_scope(body_span.start.offset, body_span.end.offset, ctx);
            record_loop_scope(body_span.start.offset, body_span.end.offset, ctx);
            extract_from_for_body(&for_stmt.body, ctx, scope_start);
        }
        Statement::Foreach(foreach_stmt) => {
            emit_keyword(&foreach_stmt.foreach, ctx);
            emit_keyword(&foreach_stmt.r#as, ctx);
            extract_from_expression(foreach_stmt.expression, ctx, scope_start);
            // key and value are accessed via the target.
            if let Some(key_expr) = foreach_stmt.target.key() {
                extract_from_expression(key_expr, ctx, scope_start);
                // Emit VarDefSite for foreach key variable.
                if let Expression::Variable(Variable::Direct(dv)) = key_expr {
                    let name = {
                        let s = bytes_to_str(dv.name);
                        s.strip_prefix('$').unwrap_or(s).to_string()
                    };
                    let offset = dv.span.start.offset;
                    ctx.var_defs.push(VarDefSite {
                        offset,
                        name,
                        kind: VarDefKind::Foreach,
                        scope_start,
                        effective_from: offset,
                        nesting_depth: ctx.cond_nesting_depth,
                        block_end: ctx.cond_block_end_stack.last().copied().unwrap_or(u32::MAX),
                    });
                }
            }
            let value_expr = foreach_stmt.target.value();
            extract_from_expression(value_expr, ctx, scope_start);
            // Emit VarDefSite for foreach value variable.
            if let Expression::Variable(Variable::Direct(dv)) = value_expr {
                let name = {
                    let s = bytes_to_str(dv.name);
                    s.strip_prefix('$').unwrap_or(s).to_string()
                };
                let offset = dv.span.start.offset;
                ctx.var_defs.push(VarDefSite {
                    offset,
                    name,
                    kind: VarDefKind::Foreach,
                    scope_start,
                    effective_from: offset,
                    nesting_depth: ctx.cond_nesting_depth,
                    block_end: ctx.cond_block_end_stack.last().copied().unwrap_or(u32::MAX),
                });
            } else if let Expression::Array(arr) = value_expr {
                // Destructuring: `foreach ($items as [$name, $value])`
                collect_destructuring_var_defs(
                    &arr.elements,
                    &mut ctx.var_defs,
                    scope_start,
                    VarDefKind::Foreach,
                    value_expr.span().start.offset,
                );
            } else if let Expression::List(list) = value_expr {
                // Destructuring: `foreach ($items as list($name, $value))`
                collect_destructuring_var_defs(
                    &list.elements,
                    &mut ctx.var_defs,
                    scope_start,
                    VarDefKind::Foreach,
                    value_expr.span().start.offset,
                );
            }
            let body_span = foreach_stmt.body.span();
            record_breakable_scope(body_span.start.offset, body_span.end.offset, ctx);
            record_loop_scope(body_span.start.offset, body_span.end.offset, ctx);
            for inner in foreach_stmt.body.statements() {
                extract_from_statement(inner, ctx, scope_start);
            }
            if let ForeachBody::ColonDelimited(body) = &foreach_stmt.body {
                emit_keyword(&body.end_foreach, ctx);
            }
        }
        Statement::Switch(switch_stmt) => {
            emit_keyword(&switch_stmt.switch, ctx);
            extract_from_expression(switch_stmt.expression, ctx, scope_start);
            let switch_span = switch_stmt.body.span();
            record_breakable_scope(switch_span.start.offset, switch_span.end.offset, ctx);
            ctx.switch_scopes
                .push((switch_span.start.offset, switch_span.end.offset));
            extract_from_switch_body(&switch_stmt.body, ctx, scope_start);
        }
        Statement::Try(try_stmt) => {
            emit_keyword(&try_stmt.r#try, ctx);
            let try_block_end = try_stmt.block.span().end.offset;
            push_cond_nesting(ctx, try_block_end);
            for s in try_stmt.block.statements.iter() {
                extract_from_statement(s, ctx, scope_start);
            }
            pop_cond_nesting(ctx);
            for catch in try_stmt.catch_clauses.iter() {
                emit_keyword(&catch.r#catch, ctx);
                // Catch type hint is a navigable class reference.
                extract_from_hint_ctx(&catch.hint, &mut ctx.spans, ClassRefContext::Catch);
                // The caught variable.
                if let Some(ref var) = catch.variable {
                    let var_name = {
                        let s = bytes_to_str(var.name);
                        s.strip_prefix('$').unwrap_or(s).to_string()
                    };
                    ctx.spans.push(SymbolSpan {
                        start: var.span.start.offset,
                        end: var.span.end.offset,
                        kind: SymbolKind::Variable {
                            name: var_name.clone(),
                        },
                    });
                    // Emit VarDefSite for catch variable.
                    let catch_var_offset = var.span.start.offset;
                    ctx.var_defs.push(VarDefSite {
                        offset: catch_var_offset,
                        name: var_name,
                        kind: VarDefKind::Catch,
                        scope_start,
                        effective_from: catch_var_offset,
                        nesting_depth: ctx.cond_nesting_depth,
                        block_end: ctx.cond_block_end_stack.last().copied().unwrap_or(u32::MAX),
                    });
                }
                let catch_block_end = catch.block.span().end.offset;
                push_cond_nesting(ctx, catch_block_end);
                for s in catch.block.statements.iter() {
                    extract_from_statement(s, ctx, scope_start);
                }
                pop_cond_nesting(ctx);
            }
            if let Some(ref finally) = try_stmt.finally_clause {
                emit_keyword(&finally.r#finally, ctx);
                for s in finally.block.statements.iter() {
                    extract_from_statement(s, ctx, scope_start);
                }
            }
        }
        Statement::Block(block) => {
            for s in block.statements.iter() {
                extract_from_statement(s, ctx, scope_start);
            }
        }
        Statement::Global(global) => {
            emit_keyword(&global.global, ctx);
            for var in global.variables.iter() {
                if let Variable::Direct(dv) = var {
                    let name = {
                        let s = bytes_to_str(dv.name);
                        s.strip_prefix('$').unwrap_or(s).to_string()
                    };
                    ctx.spans.push(SymbolSpan {
                        start: dv.span.start.offset,
                        end: dv.span.end.offset,
                        kind: SymbolKind::Variable { name: name.clone() },
                    });
                    // Emit VarDefSite for global variable.
                    let global_offset = dv.span.start.offset;
                    ctx.var_defs.push(VarDefSite {
                        offset: global_offset,
                        name,
                        kind: VarDefKind::GlobalDecl,
                        scope_start,
                        effective_from: global_offset,
                        nesting_depth: ctx.cond_nesting_depth,
                        block_end: ctx.cond_block_end_stack.last().copied().unwrap_or(u32::MAX),
                    });
                }
            }
        }
        Statement::Static(static_stmt) => {
            emit_keyword(&static_stmt.r#static, ctx);
            for item in static_stmt.items.iter() {
                let dv = item.variable();
                let name = {
                    let s = bytes_to_str(dv.name);
                    s.strip_prefix('$').unwrap_or(s).to_string()
                };
                ctx.spans.push(SymbolSpan {
                    start: dv.span.start.offset,
                    end: dv.span.end.offset,
                    kind: SymbolKind::Variable { name: name.clone() },
                });
                // Emit VarDefSite for static variable.
                let static_offset = dv.span.start.offset;
                ctx.var_defs.push(VarDefSite {
                    offset: static_offset,
                    name,
                    kind: VarDefKind::StaticDecl,
                    scope_start,
                    effective_from: static_offset,
                    nesting_depth: ctx.cond_nesting_depth,
                    block_end: ctx.cond_block_end_stack.last().copied().unwrap_or(u32::MAX),
                });
            }
        }
        Statement::Unset(unset_stmt) => {
            emit_keyword(&unset_stmt.unset, ctx);
            for val in unset_stmt.values.iter() {
                extract_from_expression(val, ctx, scope_start);
                if let Expression::Variable(Variable::Direct(dv)) = val {
                    let name = {
                        let s = bytes_to_str(dv.name);
                        s.strip_prefix('$').unwrap_or(s).to_string()
                    };
                    ctx.var_defs.push(VarDefSite {
                        offset: dv.span.start.offset,
                        name,
                        kind: VarDefKind::Unset,
                        scope_start,
                        effective_from: unset_stmt.span().end.offset,
                        nesting_depth: ctx.cond_nesting_depth,
                        block_end: ctx.cond_block_end_stack.last().copied().unwrap_or(u32::MAX),
                    });
                }
            }
        }
        Statement::Constant(constant) => {
            emit_keyword(&constant.r#const, ctx);
            // Top-level `const FOO = Expr;` — walk value expressions so
            // that class references like `Foo::class` produce spans.
            extract_from_attribute_lists(&constant.attribute_lists, ctx, scope_start);
            for item in constant.items.iter() {
                extract_from_expression(item.value, ctx, scope_start);
            }
        }
        Statement::Declare(declare) => {
            // `declare(strict_types=1) { ... }` — walk the body if present.
            match &declare.body {
                DeclareBody::Statement(inner) => {
                    extract_from_statement(inner, ctx, scope_start);
                }
                DeclareBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        extract_from_statement(s, ctx, scope_start);
                    }
                }
            }
        }
        Statement::EchoTag(echo_tag) => {
            // `<?= $expr ?>` — walk expressions inside short echo tags.
            for expr in echo_tag.values.iter() {
                extract_from_expression(expr, ctx, scope_start);
            }
        }
        Statement::Break(brk) => {
            emit_keyword(&brk.r#break, ctx);
        }
        Statement::Continue(cont) => {
            emit_keyword(&cont.r#continue, ctx);
        }
        Statement::HaltCompiler(hc) => {
            emit_keyword(&hc.halt_compiler, ctx);
        }
        Statement::Goto(goto_stmt) => {
            emit_keyword(&goto_stmt.goto, ctx);
        }
        _ => {}
    }
}

// ─── If / While / For / Switch body helpers ─────────────────────────────────

pub(super) fn record_breakable_scope(start: u32, end: u32, ctx: &mut ExtractionCtx<'_>) {
    if start <= end {
        ctx.breakable_scopes.push((start, end));
    }
}

pub(super) fn record_loop_scope(start: u32, end: u32, ctx: &mut ExtractionCtx<'_>) {
    if start <= end {
        ctx.loop_scopes.push((start, end));
    }
}

/// Push a new conditional nesting level with the given block end offset.
pub(super) fn push_cond_nesting(ctx: &mut ExtractionCtx<'_>, block_end: u32) {
    ctx.cond_nesting_depth += 1;
    ctx.cond_block_end_stack.push(block_end);
}

/// Pop the most recent conditional nesting level.
pub(super) fn pop_cond_nesting(ctx: &mut ExtractionCtx<'_>) {
    ctx.cond_nesting_depth = ctx.cond_nesting_depth.saturating_sub(1);
    ctx.cond_block_end_stack.pop();
}

pub(super) fn extract_from_if_body<'a>(
    body: &'a IfBody<'a>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
    match body {
        IfBody::Statement(stmt_body) => {
            // Record then-body as a narrowing block.
            let then_span = stmt_body.statement.span();
            ctx.narrowing_blocks
                .push((then_span.start.offset, then_span.end.offset));
            push_cond_nesting(ctx, then_span.end.offset);
            extract_from_statement(stmt_body.statement, ctx, scope_start);
            pop_cond_nesting(ctx);
            for else_if in stmt_body.else_if_clauses.iter() {
                emit_keyword(&else_if.elseif, ctx);
                extract_from_expression(else_if.condition, ctx, scope_start);
                // Record elseif-body as a narrowing block.
                let ei_span = else_if.statement.span();
                ctx.narrowing_blocks
                    .push((ei_span.start.offset, ei_span.end.offset));
                push_cond_nesting(ctx, ei_span.end.offset);
                extract_from_statement(else_if.statement, ctx, scope_start);
                pop_cond_nesting(ctx);
            }
            if let Some(ref else_clause) = stmt_body.else_clause {
                emit_keyword(&else_clause.r#else, ctx);
                // Record else-body as a narrowing block.
                let el_span = else_clause.statement.span();
                ctx.narrowing_blocks
                    .push((el_span.start.offset, el_span.end.offset));
                push_cond_nesting(ctx, el_span.end.offset);
                extract_from_statement(else_clause.statement, ctx, scope_start);
                pop_cond_nesting(ctx);
            }
        }
        IfBody::ColonDelimited(colon_body) => {
            // Record the then-body span (first statement to last).
            if let (Some(first), Some(last)) =
                (colon_body.statements.first(), colon_body.statements.last())
            {
                ctx.narrowing_blocks
                    .push((first.span().start.offset, last.span().end.offset));
            }
            let colon_end = colon_body
                .statements
                .last()
                .map(|s| s.span().end.offset)
                .unwrap_or(0);
            push_cond_nesting(ctx, colon_end);
            for inner in colon_body.statements.iter() {
                extract_from_statement(inner, ctx, scope_start);
            }
            pop_cond_nesting(ctx);
            for else_if in colon_body.else_if_clauses.iter() {
                emit_keyword(&else_if.elseif, ctx);
                extract_from_expression(else_if.condition, ctx, scope_start);
                if let (Some(first), Some(last)) =
                    (else_if.statements.first(), else_if.statements.last())
                {
                    ctx.narrowing_blocks
                        .push((first.span().start.offset, last.span().end.offset));
                }
                let ei_end = else_if
                    .statements
                    .last()
                    .map(|s| s.span().end.offset)
                    .unwrap_or(0);
                push_cond_nesting(ctx, ei_end);
                for inner in else_if.statements.iter() {
                    extract_from_statement(inner, ctx, scope_start);
                }
                pop_cond_nesting(ctx);
            }
            if let Some(ref else_clause) = colon_body.else_clause {
                emit_keyword(&else_clause.r#else, ctx);
                if let (Some(first), Some(last)) = (
                    else_clause.statements.first(),
                    else_clause.statements.last(),
                ) {
                    ctx.narrowing_blocks
                        .push((first.span().start.offset, last.span().end.offset));
                }
                let el_end = else_clause
                    .statements
                    .last()
                    .map(|s| s.span().end.offset)
                    .unwrap_or(0);
                push_cond_nesting(ctx, el_end);
                for inner in else_clause.statements.iter() {
                    extract_from_statement(inner, ctx, scope_start);
                }
                pop_cond_nesting(ctx);
            }
            emit_keyword(&colon_body.endif, ctx);
        }
    }
}

pub(super) fn extract_from_while_body<'a>(
    body: &'a WhileBody<'a>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
    match body {
        WhileBody::Statement(inner) => {
            extract_from_statement(inner, ctx, scope_start);
        }
        WhileBody::ColonDelimited(colon_body) => {
            for inner in colon_body.statements.iter() {
                extract_from_statement(inner, ctx, scope_start);
            }
            emit_keyword(&colon_body.end_while, ctx);
        }
    }
}

pub(super) fn extract_from_for_body<'a>(
    body: &'a ForBody<'a>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
    match body {
        ForBody::Statement(inner) => {
            extract_from_statement(inner, ctx, scope_start);
        }
        ForBody::ColonDelimited(colon_body) => {
            for inner in colon_body.statements.iter() {
                extract_from_statement(inner, ctx, scope_start);
            }
            emit_keyword(&colon_body.end_for, ctx);
        }
    }
}

pub(super) fn extract_from_switch_body<'a>(
    body: &'a SwitchBody<'a>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
    let cases = match body {
        SwitchBody::BraceDelimited(b) => &b.cases,
        SwitchBody::ColonDelimited(b) => &b.cases,
    };
    for case in cases.iter() {
        match case {
            SwitchCase::Expression(expr_case) => emit_keyword(&expr_case.case, ctx),
            SwitchCase::Default(def_case) => emit_keyword(&def_case.default, ctx),
        }
        let case_end = case
            .statements()
            .last()
            .map(|s| s.span().end.offset)
            .unwrap_or(0);
        push_cond_nesting(ctx, case_end);
        for inner in case.statements().iter() {
            extract_from_statement(inner, ctx, scope_start);
        }
        pop_cond_nesting(ctx);
    }
    if let SwitchBody::ColonDelimited(b) = body {
        emit_keyword(&b.end_switch, ctx);
    }
}
