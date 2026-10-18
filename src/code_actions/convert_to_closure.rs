//! **Convert to closure** code action (`refactor.rewrite`).
//!
//! Converts an arrow function to an anonymous function (closure):
//! `fn($x) => $x * 2` -> `function($x) { return $x * 2; }`.
//!
//! Variables from the outer scope that are used in the expression are
//! captured via a `use()` clause, since closures (unlike arrow functions)
//! do not auto-capture.
//!
//! The action is always offered when the cursor is on an arrow function.

use std::collections::HashSet;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::code_actions::cursor_context::innermost_at;
use crate::scope_collector::AUTO_GLOBALS;
use crate::text_position::position_to_byte_offset;

impl Backend {
    /// Collect "Convert to closure" code actions for arrow functions at the
    /// cursor position.
    pub(crate) fn collect_convert_to_closure_actions(
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

        let found = crate::parser::with_parsed_program(
            content,
            "convert_to_closure",
            |program, content| arrow_function_at(program, cursor_offset, content),
        );
        let Some((span, replacement)) = found else {
            return;
        };

        out.push(crate::code_actions::rewrite_action(
            "Convert to closure".to_string(),
            doc_uri,
            content,
            (span.start.offset as usize, span.end.offset as usize),
            replacement,
            false,
        ));
    }
}

/// The innermost arrow function under `cursor`, with its span and the
/// closure's text.
fn arrow_function_at(
    program: &Program<'_>,
    cursor: u32,
    content: &str,
) -> Option<(mago_span::Span, String)> {
    innermost_at(program, cursor, |node| match node {
        Node::ArrowFunction(af) => Some((af.span(), closure_text(af, content))),
        _ => None,
    })
}

// ─── Conversion logic ───────────────────────────────────────────────────────

/// The closure text an arrow function converts to.
fn closure_text(af: &function_like::arrow_function::ArrowFunction<'_>, content: &str) -> String {
    // The variables the body reads from the enclosing scope, which a
    // closure has to capture with `use()`.
    let params = parameter_names(&af.parameter_list, content).collect();
    let mut captured: Vec<String> = Vec::new();
    collect_outer_variables(
        Node::Expression(af.expression),
        content,
        &params,
        &mut captured,
    );
    // Deduplicate while preserving order.
    let mut seen = HashSet::new();
    captured.retain(|v| seen.insert(v.clone()));

    let mut result = String::new();

    if af.r#static.is_some() {
        result.push_str("static ");
    }

    result.push_str("function");
    let params_text = source_text(content, af.parameter_list.span());
    result.push_str(params_text);

    if !captured.is_empty() {
        result.push_str(" use (");
        result.push_str(&captured.join(", "));
        result.push(')');
    }

    if let Some(return_type) = &af.return_type_hint {
        let hint_text = source_text(content, return_type.span());
        result.push_str(hint_text);
    }

    result.push_str(" { return ");
    let expr_text = source_text(content, af.expression.span());
    result.push_str(expr_text);
    result.push_str("; }");

    result
}

/// Push every variable `node` reads from the scope it is written in,
/// leaving out `$this`, the auto-globals and `params`.
///
/// A nested arrow function reads that scope too, less its own parameters,
/// while a nested closure reads only what its `use` clause names and an
/// anonymous class only its constructor arguments.  The `$name` in
/// `Foo::$name` is a static property, not a variable.
fn collect_outer_variables(
    node: Node<'_, '_>,
    content: &str,
    params: &HashSet<String>,
    out: &mut Vec<String>,
) {
    match node {
        Node::DirectVariable(variable) => {
            let name = source_text(content, variable.span());
            if name != "$this" && !AUTO_GLOBALS.contains(&name) && !params.contains(name) {
                out.push(name.to_string());
            }
        }
        Node::ArrowFunction(af) => {
            let mut inner = params.clone();
            inner.extend(parameter_names(&af.parameter_list, content));
            collect_outer_variables(Node::Expression(af.expression), content, &inner, out);
        }
        Node::Closure(closure) => {
            if let Some(use_clause) = &closure.use_clause {
                collect_outer_variables(Node::ClosureUseClause(use_clause), content, params, out);
            }
        }
        Node::AnonymousClass(class) => {
            if let Some(arguments) = &class.argument_list {
                collect_outer_variables(Node::PartialArgumentList(arguments), content, params, out);
            }
        }
        Node::StaticPropertyAccess(access) => {
            collect_outer_variables(Node::Expression(access.class), content, params, out);
        }
        _ => node.visit_children(|child| collect_outer_variables(child, content, params, out)),
    }
}

/// The names of a parameter list's parameters, `$` included.
fn parameter_names<'a>(
    list: &'a FunctionLikeParameterList<'_>,
    content: &'a str,
) -> impl Iterator<Item = String> + 'a {
    list.parameters
        .iter()
        .map(|p| source_text(content, p.variable.span()).to_string())
}

/// Extract a slice of the source text corresponding to a span.
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

        // Place cursor inside the arrow function (after `fn`).
        let cursor = php.find("fn").unwrap_or(0) as u32 + 1;
        arrow_function_at(program, cursor, php).map(|(_, replacement)| replacement)
    }

    #[test]
    fn simple_expression() {
        let php = r#"<?php $f = fn($x) => $x * 2;"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, "function($x) { return $x * 2; }");
    }

    #[test]
    fn with_type_hints() {
        let php = r#"<?php $f = fn(int $x): int => $x * 2;"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, "function(int $x): int { return $x * 2; }");
    }

    #[test]
    fn static_arrow() {
        let php = r#"<?php $f = static fn($x) => $x + 1;"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, "static function($x) { return $x + 1; }");
    }

    #[test]
    fn captures_outer_variable() {
        let php = r#"<?php $f = fn($x) => $x + $y;"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, "function($x) use ($y) { return $x + $y; }");
    }

    #[test]
    fn captures_multiple_variables() {
        let php = r#"<?php $f = fn($x) => $x + $y + $z;"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, "function($x) use ($y, $z) { return $x + $y + $z; }");
    }

    #[test]
    fn no_capture_for_this() {
        let php = r#"<?php $f = fn() => $this->name;"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, "function() { return $this->name; }");
    }

    #[test]
    fn no_capture_for_params() {
        let php = r#"<?php $f = fn($a, $b) => $a + $b;"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, "function($a, $b) { return $a + $b; }");
    }

    #[test]
    fn no_params_no_captures() {
        let php = r#"<?php $f = fn() => 42;"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, "function() { return 42; }");
    }

    #[test]
    fn with_method_call() {
        let php = r#"<?php $f = fn($item) => $formatter->format($item);"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(
            result,
            "function($item) use ($formatter) { return $formatter->format($item); }"
        );
    }

    #[test]
    fn deduplicates_captures() {
        let php = r#"<?php $f = fn($x) => $y + $y;"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, "function($x) use ($y) { return $y + $y; }");
    }

    #[test]
    fn captures_variable_in_string_interpolation() {
        let php = r#"<?php $f = fn($x) => "value: $y";"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, r#"function($x) use ($y) { return "value: $y"; }"#);
    }

    #[test]
    fn captures_variable_in_braced_string_interpolation() {
        let php = r#"<?php $f = fn($x) => "value: {$y->prop}";"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(
            result,
            r#"function($x) use ($y) { return "value: {$y->prop}"; }"#
        );
    }

    #[test]
    fn captures_variable_in_match_expression() {
        let php = r#"<?php $f = fn($x) => match($x) { 1 => $y, default => $z };"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(
            result,
            "function($x) use ($y, $z) { return match($x) { 1 => $y, default => $z }; }"
        );
    }

    #[test]
    fn captures_variable_in_isset() {
        let php = r#"<?php $f = fn($x) => isset($y) ? $y : $x;"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(
            result,
            "function($x) use ($y) { return isset($y) ? $y : $x; }"
        );
    }

    #[test]
    fn captures_variable_in_throw() {
        let php = r#"<?php $f = fn($x) => throw new Exception($y);"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(
            result,
            "function($x) use ($y) { return throw new Exception($y); }"
        );
    }

    #[test]
    fn captures_variable_used_as_dynamic_class_name() {
        let php = r#"<?php $f = fn($x) => new $className($x);"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(
            result,
            "function($x) use ($className) { return new $className($x); }"
        );
    }

    #[test]
    fn captures_what_a_nested_closure_uses() {
        let php = r#"<?php $f = fn($x) => function() use ($y) { return $y + $z; };"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(
            result,
            "function($x) use ($y) { return function() use ($y) { return $y + $z; }; }"
        );
    }

    #[test]
    fn captures_spread_and_dynamic_member_names() {
        let php = r#"<?php $f = fn($o) => [...$items, $o->$prop, $o->$method()];"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(
            result,
            "function($o) use ($items, $prop, $method) { return [...$items, $o->$prop, $o->$method()]; }"
        );
    }

    #[test]
    fn static_property_name_is_not_captured() {
        let php = r#"<?php $f = fn() => Registry::$items[$key];"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(
            result,
            "function() use ($key) { return Registry::$items[$key]; }"
        );
    }

    #[test]
    fn auto_globals_are_not_captured() {
        let php = r#"<?php $f = fn($k) => $_GET[$k] ?? $GLOBALS[$k] ?? $argv[0];"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(
            result,
            "function($k) use ($argv) { return $_GET[$k] ?? $GLOBALS[$k] ?? $argv[0]; }"
        );
    }

    #[test]
    fn arrow_function_in_else_branch() {
        let php = r#"<?php if ($a) { foo(); } else { $f = fn($x) => $x; }"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, "function($x) { return $x; }");
    }
}
