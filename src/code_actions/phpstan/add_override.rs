//! "Add `#[Override]`" code action for PHPStan `method.missingOverride`.
//!
//! When PHPStan reports that a method overrides a parent/interface
//! method but is missing the `#[\Override]` attribute (PHP 8.3+),
//! this code action offers to insert the attribute on the line above
//! the method declaration, with correct indentation.
//!
//! **Trigger:** A PHPStan diagnostic with identifier
//! `method.missingOverride` overlaps the cursor.
//!
//! **Code action kind:** `quickfix`.
//!
//! ## Two-phase resolve
//!
//! Phase 1 (`collect_add_override_actions`) performs all validation and
//! emits a lightweight `CodeAction` with a `data` payload but no `edit`.
//! Phase 2 (`resolve_add_override`) recomputes the workspace edit on
//! demand when the user picks the action.

use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use super::{InsertionPoint, find_method_insertion_point};
use crate::Backend;
use crate::code_actions::phpstan::contains_php_attribute;
use crate::code_actions::{CodeActionData, make_code_action_data};
use crate::completion::use_edit::{analyze_use_block, build_use_edit, use_import_conflicts};
use crate::text_position::{offset_to_position, ranges_overlap};

/// The PHPStan identifier we match on.
const MISSING_OVERRIDE_ID: &str = "method.missingOverride";

impl Backend {
    /// Collect "Add `#[Override]`" code actions for PHPStan
    /// `method.missingOverride` diagnostics.
    ///
    /// **Phase 1**: validates the action is applicable and emits a
    /// lightweight `CodeAction` with a `data` payload but **no `edit`**.
    /// The edit is computed lazily in [`resolve_add_override`](Self::resolve_add_override).
    pub(crate) fn collect_add_override_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let phpstan_diags: Vec<Diagnostic> = {
            let cache = self.phpstan_tool.last_diags.lock();
            cache.get(uri).cloned().unwrap_or_default()
        };

        for diag in &phpstan_diags {
            if !ranges_overlap(&diag.range, &params.range) {
                continue;
            }

            let identifier = match &diag.code {
                Some(NumberOrString::String(s)) => s.as_str(),
                _ => continue,
            };

            if identifier != MISSING_OVERRIDE_ID {
                continue;
            }

            // The diagnostic range covers the method signature line.
            // Find the insertion point: just before the first token of
            // the method declaration (attribute list, modifier, or
            // `function` keyword).
            let diag_line = diag.range.start.line as usize;

            let Some(insertion) = find_method_insertion_point(content, diag_line) else {
                continue;
            };

            // Check if `#[Override]` or `#[\Override]` is already present
            // on the method (could have been added manually since PHPStan
            // last ran).
            if already_has_override(content, &insertion) {
                continue;
            }

            // ── Phase 1: emit lightweight action with data ──────────
            let method_name = extract_method_name(&diag.message).unwrap_or("method");
            let title = format!("Add #[Override] to {}", method_name);

            let extra = serde_json::json!({
                "diagnostic_message": diag.message,
                "diagnostic_line": diag.range.start.line,
                "diagnostic_code": MISSING_OVERRIDE_ID,
            });

            let data = make_code_action_data("phpstan.addOverride", uri, &params.range, extra);

            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title,
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![diag.clone()]),
                edit: None,
                command: None,
                is_preferred: Some(true),
                disabled: None,
                data: Some(data),
            }));
        }
    }

    /// Resolve the "Add `#[Override]`" code action by computing the full
    /// workspace edit.
    ///
    /// **Phase 2**: called from
    /// [`resolve_code_action`](Self::resolve_code_action) when the user
    /// picks this action.  Recomputes the attribute insertion edit and
    /// (optionally) the import edit from the data payload.
    pub(crate) fn resolve_add_override(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let uri = &data.uri;

        let diag_line = data.extra.get("diagnostic_line")?.as_u64()? as usize;

        let insertion = find_method_insertion_point(content, diag_line)?;

        // If Override was added since the action was offered, bail out.
        if already_has_override(content, &insertion) {
            return None;
        }

        let file_use_map: HashMap<String, String> = self.file_use_map(uri);
        let file_namespace: Option<String> = self.first_file_namespace(uri);

        // Decide whether to use the short form `#[Override]` with a
        // `use Override;` import, or the FQN `#[\Override]`.
        //
        // `Override` lives in the global namespace.  When the file
        // declares a namespace we need a `use Override;` import
        // (just like any other global class).  When the file has no
        // namespace, no import is needed.
        let already_imported = file_use_map.iter().any(|(alias, fqn)| {
            alias.eq_ignore_ascii_case("Override") && fqn.eq_ignore_ascii_case("Override")
        });

        let same_namespace = file_namespace.is_none();

        let needs_import = !already_imported && !same_namespace;

        // Check for import conflicts (e.g. a different class named
        // `Override` is already imported).
        let use_fqn = needs_import && use_import_conflicts("Override", &file_use_map);

        let attr_text = if use_fqn {
            "#[\\Override]"
        } else {
            "#[Override]"
        };

        // Build the text edit: insert `#[Override]\n<indent>` at the
        // start of the method declaration line (before any existing
        // attributes or modifiers).
        let insert_text = format!("{}{}\n", insertion.indent, attr_text);

        let insert_pos = offset_to_position(content, insertion.insert_offset);

        let mut edits = vec![TextEdit {
            range: Range {
                start: insert_pos,
                end: insert_pos,
            },
            new_text: insert_text,
        }];

        // Add `use Override;` import when needed and possible.
        if needs_import && !use_fqn {
            let use_block = analyze_use_block(content);
            if let Some(import_edits) = build_use_edit("Override", &use_block, &file_namespace) {
                edits.extend(import_edits);
            }
        }

        let doc_uri: Url = uri.parse().ok()?;
        Some(crate::code_actions::single_file_edit(doc_uri, edits))
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Extract the method name from a PHPStan `method.missingOverride`
/// message.
///
/// Expected format:
/// - `"Method App\Foo::bar() overrides method App\Base::bar() but is
///    missing the #[\Override] attribute."`
///
/// We extract just the short method name (`bar`).
fn extract_method_name(message: &str) -> Option<&str> {
    // Find `Method <class>::<name>()`.
    let after_method = message.strip_prefix("Method ")?;
    let paren_pos = after_method.find('(')?;
    let class_and_name = &after_method[..paren_pos];
    // Take the part after the last `::`.
    let name = class_and_name.rsplit("::").next()?;
    if name.is_empty() {
        return None;
    }
    Some(name)
}

/// Check if the method already has a `#[Override]` or `#[\Override]`
/// attribute.
fn already_has_override(content: &str, insertion: &InsertionPoint) -> bool {
    // If there are no attribute lines, there's nothing to check.
    if insertion.attrs_end_offset <= insertion.first_token_offset {
        return false;
    }
    let attr_region = &content[insertion.first_token_offset..insertion.attrs_end_offset];
    // Look for `Override` in the attribute region, accounting for
    // both `#[Override]` and `#[\Override]`.
    for line in attr_region.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[") {
            // Crude but effective: check if `Override` appears as an
            // attribute name in this line.
            if contains_php_attribute(trimmed, b"Override") {
                return true;
            }
        }
    }
    false
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::super::{is_attribute_line, is_modifier_line};
    use super::*;
    use crate::util::contains_function_keyword;

    // ── extract_method_name ─────────────────────────────────────────

    #[test]
    fn extracts_method_name_from_standard_message() {
        let msg = "Method App\\Foo::bar() overrides method App\\Base::bar() but is missing the #[Override] attribute.";
        assert_eq!(extract_method_name(msg), Some("bar"));
    }

    #[test]
    fn extracts_method_name_with_deep_namespace() {
        let msg = "Method App\\Http\\Controllers\\UserController::index() overrides method App\\Http\\Controllers\\Controller::index() but is missing the #[Override] attribute.";
        assert_eq!(extract_method_name(msg), Some("index"));
    }

    #[test]
    fn returns_none_for_unrelated_message() {
        let msg = "Some other PHPStan error about something.";
        assert_eq!(extract_method_name(msg), None);
    }

    #[test]
    fn extracts_constructor_name() {
        let msg = "Method App\\Foo::__construct() overrides method App\\Base::__construct() but is missing the #[Override] attribute.";
        assert_eq!(extract_method_name(msg), Some("__construct"));
    }

    // ── contains_function_keyword ───────────────────────────────────

    #[test]
    fn detects_function_keyword() {
        assert!(contains_function_keyword(
            "    public function bar(): void {"
        ));
        assert!(contains_function_keyword("function foo()"));
        assert!(contains_function_keyword(
            "    protected static function baz()"
        ));
    }

    #[test]
    fn rejects_function_in_string() {
        assert!(!contains_function_keyword("    $functionality = true;"));
        assert!(!contains_function_keyword("    // some_function()"));
    }

    // ── is_modifier_line ────────────────────────────────────────────

    #[test]
    fn detects_modifier_lines() {
        assert!(is_modifier_line("public function"));
        assert!(is_modifier_line("protected static"));
        assert!(is_modifier_line("abstract public"));
        assert!(is_modifier_line("final protected"));
    }

    #[test]
    fn rejects_non_modifier_lines() {
        assert!(!is_modifier_line("function foo()"));
        assert!(!is_modifier_line("$public = true;"));
        assert!(!is_modifier_line("// public function"));
    }

    // ── is_attribute_line ───────────────────────────────────────────

    #[test]
    fn detects_attribute_lines() {
        assert!(is_attribute_line("#[Override]"));
        assert!(is_attribute_line("#[\\Override]"));
        assert!(is_attribute_line("#[Route('/foo')]"));
        assert!(is_attribute_line("#[Override, Deprecated]"));
    }

    #[test]
    fn rejects_non_attribute_lines() {
        assert!(!is_attribute_line("// #[Override]"));
        assert!(!is_attribute_line("public function foo()"));
    }

    // ── contains_php_attribute ──────────────────────────────────────

    #[test]
    fn finds_override_simple() {
        assert!(contains_php_attribute("#[Override]", b"Override"));
    }

    #[test]
    fn finds_override_with_backslash() {
        assert!(contains_php_attribute("#[\\Override]", b"Override"));
    }

    #[test]
    fn finds_override_in_list() {
        assert!(contains_php_attribute(
            "#[Override, Deprecated]",
            b"Override"
        ));
        assert!(contains_php_attribute(
            "#[Deprecated, Override]",
            b"Override"
        ));
        assert!(contains_php_attribute(
            "#[Deprecated, \\Override]",
            b"Override"
        ));
    }

    #[test]
    fn does_not_match_partial() {
        assert!(!contains_php_attribute("#[OverrideSomething]", b"Override"));
        assert!(!contains_php_attribute("#[MyOverride]", b"Override"));
    }

    // ── already_has_override ────────────────────────────────────────

    #[test]
    fn detects_existing_override() {
        let content =
            "<?php\nclass Foo {\n    #[\\Override]\n    public function bar(): void {}\n}\n";
        let insertion = InsertionPoint {
            insert_offset: content.find("#[\\Override]").unwrap(),
            indent: "    ".to_string(),
            first_token_offset: content.find("#[\\Override]").unwrap(),
            attrs_end_offset: content.find("    public function").unwrap(),
        };
        assert!(already_has_override(content, &insertion));
    }

    #[test]
    fn no_override_when_no_attrs() {
        let content = "<?php\nclass Foo {\n    public function bar(): void {}\n}\n";
        let offset = content.find("    public function").unwrap();
        let insertion = InsertionPoint {
            insert_offset: offset,
            indent: "    ".to_string(),
            first_token_offset: offset,
            attrs_end_offset: offset,
        };
        assert!(!already_has_override(content, &insertion));
    }

    // ── find_method_insertion_point ──────────────────────────────────

    #[test]
    fn finds_insertion_for_simple_method() {
        let content = "<?php\nclass Foo {\n    public function bar(): void {}\n}\n";
        let line = 2; // `public function bar()`
        let ins = find_method_insertion_point(content, line).unwrap();
        assert_eq!(ins.indent, "    ");
        // insert_offset should be at the start of the `    public function` line
        let expected_offset = content.find("    public function").unwrap();
        assert_eq!(ins.insert_offset, expected_offset);
    }

    #[test]
    fn finds_insertion_for_method_with_existing_attributes() {
        let content =
            "<?php\nclass Foo {\n    #[Route('/bar')]\n    public function bar(): void {}\n}\n";
        let line = 3; // `public function bar()` line
        let ins = find_method_insertion_point(content, line).unwrap();
        assert_eq!(ins.indent, "    ");
        // Should insert before the existing attribute line.
        let expected_offset = content.find("    #[Route").unwrap();
        assert_eq!(ins.insert_offset, expected_offset);
    }

    #[test]
    fn finds_insertion_with_multiple_attributes() {
        let content = "<?php\nclass Foo {\n    #[Route('/bar')]\n    #[Deprecated]\n    public function bar(): void {}\n}\n";
        let line = 4; // `public function bar()` line
        let ins = find_method_insertion_point(content, line).unwrap();
        // Should insert before the first attribute.
        let expected_offset = content.find("    #[Route").unwrap();
        assert_eq!(ins.insert_offset, expected_offset);
    }

    // ── Integration: build edit text ────────────────────────────────

    #[test]
    fn builds_correct_override_text() {
        let content = "<?php\nclass Foo {\n    public function bar(): void {}\n}\n";
        let line = 2;
        let ins = find_method_insertion_point(content, line).unwrap();
        let insert_text = format!("{}#[Override]\n", ins.indent);
        assert_eq!(insert_text, "    #[Override]\n");
    }

    #[test]
    fn builds_correct_override_text_nested() {
        let content = "<?php\nclass Foo {\n        protected function bar(): void {}\n}\n";
        let line = 2;
        let ins = find_method_insertion_point(content, line).unwrap();
        let insert_text = format!("{}#[Override]\n", ins.indent);
        assert_eq!(insert_text, "        #[Override]\n");
    }
}
