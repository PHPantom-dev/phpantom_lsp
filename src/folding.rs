/// Folding range handler for `textDocument/foldingRange`.
///
/// Parses the PHP source, walks the AST to collect foldable regions
/// (class bodies, function/method bodies, closures, arrays, control-flow
/// blocks, argument/parameter lists), and scans trivia for doc-block and
/// consecutive single-line comment ranges.
use mago_allocator::LocalArena;
use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp::lsp_types::{FoldingRange, FoldingRangeKind};

use crate::Backend;

// ─── Public entry point ─────────────────────────────────────────────────────

impl Backend {
    /// Compute folding ranges for the given file content.
    ///
    /// Re-parses the source with `mago_syntax` (the raw AST is not cached)
    /// and walks every statement/expression to emit `FoldingRange` entries.
    pub fn handle_folding_range(&self, content: &str) -> Option<Vec<FoldingRange>> {
        let arena = LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"input.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());

        // Precompute line starts once. Each folding range converts two byte
        // offsets to positions, and a large file has many nested blocks, so
        // converting each offset by rescanning from the start would be O(n²).
        let idx = crate::util::LineIndex::new(content);

        let mut ranges: Vec<FoldingRange> = Vec::new();

        // ── AST walk ──
        for stmt in program.statements.iter() {
            collect_from_statement(stmt, &idx, &mut ranges);
        }

        // ── Trivia (comments) ──
        collect_comment_ranges(&program.trivia, &idx, &mut ranges);

        // Filter out single-line ranges and sort by start position.
        ranges.retain(|r| r.start_line < r.end_line);
        ranges.sort_by(|a, b| {
            a.start_line
                .cmp(&b.start_line)
                .then(a.end_line.cmp(&b.end_line))
        });

        Some(ranges)
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Build a `FoldingRange` from byte-offset start/end (of the opening/closing
/// delimiters).  `kind` is `None` for code regions.
fn range_from_offsets(
    idx: &crate::util::LineIndex<'_>,
    start_offset: u32,
    end_offset: u32,
    kind: Option<FoldingRangeKind>,
) -> FoldingRange {
    let start_pos = idx.position(start_offset as usize);
    let end_pos = idx.position(end_offset as usize);
    FoldingRange {
        start_line: start_pos.line,
        start_character: Some(start_pos.character),
        end_line: end_pos.line,
        end_character: Some(end_pos.character),
        kind,
        collapsed_text: None,
    }
}

/// Emit a folding range for a `Block` (left-brace … right-brace).
fn emit_block(block: &Block<'_>, idx: &crate::util::LineIndex<'_>, ranges: &mut Vec<FoldingRange>) {
    ranges.push(range_from_offsets(
        idx,
        block.left_brace.start.offset,
        block.right_brace.end.offset,
        None,
    ));
}

/// Emit a folding range from two `mago_span::Span` values representing the
/// opening and closing delimiters.
fn emit_brace_pair(
    left: mago_span::Span,
    right: mago_span::Span,
    idx: &crate::util::LineIndex<'_>,
    ranges: &mut Vec<FoldingRange>,
) {
    ranges.push(range_from_offsets(
        idx,
        left.start.offset,
        right.end.offset,
        None,
    ));
}

/// Emit a folding range for a parenthesised list (argument list, parameter
/// list) only when it spans multiple lines.
fn emit_paren_pair(
    left: mago_span::Span,
    right: mago_span::Span,
    idx: &crate::util::LineIndex<'_>,
    ranges: &mut Vec<FoldingRange>,
) {
    let start_pos = idx.position(left.start.offset as usize);
    let end_pos = idx.position(right.end.offset as usize);
    if start_pos.line < end_pos.line {
        ranges.push(FoldingRange {
            start_line: start_pos.line,
            start_character: Some(start_pos.character),
            end_line: end_pos.line,
            end_character: Some(end_pos.character),
            kind: None,
            collapsed_text: None,
        });
    }
}

// ─── Statement walker ───────────────────────────────────────────────────────

fn collect_from_statement(
    stmt: &Statement<'_>,
    idx: &crate::util::LineIndex<'_>,
    ranges: &mut Vec<FoldingRange>,
) {
    match stmt {
        Statement::Namespace(ns) => {
            // Brace-delimited namespace body.
            if let NamespaceBody::BraceDelimited(block) = &ns.body {
                emit_block(block, idx, ranges);
                for inner in block.statements.iter() {
                    collect_from_statement(inner, idx, ranges);
                }
            } else {
                for inner in ns.statements().iter() {
                    collect_from_statement(inner, idx, ranges);
                }
            }
        }

        Statement::Class(class) => {
            emit_brace_pair(class.left_brace, class.right_brace, idx, ranges);
            for member in class.members.iter() {
                collect_from_class_member(member, idx, ranges);
            }
        }

        Statement::Interface(iface) => {
            emit_brace_pair(iface.left_brace, iface.right_brace, idx, ranges);
            for member in iface.members.iter() {
                collect_from_class_member(member, idx, ranges);
            }
        }

        Statement::Trait(trait_def) => {
            emit_brace_pair(trait_def.left_brace, trait_def.right_brace, idx, ranges);
            for member in trait_def.members.iter() {
                collect_from_class_member(member, idx, ranges);
            }
        }

        Statement::Enum(enum_def) => {
            emit_brace_pair(enum_def.left_brace, enum_def.right_brace, idx, ranges);
            for member in enum_def.members.iter() {
                collect_from_class_member(member, idx, ranges);
            }
        }

        Statement::Function(func) => {
            emit_block(&func.body, idx, ranges);
            // Parameter list.
            emit_paren_pair(
                func.parameter_list.left_parenthesis,
                func.parameter_list.right_parenthesis,
                idx,
                ranges,
            );
            for inner in func.body.statements.iter() {
                collect_from_statement(inner, idx, ranges);
            }
        }

        Statement::If(if_stmt) => {
            collect_from_if(if_stmt, idx, ranges);
        }

        Statement::Switch(switch_stmt) => {
            collect_from_expression(switch_stmt.expression, idx, ranges);
            match &switch_stmt.body {
                SwitchBody::BraceDelimited(body) => {
                    emit_brace_pair(body.left_brace, body.right_brace, idx, ranges);
                    for case in body.cases.iter() {
                        for inner in case.statements().iter() {
                            collect_from_statement(inner, idx, ranges);
                        }
                    }
                }
                SwitchBody::ColonDelimited(body) => {
                    for case in body.cases.iter() {
                        for inner in case.statements().iter() {
                            collect_from_statement(inner, idx, ranges);
                        }
                    }
                }
            }
        }

        Statement::While(while_stmt) => {
            collect_from_expression(while_stmt.condition, idx, ranges);
            match &while_stmt.body {
                WhileBody::Statement(inner) => {
                    collect_from_block_statement(inner, idx, ranges);
                }
                WhileBody::ColonDelimited(_body) => {
                    // Colon-delimited while loops don't have braces.
                    for inner in while_stmt.body.statements().iter() {
                        collect_from_statement(inner, idx, ranges);
                    }
                }
            }
        }

        Statement::For(for_stmt) => {
            for expr in for_stmt.initializations.iter() {
                collect_from_expression(expr, idx, ranges);
            }
            for expr in for_stmt.conditions.iter() {
                collect_from_expression(expr, idx, ranges);
            }
            for expr in for_stmt.increments.iter() {
                collect_from_expression(expr, idx, ranges);
            }
            match &for_stmt.body {
                ForBody::Statement(inner) => {
                    collect_from_block_statement(inner, idx, ranges);
                }
                ForBody::ColonDelimited(_body) => {
                    for inner in for_stmt.body.statements().iter() {
                        collect_from_statement(inner, idx, ranges);
                    }
                }
            }
        }

        Statement::Foreach(foreach_stmt) => {
            collect_from_expression(foreach_stmt.expression, idx, ranges);
            match &foreach_stmt.body {
                ForeachBody::Statement(inner) => {
                    collect_from_block_statement(inner, idx, ranges);
                }
                ForeachBody::ColonDelimited(_body) => {
                    for inner in foreach_stmt.body.statements().iter() {
                        collect_from_statement(inner, idx, ranges);
                    }
                }
            }
        }

        Statement::DoWhile(do_while) => {
            collect_from_block_statement(do_while.statement, idx, ranges);
            collect_from_expression(do_while.condition, idx, ranges);
        }

        Statement::Try(try_stmt) => {
            emit_block(&try_stmt.block, idx, ranges);
            for inner in try_stmt.block.statements.iter() {
                collect_from_statement(inner, idx, ranges);
            }
            for catch in try_stmt.catch_clauses.iter() {
                emit_block(&catch.block, idx, ranges);
                for inner in catch.block.statements.iter() {
                    collect_from_statement(inner, idx, ranges);
                }
            }
            if let Some(ref finally) = try_stmt.finally_clause {
                emit_block(&finally.block, idx, ranges);
                for inner in finally.block.statements.iter() {
                    collect_from_statement(inner, idx, ranges);
                }
            }
        }

        Statement::Block(block) => {
            emit_block(block, idx, ranges);
            for inner in block.statements.iter() {
                collect_from_statement(inner, idx, ranges);
            }
        }

        Statement::Expression(expr_stmt) => {
            collect_from_expression(expr_stmt.expression, idx, ranges);
        }

        Statement::Return(ret) => {
            if let Some(val) = ret.value {
                collect_from_expression(val, idx, ranges);
            }
        }

        Statement::Echo(echo) => {
            for expr in echo.values.iter() {
                collect_from_expression(expr, idx, ranges);
            }
        }

        Statement::Declare(declare) => match &declare.body {
            DeclareBody::Statement(inner) => {
                collect_from_statement(inner, idx, ranges);
            }
            DeclareBody::ColonDelimited(body) => {
                for s in body.statements.iter() {
                    collect_from_statement(s, idx, ranges);
                }
            }
        },

        Statement::Constant(constant) => {
            for item in constant.items.iter() {
                collect_from_expression(item.value, idx, ranges);
            }
        }

        Statement::Unset(unset_stmt) => {
            for val in unset_stmt.values.iter() {
                collect_from_expression(val, idx, ranges);
            }
        }

        Statement::EchoTag(echo_tag) => {
            for expr in echo_tag.values.iter() {
                collect_from_expression(expr, idx, ranges);
            }
        }

        // Leaves or constructs that don't produce folding ranges.
        _ => {}
    }
}

/// If the statement is a `Block`, emit it and recurse; otherwise just recurse.
fn collect_from_block_statement(
    stmt: &Statement<'_>,
    idx: &crate::util::LineIndex<'_>,
    ranges: &mut Vec<FoldingRange>,
) {
    if let Statement::Block(block) = stmt {
        emit_block(block, idx, ranges);
        for inner in block.statements.iter() {
            collect_from_statement(inner, idx, ranges);
        }
    } else {
        collect_from_statement(stmt, idx, ranges);
    }
}

// ─── If statement ───────────────────────────────────────────────────────────

fn collect_from_if(
    if_stmt: &If<'_>,
    idx: &crate::util::LineIndex<'_>,
    ranges: &mut Vec<FoldingRange>,
) {
    collect_from_expression(if_stmt.condition, idx, ranges);
    match &if_stmt.body {
        IfBody::Statement(body) => {
            collect_from_block_statement(body.statement, idx, ranges);
            for else_if in body.else_if_clauses.iter() {
                collect_from_expression(else_if.condition, idx, ranges);
                collect_from_block_statement(else_if.statement, idx, ranges);
            }
            if let Some(ref else_clause) = body.else_clause {
                collect_from_block_statement(else_clause.statement, idx, ranges);
            }
        }
        IfBody::ColonDelimited(body) => {
            for inner in body.statements.iter() {
                collect_from_statement(inner, idx, ranges);
            }
            for else_if in body.else_if_clauses.iter() {
                collect_from_expression(else_if.condition, idx, ranges);
                for inner in else_if.statements.iter() {
                    collect_from_statement(inner, idx, ranges);
                }
            }
            if let Some(ref else_clause) = body.else_clause {
                for inner in else_clause.statements.iter() {
                    collect_from_statement(inner, idx, ranges);
                }
            }
        }
    }
}

// ─── Class member walker ────────────────────────────────────────────────────

fn collect_from_class_member(
    member: &class_like::member::ClassLikeMember<'_>,
    idx: &crate::util::LineIndex<'_>,
    ranges: &mut Vec<FoldingRange>,
) {
    match member {
        class_like::member::ClassLikeMember::Method(method) => {
            if let class_like::method::MethodBody::Concrete(block) = &method.body {
                emit_block(block, idx, ranges);
                for inner in block.statements.iter() {
                    collect_from_statement(inner, idx, ranges);
                }
            }
            // Parameter list.
            emit_paren_pair(
                method.parameter_list.left_parenthesis,
                method.parameter_list.right_parenthesis,
                idx,
                ranges,
            );
        }
        class_like::member::ClassLikeMember::TraitUse(trait_use) => {
            if let class_like::trait_use::TraitUseSpecification::Concrete(concrete) =
                &trait_use.specification
            {
                emit_brace_pair(concrete.left_brace, concrete.right_brace, idx, ranges);
            }
        }
        class_like::member::ClassLikeMember::Property(prop) => {
            // Property hooks (PHP 8.4) can have bodies.
            collect_from_property_hooks(prop, idx, ranges);
        }
        // Constants, enum cases — no sub-blocks to fold.
        _ => {}
    }
}

/// Collect folding ranges from property hooks if present.
fn collect_from_property_hooks(
    prop: &class_like::property::Property<'_>,
    idx: &crate::util::LineIndex<'_>,
    ranges: &mut Vec<FoldingRange>,
) {
    let hook_list = match prop {
        class_like::property::Property::Hooked(h) => &h.hook_list,
        class_like::property::Property::Plain(_) => return,
    };
    emit_brace_pair(hook_list.left_brace, hook_list.right_brace, idx, ranges);
    for hook in hook_list.hooks.iter() {
        if let class_like::property::PropertyHookBody::Concrete(concrete) = &hook.body
            && let class_like::property::PropertyHookConcreteBody::Block(block) = concrete
        {
            emit_block(block, idx, ranges);
            for inner in block.statements.iter() {
                collect_from_statement(inner, idx, ranges);
            }
        }
    }
}

// ─── Expression walker ──────────────────────────────────────────────────────

fn collect_from_expression(
    expr: &Expression<'_>,
    idx: &crate::util::LineIndex<'_>,
    ranges: &mut Vec<FoldingRange>,
) {
    match expr {
        Expression::Closure(closure) => {
            emit_block(&closure.body, idx, ranges);
            emit_paren_pair(
                closure.parameter_list.left_parenthesis,
                closure.parameter_list.right_parenthesis,
                idx,
                ranges,
            );
            for inner in closure.body.statements.iter() {
                collect_from_statement(inner, idx, ranges);
            }
        }

        Expression::ArrowFunction(arrow) => {
            // Arrow functions don't have braces, but the expression may
            // span multiple lines.
            let arrow_span = arrow.span();
            let start_pos = idx.position(arrow_span.start.offset as usize);
            let end_pos = idx.position(arrow_span.end.offset as usize);
            if start_pos.line < end_pos.line {
                ranges.push(FoldingRange {
                    start_line: start_pos.line,
                    start_character: Some(start_pos.character),
                    end_line: end_pos.line,
                    end_character: Some(end_pos.character),
                    kind: None,
                    collapsed_text: None,
                });
            }
            collect_from_expression(arrow.expression, idx, ranges);
        }

        Expression::Array(array) => {
            emit_brace_pair(array.left_bracket, array.right_bracket, idx, ranges);
            for elem in array.elements.iter() {
                collect_from_array_element(elem, idx, ranges);
            }
        }

        Expression::LegacyArray(array) => {
            emit_paren_pair(array.left_parenthesis, array.right_parenthesis, idx, ranges);
            for elem in array.elements.iter() {
                collect_from_array_element(elem, idx, ranges);
            }
        }

        Expression::Match(match_expr) => {
            emit_brace_pair(match_expr.left_brace, match_expr.right_brace, idx, ranges);
            collect_from_expression(match_expr.expression, idx, ranges);
            for arm in match_expr.arms.iter() {
                match arm {
                    MatchArm::Expression(arm) => {
                        for cond in arm.conditions.iter() {
                            collect_from_expression(cond, idx, ranges);
                        }
                        collect_from_expression(arm.expression, idx, ranges);
                    }
                    MatchArm::Default(arm) => {
                        collect_from_expression(arm.expression, idx, ranges);
                    }
                }
            }
        }

        Expression::Call(call) => {
            collect_from_call(call, idx, ranges);
        }

        Expression::Instantiation(inst) => {
            collect_from_expression(inst.class, idx, ranges);
            if let Some(ref arg_list) = inst.argument_list {
                emit_paren_pair(
                    arg_list.left_parenthesis,
                    arg_list.right_parenthesis,
                    idx,
                    ranges,
                );
                for arg in arg_list.arguments.iter() {
                    collect_from_expression(arg.value(), idx, ranges);
                }
            }
        }

        Expression::AnonymousClass(anon) => {
            emit_brace_pair(anon.left_brace, anon.right_brace, idx, ranges);
            if let Some(ref arg_list) = anon.argument_list {
                emit_paren_pair(
                    arg_list.left_parenthesis,
                    arg_list.right_parenthesis,
                    idx,
                    ranges,
                );
                for arg in arg_list.arguments.iter() {
                    if let Some(value) = arg.value() {
                        collect_from_expression(value, idx, ranges);
                    }
                }
            }
            for member in anon.members.iter() {
                collect_from_class_member(member, idx, ranges);
            }
        }

        Expression::Assignment(assign) => {
            collect_from_expression(assign.lhs, idx, ranges);
            collect_from_expression(assign.rhs, idx, ranges);
        }

        Expression::Binary(bin) => {
            collect_from_expression(bin.lhs, idx, ranges);
            collect_from_expression(bin.rhs, idx, ranges);
        }

        Expression::UnaryPrefix(u) => {
            collect_from_expression(u.operand, idx, ranges);
        }

        Expression::UnaryPostfix(u) => {
            collect_from_expression(u.operand, idx, ranges);
        }

        Expression::Parenthesized(p) => {
            collect_from_expression(p.expression, idx, ranges);
        }

        Expression::Conditional(cond) => {
            collect_from_expression(cond.condition, idx, ranges);
            if let Some(then_expr) = cond.then {
                collect_from_expression(then_expr, idx, ranges);
            }
            collect_from_expression(cond.r#else, idx, ranges);
        }

        Expression::ArrayAccess(access) => {
            collect_from_expression(access.array, idx, ranges);
            collect_from_expression(access.index, idx, ranges);
        }

        Expression::Access(access) => match access {
            Access::Property(pa) => collect_from_expression(pa.object, idx, ranges),
            Access::NullSafeProperty(pa) => collect_from_expression(pa.object, idx, ranges),
            Access::StaticProperty(spa) => collect_from_expression(spa.class, idx, ranges),
            Access::ClassConstant(cca) => collect_from_expression(cca.class, idx, ranges),
        },

        Expression::Throw(throw_expr) => {
            collect_from_expression(throw_expr.exception, idx, ranges);
        }

        Expression::Clone(clone) => {
            collect_from_expression(clone.object, idx, ranges);
        }

        Expression::Yield(yield_expr) => match yield_expr {
            Yield::Value(yv) => {
                if let Some(value) = yv.value {
                    collect_from_expression(value, idx, ranges);
                }
            }
            Yield::Pair(yp) => {
                collect_from_expression(yp.key, idx, ranges);
                collect_from_expression(yp.value, idx, ranges);
            }
            Yield::From(yf) => {
                collect_from_expression(yf.iterator, idx, ranges);
            }
        },

        Expression::List(list) => {
            for elem in list.elements.iter() {
                collect_from_array_element(elem, idx, ranges);
            }
        }

        Expression::Construct(construct) => {
            collect_from_construct(construct, idx, ranges);
        }

        Expression::Pipe(pipe) => {
            collect_from_expression(pipe.input, idx, ranges);
            collect_from_expression(pipe.callable, idx, ranges);
        }

        Expression::CompositeString(cs) => {
            for part in cs.parts() {
                match part {
                    StringPart::Expression(expr) => {
                        collect_from_expression(expr, idx, ranges);
                    }
                    StringPart::BracedExpression(braced) => {
                        collect_from_expression(braced.expression, idx, ranges);
                    }
                    StringPart::Literal(_) => {}
                }
            }
        }

        // Leaves: literals, variables, identifiers, magic constants, etc.
        _ => {}
    }
}

/// Process call expressions and emit folding ranges for multi-line argument
/// lists.
fn collect_from_call(
    call: &Call<'_>,
    idx: &crate::util::LineIndex<'_>,
    ranges: &mut Vec<FoldingRange>,
) {
    match call {
        Call::Function(fc) => {
            collect_from_expression(fc.function, idx, ranges);
            emit_paren_pair(
                fc.argument_list.left_parenthesis,
                fc.argument_list.right_parenthesis,
                idx,
                ranges,
            );
            for arg in fc.argument_list.arguments.iter() {
                collect_from_expression(arg.value(), idx, ranges);
            }
        }
        Call::Method(mc) => {
            collect_from_expression(mc.object, idx, ranges);
            emit_paren_pair(
                mc.argument_list.left_parenthesis,
                mc.argument_list.right_parenthesis,
                idx,
                ranges,
            );
            for arg in mc.argument_list.arguments.iter() {
                collect_from_expression(arg.value(), idx, ranges);
            }
        }
        Call::NullSafeMethod(mc) => {
            collect_from_expression(mc.object, idx, ranges);
            emit_paren_pair(
                mc.argument_list.left_parenthesis,
                mc.argument_list.right_parenthesis,
                idx,
                ranges,
            );
            for arg in mc.argument_list.arguments.iter() {
                collect_from_expression(arg.value(), idx, ranges);
            }
        }
        Call::StaticMethod(mc) => {
            collect_from_expression(mc.class, idx, ranges);
            emit_paren_pair(
                mc.argument_list.left_parenthesis,
                mc.argument_list.right_parenthesis,
                idx,
                ranges,
            );
            for arg in mc.argument_list.arguments.iter() {
                collect_from_expression(arg.value(), idx, ranges);
            }
        }
    }
}

fn collect_from_array_element(
    elem: &array::ArrayElement<'_>,
    idx: &crate::util::LineIndex<'_>,
    ranges: &mut Vec<FoldingRange>,
) {
    match elem {
        array::ArrayElement::KeyValue(kv) => {
            collect_from_expression(kv.key, idx, ranges);
            collect_from_expression(kv.value, idx, ranges);
        }
        array::ArrayElement::Value(v) => {
            collect_from_expression(v.value, idx, ranges);
        }
        array::ArrayElement::Variadic(v) => {
            collect_from_expression(v.value, idx, ranges);
        }
        array::ArrayElement::Missing(_) => {}
    }
}

fn collect_from_construct(
    construct: &construct::Construct<'_>,
    idx: &crate::util::LineIndex<'_>,
    ranges: &mut Vec<FoldingRange>,
) {
    match construct {
        construct::Construct::Print(print) => {
            collect_from_expression(print.value, idx, ranges);
        }
        construct::Construct::Exit(exit) => {
            if let Some(ref args) = exit.arguments {
                for arg in args.arguments.iter() {
                    collect_from_expression(arg.value(), idx, ranges);
                }
            }
        }
        construct::Construct::Die(die) => {
            if let Some(ref args) = die.arguments {
                for arg in args.arguments.iter() {
                    collect_from_expression(arg.value(), idx, ranges);
                }
            }
        }
        construct::Construct::Isset(isset) => {
            for val in isset.values.iter() {
                collect_from_expression(val, idx, ranges);
            }
        }
        construct::Construct::Empty(empty) => {
            collect_from_expression(empty.value, idx, ranges);
        }
        construct::Construct::Eval(eval) => {
            collect_from_expression(eval.value, idx, ranges);
        }
        construct::Construct::Include(include) => {
            collect_from_expression(include.value, idx, ranges);
        }
        construct::Construct::IncludeOnce(include) => {
            collect_from_expression(include.value, idx, ranges);
        }
        construct::Construct::Require(require) => {
            collect_from_expression(require.value, idx, ranges);
        }
        construct::Construct::RequireOnce(require) => {
            collect_from_expression(require.value, idx, ranges);
        }
    }
}

// ─── Comment folding ────────────────────────────────────────────────────────

/// Scan trivia for doc-block comments and groups of consecutive single-line
/// comments, emitting `FoldingRange` entries with `FoldingRangeKind::Comment`.
fn collect_comment_ranges(
    trivia: &sequence::Sequence<'_, Trivia<'_>>,
    idx: &crate::util::LineIndex<'_>,
    ranges: &mut Vec<FoldingRange>,
) {
    // ── Doc-block and multi-line comments ──
    for t in trivia.iter() {
        if matches!(
            t.kind,
            TriviaKind::DocBlockComment | TriviaKind::MultiLineComment
        ) {
            let start_pos = idx.position(t.span.start.offset as usize);
            let end_pos = idx.position(t.span.end.offset as usize);
            if start_pos.line < end_pos.line {
                ranges.push(FoldingRange {
                    start_line: start_pos.line,
                    start_character: Some(start_pos.character),
                    end_line: end_pos.line,
                    end_character: Some(end_pos.character),
                    kind: Some(FoldingRangeKind::Comment),
                    collapsed_text: None,
                });
            }
        }
    }

    // ── Consecutive single-line comments (`//` or `#`) ──
    // Group adjacent single-line comment trivia whose lines differ by
    // exactly 1 (allowing interleaved whitespace trivia).
    let single_line_comments: Vec<&Trivia<'_>> = trivia
        .iter()
        .filter(|t| t.kind.is_single_line_comment())
        .collect();

    if single_line_comments.is_empty() {
        return;
    }

    let mut group_start_offset = single_line_comments[0].span.start.offset;
    let mut group_end_offset = single_line_comments[0].span.end.offset;
    let mut group_start_line = idx.position(group_start_offset as usize).line;
    let mut group_end_line = idx.position(group_end_offset as usize).line;

    for t in single_line_comments.iter().skip(1) {
        let cur_line = idx.position(t.span.start.offset as usize).line;
        if cur_line == group_end_line + 1 {
            // Extend the current group.
            group_end_offset = t.span.end.offset;
            group_end_line = idx.position(group_end_offset as usize).line;
        } else {
            // Flush the previous group if it spans multiple lines.
            if group_start_line < group_end_line {
                let start_char = idx.position(group_start_offset as usize).character;
                let end_char = idx.position(group_end_offset as usize).character;
                ranges.push(FoldingRange {
                    start_line: group_start_line,
                    start_character: Some(start_char),
                    end_line: group_end_line,
                    end_character: Some(end_char),
                    kind: Some(FoldingRangeKind::Comment),
                    collapsed_text: None,
                });
            }
            // Start a new group.
            group_start_offset = t.span.start.offset;
            group_end_offset = t.span.end.offset;
            group_start_line = cur_line;
            group_end_line = idx.position(group_end_offset as usize).line;
        }
    }

    // Flush the last group.
    if group_start_line < group_end_line {
        let start_char = idx.position(group_start_offset as usize).character;
        let end_char = idx.position(group_end_offset as usize).character;
        ranges.push(FoldingRange {
            start_line: group_start_line,
            start_character: Some(start_char),
            end_line: group_end_line,
            end_character: Some(end_char),
            kind: Some(FoldingRangeKind::Comment),
            collapsed_text: None,
        });
    }
}
