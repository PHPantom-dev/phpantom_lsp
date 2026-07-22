#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use phpantom_lsp::Backend;
    use tower_lsp::lsp_types::*;

    fn collect(php: &str) -> Vec<Diagnostic> {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        // update_ast populates parse_errors
        backend.update_ast(uri, &Arc::new(php.to_string()));
        let mut out = Vec::new();
        backend.collect_syntax_error_diagnostics(uri, php, &mut out);
        out
    }

    #[test]
    fn no_errors_for_valid_php() {
        let php = r#"<?php
function greet(string $name): string {
    return "Hello, " . $name;
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Valid PHP should produce no syntax errors"
        );
    }

    #[test]
    fn error_for_unexpected_token() {
        let php = "<?php\nfunction { broken }\n";
        let diags = collect(php);
        assert!(
            !diags.is_empty(),
            "Should produce at least one syntax error"
        );
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
    }

    #[test]
    fn error_for_missing_semicolon() {
        let php = "<?php\n$x = 1\n$y = 2;\n";
        let diags = collect(php);
        assert!(
            !diags.is_empty(),
            "Missing semicolon should produce a syntax error"
        );
    }

    #[test]
    fn error_has_correct_code_and_source() {
        let php = "<?php\nfunction { broken }\n";
        let diags = collect(php);
        assert!(!diags.is_empty());
        assert_eq!(
            diags[0].code,
            Some(NumberOrString::String("syntax_error".to_string()))
        );
        assert_eq!(diags[0].source, Some("phpantom".to_string()));
    }

    #[test]
    fn error_has_nonempty_message() {
        let php = "<?php\nfunction { broken }\n";
        let diags = collect(php);
        assert!(!diags.is_empty());
        assert!(
            !diags[0].message.is_empty(),
            "Syntax error should have a descriptive message"
        );
    }

    #[test]
    fn error_range_is_on_correct_line() {
        // The error is on line 1 (0-indexed), because `function {` is on line 1.
        let php = "<?php\nfunction { broken }\n";
        let diags = collect(php);
        assert!(!diags.is_empty());
        // The error should be on line 1 or later (not line 0 which is `<?php`).
        assert!(
            diags[0].range.start.line >= 1,
            "Error should be on line 1 or later, got line {}",
            diags[0].range.start.line
        );
    }

    #[test]
    fn multiple_errors_reported() {
        let php = "<?php\nfunction { }\nclass { }\n";
        let diags = collect(php);
        // Should have at least 2 errors (one per broken declaration).
        assert!(
            diags.len() >= 2,
            "Expected at least 2 syntax errors, got {}",
            diags.len()
        );
    }

    #[test]
    fn valid_class_produces_no_errors() {
        let php = r#"<?php
class Foo {
    public function bar(): void {}
}
"#;
        let diags = collect(php);
        assert!(diags.is_empty());
    }

    #[test]
    fn unclosed_string_produces_error() {
        let php = "<?php\n$x = \"unclosed string\n";
        let diags = collect(php);
        assert!(
            !diags.is_empty(),
            "Unclosed string should produce a syntax error"
        );
    }
}
