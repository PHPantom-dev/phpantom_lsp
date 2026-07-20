//! Selection range handler for `textDocument/selectionRange`.
//!
//! "Smart select" / expand selection.  Given a cursor position, returns a
//! nested chain of ranges from innermost to outermost (e.g. identifier →
//! expression → statement → block → function → class → file).  AST-aware
//! selection ranges produce much tighter expansions than word/line/block.
//!
//! The implementation parses the file with `mago_syntax`, walks the AST
//! to collect all nodes whose span contains the cursor position, sorts
//! them from outermost to innermost, and builds the linked `SelectionRange`
//! list that the LSP protocol expects.

use mago_allocator::LocalArena;
use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp::lsp_types::{Position, Range, SelectionRange};

use crate::Backend;
use crate::util::{offset_to_position, position_to_offset};

// ─── Public entry point ─────────────────────────────────────────────────────

impl Backend {
    /// Compute selection ranges for the given positions in the file.
    pub fn handle_selection_range(
        &self,
        content: &str,
        positions: &[Position],
    ) -> Option<Vec<SelectionRange>> {
        let arena = LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"input.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());

        let mut results = Vec::with_capacity(positions.len());

        for pos in positions {
            let offset = position_to_offset(content, *pos);

            // Collect all spans that contain the cursor, from the AST walk.
            let mut spans: Vec<(u32, u32)> = Vec::new();

            // Add the whole-file span as the outermost range.
            let file_span = (0u32, content.len() as u32);
            spans.push(file_span);

            for stmt in program.statements.iter() {
                collect_spans_from_statement(stmt, offset, &mut spans);
            }

            // Deduplicate identical spans and sort outermost-first (largest
            // span first).  When two spans have the same length, the one
            // starting earlier comes first.
            spans.sort_unstable();
            spans.dedup();
            spans.sort_by(|a, b| {
                let len_a = a.1.saturating_sub(a.0);
                let len_b = b.1.saturating_sub(b.0);
                len_b.cmp(&len_a).then(a.0.cmp(&b.0))
            });

            // Build the linked list from outermost to innermost.
            let selection_range = build_selection_range(content, &spans);
            results.push(selection_range);
        }

        Some(results)
    }
}

// ─── Linked-list builder ────────────────────────────────────────────────────

/// Build a `SelectionRange` linked list from a list of spans sorted
/// outermost-first.
fn build_selection_range(content: &str, spans: &[(u32, u32)]) -> SelectionRange {
    if spans.is_empty() {
        let range = Range::new(Position::new(0, 0), Position::new(0, 0));
        return SelectionRange {
            range,
            parent: None,
        };
    }

    // Start from the outermost and wrap inward.
    let mut current = to_selection_range(content, spans[0], None);

    for &span in &spans[1..] {
        current = to_selection_range(content, span, Some(current));
    }

    current
}

fn to_selection_range(
    content: &str,
    span: (u32, u32),
    parent: Option<SelectionRange>,
) -> SelectionRange {
    let start = offset_to_position(content, span.0 as usize);
    let end = offset_to_position(content, span.1 as usize);
    SelectionRange {
        range: Range::new(start, end),
        parent: parent.map(Box::new),
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// If the span contains the cursor offset, push it and return `true`.
fn push_if_contains(span: mago_span::Span, offset: u32, spans: &mut Vec<(u32, u32)>) -> bool {
    let start = span.start.offset;
    let end = span.end.offset;
    if start <= offset && offset <= end {
        spans.push((start, end));
        true
    } else {
        false
    }
}

/// Push a brace-delimited range (left_brace..right_brace) if it contains the cursor.
fn push_brace_pair(
    left: mago_span::Span,
    right: mago_span::Span,
    offset: u32,
    spans: &mut Vec<(u32, u32)>,
) {
    let start = left.start.offset;
    let end = right.end.offset;
    if start <= offset && offset <= end {
        spans.push((start, end));
    }
}

/// Push a block's span if it contains the cursor, and recurse into its statements.
fn push_block(block: &Block<'_>, offset: u32, spans: &mut Vec<(u32, u32)>) {
    push_brace_pair(block.left_brace, block.right_brace, offset, spans);
    for stmt in block.statements.iter() {
        collect_spans_from_statement(stmt, offset, spans);
    }
}

// ─── Statement walker ───────────────────────────────────────────────────────

fn collect_spans_from_statement(stmt: &Statement<'_>, offset: u32, spans: &mut Vec<(u32, u32)>) {
    let stmt_span = stmt.span();
    if !push_if_contains(stmt_span, offset, spans) {
        return;
    }

    match stmt {
        Statement::Namespace(ns) => match &ns.body {
            NamespaceBody::BraceDelimited(block) => {
                push_block(block, offset, spans);
            }
            NamespaceBody::Implicit(body) => {
                for inner in body.statements.iter() {
                    collect_spans_from_statement(inner, offset, spans);
                }
            }
        },

        Statement::Class(class) => {
            push_brace_pair(class.left_brace, class.right_brace, offset, spans);
            for member in class.members.iter() {
                collect_spans_from_class_member(member, offset, spans);
            }
        }

        Statement::Interface(iface) => {
            push_brace_pair(iface.left_brace, iface.right_brace, offset, spans);
            for member in iface.members.iter() {
                collect_spans_from_class_member(member, offset, spans);
            }
        }

        Statement::Trait(trait_def) => {
            push_brace_pair(trait_def.left_brace, trait_def.right_brace, offset, spans);
            for member in trait_def.members.iter() {
                collect_spans_from_class_member(member, offset, spans);
            }
        }

        Statement::Enum(enum_def) => {
            push_brace_pair(enum_def.left_brace, enum_def.right_brace, offset, spans);
            for member in enum_def.members.iter() {
                collect_spans_from_class_member(member, offset, spans);
            }
        }

        Statement::Function(func) => {
            push_block(&func.body, offset, spans);
            push_paren_pair(
                func.parameter_list.left_parenthesis,
                func.parameter_list.right_parenthesis,
                offset,
                spans,
            );
            for param in func.parameter_list.parameters.iter() {
                collect_spans_from_parameter(param, offset, spans);
            }
            for inner in func.body.statements.iter() {
                collect_spans_from_statement(inner, offset, spans);
            }
        }

        Statement::If(if_stmt) => {
            collect_spans_from_if(if_stmt, offset, spans);
        }

        Statement::Switch(switch_stmt) => {
            collect_spans_from_expression(switch_stmt.expression, offset, spans);
            match &switch_stmt.body {
                SwitchBody::BraceDelimited(body) => {
                    push_brace_pair(body.left_brace, body.right_brace, offset, spans);
                    for case in body.cases.iter() {
                        let case_span = case.span();
                        if push_if_contains(case_span, offset, spans) {
                            for inner in case.statements().iter() {
                                collect_spans_from_statement(inner, offset, spans);
                            }
                        }
                    }
                }
                SwitchBody::ColonDelimited(body) => {
                    for case in body.cases.iter() {
                        let case_span = case.span();
                        if push_if_contains(case_span, offset, spans) {
                            for inner in case.statements().iter() {
                                collect_spans_from_statement(inner, offset, spans);
                            }
                        }
                    }
                }
            }
        }

        Statement::Foreach(foreach) => {
            collect_spans_from_expression(foreach.expression, offset, spans);
            let target_span = foreach.target.span();
            let _ = push_if_contains(target_span, offset, spans);
            match &foreach.target {
                r#loop::foreach::ForeachTarget::Value(val) => {
                    collect_spans_from_expression(val.value, offset, spans);
                }
                r#loop::foreach::ForeachTarget::KeyValue(kv) => {
                    collect_spans_from_expression(kv.key, offset, spans);
                    collect_spans_from_expression(kv.value, offset, spans);
                }
            }
            match &foreach.body {
                ForeachBody::Statement(body) => {
                    collect_spans_from_statement(body, offset, spans);
                }
                ForeachBody::ColonDelimited(body) => {
                    for inner in body.statements.iter() {
                        collect_spans_from_statement(inner, offset, spans);
                    }
                }
            }
        }

        Statement::For(for_stmt) => {
            for expr in for_stmt.initializations.iter() {
                collect_spans_from_expression(expr, offset, spans);
            }
            for expr in for_stmt.conditions.iter() {
                collect_spans_from_expression(expr, offset, spans);
            }
            for expr in for_stmt.increments.iter() {
                collect_spans_from_expression(expr, offset, spans);
            }
            match &for_stmt.body {
                ForBody::Statement(body) => {
                    collect_spans_from_statement(body, offset, spans);
                }
                ForBody::ColonDelimited(body) => {
                    for inner in body.statements.iter() {
                        collect_spans_from_statement(inner, offset, spans);
                    }
                }
            }
        }

        Statement::While(while_stmt) => {
            collect_spans_from_expression(while_stmt.condition, offset, spans);
            match &while_stmt.body {
                WhileBody::Statement(body) => {
                    collect_spans_from_statement(body, offset, spans);
                }
                WhileBody::ColonDelimited(body) => {
                    for inner in body.statements.iter() {
                        collect_spans_from_statement(inner, offset, spans);
                    }
                }
            }
        }

        Statement::DoWhile(do_while) => {
            collect_spans_from_expression(do_while.condition, offset, spans);
            collect_spans_from_statement(do_while.statement, offset, spans);
        }

        Statement::Try(try_stmt) => {
            push_block(&try_stmt.block, offset, spans);
            for inner in try_stmt.block.statements.iter() {
                collect_spans_from_statement(inner, offset, spans);
            }
            for catch in try_stmt.catch_clauses.iter() {
                let catch_span = catch.span();
                if push_if_contains(catch_span, offset, spans) {
                    push_block(&catch.block, offset, spans);
                    for inner in catch.block.statements.iter() {
                        collect_spans_from_statement(inner, offset, spans);
                    }
                }
            }
            if let Some(ref finally) = try_stmt.finally_clause {
                let finally_span = finally.span();
                if push_if_contains(finally_span, offset, spans) {
                    push_block(&finally.block, offset, spans);
                    for inner in finally.block.statements.iter() {
                        collect_spans_from_statement(inner, offset, spans);
                    }
                }
            }
        }

        Statement::Return(ret) => {
            if let Some(value) = ret.value {
                collect_spans_from_expression(value, offset, spans);
            }
        }

        Statement::Expression(expr_stmt) => {
            collect_spans_from_expression(expr_stmt.expression, offset, spans);
        }

        Statement::Echo(echo) => {
            for expr in echo.values.iter() {
                collect_spans_from_expression(expr, offset, spans);
            }
        }

        Statement::Unset(unset) => {
            for expr in unset.values.iter() {
                collect_spans_from_expression(expr, offset, spans);
            }
        }

        Statement::Block(block) => {
            push_block(block, offset, spans);
        }

        Statement::Declare(declare) => match &declare.body {
            DeclareBody::Statement(body) => {
                collect_spans_from_statement(body, offset, spans);
            }
            DeclareBody::ColonDelimited(body) => {
                for inner in body.statements.iter() {
                    collect_spans_from_statement(inner, offset, spans);
                }
            }
        },

        Statement::Global(_)
        | Statement::Static(_)
        | Statement::Use(_)
        | Statement::Constant(_)
        | Statement::Goto(_)
        | Statement::Label(_)
        | Statement::Continue(_)
        | Statement::Break(_)
        | Statement::OpeningTag(_)
        | Statement::ClosingTag(_)
        | Statement::Inline(_)
        | Statement::EchoTag(_)
        | Statement::HaltCompiler(_)
        | Statement::Noop(_) => {}

        // Non-exhaustive: future variants get the statement-level span only.
        _ => {}
    }
}

// ─── Class member walker ────────────────────────────────────────────────────

fn collect_spans_from_class_member(
    member: &class_like::member::ClassLikeMember<'_>,
    offset: u32,
    spans: &mut Vec<(u32, u32)>,
) {
    use class_like::member::ClassLikeMember;

    let member_span = member.span();
    if !push_if_contains(member_span, offset, spans) {
        return;
    }

    match member {
        ClassLikeMember::Method(method) => {
            // Parameter list.
            push_paren_pair(
                method.parameter_list.left_parenthesis,
                method.parameter_list.right_parenthesis,
                offset,
                spans,
            );
            for param in method.parameter_list.parameters.iter() {
                collect_spans_from_parameter(param, offset, spans);
            }

            use class_like::method::MethodBody;
            match &method.body {
                MethodBody::Concrete(block) => {
                    push_block(block, offset, spans);
                    for inner in block.statements.iter() {
                        collect_spans_from_statement(inner, offset, spans);
                    }
                }
                MethodBody::Abstract(_) => {}
            }
        }

        ClassLikeMember::Property(prop) => {
            use class_like::property::{Property, PropertyItem};
            match prop {
                Property::Plain(plain) => {
                    for item in plain.items.iter() {
                        match item {
                            PropertyItem::Abstract(abs) => {
                                let _ = push_if_contains(abs.variable.span(), offset, spans);
                            }
                            PropertyItem::Concrete(concrete) => {
                                let item_span = concrete.span();
                                if push_if_contains(item_span, offset, spans) {
                                    let _ =
                                        push_if_contains(concrete.variable.span(), offset, spans);
                                    collect_spans_from_expression(concrete.value, offset, spans);
                                }
                            }
                        }
                    }
                }
                Property::Hooked(hooked) => match &hooked.item {
                    PropertyItem::Abstract(abs) => {
                        let _ = push_if_contains(abs.variable.span(), offset, spans);
                    }
                    PropertyItem::Concrete(concrete) => {
                        let item_span = concrete.span();
                        if push_if_contains(item_span, offset, spans) {
                            let _ = push_if_contains(concrete.variable.span(), offset, spans);
                            collect_spans_from_expression(concrete.value, offset, spans);
                        }
                    }
                },
            }
        }

        ClassLikeMember::Constant(constant) => {
            for item in constant.items.iter() {
                let item_span = item.span();
                if push_if_contains(item_span, offset, spans) {
                    collect_spans_from_expression(item.value, offset, spans);
                }
            }
        }

        ClassLikeMember::EnumCase(enum_case) => {
            use class_like::enum_case::EnumCaseItem;
            match &enum_case.item {
                EnumCaseItem::Unit(unit) => {
                    let _ = push_if_contains(unit.span(), offset, spans);
                }
                EnumCaseItem::Backed(backed) => {
                    let item_span = backed.span();
                    if push_if_contains(item_span, offset, spans) {
                        collect_spans_from_expression(backed.value, offset, spans);
                    }
                }
            }
        }

        ClassLikeMember::TraitUse(_) => {
            // Trait use statements are simple; the member span is enough.
        }
    }
}

// ─── Parameter walker ───────────────────────────────────────────────────────

fn collect_spans_from_parameter(
    param: &function_like::parameter::FunctionLikeParameter<'_>,
    offset: u32,
    spans: &mut Vec<(u32, u32)>,
) {
    let param_span = param.span();
    if push_if_contains(param_span, offset, spans) {
        let _ = push_if_contains(param.variable.span(), offset, spans);
        if let Some(ref default) = param.default_value {
            collect_spans_from_expression(default.value, offset, spans);
        }
    }
}

// ─── If walker ──────────────────────────────────────────────────────────────

fn collect_spans_from_if(
    if_stmt: &control_flow::r#if::If<'_>,
    offset: u32,
    spans: &mut Vec<(u32, u32)>,
) {
    collect_spans_from_expression(if_stmt.condition, offset, spans);

    match &if_stmt.body {
        IfBody::Statement(body) => {
            collect_spans_from_statement(body.statement, offset, spans);
            for elseif in body.else_if_clauses.iter() {
                let elseif_span: mago_span::Span = elseif.span();
                if push_if_contains(elseif_span, offset, spans) {
                    collect_spans_from_expression(elseif.condition, offset, spans);
                    collect_spans_from_statement(elseif.statement, offset, spans);
                }
            }
            if let Some(ref else_clause) = body.else_clause {
                let else_span: mago_span::Span = else_clause.span();
                if push_if_contains(else_span, offset, spans) {
                    collect_spans_from_statement(else_clause.statement, offset, spans);
                }
            }
        }
        IfBody::ColonDelimited(body) => {
            for inner in body.statements.iter() {
                collect_spans_from_statement(inner, offset, spans);
            }
            for elseif in body.else_if_clauses.iter() {
                let elseif_span: mago_span::Span = elseif.span();
                if push_if_contains(elseif_span, offset, spans) {
                    collect_spans_from_expression(elseif.condition, offset, spans);
                    for inner in elseif.statements.iter() {
                        collect_spans_from_statement(inner, offset, spans);
                    }
                }
            }
            if let Some(ref else_clause) = body.else_clause {
                let else_span: mago_span::Span = else_clause.span();
                if push_if_contains(else_span, offset, spans) {
                    for inner in else_clause.statements.iter() {
                        collect_spans_from_statement(inner, offset, spans);
                    }
                }
            }
        }
    }
}

// ─── Expression walker ──────────────────────────────────────────────────────

fn collect_spans_from_expression(expr: &Expression<'_>, offset: u32, spans: &mut Vec<(u32, u32)>) {
    let expr_span = expr.span();
    if !push_if_contains(expr_span, offset, spans) {
        return;
    }

    match expr {
        Expression::Binary(bin) => {
            collect_spans_from_expression(bin.lhs, offset, spans);
            collect_spans_from_expression(bin.rhs, offset, spans);
        }

        Expression::UnaryPrefix(unary) => {
            collect_spans_from_expression(unary.operand, offset, spans);
        }

        Expression::UnaryPostfix(unary) => {
            collect_spans_from_expression(unary.operand, offset, spans);
        }

        Expression::Parenthesized(paren) => {
            collect_spans_from_expression(paren.expression, offset, spans);
        }

        Expression::Assignment(assign) => {
            collect_spans_from_expression(assign.lhs, offset, spans);
            collect_spans_from_expression(assign.rhs, offset, spans);
        }

        Expression::Conditional(cond) => {
            collect_spans_from_expression(cond.condition, offset, spans);
            if let Some(then_expr) = cond.then {
                collect_spans_from_expression(then_expr, offset, spans);
            }
            collect_spans_from_expression(cond.r#else, offset, spans);
        }

        Expression::Call(call) => {
            match call {
                Call::Function(func_call) => {
                    collect_spans_from_expression(func_call.function, offset, spans);
                    collect_spans_from_argument_list(&func_call.argument_list, offset, spans);
                }
                Call::Method(method_call) => {
                    collect_spans_from_expression(method_call.object, offset, spans);
                    // method selector
                    let sel_span = method_call.method.span();
                    let _ = push_if_contains(sel_span, offset, spans);
                    collect_spans_from_argument_list(&method_call.argument_list, offset, spans);
                }
                Call::NullSafeMethod(method_call) => {
                    collect_spans_from_expression(method_call.object, offset, spans);
                    let sel_span = method_call.method.span();
                    let _ = push_if_contains(sel_span, offset, spans);
                    collect_spans_from_argument_list(&method_call.argument_list, offset, spans);
                }
                Call::StaticMethod(static_call) => {
                    collect_spans_from_expression(static_call.class, offset, spans);
                    let sel_span = static_call.method.span();
                    let _ = push_if_contains(sel_span, offset, spans);
                    collect_spans_from_argument_list(&static_call.argument_list, offset, spans);
                }
            }
        }

        Expression::Access(access) => match access {
            Access::Property(prop) => {
                collect_spans_from_expression(prop.object, offset, spans);
                let sel_span = prop.property.span();
                let _ = push_if_contains(sel_span, offset, spans);
            }
            Access::NullSafeProperty(prop) => {
                collect_spans_from_expression(prop.object, offset, spans);
                let sel_span = prop.property.span();
                let _ = push_if_contains(sel_span, offset, spans);
            }
            Access::StaticProperty(prop) => {
                collect_spans_from_expression(prop.class, offset, spans);
                let var_span = prop.property.span();
                let _ = push_if_contains(var_span, offset, spans);
            }
            Access::ClassConstant(cc) => {
                collect_spans_from_expression(cc.class, offset, spans);
                let sel_span = cc.constant.span();
                let _ = push_if_contains(sel_span, offset, spans);
            }
        },

        Expression::Instantiation(inst) => {
            collect_spans_from_expression(inst.class, offset, spans);
            if let Some(ref args) = inst.argument_list {
                collect_spans_from_argument_list(args, offset, spans);
            }
        }

        Expression::Array(array) => {
            for element in array.elements.iter() {
                let el_span = element.span();
                if push_if_contains(el_span, offset, spans) {
                    match element {
                        ArrayElement::KeyValue(kv) => {
                            collect_spans_from_expression(kv.key, offset, spans);
                            collect_spans_from_expression(kv.value, offset, spans);
                        }
                        ArrayElement::Value(val) => {
                            collect_spans_from_expression(val.value, offset, spans);
                        }
                        ArrayElement::Variadic(var) => {
                            collect_spans_from_expression(var.value, offset, spans);
                        }
                        ArrayElement::Missing(_) => {}
                    }
                }
            }
        }

        Expression::LegacyArray(array) => {
            for element in array.elements.iter() {
                let el_span = element.span();
                if push_if_contains(el_span, offset, spans) {
                    match element {
                        ArrayElement::KeyValue(kv) => {
                            collect_spans_from_expression(kv.key, offset, spans);
                            collect_spans_from_expression(kv.value, offset, spans);
                        }
                        ArrayElement::Value(val) => {
                            collect_spans_from_expression(val.value, offset, spans);
                        }
                        ArrayElement::Variadic(var) => {
                            collect_spans_from_expression(var.value, offset, spans);
                        }
                        ArrayElement::Missing(_) => {}
                    }
                }
            }
        }

        Expression::List(list) => {
            for element in list.elements.iter() {
                let el_span = element.span();
                let _ = push_if_contains(el_span, offset, spans);
            }
        }

        Expression::ArrayAccess(access) => {
            collect_spans_from_expression(access.array, offset, spans);
            collect_spans_from_expression(access.index, offset, spans);
        }

        Expression::ArrayAppend(append) => {
            collect_spans_from_expression(append.array, offset, spans);
        }

        Expression::Closure(closure) => {
            push_paren_pair(
                closure.parameter_list.left_parenthesis,
                closure.parameter_list.right_parenthesis,
                offset,
                spans,
            );
            for param in closure.parameter_list.parameters.iter() {
                collect_spans_from_parameter(param, offset, spans);
            }
            push_block(&closure.body, offset, spans);
            for inner in closure.body.statements.iter() {
                collect_spans_from_statement(inner, offset, spans);
            }
        }

        Expression::ArrowFunction(arrow) => {
            push_paren_pair(
                arrow.parameter_list.left_parenthesis,
                arrow.parameter_list.right_parenthesis,
                offset,
                spans,
            );
            for param in arrow.parameter_list.parameters.iter() {
                collect_spans_from_parameter(param, offset, spans);
            }
            collect_spans_from_expression(arrow.expression, offset, spans);
        }

        Expression::AnonymousClass(anon) => {
            push_brace_pair(anon.left_brace, anon.right_brace, offset, spans);
            for member in anon.members.iter() {
                collect_spans_from_class_member(member, offset, spans);
            }
        }

        Expression::Match(match_expr) => {
            collect_spans_from_expression(match_expr.expression, offset, spans);
            push_brace_pair(match_expr.left_brace, match_expr.right_brace, offset, spans);
            for arm in match_expr.arms.iter() {
                let arm_span = arm.span();
                if push_if_contains(arm_span, offset, spans) {
                    collect_spans_from_expression(arm.expression(), offset, spans);
                }
            }
        }

        Expression::Yield(yield_expr) => match yield_expr {
            Yield::Value(yv) => {
                if let Some(value) = yv.value {
                    collect_spans_from_expression(value, offset, spans);
                }
            }
            Yield::Pair(yp) => {
                collect_spans_from_expression(yp.key, offset, spans);
                collect_spans_from_expression(yp.value, offset, spans);
            }
            Yield::From(yf) => {
                collect_spans_from_expression(yf.iterator, offset, spans);
            }
        },

        Expression::Throw(throw) => {
            collect_spans_from_expression(throw.exception, offset, spans);
        }

        Expression::Clone(clone) => {
            collect_spans_from_expression(clone.object, offset, spans);
        }

        Expression::Construct(construct) => match construct {
            Construct::Isset(isset) => {
                for expr in isset.values.iter() {
                    collect_spans_from_expression(expr, offset, spans);
                }
            }
            Construct::Empty(empty) => {
                collect_spans_from_expression(empty.value, offset, spans);
            }
            Construct::Eval(eval) => {
                collect_spans_from_expression(eval.value, offset, spans);
            }
            Construct::Include(inc) => {
                collect_spans_from_expression(inc.value, offset, spans);
            }
            Construct::IncludeOnce(inc) => {
                collect_spans_from_expression(inc.value, offset, spans);
            }
            Construct::Require(req) => {
                collect_spans_from_expression(req.value, offset, spans);
            }
            Construct::RequireOnce(req) => {
                collect_spans_from_expression(req.value, offset, spans);
            }
            Construct::Print(print) => {
                collect_spans_from_expression(print.value, offset, spans);
            }
            Construct::Exit(exit) => {
                if let Some(ref args) = exit.arguments {
                    collect_spans_from_argument_list(args, offset, spans);
                }
            }
            Construct::Die(die) => {
                if let Some(ref args) = die.arguments {
                    collect_spans_from_argument_list(args, offset, spans);
                }
            }
        },

        Expression::CompositeString(composite) => {
            for part in composite.parts().iter() {
                match part {
                    string::StringPart::Expression(expr_ref) => {
                        collect_spans_from_expression(expr_ref, offset, spans);
                    }
                    string::StringPart::BracedExpression(braced) => {
                        collect_spans_from_expression(braced.expression, offset, spans);
                    }
                    _ => {}
                }
            }
        }

        Expression::Pipe(pipe) => {
            collect_spans_from_expression(pipe.input, offset, spans);
            collect_spans_from_expression(pipe.callable, offset, spans);
        }

        // Leaf expressions — the expression span itself is sufficient.
        Expression::Literal(_)
        | Expression::Variable(_)
        | Expression::ConstantAccess(_)
        | Expression::Identifier(_)
        | Expression::Parent(_)
        | Expression::Static(_)
        | Expression::Self_(_)
        | Expression::MagicConstant(_)
        | Expression::PartialApplication(_)
        | Expression::Error(_) => {}

        // Non-exhaustive: future variants get the expression-level span only.
        _ => {}
    }
}

// ─── Argument list walker ───────────────────────────────────────────────────

fn collect_spans_from_argument_list(
    args: &argument::ArgumentList<'_>,
    offset: u32,
    spans: &mut Vec<(u32, u32)>,
) {
    push_paren_pair(args.left_parenthesis, args.right_parenthesis, offset, spans);
    for arg in args.arguments.iter() {
        let arg_span = arg.span();
        if push_if_contains(arg_span, offset, spans) {
            match arg {
                argument::Argument::Positional(pos) => {
                    collect_spans_from_expression(pos.value, offset, spans);
                }
                argument::Argument::Named(named) => {
                    collect_spans_from_expression(named.value, offset, spans);
                }
            }
        }
    }
}

// ─── Paren pair helper ──────────────────────────────────────────────────────

fn push_paren_pair(
    left: mago_span::Span,
    right: mago_span::Span,
    offset: u32,
    spans: &mut Vec<(u32, u32)>,
) {
    let start = left.start.offset;
    let end = right.end.offset;
    if start <= offset && offset <= end {
        spans.push((start, end));
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::make_backend;

    fn selection_ranges(content: &str, positions: &[Position]) -> Vec<SelectionRange> {
        let backend = make_backend();
        backend
            .handle_selection_range(content, positions)
            .unwrap_or_default()
    }

    /// Flatten a SelectionRange linked list into a Vec of Ranges (innermost first).
    fn flatten(sel: &SelectionRange) -> Vec<Range> {
        let mut result = vec![sel.range];
        let mut current = &sel.parent;
        while let Some(parent) = current {
            result.push(parent.range);
            current = &parent.parent;
        }
        result
    }

    #[test]
    fn single_variable_in_function() {
        let content = r#"<?php
function hello() {
    $name = "world";
    echo $name;
}
"#;
        // Position cursor on `$name` in the echo statement (line 3, char 9).
        let results = selection_ranges(content, &[Position::new(3, 9)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);

        // Should have multiple levels: at minimum variable → expression → statement → block → function → file.
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 selection range levels, got {}",
            ranges.len()
        );

        // The innermost range should be smaller than the outermost.
        let innermost = &ranges[0];
        let outermost = ranges.last().unwrap();
        assert!(
            innermost.start.line >= outermost.start.line
                || innermost.start.character >= outermost.start.character,
            "Innermost range should be within outermost"
        );
    }

    #[test]
    fn class_method_body() {
        let content = r#"<?php
class Greeter {
    public function greet(string $name): string {
        return "Hello, " . $name;
    }
}
"#;
        // Position cursor on `$name` in the return statement (line 3, char 29).
        let results = selection_ranges(content, &[Position::new(3, 29)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);

        // Should have levels: variable → expression → return → block → method → class body → class → file.
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 selection range levels, got {}",
            ranges.len()
        );
    }

    #[test]
    fn multiple_positions() {
        let content = r#"<?php
$a = 1;
$b = 2;
"#;
        let results = selection_ranges(content, &[Position::new(1, 1), Position::new(2, 1)]);
        assert_eq!(results.len(), 2);

        // Each should produce a valid chain.
        for result in &results {
            let ranges = flatten(result);
            assert!(!ranges.is_empty());
        }
    }

    #[test]
    fn nested_if_statement() {
        let content = r#"<?php
if (true) {
    if (false) {
        echo "inner";
    }
}
"#;
        // Cursor on "inner" (line 3, char 14).
        let results = selection_ranges(content, &[Position::new(3, 14)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);

        // Should have many levels: string → echo args → echo stmt → block → inner if → block → outer if → file.
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels, got {}",
            ranges.len()
        );
    }

    #[test]
    fn empty_file() {
        let content = "<?php\n";
        let results = selection_ranges(content, &[Position::new(0, 3)]);
        assert_eq!(results.len(), 1);
        // Even an empty file should return at least the file-level range.
        let ranges = flatten(&results[0]);
        assert!(!ranges.is_empty());
    }

    #[test]
    fn instanceof_in_method_has_fine_grained_levels() {
        let content = r#"<?php
class Demo {
    public function test(): void {
        $x = new User();
        if ($x instanceof User) {
            $x->getEmail();
        }
    }
}
"#;
        // Cursor on "getEmail" (line 5, char 17).
        let results = selection_ranges(content, &[Position::new(5, 17)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);

        // Expected levels (innermost first):
        //   getEmail (method selector)
        //   $x->getEmail() (call expression)
        //   $x->getEmail(); (expression statement)
        //   { ... } (if block body)
        //   if (...) { ... } (if statement)
        //   { ... } (method body block)
        //   public function test()... (method member)
        //   { ... } (class body)
        //   class Demo { ... } (class statement)
        //   file
        assert!(
            ranges.len() >= 7,
            "Expected at least 7 fine-grained levels for method call inside if, got {}: {:?}",
            ranges.len(),
            ranges,
        );
    }

    #[test]
    fn ranges_are_nested() {
        let content = r#"<?php
function test() {
    $x = [1, 2, 3];
}
"#;
        // Cursor on `2` in the array (line 2, char 13).
        let results = selection_ranges(content, &[Position::new(2, 13)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);

        // Verify that each range is contained within or equal to its parent.
        for window in ranges.windows(2) {
            let inner = &window[0];
            let outer = &window[1];
            assert!(
                (inner.start.line > outer.start.line
                    || (inner.start.line == outer.start.line
                        && inner.start.character >= outer.start.character))
                    && (inner.end.line < outer.end.line
                        || (inner.end.line == outer.end.line
                            && inner.end.character <= outer.end.character)),
                "Inner range {:?} should be contained within outer range {:?}",
                inner,
                outer,
            );
        }
    }

    /// Helper: assert every range in the chain is contained within its parent.
    fn assert_nested(ranges: &[Range]) {
        for window in ranges.windows(2) {
            let inner = &window[0];
            let outer = &window[1];
            assert!(
                (inner.start.line > outer.start.line
                    || (inner.start.line == outer.start.line
                        && inner.start.character >= outer.start.character))
                    && (inner.end.line < outer.end.line
                        || (inner.end.line == outer.end.line
                            && inner.end.character <= outer.end.character)),
                "Inner range {:?} should be contained within outer range {:?}",
                inner,
                outer,
            );
        }
    }

    // ─── 1. Switch statement ────────────────────────────────────────────

    #[test]
    fn switch_statement_case_body() {
        let content = r#"<?php
switch ($x) {
    case 1:
        echo "one";
        break;
    case 2:
        echo "two";
        break;
    default:
        echo "other";
}
"#;
        // Cursor on "one" inside first case (line 3, char 14).
        let results = selection_ranges(content, &[Position::new(3, 14)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // string → echo → case → { } → switch → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for switch case body, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 2. Foreach loop ────────────────────────────────────────────────

    #[test]
    fn foreach_value_variable() {
        let content = r#"<?php
$items = [1, 2, 3];
foreach ($items as $item) {
    echo $item;
}
"#;
        // Cursor on $item in the echo (line 3, char 10).
        let results = selection_ranges(content, &[Position::new(3, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $item → echo stmt → foreach body stmt → foreach → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for foreach value, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn foreach_key_value() {
        let content = r#"<?php
$map = ['a' => 1];
foreach ($map as $key => $val) {
    echo $key;
}
"#;
        // Cursor on $key in the echo (line 3, char 10).
        let results = selection_ranges(content, &[Position::new(3, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for foreach key-value body, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn foreach_cursor_on_key_target() {
        let content = r#"<?php
$map = ['a' => 1];
foreach ($map as $key => $val) {
    echo $val;
}
"#;
        // Cursor on $key in the foreach target (line 2, char 18).
        let results = selection_ranges(content, &[Position::new(2, 18)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $key → target → foreach → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for foreach key target, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 3. For loop ────────────────────────────────────────────────────

    #[test]
    fn for_loop_body() {
        let content = r#"<?php
for ($i = 0; $i < 10; $i++) {
    echo $i;
}
"#;
        // Cursor on $i in echo (line 2, char 10).
        let results = selection_ranges(content, &[Position::new(2, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $i → echo → for body → for → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for for loop body, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 4. While loop ─────────────────────────────────────────────────

    #[test]
    fn while_loop_body() {
        let content = r#"<?php
while (true) {
    echo "loop";
}
"#;
        // Cursor on "loop" (line 2, char 10).
        let results = selection_ranges(content, &[Position::new(2, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // string → echo → while body → while → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for while body, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 5. Do-while loop ──────────────────────────────────────────────

    #[test]
    fn do_while_body() {
        let content = r#"<?php
do {
    echo "loop";
} while (true);
"#;
        // Cursor on "loop" (line 2, char 10).
        let results = selection_ranges(content, &[Position::new(2, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // string → echo → block → do-while → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for do-while body, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 6. Try/catch/finally ───────────────────────────────────────────

    #[test]
    fn try_body() {
        let content = r#"<?php
try {
    echo "try";
} catch (\Exception $e) {
    echo "catch";
} finally {
    echo "finally";
}
"#;
        // Cursor on "try" string (line 2, char 10).
        let results = selection_ranges(content, &[Position::new(2, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // string → echo → try block body → try block → try stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for try body, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn catch_body() {
        let content = r#"<?php
try {
    echo "try";
} catch (\Exception $e) {
    echo "catch";
} finally {
    echo "finally";
}
"#;
        // Cursor on "catch" string (line 4, char 10).
        let results = selection_ranges(content, &[Position::new(4, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // string → echo → catch block body → catch block → catch clause → try stmt → file
        assert!(
            ranges.len() >= 5,
            "Expected at least 5 levels for catch body, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn finally_body() {
        let content = r#"<?php
try {
    echo "try";
} catch (\Exception $e) {
    echo "catch";
} finally {
    echo "finally";
}
"#;
        // Cursor on "finally" string (line 6, char 10).
        let results = selection_ranges(content, &[Position::new(6, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // string → echo → finally block body → finally block → finally clause → try stmt → file
        assert!(
            ranges.len() >= 5,
            "Expected at least 5 levels for finally body, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 7. Return statement ────────────────────────────────────────────

    #[test]
    fn return_statement_value() {
        let content = r#"<?php
function foo() {
    return 42;
}
"#;
        // Cursor on 42 (line 2, char 11).
        let results = selection_ranges(content, &[Position::new(2, 11)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // 42 → return → block → function → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for return value, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 8. Echo statement ──────────────────────────────────────────────

    #[test]
    fn echo_statement_value() {
        let content = r#"<?php
echo "hello", "world";
"#;
        // Cursor on "world" (line 1, char 15).
        let results = selection_ranges(content, &[Position::new(1, 15)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // string → echo → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for echo value, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 9. Closure expression ──────────────────────────────────────────

    #[test]
    fn closure_body() {
        let content = r#"<?php
$fn = function ($x) {
    return $x + 1;
};
"#;
        // Cursor on $x in return (line 2, char 11).
        let results = selection_ranges(content, &[Position::new(2, 11)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $x → binary → return → closure body block → closure body → closure expr → assignment → expr stmt → file
        assert!(
            ranges.len() >= 5,
            "Expected at least 5 levels for closure body, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 10. Arrow function ─────────────────────────────────────────────

    #[test]
    fn arrow_function_expression_body() {
        let content = r#"<?php
$fn = fn($x) => $x + 1;
"#;
        // Cursor on $x in the arrow body expression (line 1, char 17).
        let results = selection_ranges(content, &[Position::new(1, 17)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $x → binary → arrow fn → assignment → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for arrow function body, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 11. Match expression ───────────────────────────────────────────

    #[test]
    fn match_arm_expression() {
        let content = r#"<?php
$result = match($x) {
    1 => "one",
    2 => "two",
    default => "other",
};
"#;
        // Cursor on "two" (line 3, char 10).
        let results = selection_ranges(content, &[Position::new(3, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // "two" → arm → { } → match → assignment → expr stmt → file
        assert!(
            ranges.len() >= 5,
            "Expected at least 5 levels for match arm, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 12. Anonymous class ────────────────────────────────────────────

    #[test]
    fn anonymous_class_method() {
        let content = r#"<?php
$obj = new class {
    public function hello() {
        echo "hi";
    }
};
"#;
        // Cursor on "hi" (line 3, char 14).
        let results = selection_ranges(content, &[Position::new(3, 14)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // "hi" → echo → method block → method → anon class body → anon class → instantiation → assignment → expr stmt → file
        assert!(
            ranges.len() >= 6,
            "Expected at least 6 levels for anonymous class method, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 13. Array literal (short syntax) ───────────────────────────────

    #[test]
    fn array_key_value_element() {
        let content = r#"<?php
$x = ['key' => 'value', 'b' => 'c'];
"#;
        // Cursor on 'value' (line 1, char 17).
        let results = selection_ranges(content, &[Position::new(1, 17)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // 'value' → key-value element → array → assignment → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for array key-value element, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 14. Legacy array() ─────────────────────────────────────────────

    #[test]
    fn legacy_array_elements() {
        let content = r#"<?php
$x = array('a' => 1, 'b' => 2);
"#;
        // Cursor on 1 (line 1, char 19).
        let results = selection_ranges(content, &[Position::new(1, 19)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // 1 → kv element → array() → assignment → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for legacy array element, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 15. List/destructuring ─────────────────────────────────────────

    #[test]
    fn list_expression() {
        let content = r#"<?php
list($a, $b) = [1, 2];
"#;
        // Cursor on $a (line 1, char 5).
        let results = selection_ranges(content, &[Position::new(1, 5)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // element → list → assignment → expr stmt → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for list expression, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 16. Binary expression ──────────────────────────────────────────

    #[test]
    fn binary_expression_rhs() {
        let content = r#"<?php
$c = $a + $b;
"#;
        // Cursor on $b (line 1, char 11).
        let results = selection_ranges(content, &[Position::new(1, 11)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $b → binary ($a + $b) → assignment → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for binary rhs, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 17. Conditional/ternary ────────────────────────────────────────

    #[test]
    fn ternary_else_branch() {
        let content = r#"<?php
$x = true ? "yes" : "no";
"#;
        // Cursor on "no" (line 1, char 22).
        let results = selection_ranges(content, &[Position::new(1, 22)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // "no" → ternary → assignment → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for ternary else, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 18. Property access ────────────────────────────────────────────

    #[test]
    fn property_access() {
        let content = r#"<?php
$x = $obj->prop;
"#;
        // Cursor on prop (line 1, char 12).
        let results = selection_ranges(content, &[Position::new(1, 12)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // prop → $obj->prop → assignment → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for property access, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 19. Static method call ─────────────────────────────────────────

    #[test]
    fn static_method_call() {
        let content = r#"<?php
$x = Foo::bar();
"#;
        // Cursor on bar (line 1, char 11).
        let results = selection_ranges(content, &[Position::new(1, 11)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // bar → Foo::bar() → assignment → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for static method call, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 20. Instantiation ──────────────────────────────────────────────

    #[test]
    fn instantiation_expression() {
        let content = r#"<?php
$x = new Foo();
"#;
        // Cursor on Foo (line 1, char 10).
        let results = selection_ranges(content, &[Position::new(1, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // Foo → new Foo() → assignment → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for instantiation, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 21. Yield expression ───────────────────────────────────────────

    #[test]
    fn yield_value() {
        let content = r#"<?php
function gen() {
    yield 42;
}
"#;
        // Cursor on 42 (line 2, char 10).
        let results = selection_ranges(content, &[Position::new(2, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // 42 → yield → expr stmt → block → function → file
        assert!(
            ranges.len() >= 5,
            "Expected at least 5 levels for yield value, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn yield_pair() {
        let content = r#"<?php
function gen() {
    yield 'key' => 'value';
}
"#;
        // Cursor on 'value' (line 2, char 20).
        let results = selection_ranges(content, &[Position::new(2, 20)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // 'value' → yield pair → expr stmt → block → function → file
        assert!(
            ranges.len() >= 5,
            "Expected at least 5 levels for yield pair, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn yield_from() {
        let content = r#"<?php
function gen() {
    yield from other();
}
"#;
        // Cursor on other (line 2, char 16).
        let results = selection_ranges(content, &[Position::new(2, 16)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // other() → yield from → expr stmt → block → function → file
        assert!(
            ranges.len() >= 5,
            "Expected at least 5 levels for yield from, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 22. Throw expression ───────────────────────────────────────────

    #[test]
    fn throw_expression() {
        let content = r#"<?php
throw new \Exception("error");
"#;
        // Cursor on Exception (line 1, char 12).
        let results = selection_ranges(content, &[Position::new(1, 12)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // Exception → new \Exception(...) → throw → expr stmt → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for throw expression, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 23. Clone expression ───────────────────────────────────────────

    #[test]
    fn clone_expression() {
        let content = r#"<?php
$y = clone $x;
"#;
        // Cursor on $x (line 1, char 12).
        let results = selection_ranges(content, &[Position::new(1, 12)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $x → clone $x → assignment → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for clone expression, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 24. Construct expressions ──────────────────────────────────────

    #[test]
    fn construct_isset() {
        let content = r#"<?php
$x = isset($a, $b);
"#;
        // Cursor on $a (line 1, char 12).
        let results = selection_ranges(content, &[Position::new(1, 12)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $a → isset(...) → assignment → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for isset, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn construct_empty() {
        let content = r#"<?php
$x = empty($a);
"#;
        // Cursor on $a (line 1, char 12).
        let results = selection_ranges(content, &[Position::new(1, 12)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $a → empty(...) → assignment → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for empty, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn construct_eval() {
        let content = r#"<?php
eval('echo 1;');
"#;
        // Cursor on 'echo 1;' (line 1, char 6).
        let results = selection_ranges(content, &[Position::new(1, 6)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // string → eval → expr stmt → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for eval, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn construct_include() {
        let content = r#"<?php
include 'file.php';
"#;
        // Cursor on 'file.php' (line 1, char 10).
        let results = selection_ranges(content, &[Position::new(1, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // string → include → expr stmt → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for include, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn construct_include_once() {
        let content = r#"<?php
include_once 'file.php';
"#;
        // Cursor on 'file.php' (line 1, char 15).
        let results = selection_ranges(content, &[Position::new(1, 15)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for include_once, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn construct_require() {
        let content = r#"<?php
require 'file.php';
"#;
        // Cursor on 'file.php' (line 1, char 10).
        let results = selection_ranges(content, &[Position::new(1, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for require, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn construct_require_once() {
        let content = r#"<?php
require_once 'file.php';
"#;
        // Cursor on 'file.php' (line 1, char 15).
        let results = selection_ranges(content, &[Position::new(1, 15)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for require_once, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 25. Namespace (brace-delimited) ────────────────────────────────

    #[test]
    fn namespace_brace_delimited() {
        let content = r#"<?php
namespace App {
    function foo() {
        echo "hello";
    }
}
"#;
        // Cursor on "hello" (line 3, char 14).
        let results = selection_ranges(content, &[Position::new(3, 14)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // string → echo → block → function → { } → namespace → file
        assert!(
            ranges.len() >= 5,
            "Expected at least 5 levels for braced namespace, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 26. Namespace (implicit) ───────────────────────────────────────

    #[test]
    fn namespace_implicit() {
        let content = r#"<?php
namespace App;
function foo() {
    echo "hello";
}
"#;
        // Cursor on "hello" (line 3, char 10).
        let results = selection_ranges(content, &[Position::new(3, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // string → echo → block → function → namespace → file
        assert!(
            ranges.len() >= 5,
            "Expected at least 5 levels for implicit namespace, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 27. Interface members ──────────────────────────────────────────

    #[test]
    fn interface_method() {
        let content = r#"<?php
interface Greetable {
    public function greet(string $name): string;
}
"#;
        // Cursor on $name (line 2, char 35).
        let results = selection_ranges(content, &[Position::new(2, 35)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $name → parameter → param list → method → { } → interface → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for interface method, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 28. Trait members ──────────────────────────────────────────────

    #[test]
    fn trait_method_body() {
        let content = r#"<?php
trait Greeter {
    public function greet(): string {
        return "hello";
    }
}
"#;
        // Cursor on "hello" (line 3, char 16).
        let results = selection_ranges(content, &[Position::new(3, 16)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // "hello" → return → block → method → { } → trait → file
        assert!(
            ranges.len() >= 5,
            "Expected at least 5 levels for trait method body, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 29. Enum members ───────────────────────────────────────────────

    #[test]
    fn backed_enum_case() {
        let content = r#"<?php
enum Color: string {
    case Red = 'red';
    case Blue = 'blue';
}
"#;
        // Cursor on 'red' (line 2, char 16).
        let results = selection_ranges(content, &[Position::new(2, 16)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // 'red' → backed item → enum case → { } → enum → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for backed enum case, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 30. Method parameter ───────────────────────────────────────────

    #[test]
    fn method_parameter_variable() {
        let content = r#"<?php
class Foo {
    public function bar(int $x, string $y): void {}
}
"#;
        // Cursor on $y (line 2, char 40).
        let results = selection_ranges(content, &[Position::new(2, 40)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $y → parameter → param list → method → { } → class → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for method parameter, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 31. Function parameter with default ────────────────────────────

    #[test]
    fn function_parameter_default() {
        let content = r#"<?php
function foo(int $x = 42) {
    echo $x;
}
"#;
        // Cursor on 42 (line 1, char 22).
        let results = selection_ranges(content, &[Position::new(1, 22)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // 42 → parameter → param list → function → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for param default, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 32. Named argument ─────────────────────────────────────────────

    #[test]
    fn named_argument_value() {
        let content = r#"<?php
foo(name: "John");
"#;
        // Cursor on "John" (line 1, char 11).
        let results = selection_ranges(content, &[Position::new(1, 11)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // "John" → named arg → arg list → call → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for named argument, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 33. Unset statement ────────────────────────────────────────────

    #[test]
    fn unset_statement() {
        let content = r#"<?php
unset($a, $b);
"#;
        // Cursor on $a (line 1, char 6).
        let results = selection_ranges(content, &[Position::new(1, 6)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $a → unset → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for unset, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 34. Declare statement ──────────────────────────────────────────

    #[test]
    fn declare_statement_body() {
        let content = r#"<?php
declare(strict_types=1) {
    echo "strict";
}
"#;
        // Cursor on "strict" (line 2, char 10).
        let results = selection_ranges(content, &[Position::new(2, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // "strict" → echo → block → declare → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for declare body, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 35. CompositeString ────────────────────────────────────────────

    #[test]
    fn composite_string_expression() {
        let content = r#"<?php
$name = "world";
echo "hello {$name}!";
"#;
        // Cursor on $name inside the string (line 2, char 14).
        let results = selection_ranges(content, &[Position::new(2, 14)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $name → composite string → echo → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for composite string, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 36. Pipe expression ────────────────────────────────────────────

    #[test]
    fn pipe_expression() {
        // Pipe operator may not parse in all PHP versions, but the branch exists.
        // If it doesn't parse, the test simply passes with fewer levels.
        let content = r#"<?php
$x = $a |> 'strtoupper';
"#;
        let results = selection_ranges(content, &[Position::new(1, 6)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // At minimum: some expr → assignment → expr stmt → file
        assert!(
            ranges.len() >= 2,
            "Expected at least 2 levels for pipe expression, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 37. Elseif/else clause ─────────────────────────────────────────

    #[test]
    fn elseif_body() {
        let content = r#"<?php
if (true) {
    echo "a";
} elseif (false) {
    echo "b";
} else {
    echo "c";
}
"#;
        // Cursor on "b" in the elseif body (line 4, char 10).
        let results = selection_ranges(content, &[Position::new(4, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // "b" → echo → block stmt → elseif clause → if → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for elseif body, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn else_body() {
        let content = r#"<?php
if (true) {
    echo "a";
} else {
    echo "c";
}
"#;
        // Cursor on "c" in the else body (line 4, char 10).
        let results = selection_ranges(content, &[Position::new(4, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // "c" → echo → block stmt → else clause → if → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for else body, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 38. Colon-delimited if ─────────────────────────────────────────

    #[test]
    fn colon_delimited_if_body() {
        let content = r#"<?php
if (true):
    echo "a";
elseif (false):
    echo "b";
else:
    echo "c";
endif;
"#;
        // Cursor on "a" in the if body (line 2, char 10).
        let results = selection_ranges(content, &[Position::new(2, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // "a" → echo → if → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for colon-delimited if body, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn colon_delimited_elseif_body() {
        let content = r#"<?php
if (true):
    echo "a";
elseif (false):
    echo "b";
else:
    echo "c";
endif;
"#;
        // Cursor on "b" in the elseif body (line 4, char 10).
        let results = selection_ranges(content, &[Position::new(4, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // "b" → echo → elseif clause → if → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for colon-delimited elseif body, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn colon_delimited_else_body() {
        let content = r#"<?php
if (true):
    echo "a";
else:
    echo "c";
endif;
"#;
        // Cursor on "c" in the else body (line 4, char 10).
        let results = selection_ranges(content, &[Position::new(4, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // "c" → echo → else clause → if → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for colon-delimited else body, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 39. Class constant ─────────────────────────────────────────────

    #[test]
    fn class_constant_value() {
        let content = r#"<?php
class Foo {
    const BAR = 42;
}
"#;
        // Cursor on 42 (line 2, char 16).
        let results = selection_ranges(content, &[Position::new(2, 16)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // 42 → constant item → constant member → { } → class → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for class constant, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 40. Enum case (backed) value ───────────────────────────────────

    #[test]
    fn enum_backed_case_value() {
        let content = r#"<?php
enum Status: int {
    case Active = 1;
    case Inactive = 0;
}
"#;
        // Cursor on 0 (line 3, char 20).
        let results = selection_ranges(content, &[Position::new(3, 20)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // 0 → backed item → enum case → { } → enum → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for enum backed case value, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 41. Property with default ──────────────────────────────────────

    #[test]
    fn property_with_default() {
        let content = r#"<?php
class Foo {
    public int $x = 42;
}
"#;
        // Cursor on 42 (line 2, char 21).
        let results = selection_ranges(content, &[Position::new(2, 21)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // 42 → property item → property → { } → class → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for property default, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 42. Array access ───────────────────────────────────────────────

    #[test]
    fn array_access_expression() {
        let content = r#"<?php
$x = $arr[0];
"#;
        // Cursor on 0 (line 1, char 10).
        let results = selection_ranges(content, &[Position::new(1, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // 0 → $arr[0] → assignment → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for array access, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── 43. Array append ───────────────────────────────────────────────

    #[test]
    fn array_append_expression() {
        let content = r#"<?php
$arr[] = 42;
"#;
        // Cursor on $arr (line 1, char 1).
        let results = selection_ranges(content, &[Position::new(1, 1)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $arr → $arr[] → assignment → expr stmt → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for array append, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── Additional branch coverage ─────────────────────────────────────

    #[test]
    fn unary_prefix_expression() {
        let content = r#"<?php
$x = !$flag;
"#;
        // Cursor on $flag (line 1, char 7).
        let results = selection_ranges(content, &[Position::new(1, 7)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $flag → !$flag → assignment → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for unary prefix, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn unary_postfix_expression() {
        let content = r#"<?php
$x++;
"#;
        // Cursor on $x (line 1, char 1).
        let results = selection_ranges(content, &[Position::new(1, 1)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $x → $x++ → expr stmt → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for unary postfix, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn parenthesized_expression() {
        let content = r#"<?php
$x = (1 + 2);
"#;
        // Cursor on 1 (line 1, char 6).
        let results = selection_ranges(content, &[Position::new(1, 6)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // 1 → 1+2 → (1+2) → assignment → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for parenthesized, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn assignment_expression() {
        let content = r#"<?php
$x = $y = 5;
"#;
        // Cursor on 5 (line 1, char 10).
        let results = selection_ranges(content, &[Position::new(1, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // 5 → $y = 5 → $x = $y = 5 → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for chained assignment, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn method_call_expression() {
        let content = r#"<?php
$obj->method(42);
"#;
        // Cursor on 42 (line 1, char 13).
        let results = selection_ranges(content, &[Position::new(1, 13)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // 42 → arg → arg list → $obj->method(42) → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for method call arg, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn null_safe_property_access() {
        let content = r#"<?php
$x = $obj?->prop;
"#;
        // Cursor on prop (line 1, char 13).
        let results = selection_ranges(content, &[Position::new(1, 13)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // prop → $obj?->prop → assignment → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for null-safe property access, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn static_property_access() {
        let content = r#"<?php
$x = Foo::$bar;
"#;
        // Cursor on $bar (line 1, char 11).
        let results = selection_ranges(content, &[Position::new(1, 11)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $bar → Foo::$bar → assignment → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for static property access, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn class_constant_access() {
        let content = r#"<?php
$x = Foo::BAR;
"#;
        // Cursor on BAR (line 1, char 11).
        let results = selection_ranges(content, &[Position::new(1, 11)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // BAR → Foo::BAR → assignment → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for class constant access, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn null_safe_method_call() {
        let content = r#"<?php
$x = $obj?->method(1);
"#;
        // Cursor on method (line 1, char 14).
        let results = selection_ranges(content, &[Position::new(1, 14)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // method → $obj?->method(1) → assignment → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for null-safe method call, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn function_call_expression() {
        let content = r#"<?php
strlen("hello");
"#;
        // Cursor on "hello" (line 1, char 8).
        let results = selection_ranges(content, &[Position::new(1, 8)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // "hello" → positional arg → arg list → strlen(...) → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for function call, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn array_variadic_element() {
        let content = r#"<?php
$x = [...$arr];
"#;
        // Cursor on $arr (line 1, char 10).
        let results = selection_ranges(content, &[Position::new(1, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $arr → variadic element → array → assignment → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for array variadic, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn closure_parameter() {
        let content = r#"<?php
$fn = function (int $x) {
    return $x;
};
"#;
        // Cursor on $x in parameter (line 1, char 20).
        let results = selection_ranges(content, &[Position::new(1, 20)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $x → parameter → param list → closure → assignment → expr stmt → file
        assert!(
            ranges.len() >= 5,
            "Expected at least 5 levels for closure parameter, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn arrow_function_parameter() {
        let content = r#"<?php
$fn = fn(int $x) => $x + 1;
"#;
        // Cursor on $x in parameter (line 1, char 14).
        let results = selection_ranges(content, &[Position::new(1, 14)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $x → parameter → param list → arrow fn → assignment → expr stmt → file
        assert!(
            ranges.len() >= 5,
            "Expected at least 5 levels for arrow fn parameter, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn construct_print() {
        let content = r#"<?php
print "hello";
"#;
        // Cursor on "hello" (line 1, char 7).
        let results = selection_ranges(content, &[Position::new(1, 7)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // "hello" → print → expr stmt → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for print, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn for_loop_colon_delimited() {
        let content = r#"<?php
for ($i = 0; $i < 10; $i++):
    echo $i;
endfor;
"#;
        // Cursor on $i in echo (line 2, char 10).
        let results = selection_ranges(content, &[Position::new(2, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $i → echo → for → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for colon-delimited for, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn while_loop_colon_delimited() {
        let content = r#"<?php
while (true):
    echo "loop";
endwhile;
"#;
        // Cursor on "loop" (line 2, char 10).
        let results = selection_ranges(content, &[Position::new(2, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // "loop" → echo → while → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for colon-delimited while, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn foreach_colon_delimited() {
        let content = r#"<?php
foreach ([1, 2] as $v):
    echo $v;
endforeach;
"#;
        // Cursor on $v in echo (line 2, char 10).
        let results = selection_ranges(content, &[Position::new(2, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $v → echo → foreach → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for colon-delimited foreach, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn switch_colon_delimited() {
        let content = r#"<?php
switch ($x):
    case 1:
        echo "one";
        break;
endswitch;
"#;
        // Cursor on "one" (line 3, char 14).
        let results = selection_ranges(content, &[Position::new(3, 14)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // "one" → echo → case → switch → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for colon-delimited switch, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn declare_colon_delimited() {
        let content = r#"<?php
declare(strict_types=1):
    echo "hello";
enddeclare;
"#;
        // Cursor on "hello" (line 2, char 10).
        let results = selection_ranges(content, &[Position::new(2, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // "hello" → echo → declare → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for colon-delimited declare, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn block_statement() {
        let content = r#"<?php
{
    echo "inside";
}
"#;
        // Cursor on "inside" (line 2, char 10).
        let results = selection_ranges(content, &[Position::new(2, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // "inside" → echo → { } → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for block statement, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn enum_unit_case() {
        let content = r#"<?php
enum Suit {
    case Hearts;
    case Diamonds;
}
"#;
        // Cursor on Hearts (line 2, char 10).
        let results = selection_ranges(content, &[Position::new(2, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // unit item → enum case → { } → enum → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for unit enum case, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn abstract_property() {
        let content = r#"<?php
class Foo {
    public int $x;
}
"#;
        // Cursor on $x (line 2, char 16).
        let results = selection_ranges(content, &[Position::new(2, 16)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $x → property → { } → class → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for abstract property, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn ternary_short_form() {
        // Short ternary: $a ?: $b (no then branch).
        let content = r#"<?php
$x = $a ?: $b;
"#;
        // Cursor on $b (line 1, char 12).
        let results = selection_ranges(content, &[Position::new(1, 12)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $b → ternary → assignment → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for short ternary, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn instantiation_without_args() {
        let content = r#"<?php
$x = new Foo;
"#;
        // Cursor on Foo (line 1, char 10).
        let results = selection_ranges(content, &[Position::new(1, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // Foo → new Foo → assignment → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for new without args, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn legacy_array_value_element() {
        let content = r#"<?php
$x = array(1, 2, 3);
"#;
        // Cursor on 2 (line 1, char 15).
        let results = selection_ranges(content, &[Position::new(1, 15)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // 2 → value element → array() → assignment → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for legacy array value element, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn enum_with_method() {
        let content = r#"<?php
enum Color: string {
    case Red = 'red';

    public function label(): string {
        return "Color";
    }
}
"#;
        // Cursor on "Color" in the method body (line 5, char 16).
        let results = selection_ranges(content, &[Position::new(5, 16)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // "Color" → return → block → method → { } → enum → file
        assert!(
            ranges.len() >= 5,
            "Expected at least 5 levels for enum method body, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn for_loop_initializer() {
        let content = r#"<?php
for ($i = 0; $i < 10; $i++) {
    echo $i;
}
"#;
        // Cursor on $i in the initializer (line 1, char 6).
        let results = selection_ranges(content, &[Position::new(1, 6)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $i → $i = 0 → for → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for for initializer, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn for_loop_condition() {
        let content = r#"<?php
for ($i = 0; $i < 10; $i++) {
    echo $i;
}
"#;
        // Cursor on 10 in the condition (line 1, char 19).
        let results = selection_ranges(content, &[Position::new(1, 19)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // 10 → $i < 10 → for → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for for condition, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn for_loop_increment() {
        let content = r#"<?php
for ($i = 0; $i < 10; $i++) {
    echo $i;
}
"#;
        // Cursor on $i in the increment (line 1, char 23).
        let results = selection_ranges(content, &[Position::new(1, 23)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $i → $i++ → for → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for for increment, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn while_condition() {
        let content = r#"<?php
while ($x > 0) {
    $x--;
}
"#;
        // Cursor on $x in the condition (line 1, char 8).
        let results = selection_ranges(content, &[Position::new(1, 8)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $x → $x > 0 → while → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for while condition, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn do_while_condition() {
        let content = r#"<?php
do {
    $x--;
} while ($x > 0);
"#;
        // Cursor on $x in the condition (line 3, char 10).
        let results = selection_ranges(content, &[Position::new(3, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $x → $x > 0 → do-while → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for do-while condition, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn foreach_source_expression() {
        let content = r#"<?php
foreach ($items as $item) {
    echo $item;
}
"#;
        // Cursor on $items in the foreach expression (line 1, char 10).
        let results = selection_ranges(content, &[Position::new(1, 10)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $items → foreach → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for foreach source expression, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn switch_expression() {
        let content = r#"<?php
switch ($x) {
    case 1:
        break;
}
"#;
        // Cursor on $x in switch expression (line 1, char 9).
        let results = selection_ranges(content, &[Position::new(1, 9)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $x → switch → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for switch expression, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn match_subject_expression() {
        let content = r#"<?php
$r = match($x) {
    default => 1,
};
"#;
        // Cursor on $x (line 1, char 12).
        let results = selection_ranges(content, &[Position::new(1, 12)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $x → match → assignment → expr stmt → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for match subject, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    #[test]
    fn if_condition() {
        let content = r#"<?php
if ($x > 0) {
    echo "positive";
}
"#;
        // Cursor on $x (line 1, char 5).
        let results = selection_ranges(content, &[Position::new(1, 5)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $x → $x > 0 → if → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for if condition, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── Construct: exit() ──────────────────────────────────────────────

    #[test]
    fn construct_exit_with_args() {
        let content = r#"<?php
exit(1);
"#;
        // Cursor on 1 (line 1, char 5).
        let results = selection_ranges(content, &[Position::new(1, 5)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // 1 → arg → arg list → exit(...) → expr stmt → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for exit with args, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── Construct: die() ───────────────────────────────────────────────

    #[test]
    fn construct_die_with_args() {
        let content = r#"<?php
die("error");
"#;
        // Cursor on "error" (line 1, char 5).
        let results = selection_ranges(content, &[Position::new(1, 5)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // "error" → arg → arg list → die(...) → expr stmt → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for die with args, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── Trait use statement ────────────────────────────────────────────

    #[test]
    fn trait_use_member() {
        let content = r#"<?php
trait Greeter {
    public function greet(): string {
        return "hello";
    }
}

class Foo {
    use Greeter;

    public function bar(): void {}
}
"#;
        // Cursor on Greeter in `use Greeter;` (line 8, char 8).
        let results = selection_ranges(content, &[Position::new(8, 8)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // trait use member → { } → class → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for trait use member, got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── Hooked property (abstract / no default) ────────────────────────

    #[test]
    fn hooked_property_abstract() {
        let content = r#"<?php
class Foo {
    public string $name {
        get => $this->name;
    }
}
"#;
        // Cursor on $name (line 2, char 19).
        let results = selection_ranges(content, &[Position::new(2, 19)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // $name → property member → { } → class → file
        assert!(
            ranges.len() >= 3,
            "Expected at least 3 levels for hooked property (abstract), got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }

    // ─── Hooked property (concrete / with default) ──────────────────────

    #[test]
    fn hooked_property_concrete() {
        let content = r#"<?php
class Foo {
    public string $name = "default" {
        get => $this->name;
    }
}
"#;
        // Cursor on "default" (line 2, char 27).
        let results = selection_ranges(content, &[Position::new(2, 27)]);
        assert_eq!(results.len(), 1);
        let ranges = flatten(&results[0]);
        // "default" → concrete item → property member → { } → class → file
        assert!(
            ranges.len() >= 4,
            "Expected at least 4 levels for hooked property (concrete), got {}",
            ranges.len()
        );
        assert_nested(&ranges);
    }
}
