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
#[path = "selection_range_tests.rs"]
mod tests;
