//! Simplify with null coalescing / null-safe operator.
//!
//! Offers code actions to simplify common nullable patterns:
//!
//! - `isset($x) ? $x : $default` → `$x ?? $default`
//! - `$x !== null ? $x : $default` → `$x ?? $default`
//! - `$x === null ? $default : $x` → `$x ?? $default`
//! - `$x !== null ? $x->foo() : null` → `$x?->foo()` (PHP 8.0+)
//! - `$x !== null ? $x->foo : null` → `$x?->foo` (PHP 8.0+)
//!
//! **Code action kind:** `refactor.rewrite`.
//!
//! Only ternary expressions are handled (not if-statement patterns).

use std::collections::HashMap;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::types::PhpVersion;
use crate::util::{offset_to_position, position_to_byte_offset};

// ─── Detection types ────────────────────────────────────────────────────────

/// A detected simplification opportunity.
enum Simplification {
    /// The ternary can be replaced with `$x ?? $default`.
    NullCoalescing {
        /// Source text of the left-hand side of `??`.
        lhs: String,
        /// Source text of the right-hand side of `??`.
        rhs: String,
    },
    /// The ternary can be replaced with `$x?->member` or `$x?->method()`.
    NullSafe {
        /// The full replacement text (e.g. `$x?->foo()` or `$x?->bar`).
        replacement: String,
    },
}

impl Backend {
    /// Collect "Simplify with null coalescing" code actions for ternary
    /// expressions at the cursor position.
    pub(crate) fn collect_simplify_null_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let doc_uri: Url = match uri.parse() {
            Ok(u) => u,
            Err(_) => return,
        };

        let cursor_offset = position_to_byte_offset(content, params.range.start) as u32;

        let php_version = self.php_version();

        // Walk the entire AST looking for ternary expressions that
        // contain the cursor. We collect the innermost match.
        let best =
            crate::parser::with_parsed_program(content, "simplify_null", |program, content| {
                let mut best: Option<(Simplification, u32, u32)> = None;
                for stmt in program.statements.iter() {
                    find_in_statement(stmt, cursor_offset, content, php_version, &mut best);
                }
                best
            });

        let (simplification, ternary_start, ternary_end) = match best {
            Some(b) => b,
            None => return,
        };

        let start_pos = offset_to_position(content, ternary_start as usize);
        let end_pos = offset_to_position(content, ternary_end as usize);

        let (title, replacement) = match &simplification {
            Simplification::NullCoalescing { lhs, rhs } => {
                ("Simplify to ??".to_string(), format!("{lhs} ?? {rhs}"))
            }
            Simplification::NullSafe { replacement } => {
                ("Simplify to ?->".to_string(), replacement.clone())
            }
        };

        let mut changes = HashMap::new();
        changes.insert(
            doc_uri,
            vec![TextEdit {
                range: Range {
                    start: start_pos,
                    end: end_pos,
                },
                new_text: replacement,
            }],
        );

        out.push(CodeActionOrCommand::CodeAction(CodeAction {
            title,
            kind: Some(CodeActionKind::new("refactor.rewrite")),
            diagnostics: None,
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                document_changes: None,
                change_annotations: None,
            }),
            command: None,
            is_preferred: Some(true),
            disabled: None,
            data: None,
        }));
    }
}

// ─── AST walking ────────────────────────────────────────────────────────────

/// Walk a statement looking for ternary expressions at the cursor.
fn find_in_statement(
    stmt: &Statement<'_>,
    cursor: u32,
    content: &str,
    php_version: PhpVersion,
    best: &mut Option<(Simplification, u32, u32)>,
) {
    match stmt {
        Statement::Expression(expr_stmt) => {
            find_in_expression(expr_stmt.expression, cursor, content, php_version, best);
        }
        Statement::Return(ret) => {
            if let Some(expr) = &ret.value {
                find_in_expression(expr, cursor, content, php_version, best);
            }
        }
        Statement::Echo(echo) => {
            for expr in echo.values.iter() {
                find_in_expression(expr, cursor, content, php_version, best);
            }
        }
        Statement::Block(block) => {
            for s in block.statements.iter() {
                find_in_statement(s, cursor, content, php_version, best);
            }
        }
        Statement::Namespace(ns) => {
            for s in ns.statements().iter() {
                find_in_statement(s, cursor, content, php_version, best);
            }
        }
        Statement::Class(class) => {
            for member in class.members.iter() {
                find_in_member(member, cursor, content, php_version, best);
            }
        }
        Statement::Trait(tr) => {
            for member in tr.members.iter() {
                find_in_member(member, cursor, content, php_version, best);
            }
        }
        Statement::Enum(en) => {
            for member in en.members.iter() {
                find_in_member(member, cursor, content, php_version, best);
            }
        }
        Statement::Interface(iface) => {
            for member in iface.members.iter() {
                find_in_member(member, cursor, content, php_version, best);
            }
        }
        Statement::Function(func) => {
            for s in func.body.statements.iter() {
                find_in_statement(s, cursor, content, php_version, best);
            }
        }
        Statement::If(if_stmt) => {
            find_in_expression(if_stmt.condition, cursor, content, php_version, best);
            find_in_if_body(&if_stmt.body, cursor, content, php_version, best);
        }
        Statement::While(w) => {
            find_in_expression(w.condition, cursor, content, php_version, best);
            find_in_while_body(&w.body, cursor, content, php_version, best);
        }
        Statement::DoWhile(dw) => {
            find_in_expression(dw.condition, cursor, content, php_version, best);
            find_in_statement(dw.statement, cursor, content, php_version, best);
        }
        Statement::For(f) => {
            for expr in f.initializations.iter() {
                find_in_expression(expr, cursor, content, php_version, best);
            }
            for expr in f.conditions.iter() {
                find_in_expression(expr, cursor, content, php_version, best);
            }
            for expr in f.increments.iter() {
                find_in_expression(expr, cursor, content, php_version, best);
            }
            find_in_for_body(&f.body, cursor, content, php_version, best);
        }
        Statement::Foreach(fe) => {
            find_in_expression(fe.expression, cursor, content, php_version, best);
            find_in_foreach_body(&fe.body, cursor, content, php_version, best);
        }
        Statement::Switch(sw) => {
            find_in_expression(sw.expression, cursor, content, php_version, best);
            for case in sw.body.cases().iter() {
                let stmts = match case {
                    SwitchCase::Expression(e) => &e.statements,
                    SwitchCase::Default(d) => &d.statements,
                };
                for s in stmts.iter() {
                    find_in_statement(s, cursor, content, php_version, best);
                }
            }
        }
        Statement::Try(tr) => {
            for s in tr.block.statements.iter() {
                find_in_statement(s, cursor, content, php_version, best);
            }
            for catch in tr.catch_clauses.iter() {
                for s in catch.block.statements.iter() {
                    find_in_statement(s, cursor, content, php_version, best);
                }
            }
            if let Some(finally) = &tr.finally_clause {
                for s in finally.block.statements.iter() {
                    find_in_statement(s, cursor, content, php_version, best);
                }
            }
        }
        _ => {}
    }
}

/// Walk an if-statement body (handles both statement and colon-delimited forms).
fn find_in_if_body(
    body: &IfBody<'_>,
    cursor: u32,
    content: &str,
    php_version: PhpVersion,
    best: &mut Option<(Simplification, u32, u32)>,
) {
    match body {
        IfBody::Statement(body) => {
            find_in_statement(body.statement, cursor, content, php_version, best);
            for clause in body.else_if_clauses.iter() {
                find_in_expression(clause.condition, cursor, content, php_version, best);
                find_in_statement(clause.statement, cursor, content, php_version, best);
            }
            if let Some(else_clause) = &body.else_clause {
                find_in_statement(else_clause.statement, cursor, content, php_version, best);
            }
        }
        IfBody::ColonDelimited(body) => {
            for s in body.statements.iter() {
                find_in_statement(s, cursor, content, php_version, best);
            }
            for clause in body.else_if_clauses.iter() {
                find_in_expression(clause.condition, cursor, content, php_version, best);
                for s in clause.statements.iter() {
                    find_in_statement(s, cursor, content, php_version, best);
                }
            }
            if let Some(else_clause) = &body.else_clause {
                for s in else_clause.statements.iter() {
                    find_in_statement(s, cursor, content, php_version, best);
                }
            }
        }
    }
}

/// Walk a while-statement body.
fn find_in_while_body(
    body: &WhileBody<'_>,
    cursor: u32,
    content: &str,
    php_version: PhpVersion,
    best: &mut Option<(Simplification, u32, u32)>,
) {
    match body {
        WhileBody::Statement(s) => {
            find_in_statement(s, cursor, content, php_version, best);
        }
        WhileBody::ColonDelimited(b) => {
            for s in b.statements.iter() {
                find_in_statement(s, cursor, content, php_version, best);
            }
        }
    }
}

/// Walk a for-statement body.
fn find_in_for_body(
    body: &ForBody<'_>,
    cursor: u32,
    content: &str,
    php_version: PhpVersion,
    best: &mut Option<(Simplification, u32, u32)>,
) {
    match body {
        ForBody::Statement(s) => {
            find_in_statement(s, cursor, content, php_version, best);
        }
        ForBody::ColonDelimited(b) => {
            for s in b.statements.iter() {
                find_in_statement(s, cursor, content, php_version, best);
            }
        }
    }
}

/// Walk a foreach-statement body.
fn find_in_foreach_body(
    body: &ForeachBody<'_>,
    cursor: u32,
    content: &str,
    php_version: PhpVersion,
    best: &mut Option<(Simplification, u32, u32)>,
) {
    match body {
        ForeachBody::Statement(s) => {
            find_in_statement(s, cursor, content, php_version, best);
        }
        ForeachBody::ColonDelimited(b) => {
            for s in b.statements.iter() {
                find_in_statement(s, cursor, content, php_version, best);
            }
        }
    }
}

/// Walk a class-like member looking for ternary expressions.
fn find_in_member(
    member: &class_like::member::ClassLikeMember<'_>,
    cursor: u32,
    content: &str,
    php_version: PhpVersion,
    best: &mut Option<(Simplification, u32, u32)>,
) {
    match member {
        class_like::member::ClassLikeMember::Method(method) => {
            if let class_like::method::MethodBody::Concrete(body) = &method.body {
                for s in body.statements.iter() {
                    find_in_statement(s, cursor, content, php_version, best);
                }
            }
        }
        class_like::member::ClassLikeMember::Property(prop) => match prop {
            Property::Plain(plain) => {
                for item in plain.items.iter() {
                    if let class_like::property::PropertyItem::Concrete(concrete) = item {
                        find_in_expression(concrete.value, cursor, content, php_version, best);
                    }
                }
            }
            Property::Hooked(hooked) => {
                if let class_like::property::PropertyItem::Concrete(concrete) = &hooked.item {
                    find_in_expression(concrete.value, cursor, content, php_version, best);
                }
            }
        },
        class_like::member::ClassLikeMember::Constant(constant) => {
            for item in constant.items.iter() {
                find_in_expression(item.value, cursor, content, php_version, best);
            }
        }
        _ => {}
    }
}

/// Walk an expression looking for simplifiable ternary patterns at
/// the cursor position. Updates `best` with the innermost match.
fn find_in_expression(
    expr: &Expression<'_>,
    cursor: u32,
    content: &str,
    php_version: PhpVersion,
    best: &mut Option<(Simplification, u32, u32)>,
) {
    let span = expr.span();
    let start = span.start.offset;
    let end = span.end.offset;

    // Only descend if the cursor is within this expression.
    if cursor < start || cursor > end {
        return;
    }

    // Check if this is a simplifiable ternary.
    if let Expression::Conditional(cond) = expr
        && let Some(simplification) = try_simplify_ternary(cond, content, php_version)
    {
        // Prefer the innermost (smallest span) match.
        let span_size = end - start;
        let should_update = match best {
            Some((_, bs, be)) => span_size < (*be - *bs),
            None => true,
        };
        if should_update {
            *best = Some((simplification, start, end));
        }
    }

    // Recurse into sub-expressions regardless so we find nested ternaries.
    walk_sub_expressions(expr, cursor, content, php_version, best);
}

/// Recurse into the sub-expressions of an expression node.
fn walk_sub_expressions(
    expr: &Expression<'_>,
    cursor: u32,
    content: &str,
    php_version: PhpVersion,
    best: &mut Option<(Simplification, u32, u32)>,
) {
    match expr {
        Expression::Parenthesized(p) => {
            find_in_expression(p.expression, cursor, content, php_version, best);
        }
        Expression::Binary(bin) => {
            find_in_expression(bin.lhs, cursor, content, php_version, best);
            find_in_expression(bin.rhs, cursor, content, php_version, best);
        }
        Expression::UnaryPrefix(u) => {
            find_in_expression(u.operand, cursor, content, php_version, best);
        }
        Expression::UnaryPostfix(u) => {
            find_in_expression(u.operand, cursor, content, php_version, best);
        }
        Expression::Conditional(cond) => {
            find_in_expression(cond.condition, cursor, content, php_version, best);
            if let Some(then) = cond.then {
                find_in_expression(then, cursor, content, php_version, best);
            }
            find_in_expression(cond.r#else, cursor, content, php_version, best);
        }
        Expression::Assignment(a) => {
            find_in_expression(a.lhs, cursor, content, php_version, best);
            find_in_expression(a.rhs, cursor, content, php_version, best);
        }
        Expression::Call(call) => match call {
            Call::Function(fc) => {
                find_in_expression(fc.function, cursor, content, php_version, best);
                for arg in fc.argument_list.arguments.iter() {
                    find_in_expression(arg.value(), cursor, content, php_version, best);
                }
            }
            Call::Method(mc) => {
                find_in_expression(mc.object, cursor, content, php_version, best);
                for arg in mc.argument_list.arguments.iter() {
                    find_in_expression(arg.value(), cursor, content, php_version, best);
                }
            }
            Call::NullSafeMethod(mc) => {
                find_in_expression(mc.object, cursor, content, php_version, best);
                for arg in mc.argument_list.arguments.iter() {
                    find_in_expression(arg.value(), cursor, content, php_version, best);
                }
            }
            Call::StaticMethod(sc) => {
                find_in_expression(sc.class, cursor, content, php_version, best);
                for arg in sc.argument_list.arguments.iter() {
                    find_in_expression(arg.value(), cursor, content, php_version, best);
                }
            }
        },
        Expression::Access(access) => match access {
            Access::Property(pa) => {
                find_in_expression(pa.object, cursor, content, php_version, best);
            }
            Access::NullSafeProperty(pa) => {
                find_in_expression(pa.object, cursor, content, php_version, best);
            }
            Access::StaticProperty(pa) => {
                find_in_expression(pa.class, cursor, content, php_version, best);
            }
            Access::ClassConstant(pa) => {
                find_in_expression(pa.class, cursor, content, php_version, best);
            }
        },
        Expression::Array(arr) => {
            for element in arr.elements.iter() {
                if let array::ArrayElement::KeyValue(kv) = element {
                    find_in_expression(kv.key, cursor, content, php_version, best);
                    find_in_expression(kv.value, cursor, content, php_version, best);
                } else if let array::ArrayElement::Value(val) = element {
                    find_in_expression(val.value, cursor, content, php_version, best);
                }
            }
        }
        Expression::LegacyArray(arr) => {
            for element in arr.elements.iter() {
                if let array::ArrayElement::KeyValue(kv) = element {
                    find_in_expression(kv.key, cursor, content, php_version, best);
                    find_in_expression(kv.value, cursor, content, php_version, best);
                } else if let array::ArrayElement::Value(val) = element {
                    find_in_expression(val.value, cursor, content, php_version, best);
                }
            }
        }
        Expression::Construct(construct) => match construct {
            Construct::Isset(isset) => {
                for val in isset.values.iter() {
                    find_in_expression(val, cursor, content, php_version, best);
                }
            }
            Construct::Empty(empty) => {
                find_in_expression(empty.value, cursor, content, php_version, best);
            }
            Construct::Eval(eval) => {
                find_in_expression(eval.value, cursor, content, php_version, best);
            }
            Construct::Print(print) => {
                find_in_expression(print.value, cursor, content, php_version, best);
            }
            _ => {}
        },
        Expression::Instantiation(inst) => {
            if let Some(arg_list) = &inst.argument_list {
                for a in arg_list.arguments.iter() {
                    find_in_expression(a.value(), cursor, content, php_version, best);
                }
            }
        }
        Expression::Literal(_)
        | Expression::Variable(_)
        | Expression::Identifier(_)
        | Expression::Self_(_)
        | Expression::Static(_)
        | Expression::Parent(_) => {}
        _ => {
            // For expression kinds we don't explicitly handle, skip.
            // We've covered the most common nesting cases above.
        }
    }
}

// ─── Pattern matching ───────────────────────────────────────────────────────

/// Try to simplify a ternary expression into `??` or `?->`.
fn try_simplify_ternary(
    cond: &Conditional<'_>,
    content: &str,
    php_version: PhpVersion,
) -> Option<Simplification> {
    // Only handle full ternaries (not short ternary `$a ?: $b`).
    let then_expr = cond.then?;

    // Pattern 1: isset($x) ? $x : $default → $x ?? $default
    if let Some(s) = try_isset_coalescing(cond.condition, then_expr, cond.r#else, content) {
        return Some(s);
    }

    // Pattern 2: $x !== null ? $x : $default → $x ?? $default
    // Pattern 3: $x === null ? $default : $x → $x ?? $default
    // Pattern 4: $x !== null ? $x->foo() : null → $x?->foo()  (PHP 8.0+)
    // Pattern 5: $x !== null ? $x->foo : null → $x?->foo  (PHP 8.0+)
    if let Some(s) =
        try_null_comparison_simplify(cond.condition, then_expr, cond.r#else, content, php_version)
    {
        return Some(s);
    }

    None
}

/// Pattern: `isset($x) ? $x : $default` → `$x ?? $default`
///
/// Only matches when `isset()` has exactly one argument, and that
/// argument's source text matches the then-branch's source text.
fn try_isset_coalescing(
    condition: &Expression<'_>,
    then_expr: &Expression<'_>,
    else_expr: &Expression<'_>,
    content: &str,
) -> Option<Simplification> {
    // Unwrap parentheses around the condition.
    let condition = unwrap_parens(condition);

    let isset = match condition {
        Expression::Construct(Construct::Isset(isset)) => isset,
        _ => return None,
    };

    // Only handle single-argument isset.
    let values: Vec<_> = isset.values.iter().collect();
    if values.len() != 1 {
        return None;
    }

    let isset_arg = values[0];
    let isset_arg_text = expr_source_text(isset_arg, content);
    let then_text = expr_source_text(then_expr, content);

    if isset_arg_text != then_text {
        return None;
    }

    let else_text = expr_source_text(else_expr, content);

    Some(Simplification::NullCoalescing {
        lhs: isset_arg_text.to_string(),
        rhs: else_text.to_string(),
    })
}

/// Patterns involving `$x !== null` / `$x === null` conditions.
fn try_null_comparison_simplify(
    condition: &Expression<'_>,
    then_expr: &Expression<'_>,
    else_expr: &Expression<'_>,
    content: &str,
    php_version: PhpVersion,
) -> Option<Simplification> {
    // Unwrap parentheses around the condition.
    let condition = unwrap_parens(condition);

    let bin = match condition {
        Expression::Binary(bin) => bin,
        _ => return None,
    };

    let (is_not_null, subject_expr) = match &bin.operator {
        BinaryOperator::NotIdentical(_) => {
            // $x !== null → subject is whichever side is not null
            if is_null_literal(bin.rhs) {
                (true, bin.lhs)
            } else if is_null_literal(bin.lhs) {
                (true, bin.rhs)
            } else {
                return None;
            }
        }
        BinaryOperator::Identical(_) => {
            // $x === null → subject is whichever side is not null
            if is_null_literal(bin.rhs) {
                (false, bin.lhs)
            } else if is_null_literal(bin.lhs) {
                (false, bin.rhs)
            } else {
                return None;
            }
        }
        _ => return None,
    };

    let subject_text = expr_source_text(subject_expr, content);

    if is_not_null {
        // $x !== null ? THEN : ELSE
        let then_text = expr_source_text(then_expr, content);

        // Pattern: $x !== null ? $x : $default → $x ?? $default
        if then_text == subject_text {
            let else_text = expr_source_text(else_expr, content);
            return Some(Simplification::NullCoalescing {
                lhs: subject_text.to_string(),
                rhs: else_text.to_string(),
            });
        }

        // Pattern: $x !== null ? $x->foo() : null → $x?->foo()
        // Pattern: $x !== null ? $x->foo : null → $x?->foo
        if is_null_literal(else_expr)
            && php_version >= PhpVersion::new(8, 0)
            && let Some(replacement) = try_nullsafe_replacement(then_expr, subject_text, content)
        {
            return Some(Simplification::NullSafe { replacement });
        }
    } else {
        // $x === null ? THEN : ELSE
        let else_text = expr_source_text(else_expr, content);

        // Pattern: $x === null ? $default : $x → $x ?? $default
        if else_text == subject_text {
            let then_text = expr_source_text(then_expr, content);
            return Some(Simplification::NullCoalescing {
                lhs: subject_text.to_string(),
                rhs: then_text.to_string(),
            });
        }

        // Pattern: $x === null ? null : $x->foo() → $x?->foo()
        if is_null_literal(then_expr)
            && php_version >= PhpVersion::new(8, 0)
            && let Some(replacement) = try_nullsafe_replacement(else_expr, subject_text, content)
        {
            return Some(Simplification::NullSafe { replacement });
        }
    }

    None
}

/// Try to rewrite `$x->foo()` or `$x->foo` as `$x?->foo()` / `$x?->foo`,
/// given that `subject_text` is the text of `$x`.
///
/// Handles both property access (`$x->foo`) and method calls (`$x->foo()`),
/// including chained calls (`$x->foo()->bar()`), where only the `->`
/// immediately after the subject is converted to `?->`.
fn try_nullsafe_replacement(
    expr: &Expression<'_>,
    subject_text: &str,
    content: &str,
) -> Option<String> {
    match expr {
        Expression::Call(Call::Method(mc)) => {
            let object_text = expr_source_text(mc.object, content);
            if object_text == subject_text {
                let full_text = expr_source_text(expr, content);
                return Some(replace_arrow_after_subject(full_text, subject_text));
            }
            // Check for chains: $x->foo()->bar() where root object is $x
            if let Some(root) = find_root_object(mc.object, content)
                && root == subject_text
            {
                let full_text = expr_source_text(expr, content);
                return Some(replace_arrow_after_subject(full_text, subject_text));
            }
            None
        }
        Expression::Access(Access::Property(pa)) => {
            let object_text = expr_source_text(pa.object, content);
            if object_text == subject_text {
                let full_text = expr_source_text(expr, content);
                return Some(replace_arrow_after_subject(full_text, subject_text));
            }
            if let Some(root) = find_root_object(pa.object, content)
                && root == subject_text
            {
                let full_text = expr_source_text(expr, content);
                return Some(replace_arrow_after_subject(full_text, subject_text));
            }
            None
        }
        _ => None,
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Extract the source text of an expression by its span.
fn expr_source_text<'c>(expr: &Expression<'_>, content: &'c str) -> &'c str {
    let span = expr.span();
    let start = span.start.offset as usize;
    let end = span.end.offset as usize;
    if end <= content.len() {
        content[start..end].trim()
    } else {
        ""
    }
}

/// Check if an expression is a `null` literal.
fn is_null_literal(expr: &Expression<'_>) -> bool {
    matches!(unwrap_parens(expr), Expression::Literal(Literal::Null(_)))
}

/// Unwrap any number of surrounding parentheses from an expression.
fn unwrap_parens<'a, 'b>(expr: &'b Expression<'a>) -> &'b Expression<'a> {
    match expr {
        Expression::Parenthesized(p) => unwrap_parens(p.expression),
        other => other,
    }
}

/// Find the root object of a member access chain.
///
/// E.g., for `$x->foo()->bar()->baz()`, returns the source text of `$x`.
fn find_root_object<'c>(expr: &Expression<'_>, content: &'c str) -> Option<&'c str> {
    match expr {
        Expression::Call(Call::Method(mc)) => Some(
            find_root_object(mc.object, content)
                .unwrap_or_else(|| expr_source_text(mc.object, content)),
        ),
        Expression::Access(Access::Property(pa)) => Some(
            find_root_object(pa.object, content)
                .unwrap_or_else(|| expr_source_text(pa.object, content)),
        ),
        Expression::Call(Call::NullSafeMethod(mc)) => Some(
            find_root_object(mc.object, content)
                .unwrap_or_else(|| expr_source_text(mc.object, content)),
        ),
        Expression::Access(Access::NullSafeProperty(pa)) => Some(
            find_root_object(pa.object, content)
                .unwrap_or_else(|| expr_source_text(pa.object, content)),
        ),
        _ => None,
    }
}

/// Replace the `->` that comes immediately after `subject` with `?->`.
///
/// For simple cases like `$x->foo()` with subject `$x`, the `->` right
/// after `$x` is converted.  For compound subjects like `$this->bar`,
/// the `->` that appears after the full subject text is converted, so
/// `$this->bar->getName()` becomes `$this->bar?->getName()` rather than
/// `$this?->bar->getName()`.
fn replace_arrow_after_subject(text: &str, subject: &str) -> String {
    // Find the `->` that comes after the subject text.
    if let Some(start) = text.find(subject) {
        let after_subject = start + subject.len();
        if let Some(arrow_offset) = text[after_subject..].find("->") {
            let pos = after_subject + arrow_offset;
            let mut result = String::with_capacity(text.len() + 1);
            result.push_str(&text[..pos]);
            result.push_str("?->");
            result.push_str(&text[pos + 2..]);
            return result;
        }
    }
    text.to_string()
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse PHP code and find a simplification at the given byte offset.
    fn find_simplification(
        php: &str,
        offset: u32,
        php_version: PhpVersion,
    ) -> Option<(Simplification, u32, u32)> {
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"input.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());

        let mut best: Option<(Simplification, u32, u32)> = None;
        for stmt in program.statements.iter() {
            find_in_statement(stmt, offset, php, php_version, &mut best);
        }
        best
    }

    fn php80() -> PhpVersion {
        PhpVersion::new(8, 0)
    }

    fn php74() -> PhpVersion {
        PhpVersion::new(7, 4)
    }

    /// Helper to get the replacement text for a simplification.
    fn replacement_text(s: &Simplification) -> String {
        match s {
            Simplification::NullCoalescing { lhs, rhs } => format!("{lhs} ?? {rhs}"),
            Simplification::NullSafe { replacement } => replacement.clone(),
        }
    }

    // ── isset patterns ──────────────────────────────────────────────────

    #[test]
    fn isset_ternary_to_coalescing() {
        let php = "<?php $result = isset($x) ? $x : 'default';";
        let offset = php.find("isset").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x ?? 'default'");
    }

    #[test]
    fn isset_ternary_array_access() {
        let php = "<?php $result = isset($data['key']) ? $data['key'] : null;";
        let offset = php.find("isset").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$data['key'] ?? null");
    }

    #[test]
    fn isset_ternary_no_match_when_then_differs() {
        let php = "<?php $result = isset($x) ? $y : 'default';";
        let offset = php.find("isset").unwrap() as u32;
        assert!(find_simplification(php, offset, php80()).is_none());
    }

    #[test]
    fn isset_multi_arg_not_simplified() {
        let php = "<?php $result = isset($x, $y) ? $x : 'default';";
        let offset = php.find("isset").unwrap() as u32;
        assert!(find_simplification(php, offset, php80()).is_none());
    }

    // ── $x !== null patterns ────────────────────────────────────────────

    #[test]
    fn not_identical_null_to_coalescing() {
        let php = "<?php $result = $x !== null ? $x : 'fallback';";
        let offset = php.find("$x !==").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x ?? 'fallback'");
    }

    #[test]
    fn null_not_identical_reversed_to_coalescing() {
        // null !== $x ? $x : 'fallback'
        let php = "<?php $result = null !== $x ? $x : 'fallback';";
        let offset = php.find("null !==").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x ?? 'fallback'");
    }

    // ── $x === null patterns ────────────────────────────────────────────

    #[test]
    fn identical_null_to_coalescing() {
        let php = "<?php $result = $x === null ? 'default' : $x;";
        let offset = php.find("$x ===").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x ?? 'default'");
    }

    #[test]
    fn null_identical_reversed_to_coalescing() {
        // null === $x ? 'default' : $x
        let php = "<?php $result = null === $x ? 'default' : $x;";
        let offset = php.find("null ===").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x ?? 'default'");
    }

    // ── null-safe patterns ──────────────────────────────────────────────

    #[test]
    fn not_null_method_call_to_nullsafe() {
        let php = "<?php $result = $x !== null ? $x->getName() : null;";
        let offset = php.find("$x !==").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x?->getName()");
    }

    #[test]
    fn not_null_property_access_to_nullsafe() {
        let php = "<?php $result = $x !== null ? $x->name : null;";
        let offset = php.find("$x !==").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x?->name");
    }

    #[test]
    fn identical_null_else_method_to_nullsafe() {
        // $x === null ? null : $x->getName()
        let php = "<?php $result = $x === null ? null : $x->getName();";
        let offset = php.find("$x ===").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x?->getName()");
    }

    #[test]
    fn nullsafe_not_offered_on_php74() {
        let php = "<?php $result = $x !== null ? $x->getName() : null;";
        let offset = php.find("$x !==").unwrap() as u32;
        // On PHP 7.4, ?-> doesn't exist, so no simplification.
        assert!(find_simplification(php, offset, php74()).is_none());
    }

    #[test]
    fn coalescing_still_offered_on_php74() {
        let php = "<?php $result = $x !== null ? $x : 'default';";
        let offset = php.find("$x !==").unwrap() as u32;
        // ?? exists in PHP 7.0+, so this should still work.
        let (s, _, _) = find_simplification(php, offset, php74()).unwrap();
        assert_eq!(replacement_text(&s), "$x ?? 'default'");
    }

    // ── Edge cases ──────────────────────────────────────────────────────

    #[test]
    fn no_simplification_for_unrelated_ternary() {
        let php = "<?php $result = $x > 0 ? 'positive' : 'negative';";
        let offset = php.find("$x >").unwrap() as u32;
        assert!(find_simplification(php, offset, php80()).is_none());
    }

    #[test]
    fn no_simplification_for_short_ternary() {
        let php = "<?php $result = $x ?: 'default';";
        let offset = php.find("$x ?:").unwrap() as u32;
        assert!(find_simplification(php, offset, php80()).is_none());
    }

    #[test]
    fn cursor_outside_ternary_no_match() {
        let php = "<?php $a = 1;\n$result = isset($x) ? $x : 'default';";
        // Cursor on `$a = 1`.
        let offset = php.find("$a").unwrap() as u32;
        assert!(find_simplification(php, offset, php80()).is_none());
    }

    #[test]
    fn nested_in_function() {
        let php = "<?php\nfunction foo() {\n    return isset($x) ? $x : 'default';\n}";
        let offset = php.find("isset").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x ?? 'default'");
    }

    #[test]
    fn nested_in_class_method() {
        let php = "<?php\nclass Foo {\n    public function bar() {\n        return $x !== null ? $x : 'default';\n    }\n}";
        let offset = php.find("$x !==").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x ?? 'default'");
    }

    #[test]
    fn parenthesized_condition() {
        let php = "<?php $result = ($x !== null) ? $x : 'default';";
        let offset = php.find("$x !==").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x ?? 'default'");
    }

    #[test]
    fn parenthesized_null() {
        let php = "<?php $result = $x !== (null) ? $x : 'default';";
        let offset = php.find("$x !==").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x ?? 'default'");
    }

    // ── replace_first_arrow helper ──────────────────────────────────────

    #[test]
    fn replace_arrow_simple() {
        assert_eq!(replace_arrow_after_subject("$x->foo()", "$x"), "$x?->foo()");
    }

    #[test]
    fn replace_arrow_property() {
        assert_eq!(replace_arrow_after_subject("$x->name", "$x"), "$x?->name");
    }

    #[test]
    fn replace_arrow_chain() {
        assert_eq!(
            replace_arrow_after_subject("$x->foo()->bar()", "$x"),
            "$x?->foo()->bar()"
        );
    }

    #[test]
    fn replace_arrow_compound_subject() {
        assert_eq!(
            replace_arrow_after_subject("$this->bar->getName()", "$this->bar"),
            "$this->bar?->getName()"
        );
    }

    #[test]
    fn replace_arrow_no_arrow() {
        assert_eq!(replace_arrow_after_subject("$x", "$x"), "$x");
    }

    // ── is_null_literal helper ──────────────────────────────────────────

    #[test]
    fn null_literal_detected() {
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"input.php");
        let php = "<?php $x = null;";
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
        let mut found = false;
        for stmt in program.statements.iter() {
            if let Statement::Expression(expr_stmt) = stmt
                && let Expression::Assignment(assign) = &expr_stmt.expression
            {
                assert!(is_null_literal(assign.rhs));
                found = true;
            }
        }
        assert!(found, "expected to find an assignment with null RHS");
    }
}
