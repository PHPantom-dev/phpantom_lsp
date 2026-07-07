//! **Convert to arrow function** code action (`refactor.rewrite`).
//!
//! Converts a single-expression anonymous function (closure) to an arrow
//! function: `function($x) { return $x * 2; }` → `fn($x) => $x * 2`.
//!
//! The action is only offered when:
//! - The closure body contains exactly one statement: a `return` with an expression.
//! - The closure does not have a `use()` clause that captures by reference.
//! - The closure does not have a `void` or `never` return type hint.
//! - PHP version is >= 7.4.

use std::collections::HashMap;

use mago_span::HasSpan;
use mago_syntax::ast::access::Access;
use mago_syntax::ast::call::Call;
use mago_syntax::ast::*;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::util::{offset_to_position, position_to_byte_offset};

impl Backend {
    /// Collect "Convert to arrow function" code actions for closures at the
    /// cursor position.
    pub(crate) fn collect_convert_to_arrow_function_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let php_version = self.php_version();
        if php_version < crate::types::PhpVersion::new(7, 4) {
            return;
        }

        let doc_uri: Url = match uri.parse() {
            Ok(u) => u,
            Err(_) => return,
        };

        let cursor_offset = position_to_byte_offset(content, params.range.start) as u32;

        let best = crate::parser::with_parsed_program(
            content,
            "convert_to_arrow_function",
            |program, content| {
                let mut best: Option<(u32, u32, String)> = None;
                for stmt in program.statements.iter() {
                    find_in_statement(stmt, cursor_offset, content, &mut best);
                }
                best
            },
        );

        let (closure_start, closure_end, replacement) = match best {
            Some(b) => b,
            None => return,
        };

        let start_pos = offset_to_position(content, closure_start as usize);
        let end_pos = offset_to_position(content, closure_end as usize);

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
            title: "Convert to arrow function".to_string(),
            kind: Some(CodeActionKind::new("refactor.rewrite")),
            diagnostics: None,
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                document_changes: None,
                change_annotations: None,
            }),
            command: None,
            is_preferred: Some(false),
            disabled: None,
            data: None,
        }));
    }
}

// ─── AST walking ────────────────────────────────────────────────────────────

/// Search for a convertible closure at the cursor position within a statement.
fn find_in_statement(
    stmt: &Statement<'_>,
    cursor: u32,
    content: &str,
    best: &mut Option<(u32, u32, String)>,
) {
    let span = stmt.span();
    if cursor < span.start.offset || cursor > span.end.offset {
        return;
    }

    match stmt {
        Statement::Expression(expr_stmt) => {
            find_in_expression(expr_stmt.expression, cursor, content, best);
        }
        Statement::Return(ret) => {
            if let Some(expr) = &ret.value {
                find_in_expression(expr, cursor, content, best);
            }
        }
        Statement::Echo(echo) => {
            for expr in echo.values.iter() {
                find_in_expression(expr, cursor, content, best);
            }
        }
        Statement::Block(block) => {
            for s in block.statements.iter() {
                find_in_statement(s, cursor, content, best);
            }
        }
        Statement::Namespace(ns) => {
            for s in ns.statements().iter() {
                find_in_statement(s, cursor, content, best);
            }
        }
        Statement::Class(class) => {
            for member in class.members.iter() {
                find_in_member(member, cursor, content, best);
            }
        }
        Statement::Trait(tr) => {
            for member in tr.members.iter() {
                find_in_member(member, cursor, content, best);
            }
        }
        Statement::Enum(en) => {
            for member in en.members.iter() {
                find_in_member(member, cursor, content, best);
            }
        }
        Statement::Interface(iface) => {
            for member in iface.members.iter() {
                find_in_member(member, cursor, content, best);
            }
        }
        Statement::Function(func) => {
            for s in func.body.statements.iter() {
                find_in_statement(s, cursor, content, best);
            }
        }
        Statement::If(if_stmt) => {
            find_in_expression(if_stmt.condition, cursor, content, best);
            for s in if_stmt.body.statements().iter() {
                find_in_statement(s, cursor, content, best);
            }
        }
        Statement::While(w) => {
            find_in_expression(w.condition, cursor, content, best);
            for s in w.body.statements().iter() {
                find_in_statement(s, cursor, content, best);
            }
        }
        Statement::DoWhile(dw) => {
            find_in_expression(dw.condition, cursor, content, best);
            find_in_statement(dw.statement, cursor, content, best);
        }
        Statement::For(f) => {
            for expr in f.initializations.iter() {
                find_in_expression(expr, cursor, content, best);
            }
            for expr in f.conditions.iter() {
                find_in_expression(expr, cursor, content, best);
            }
            for expr in f.increments.iter() {
                find_in_expression(expr, cursor, content, best);
            }
            for s in f.body.statements().iter() {
                find_in_statement(s, cursor, content, best);
            }
        }
        Statement::Foreach(fe) => {
            find_in_expression(fe.expression, cursor, content, best);
            for s in fe.body.statements().iter() {
                find_in_statement(s, cursor, content, best);
            }
        }
        Statement::Switch(sw) => {
            find_in_expression(sw.expression, cursor, content, best);
            for case in sw.body.cases().iter() {
                let stmts = match case {
                    SwitchCase::Expression(e) => &e.statements,
                    SwitchCase::Default(d) => &d.statements,
                };
                for s in stmts.iter() {
                    find_in_statement(s, cursor, content, best);
                }
            }
        }
        Statement::Try(tr) => {
            for s in tr.block.statements.iter() {
                find_in_statement(s, cursor, content, best);
            }
            for catch in tr.catch_clauses.iter() {
                for s in catch.block.statements.iter() {
                    find_in_statement(s, cursor, content, best);
                }
            }
            if let Some(finally) = &tr.finally_clause {
                for s in finally.block.statements.iter() {
                    find_in_statement(s, cursor, content, best);
                }
            }
        }
        _ => {}
    }
}

/// Walk class-like members to find closures inside method bodies.
fn find_in_member(
    member: &ClassLikeMember<'_>,
    cursor: u32,
    content: &str,
    best: &mut Option<(u32, u32, String)>,
) {
    match member {
        ClassLikeMember::Method(method) => {
            if let MethodBody::Concrete(body) = &method.body {
                for s in body.statements.iter() {
                    find_in_statement(s, cursor, content, best);
                }
            }
        }
        ClassLikeMember::Property(prop) => match prop {
            Property::Plain(plain) => {
                for item in plain.items.iter() {
                    if let class_like::property::PropertyItem::Concrete(concrete) = item {
                        find_in_expression(concrete.value, cursor, content, best);
                    }
                }
            }
            Property::Hooked(hooked) => {
                if let class_like::property::PropertyItem::Concrete(concrete) = &hooked.item {
                    find_in_expression(concrete.value, cursor, content, best);
                }
            }
        },
        ClassLikeMember::Constant(constant) => {
            for item in constant.items.iter() {
                find_in_expression(item.value, cursor, content, best);
            }
        }
        _ => {}
    }
}

/// Walk an expression, checking if it's a convertible closure or recursing.
fn find_in_expression(
    expr: &Expression<'_>,
    cursor: u32,
    content: &str,
    best: &mut Option<(u32, u32, String)>,
) {
    let span = expr.span();
    if cursor < span.start.offset || cursor > span.end.offset {
        return;
    }

    // Check if this expression IS a closure we can convert.
    if let Expression::Closure(closure) = expr
        && let Some(replacement) = try_convert_closure(closure, content)
    {
        let start = span.start.offset;
        let end = span.end.offset;
        if best.is_none() || (end - start) < (best.as_ref().unwrap().1 - best.as_ref().unwrap().0) {
            *best = Some((start, end, replacement));
        }
    }

    // Recurse into sub-expressions.
    match expr {
        Expression::Parenthesized(p) => find_in_expression(p.expression, cursor, content, best),
        Expression::UnaryPrefix(u) => find_in_expression(u.operand, cursor, content, best),
        Expression::UnaryPostfix(u) => find_in_expression(u.operand, cursor, content, best),
        Expression::Binary(b) => {
            find_in_expression(b.lhs, cursor, content, best);
            find_in_expression(b.rhs, cursor, content, best);
        }
        Expression::Assignment(a) => {
            find_in_expression(a.lhs, cursor, content, best);
            find_in_expression(a.rhs, cursor, content, best);
        }
        Expression::Conditional(c) => {
            find_in_expression(c.condition, cursor, content, best);
            if let Some(then) = c.then {
                find_in_expression(then, cursor, content, best);
            }
            find_in_expression(c.r#else, cursor, content, best);
        }
        Expression::Call(call) => match call {
            Call::Function(fc) => {
                find_in_expression(fc.function, cursor, content, best);
                for arg in fc.argument_list.arguments.iter() {
                    find_in_expression(arg.value(), cursor, content, best);
                }
            }
            Call::Method(mc) => {
                find_in_expression(mc.object, cursor, content, best);
                for arg in mc.argument_list.arguments.iter() {
                    find_in_expression(arg.value(), cursor, content, best);
                }
            }
            Call::NullSafeMethod(mc) => {
                find_in_expression(mc.object, cursor, content, best);
                for arg in mc.argument_list.arguments.iter() {
                    find_in_expression(arg.value(), cursor, content, best);
                }
            }
            Call::StaticMethod(sc) => {
                find_in_expression(sc.class, cursor, content, best);
                for arg in sc.argument_list.arguments.iter() {
                    find_in_expression(arg.value(), cursor, content, best);
                }
            }
        },
        Expression::Access(access) => match access {
            Access::Property(pa) => find_in_expression(pa.object, cursor, content, best),
            Access::NullSafeProperty(pa) => find_in_expression(pa.object, cursor, content, best),
            Access::StaticProperty(pa) => find_in_expression(pa.class, cursor, content, best),
            Access::ClassConstant(pa) => find_in_expression(pa.class, cursor, content, best),
        },
        Expression::Closure(closure) => {
            for s in closure.body.statements.iter() {
                find_in_statement(s, cursor, content, best);
            }
        }
        Expression::ArrowFunction(af) => {
            find_in_expression(af.expression, cursor, content, best);
        }
        Expression::Array(arr) => {
            for element in arr.elements.iter() {
                if let array::ArrayElement::KeyValue(kv) = element {
                    find_in_expression(kv.key, cursor, content, best);
                    find_in_expression(kv.value, cursor, content, best);
                } else if let array::ArrayElement::Value(val) = element {
                    find_in_expression(val.value, cursor, content, best);
                }
            }
        }
        Expression::LegacyArray(arr) => {
            for element in arr.elements.iter() {
                if let array::ArrayElement::KeyValue(kv) = element {
                    find_in_expression(kv.key, cursor, content, best);
                    find_in_expression(kv.value, cursor, content, best);
                } else if let array::ArrayElement::Value(val) = element {
                    find_in_expression(val.value, cursor, content, best);
                }
            }
        }
        Expression::ArrayAccess(aa) => {
            find_in_expression(aa.array, cursor, content, best);
            find_in_expression(aa.index, cursor, content, best);
        }
        Expression::Instantiation(inst) => {
            if let Some(arg_list) = &inst.argument_list {
                for a in arg_list.arguments.iter() {
                    find_in_expression(a.value(), cursor, content, best);
                }
            }
        }
        _ => {}
    }
}

// ─── Conversion logic ───────────────────────────────────────────────────────

/// Try to convert a closure to an arrow function. Returns the replacement text
/// if the closure is eligible.
fn try_convert_closure(
    closure: &function_like::closure::Closure<'_>,
    content: &str,
) -> Option<String> {
    // Must have exactly one statement: `return expr;`
    if closure.body.statements.len() != 1 {
        return None;
    }

    let stmt = closure.body.statements.first()?;
    let return_expr = match stmt {
        Statement::Return(ret) => ret.value.as_ref()?,
        _ => return None,
    };

    // Must not have by-reference captures in use clause.
    if let Some(use_clause) = &closure.use_clause {
        for var in use_clause.variables.iter() {
            if var.ampersand.is_some() {
                return None;
            }
        }
    }

    // Must not have void/never return type hint.
    if let Some(return_type) = &closure.return_type_hint {
        let hint_text = source_text(content, return_type.hint.span());
        let lower = hint_text.trim().to_lowercase();
        if lower == "void" || lower == "never" {
            return None;
        }
    }

    // Build the arrow function text.
    let mut result = String::new();

    // Static keyword
    if closure.r#static.is_some() {
        result.push_str("static ");
    }

    // fn keyword + parameters
    result.push_str("fn");
    let params_text = source_text(content, closure.parameter_list.span());
    result.push_str(params_text);

    // Return type hint (if any)
    if let Some(return_type) = &closure.return_type_hint {
        let hint_text = source_text(content, return_type.span());
        result.push_str(hint_text);
    }

    // Arrow and expression
    result.push_str(" => ");
    let expr_text = source_text(content, return_expr.span());
    result.push_str(expr_text);

    Some(result)
}

/// Extract a slice of the source text corresponding to a span.
fn source_text(content: &str, span: mago_span::Span) -> &str {
    &content[span.start.offset as usize..span.end.offset as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_conversion(php: &str) -> Option<String> {
        let arena = bumpalo::Bump::new();
        let file_id = mago_database::file::FileId::new(b"input.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());

        let mut best: Option<(u32, u32, String)> = None;
        // Place cursor inside the closure (after `function`).
        let cursor = php.find("function").unwrap_or(0) as u32 + 1;
        for stmt in program.statements.iter() {
            find_in_statement(stmt, cursor, php, &mut best);
        }
        best.map(|(_, _, replacement)| replacement)
    }

    #[test]
    fn simple_return() {
        let php = r#"<?php $f = function($x) { return $x * 2; };"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, "fn($x) => $x * 2");
    }

    #[test]
    fn with_type_hints() {
        let php = r#"<?php $f = function(int $x): int { return $x * 2; };"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, "fn(int $x): int => $x * 2");
    }

    #[test]
    fn static_closure() {
        let php = r#"<?php $f = static function($x) { return $x + 1; };"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, "static fn($x) => $x + 1");
    }

    #[test]
    fn with_use_clause_no_ref() {
        let php = r#"<?php $f = function($x) use ($y) { return $x + $y; };"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, "fn($x) => $x + $y");
    }

    #[test]
    fn rejected_by_ref_use() {
        let php = r#"<?php $f = function($x) use (&$y) { return $x; };"#;
        assert!(find_conversion(php).is_none());
    }

    #[test]
    fn rejected_void_return() {
        let php = r#"<?php $f = function($x): void { return; };"#;
        assert!(find_conversion(php).is_none());
    }

    #[test]
    fn rejected_never_return() {
        let php = r#"<?php $f = function(): never { return throw new \Exception(); };"#;
        assert!(find_conversion(php).is_none());
    }

    #[test]
    fn rejected_multiple_statements() {
        let php = r#"<?php $f = function($x) { $y = $x + 1; return $y; };"#;
        assert!(find_conversion(php).is_none());
    }

    #[test]
    fn rejected_no_return_value() {
        let php = r#"<?php $f = function() { return; };"#;
        assert!(find_conversion(php).is_none());
    }
}
