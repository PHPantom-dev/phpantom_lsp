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

use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::code_actions::cursor_context::innermost_at;
use crate::text_position::position_to_byte_offset;

impl Backend {
    /// Collect "Convert to arrow function" code actions for closures at the
    /// cursor position.
    pub(crate) fn collect_convert_to_arrow_function_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionResponse>,
    ) {
        let php_version = self.php_version();
        if php_version < crate::types::PhpVersion::new(7, 4) {
            return;
        }

        let doc_uri: Uri = match uri.parse() {
            Ok(u) => u,
            Err(_) => return,
        };

        let cursor_offset = position_to_byte_offset(content, params.range.start) as u32;

        let found = crate::parser::with_parsed_program(
            content,
            "convert_to_arrow_function",
            |program, content| convertible_closure_at(program, cursor_offset, content),
        );
        let Some((span, replacement)) = found else {
            return;
        };

        out.push(crate::code_actions::rewrite_action(
            "Convert to arrow function".to_string(),
            doc_uri,
            content,
            (span.start.offset as usize, span.end.offset as usize),
            replacement,
            false,
        ));
    }
}

/// The innermost closure under `cursor` that can become an arrow
/// function, with its span and the arrow function's text.
fn convertible_closure_at(
    program: &Program<'_>,
    cursor: u32,
    content: &str,
) -> Option<(mago_span::Span, String)> {
    innermost_at(program, cursor, |node| match node {
        Node::Closure(closure) => Some((closure.span(), try_convert_closure(closure, content)?)),
        _ => None,
    })
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

    if closure.r#static.is_some() {
        result.push_str("static ");
    }

    result.push_str("fn");
    let params_text = source_text(content, closure.parameter_list.span());
    result.push_str(params_text);

    if let Some(return_type) = &closure.return_type_hint {
        let hint_text = source_text(content, return_type.span());
        result.push_str(hint_text);
    }

    result.push_str(" => ");
    let expr_text = source_text(content, return_expr.span());
    result.push_str(expr_text);

    Some(result)
}

fn source_text(content: &str, span: mago_span::Span) -> &str {
    &content[span.start.offset as usize..span.end.offset as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_conversion(php: &str) -> Option<String> {
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"input.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());

        // Place cursor inside the closure (after `function`).
        let cursor = php.find("function").unwrap_or(0) as u32 + 1;
        convertible_closure_at(program, cursor, php).map(|(_, replacement)| replacement)
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

    #[test]
    fn closure_in_else_branch() {
        let php = r#"<?php if ($a) { foo(); } else { $f = function($x) { return $x; }; }"#;
        assert_eq!(find_conversion(php).as_deref(), Some("fn($x) => $x"));
    }

    #[test]
    fn closure_in_match_arm() {
        let php = r#"<?php $f = match ($a) { 1 => function($x) { return $x; }, default => null };"#;
        assert_eq!(find_conversion(php).as_deref(), Some("fn($x) => $x"));
    }

    #[test]
    fn innermost_convertible_closure_wins() {
        let php = r#"<?php $f = function() { return function($x) { return $x; }; };"#;
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"input.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
        let cursor = php.rfind("function").unwrap() as u32 + 1;
        let (_, replacement) = convertible_closure_at(program, cursor, php).unwrap();
        assert_eq!(replacement, "fn($x) => $x");
    }
}
