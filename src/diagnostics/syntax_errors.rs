//! Syntax error diagnostic.
//!
//! Surfaces parse errors from the Mago parser as LSP diagnostics.
//! This is the most fundamental diagnostic any language server can
//! provide: without it, a user with a typo like `function { broken`
//! gets no feedback until they try to run the code.
//!
//! Parse errors are cached per file during `update_ast` (see
//! `parser/ast_update.rs`) as `(message, start_byte, end_byte)` tuples.
//! This collector simply reads the cache and converts each entry to an
//! LSP `Diagnostic` with Error severity.

use tower_lsp::lsp_types::*;

use crate::Backend;

impl Backend {
    /// Collect syntax-error diagnostics for a single file.
    ///
    /// Reads cached parse errors from `self.parse_errors` (populated
    /// during `update_ast`) and converts them to LSP diagnostics.
    /// Appends to `out`; the caller is responsible for publishing or
    /// returning them.
    pub fn collect_syntax_error_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let errors = {
            let map = self.parse_errors.read();
            match map.get(uri) {
                Some(errs) => errs.clone(),
                None => return,
            }
        };

        for (message, start_byte, end_byte) in &errors {
            let range = if *start_byte == 0 && *end_byte == 0 {
                // Fallback range (e.g. parser panic) — use line 0, col 0.
                Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 0,
                    },
                }
            } else {
                match self.offset_range_to_lsp_range(
                    uri,
                    content,
                    *start_byte as usize,
                    *end_byte as usize,
                ) {
                    Some(r) => r,
                    None => {
                        // If the offset conversion fails (e.g. offset
                        // past end of file after an edit), place the
                        // diagnostic at (0,0).
                        Range {
                            start: Position {
                                line: 0,
                                character: 0,
                            },
                            end: Position {
                                line: 0,
                                character: 0,
                            },
                        }
                    }
                }
            };

            out.push(Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::ERROR),
                code: Some(NumberOrString::String("syntax_error".to_string())),
                code_description: None,
                source: Some("phpantom".to_string()),
                message: message.clone(),
                related_information: None,
                tags: None,
                data: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tower_lsp::lsp_types::*;

    use crate::Backend;

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

    #[test]
    fn parser_panic_produces_fallback_diagnostic() {
        // Simulate a parser panic by inserting a known entry into parse_errors.
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\n";
        {
            let mut errors = backend.parse_errors.write();
            errors.insert(
                uri.to_string(),
                vec![("Parse failed (internal error)".to_string(), 0, 0)],
            );
        }
        let mut out = Vec::new();
        backend.collect_syntax_error_diagnostics(uri, content, &mut out);
        assert_eq!(out.len(), 1);
        assert!(out[0].message.contains("Parse failed"));
        assert_eq!(out[0].range.start.line, 0);
        assert_eq!(out[0].range.start.character, 0);
    }

    #[test]
    fn clear_file_maps_prunes_parse_errors() {
        // A file with a syntax error populates parse_errors; closing the
        // file (which calls clear_file_maps) must drop the entry so the
        // map does not grow for the whole session.
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        backend.update_ast(uri, &Arc::new("<?php\n$x = \"unclosed\n".to_string()));
        assert!(
            backend.parse_errors.read().contains_key(uri),
            "parse errors should be recorded after update_ast"
        );

        backend.clear_file_maps(uri);
        assert!(
            !backend.parse_errors.read().contains_key(uri),
            "clear_file_maps should remove the file's parse-error entry"
        );
    }
}
