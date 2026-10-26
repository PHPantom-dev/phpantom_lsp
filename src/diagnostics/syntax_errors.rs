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

use tower_lsp_server::gen_lsp_types::*;

use super::helpers::make_diagnostic;

use crate::Backend;

impl Backend {
    /// Collect syntax-error diagnostics for a single file.
    ///
    /// Reads the parse errors `update_ast` recorded in the file's state and
    /// converts them to LSP diagnostics.
    /// Appends to `out`; the caller is responsible for publishing or
    /// returning them.
    pub fn collect_syntax_error_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let Some(state) = self.file_state(uri) else {
            return;
        };
        let errors = &state.parse_errors;

        for (message, start_byte, end_byte) in errors {
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

            out.push(make_diagnostic(
                range,
                DiagnosticSeverity::Error,
                "syntax_error",
                message.clone(),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::Backend;

    #[test]
    fn parser_panic_produces_fallback_diagnostic() {
        // Simulate a parser panic by recording the error it leaves.
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\n";
        backend.publish_parse_failure(
            uri,
            vec![("Parse failed (internal error)".to_string(), 0, 0)],
        );
        let mut out = Vec::new();
        backend.collect_syntax_error_diagnostics(uri, content, &mut out);
        assert_eq!(out.len(), 1);
        assert!(crate::util::message_text(&out[0].message).contains("Parse failed"));
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
            backend
                .file_state(uri)
                .is_some_and(|state| !state.parse_errors.is_empty()),
            "parse errors should be recorded after update_ast"
        );

        backend.clear_file_maps(uri);
        assert!(
            backend.file_state(uri).is_none(),
            "clear_file_maps should remove the file's parse-error entry"
        );
    }
}
