//! "Add @throws" code action for PHPStan `missingType.checkedException`.
//!
//! When PHPStan reports that a method or function throws a checked
//! exception that is not documented in the `@throws` tag, this code
//! action offers to:
//!
//! 1. Insert a `@throws ShortName` tag into the existing docblock
//!    (or create a new docblock if none exists).
//! 2. Add a `use FQN;` import statement when the exception class is
//!    not already imported.
//!
//! **Trigger:** A PHPStan diagnostic with identifier
//! `missingType.checkedException` overlaps the cursor.
//!
//! **Code action kind:** `quickfix`.
//!
//! ## Two-phase resolve
//!
//! Phase 1 (`collect_add_throws_actions`) performs all validation and
//! emits a lightweight `CodeAction` with a `data` payload but no `edit`.
//! Phase 2 (`resolve_add_throws`) recomputes the workspace edit on
//! demand when the user picks the action.

use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::code_actions::cursor_context::{FunctionKinds, find_enclosing_function};
use crate::code_actions::phpstan::phpstan_identifier;
use crate::code_actions::{
    CodeActionData, FunctionDocblock, deferred_quickfix, function_docblock_at_line,
};
use crate::completion::use_edit::{
    analyze_use_block_in, build_use_edit, short_name_needs_import, use_import_conflicts,
};
use crate::parser::with_parsed_program;
use crate::text_position::{line_start_byte_offset, offset_to_position};
use crate::util::strip_fqn_prefix;

/// The PHPStan identifier we match on.
const CHECKED_EXCEPTION_ID: &str = "missingType.checkedException";

impl Backend {
    /// Collect "Add @throws" code actions for PHPStan
    /// `missingType.checkedException` diagnostics.
    ///
    /// **Phase 1**: validates the action is applicable and emits a
    /// lightweight `CodeAction` with a `data` payload but **no `edit`**.
    /// The edit is computed lazily in [`resolve_add_throws`](Self::resolve_add_throws).
    pub(crate) fn collect_add_throws_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let phpstan_diags = self.phpstan_diags_overlapping(uri, &params.range);

        for diag in &phpstan_diags {
            let Some(identifier) = phpstan_identifier(diag) else {
                continue;
            };

            if identifier != CHECKED_EXCEPTION_ID {
                continue;
            }

            let exception_fqn = match extract_exception_fqn(&diag.message) {
                Some(fqn) => fqn,
                None => continue,
            };

            let short_name = crate::util::short_name(&exception_fqn);

            let block = self.import_block_at(
                uri,
                crate::text_position::position_to_byte_offset(
                    content,
                    Position::new(diag.range.start.line, 0),
                ),
            );
            let file_use_map = &block.use_map;
            let file_namespace = &block.namespace;

            // The @throws tag always uses the short name, adding a use
            // import when it does not already name the exception.
            let needs_import =
                short_name_needs_import(&exception_fqn, file_use_map, file_namespace);

            if needs_import && use_import_conflicts(&exception_fqn, file_use_map) {
                continue;
            }

            let diag_line = diag.range.start.line as usize;
            let docblock_info =
                match function_docblock_at_line(content, diag_line, FunctionKinds::Named) {
                    Some(info) => info,
                    None => continue,
                };

            if docblock_already_has_throws(
                &docblock_info,
                &exception_fqn,
                file_use_map,
                file_namespace,
            ) {
                continue;
            }

            // ── Phase 1: emit lightweight action with data ──────────
            let title = format!("Add @throws {}", short_name);

            out.push(deferred_quickfix(
                title,
                "phpstan.addThrows",
                uri,
                &params.range,
                vec![diag.clone()],
                true,
                serde_json::Value::Null,
            ));
        }
    }

    /// Resolve the "Add @throws" code action by computing the full
    /// workspace edit.
    ///
    /// **Phase 2**: called from
    /// [`resolve_code_action`](Self::resolve_code_action) when the user
    /// picks this action.  Recomputes the docblock edit and (optionally)
    /// the import edit from the data payload.
    pub(crate) fn resolve_add_throws(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let uri = &data.uri;

        let diagnostic_message = data.extra.get("diagnostic_message")?.as_str()?;
        let diagnostic_line = data.extra.get("diagnostic_line")?.as_u64()? as usize;

        let exception_fqn = extract_exception_fqn(diagnostic_message)?;
        let short_name = crate::util::short_name(&exception_fqn);

        let block = self.import_block_at(
            uri,
            crate::text_position::position_to_byte_offset(
                content,
                Position::new(diagnostic_line as u32, 0),
            ),
        );
        let file_use_map = &block.use_map;
        let file_namespace = &block.namespace;

        let needs_import = short_name_needs_import(&exception_fqn, file_use_map, file_namespace);

        let docblock_info =
            function_docblock_at_line(content, diagnostic_line, FunctionKinds::Named)?;

        let mut edits = Vec::new();

        // 1. Docblock edit: insert @throws tag.
        edits.push(docblock_info.add_throws_tag_edit(content, short_name));

        // 2. Import edit (if needed).
        if needs_import {
            let use_block = analyze_use_block_in(content, block.range);
            if let Some(import_edits) = build_use_edit(&exception_fqn, &use_block, file_namespace) {
                edits.extend(import_edits);
            }
        }

        let doc_uri: Url = uri.parse().ok()?;
        Some(crate::code_actions::single_file_edit(doc_uri, edits))
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Extract the fully-qualified exception class name from a PHPStan
/// `missingType.checkedException` message.
///
/// Expected formats:
/// - `"Method Ns\Cls::method() throws checked exception Ns\Ex but ..."`
/// - `"Function foo() throws checked exception Ns\Ex but ..."`
/// - `"Get hook for property Ns\Cls::$prop throws checked exception Ns\Ex but ..."`
pub(crate) fn extract_exception_fqn(message: &str) -> Option<String> {
    let marker = "throws checked exception ";
    let start = message.find(marker)? + marker.len();
    let rest = &message[start..];
    let end = rest.find(" but")?;
    let fqn = rest[..end].trim();
    if fqn.is_empty() {
        return None;
    }
    // Strip leading backslash if present.
    Some(strip_fqn_prefix(fqn).to_string())
}

/// Check if the existing docblock already documents a `@throws` for
/// the given exception type.
///
/// `exception_fqn` is the fully-qualified name from PHPStan (no
/// leading `\`).  Each `@throws` tag in the docblock is resolved
/// through the use-map so that `@throws RuntimeException` matches
/// `App\Exceptions\RuntimeException` when the import exists.
fn docblock_already_has_throws(
    info: &FunctionDocblock,
    exception_fqn: &str,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> bool {
    let Some(docblock) = &info.docblock else {
        return false;
    };
    let parsed = match crate::docblock::parser::parse_docblock_for_tags(docblock.text.trim()) {
        Some(parsed) => parsed,
        None => return false,
    };
    // PHPStan already reports FQNs — just normalise.
    let target_fqn = strip_fqn_prefix(exception_fqn).to_lowercase();
    for tag in parsed.tags_by_kind(crate::docblock::TagKind::Throws) {
        let rest = tag.description.trim();
        if let Some(type_name) = rest.split_whitespace().next() {
            let tag_fqn =
                crate::names::resolve_to_fqn(type_name, use_map, file_namespace).to_lowercase();
            if tag_fqn == target_fqn {
                return true;
            }
        }
    }
    false
}

/// Find the line range (start, end) of the enclosing function/method body
/// for the given diagnostic line.
///
/// Returns `(opening_brace_line, closing_brace_line)` so callers can
/// check whether two diagnostics fall within the same function body.
/// This is used to batch-clear all `missingType.checkedException`
/// diagnostics for the same exception class when the user applies the
/// "Add @throws" quick fix on any one of them.
///
/// Uses the mago AST parser to find the enclosing function or method,
/// which correctly handles braces inside strings, comments, and heredocs.
pub(crate) fn find_enclosing_function_line_range(
    content: &str,
    diag_line: usize,
) -> Option<(usize, usize)> {
    // Convert the diagnostic line to a byte offset (start of line is enough).
    content.lines().nth(diag_line)?;
    let cursor_offset = line_start_byte_offset(content, diag_line) as u32;

    let (open, close) = with_parsed_program(
        content,
        "find_enclosing_function_line_range",
        |program, _| {
            find_enclosing_function(program, cursor_offset, FunctionKinds::Named)?.body_braces()
        },
    )?;
    if cursor_offset < open || cursor_offset > close {
        return None;
    }
    let line_of = |offset: u32| offset_to_position(content, offset as usize).line as usize;
    Some((line_of(open), line_of(close)))
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_exception_fqn ───────────────────────────────────────

    #[test]
    fn extracts_fqn_from_method_message() {
        let msg = "Method App\\Controllers\\Foo::bar() throws checked exception App\\Exceptions\\BarException but it's missing from the PHPDoc @throws tag.";
        let fqn = extract_exception_fqn(msg).unwrap();
        assert_eq!(fqn, "App\\Exceptions\\BarException");
    }

    #[test]
    fn extracts_fqn_from_function_message() {
        let msg = "Function doStuff() throws checked exception App\\Exceptions\\StuffException but it's missing from the PHPDoc @throws tag.";
        let fqn = extract_exception_fqn(msg).unwrap();
        assert_eq!(fqn, "App\\Exceptions\\StuffException");
    }

    #[test]
    fn extracts_fqn_from_property_hook_message() {
        let msg = "Get hook for property App\\Foo::$bar throws checked exception App\\Exceptions\\PropException but it's missing from the PHPDoc @throws tag.";
        let fqn = extract_exception_fqn(msg).unwrap();
        assert_eq!(fqn, "App\\Exceptions\\PropException");
    }

    #[test]
    fn strips_leading_backslash() {
        let msg = "Method Foo::bar() throws checked exception \\Global\\SomeException but it's missing from the PHPDoc @throws tag.";
        let fqn = extract_exception_fqn(msg).unwrap();
        assert_eq!(fqn, "Global\\SomeException");
    }

    #[test]
    fn returns_none_for_unrelated_message() {
        let msg = "Some other PHPStan error about something.";
        assert!(extract_exception_fqn(msg).is_none());
    }

    // ── docblock_already_has_throws ─────────────────────────────────

    /// The docblock of the function holding `line` of `php`.
    fn docblock_at(php: &str, line: usize) -> FunctionDocblock {
        function_docblock_at_line(php, line, FunctionKinds::Named).unwrap()
    }

    /// `php` with the `@throws short_name` edit for the function holding
    /// `line` applied.
    fn with_throws(php: &str, line: usize, short_name: &str) -> String {
        let edit = docblock_at(php, line).add_throws_tag_edit(php, short_name);
        crate::text_position::apply_text_edits(php, &[edit])
    }

    fn has_throws(php: &str, line: usize, fqn: &str, use_map: &HashMap<String, String>) -> bool {
        docblock_already_has_throws(&docblock_at(php, line), fqn, use_map, &None)
    }

    #[test]
    fn detects_existing_throws() {
        let php = "<?php\n/**\n * @throws FooException\n */\nfunction foo() {\n    throw new FooException();\n}\n";
        // PHPStan reports FQNs; global-namespace class → bare name is the FQN.
        assert!(has_throws(php, 5, "FooException", &HashMap::new()));
    }

    #[test]
    fn detects_existing_throws_case_insensitive() {
        let php = "<?php\n/**\n * @throws fooexception\n */\nfunction foo() {\n    throw new FooException();\n}\n";
        assert!(has_throws(php, 5, "FooException", &HashMap::new()));
    }

    #[test]
    fn detects_fqn_throws() {
        let php = "<?php\n/**\n * @throws \\App\\Exceptions\\FooException\n */\nfunction foo() {\n    throw new FooException();\n}\n";
        let use_map = HashMap::from([(
            "FooException".to_string(),
            "App\\Exceptions\\FooException".to_string(),
        )]);
        // PHPStan reports the FQN `App\Exceptions\FooException`.
        assert!(has_throws(
            php,
            5,
            "App\\Exceptions\\FooException",
            &use_map
        ));
    }

    #[test]
    fn no_existing_throws() {
        let php = "<?php\n/**\n * @param string $a\n */\nfunction foo($a) {\n    throw new FooException();\n}\n";
        assert!(!has_throws(php, 5, "FooException", &HashMap::new()));
    }

    #[test]
    fn no_docblock_no_throws() {
        let php = "<?php\nfunction foo() {\n    throw new FooException();\n}\n";
        assert!(!has_throws(php, 2, "FooException", &HashMap::new()));
    }

    // ── Locating the docblock ───────────────────────────────────────

    #[test]
    fn finds_existing_docblock() {
        let php = "<?php\nclass Foo {\n    /**\n     * Summary.\n     */\n    public function bar(): void {\n        throw new \\RuntimeException();\n    }\n}\n";
        let docblock = docblock_at(php, 6).docblock.unwrap();
        assert_eq!(docblock.text, "    /**\n     * Summary.\n     */\n");
    }

    #[test]
    fn finds_no_docblock() {
        let php = "<?php\nclass Foo {\n    public function bar(): void {\n        throw new \\RuntimeException();\n    }\n}\n";
        assert!(docblock_at(php, 3).docblock.is_none());
    }

    #[test]
    fn a_brace_in_a_string_does_not_hide_the_method() {
        let php = concat!(
            "<?php\n",
            "class Foo {\n",
            "    public function first(): void {}\n",
            "\n",
            "    public function second(): void {\n",
            "        $close = '}';\n",
            "        throw new \\RuntimeException();\n",
            "    }\n",
            "}\n",
        );
        assert_eq!(
            with_throws(php, 6, "RuntimeException"),
            concat!(
                "<?php\n",
                "class Foo {\n",
                "    public function first(): void {}\n",
                "\n",
                "    /**\n",
                "     * @throws RuntimeException\n",
                "     */\n",
                "    public function second(): void {\n",
                "        $close = '}';\n",
                "        throw new \\RuntimeException();\n",
                "    }\n",
                "}\n",
            )
        );
    }

    #[test]
    fn a_throw_in_a_closure_is_charged_to_the_method() {
        let php = concat!(
            "<?php\n",
            "class Foo {\n",
            "    /** Summary. */\n",
            "    public function bar(): void {\n",
            "        array_map(function () {\n",
            "            throw new \\RuntimeException();\n",
            "        }, []);\n",
            "    }\n",
            "}\n",
        );
        let docblock = docblock_at(php, 5).docblock.unwrap();
        assert!(docblock.text.contains("Summary."));
    }

    // ── add_throws_tag_edit ─────────────────────────────────────────

    #[test]
    fn inserts_throws_into_multiline_docblock() {
        let php = "<?php\nclass Foo {\n    /**\n     * Summary.\n     */\n    public function bar(): void {\n        throw new \\RuntimeException();\n    }\n}\n";
        assert_eq!(
            with_throws(php, 6, "RuntimeException"),
            "<?php\nclass Foo {\n    /**\n     * Summary.\n     * @throws RuntimeException\n     */\n    public function bar(): void {\n        throw new \\RuntimeException();\n    }\n}\n"
        );
    }

    #[test]
    fn inserts_throws_into_single_line_docblock() {
        let php = "<?php\nclass Foo {\n    /** Summary. */\n    public function bar(): void {\n        throw new \\RuntimeException();\n    }\n}\n";
        assert_eq!(
            with_throws(php, 4, "RuntimeException"),
            "<?php\nclass Foo {\n    /**\n     * Summary.\n     *\n     * @throws RuntimeException\n     */\n    public function bar(): void {\n        throw new \\RuntimeException();\n    }\n}\n"
        );
    }

    #[test]
    fn creates_new_docblock_when_none_exists() {
        let php = "<?php\nclass Foo {\n    public function bar(): void {\n        throw new \\RuntimeException();\n    }\n}\n";
        assert_eq!(
            with_throws(php, 3, "RuntimeException"),
            "<?php\nclass Foo {\n    /**\n     * @throws RuntimeException\n     */\n    public function bar(): void {\n        throw new \\RuntimeException();\n    }\n}\n"
        );
    }

    #[test]
    fn creates_new_docblock_above_attributes() {
        let php = "<?php\nclass Foo {\n    #[\\Override]\n    public function bar(): void {\n        throw new \\RuntimeException();\n    }\n}\n";
        assert_eq!(
            with_throws(php, 4, "RuntimeException"),
            "<?php\nclass Foo {\n    /**\n     * @throws RuntimeException\n     */\n    #[\\Override]\n    public function bar(): void {\n        throw new \\RuntimeException();\n    }\n}\n"
        );
    }

    #[test]
    fn appends_after_existing_throws() {
        let php = "<?php\nclass Foo {\n    /**\n     * Summary.\n     *\n     * @throws FooException\n     */\n    public function bar(): void {\n        throw new \\RuntimeException();\n    }\n}\n";
        // PHPStan reports FQN; RuntimeException is global, so bare name is FQN.
        assert!(!has_throws(php, 8, "RuntimeException", &HashMap::new()));
        assert_eq!(
            with_throws(php, 8, "RuntimeException"),
            "<?php\nclass Foo {\n    /**\n     * Summary.\n     *\n     * @throws FooException\n     * @throws RuntimeException\n     */\n    public function bar(): void {\n        throw new \\RuntimeException();\n    }\n}\n"
        );
    }

    #[test]
    fn inserts_throws_after_return() {
        let php = concat!(
            "<?php\nclass Foo {\n",
            "    /**\n",
            "     * @return Response\n",
            "     */\n",
            "    public function clientside(): Response {\n",
            "        throw new \\RuntimeException();\n",
            "    }\n",
            "}\n",
        );
        assert_eq!(
            with_throws(php, 6, "RuntimeException"),
            concat!(
                "<?php\nclass Foo {\n",
                "    /**\n",
                "     * @return Response\n",
                "     *\n",
                "     * @throws RuntimeException\n",
                "     */\n",
                "    public function clientside(): Response {\n",
                "        throw new \\RuntimeException();\n",
                "    }\n",
                "}\n",
            )
        );
    }

    #[test]
    fn works_with_standalone_function() {
        let php = "<?php\n/**\n * Does stuff.\n */\nfunction doStuff(): void {\n    throw new \\RuntimeException();\n}\n";
        assert_eq!(
            with_throws(php, 5, "RuntimeException"),
            "<?php\n/**\n * Does stuff.\n * @throws RuntimeException\n */\nfunction doStuff(): void {\n    throw new \\RuntimeException();\n}\n"
        );
    }

    #[test]
    fn keeps_crlf_line_endings() {
        // A new docblock, a one-line docblock spread over several lines,
        // and a tag set apart from the one before it.
        let cases = [
            (
                "<?php\nclass Foo {\n    public function bar(): void {\n        throw new \\RuntimeException();\n    }\n}\n",
                3,
                "<?php\nclass Foo {\n    /**\n     * @throws RuntimeException\n     */\n    public function bar(): void {\n        throw new \\RuntimeException();\n    }\n}\n",
            ),
            (
                "<?php\nclass Foo {\n    /** Summary. */\n    public function bar(): void {\n        throw new \\RuntimeException();\n    }\n}\n",
                4,
                "<?php\nclass Foo {\n    /**\n     * Summary.\n     *\n     * @throws RuntimeException\n     */\n    public function bar(): void {\n        throw new \\RuntimeException();\n    }\n}\n",
            ),
            (
                "<?php\nclass Foo {\n    /**\n     * @return Response\n     */\n    public function clientside(): Response {\n        throw new \\RuntimeException();\n    }\n}\n",
                6,
                "<?php\nclass Foo {\n    /**\n     * @return Response\n     *\n     * @throws RuntimeException\n     */\n    public function clientside(): Response {\n        throw new \\RuntimeException();\n    }\n}\n",
            ),
        ];
        for (php, line, expected) in cases {
            let php = php.replace('\n', "\r\n");
            assert_eq!(
                with_throws(&php, line, "RuntimeException"),
                expected.replace('\n', "\r\n")
            );
        }
    }

    // ── find_enclosing_function_line_range ───────────────────────────

    #[test]
    fn function_line_range_simple_method() {
        let php = concat!(
            "<?php\n",                                   // 0
            "class Foo {\n",                             // 1
            "    public function bar(): void {\n",       // 2
            "        throw new \\RuntimeException();\n", // 3
            "        throw new \\RuntimeException();\n", // 4
            "    }\n",                                   // 5
            "}\n",                                       // 6
        );
        // Diagnostic on line 3 should find body from line 2 to line 5.
        let (start, end) = find_enclosing_function_line_range(php, 3).unwrap();
        assert_eq!(start, 2, "opening brace line");
        assert_eq!(end, 5, "closing brace line");

        // Diagnostic on line 4 should find the same range.
        let (start2, end2) = find_enclosing_function_line_range(php, 4).unwrap();
        assert_eq!((start2, end2), (start, end));
    }

    #[test]
    fn function_line_range_standalone_function() {
        let php = concat!(
            "<?php\n",                               // 0
            "function doStuff(): void {\n",          // 1
            "    throw new \\RuntimeException();\n", // 2
            "}\n",                                   // 3
        );
        let (start, end) = find_enclosing_function_line_range(php, 2).unwrap();
        assert_eq!(start, 1);
        assert_eq!(end, 3);
    }

    #[test]
    fn function_line_range_nested_braces() {
        let php = concat!(
            "<?php\n",                                       // 0
            "class Foo {\n",                                 // 1
            "    public function bar(): void {\n",           // 2
            "        if (true) {\n",                         // 3
            "            throw new \\RuntimeException();\n", // 4
            "        }\n",                                   // 5
            "        throw new \\RuntimeException();\n",     // 6
            "    }\n",                                       // 7
            "}\n",                                           // 8
        );
        // Diagnostic inside the if block should still find the method body.
        let (start, end) = find_enclosing_function_line_range(php, 4).unwrap();
        assert_eq!(start, 2, "opening brace line");
        assert_eq!(end, 7, "closing brace line");

        // Diagnostic on line 6 (outside if, inside method) gives same range.
        let (start2, end2) = find_enclosing_function_line_range(php, 6).unwrap();
        assert_eq!((start2, end2), (start, end));
    }

    #[test]
    fn function_line_range_two_methods() {
        let php = concat!(
            "<?php\n",                                   // 0
            "class Foo {\n",                             // 1
            "    public function first(): void {\n",     // 2
            "        throw new \\RuntimeException();\n", // 3
            "    }\n",                                   // 4
            "    public function second(): void {\n",    // 5
            "        throw new \\RuntimeException();\n", // 6
            "    }\n",                                   // 7
            "}\n",                                       // 8
        );
        // Line 3 should be in `first()`.
        let (s1, e1) = find_enclosing_function_line_range(php, 3).unwrap();
        assert_eq!((s1, e1), (2, 4));

        // Line 6 should be in `second()`.
        let (s2, e2) = find_enclosing_function_line_range(php, 6).unwrap();
        assert_eq!((s2, e2), (5, 7));
    }

    #[test]
    fn function_line_range_returns_none_for_out_of_range() {
        let php = "<?php\necho 'hi';\n";
        assert!(find_enclosing_function_line_range(php, 99).is_none());
    }

    #[test]
    fn function_line_range_returns_none_outside_function() {
        // Line 1 is at the top level, not inside any function body.
        let php = concat!(
            "<?php\n",      // 0
            "echo 'hi';\n", // 1
        );
        assert!(find_enclosing_function_line_range(php, 1).is_none());
    }
}
