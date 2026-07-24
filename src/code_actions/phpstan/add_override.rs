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

        // Parse the extra data to recover the diagnostic line.
        let diag_line = data.extra.get("diagnostic_line")?.as_u64()? as usize;

        // Find the insertion point for the attribute.
        let insertion = find_method_insertion_point(content, diag_line)?;

        // If Override was added since the action was offered, bail out.
        if already_has_override(content, &insertion) {
            return None;
        }

        // Look up the use_map and namespace_map for the URI.
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
        let mut changes = HashMap::new();
        changes.insert(doc_uri, edits);

        Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        })
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

    // ── Integration: full code action via Backend ───────────────────

    #[test]
    fn offers_add_override_action() {
        let backend = crate::Backend::defaults();
        let uri = "file:///test.php";
        let content = r#"<?php
class Child extends Base {
    public function foo(): void {}
}
"#;
        backend.update_ast(uri, content);
        backend
            .open_files
            .write()
            .insert(uri.to_string(), std::sync::Arc::new(content.to_string()));

        let diag = Diagnostic {
            range: Range {
                start: Position::new(2, 0),
                end: Position::new(2, 80),
            },
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(MISSING_OVERRIDE_ID.to_string())),
            source: Some("PHPStan".to_string()),
            message: "Method Child::foo() overrides method Base::foo() but is missing the #[Override] attribute.".to_string(),
            ..Default::default()
        };
        {
            let mut cache = backend.phpstan_last_diags().lock();
            cache.entry(uri.to_string()).or_default().push(diag);
        }

        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range {
                start: Position::new(2, 4),
                end: Position::new(2, 4),
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams {
                work_done_token: None,
            },
            partial_result_params: PartialResultParams {
                partial_result_token: None,
            },
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let override_action = actions.iter().find_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title.contains("#[Override]") => Some(ca),
            _ => None,
        });

        assert!(
            override_action.is_some(),
            "should offer Add #[Override] action"
        );

        let action = override_action.unwrap();
        assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
        assert_eq!(action.is_preferred, Some(true));
        assert!(
            action.title.contains("foo"),
            "title should mention method name: {}",
            action.title
        );

        // Phase 1: edit should be None, data should be Some.
        assert!(action.edit.is_none(), "Phase 1 should not compute the edit");
        assert!(
            action.data.is_some(),
            "Phase 1 should set data for deferred resolve"
        );

        // Phase 2: resolve the action to get the edit.
        let (resolved, _republish) = backend.resolve_code_action(action.clone());
        let edit = resolved
            .edit
            .as_ref()
            .expect("resolve should produce an edit");
        let changes = edit.changes.as_ref().unwrap();
        let edits: Vec<&TextEdit> = changes.values().flat_map(|v| v.iter()).collect();
        assert_eq!(edits.len(), 1);
        assert!(edits[0].new_text.contains("#[Override]"));
        assert!(
            !edits[0].new_text.contains("#[\\Override]"),
            "should use short form in non-namespaced file"
        );
    }

    #[test]
    fn no_action_when_override_already_present() {
        let backend = crate::Backend::defaults();
        let uri = "file:///test.php";
        // Both `#[\Override]` and `#[Override]` should be detected.
        let content = r#"<?php
class Child extends Base {
    #[\Override]
    public function foo(): void {}
}
"#;
        backend.update_ast(uri, content);

        let diag = Diagnostic {
            range: Range {
                start: Position::new(3, 0),
                end: Position::new(3, 80),
            },
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(MISSING_OVERRIDE_ID.to_string())),
            source: Some("PHPStan".to_string()),
            message: "Method Child::foo() overrides method Base::foo() but is missing the #[Override] attribute.".to_string(),
            ..Default::default()
        };
        {
            let mut cache = backend.phpstan_last_diags().lock();
            cache.entry(uri.to_string()).or_default().push(diag);
        }

        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range {
                start: Position::new(3, 4),
                end: Position::new(3, 4),
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams {
                work_done_token: None,
            },
            partial_result_params: PartialResultParams {
                partial_result_token: None,
            },
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let override_action = actions.iter().find_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title.contains("#[Override]") => Some(ca),
            _ => None,
        });

        assert!(
            override_action.is_none(),
            "should NOT offer action when #[Override] already present"
        );
    }

    #[test]
    fn no_action_for_other_identifiers() {
        let backend = crate::Backend::defaults();
        let uri = "file:///test.php";
        let content = r#"<?php
class Foo {
    public function bar(): void {}
}
"#;
        backend.update_ast(uri, content);

        let diag = Diagnostic {
            range: Range {
                start: Position::new(2, 0),
                end: Position::new(2, 80),
            },
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("return.unusedType".to_string())),
            source: Some("PHPStan".to_string()),
            message: "Some other error.".to_string(),
            ..Default::default()
        };
        {
            let mut cache = backend.phpstan_last_diags().lock();
            cache.entry(uri.to_string()).or_default().push(diag);
        }

        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range {
                start: Position::new(2, 4),
                end: Position::new(2, 4),
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams {
                work_done_token: None,
            },
            partial_result_params: PartialResultParams {
                partial_result_token: None,
            },
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let override_action = actions.iter().find_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title.contains("#[Override]") => Some(ca),
            _ => None,
        });

        assert!(
            override_action.is_none(),
            "should NOT offer action for non-missingOverride identifiers"
        );
    }

    #[test]
    fn inserts_before_existing_attributes() {
        let backend = crate::Backend::defaults();
        let uri = "file:///test.php";
        let content = r#"<?php
class Child extends Base {
    #[Route('/foo')]
    public function foo(): void {}
}
"#;
        backend.update_ast(uri, content);
        backend
            .open_files
            .write()
            .insert(uri.to_string(), std::sync::Arc::new(content.to_string()));

        let diag = Diagnostic {
            range: Range {
                start: Position::new(3, 0),
                end: Position::new(3, 80),
            },
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(MISSING_OVERRIDE_ID.to_string())),
            source: Some("PHPStan".to_string()),
            message: "Method Child::foo() overrides method Base::foo() but is missing the #[Override] attribute.".to_string(),
            ..Default::default()
        };
        {
            let mut cache = backend.phpstan_last_diags().lock();
            cache.entry(uri.to_string()).or_default().push(diag);
        }

        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range {
                start: Position::new(3, 4),
                end: Position::new(3, 4),
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams {
                work_done_token: None,
            },
            partial_result_params: PartialResultParams {
                partial_result_token: None,
            },
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let action = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("#[Override]") => Some(ca),
                _ => None,
            })
            .expect("should offer action");

        // Phase 1: no edit yet.
        assert!(action.edit.is_none(), "Phase 1 should not have edit");

        // Phase 2: resolve to get the edit.
        let (resolved, _) = backend.resolve_code_action(action.clone());
        let edit = resolved.edit.as_ref().expect("resolve should produce edit");
        let changes = edit.changes.as_ref().unwrap();
        let edits: Vec<&TextEdit> = changes.values().flat_map(|v| v.iter()).collect();

        // The insertion position should be before the `#[Route` line
        // (line 2), not before the `public function` line (line 3).
        assert_eq!(
            edits[0].range.start.line, 2,
            "should insert before existing attributes"
        );
    }

    // ── Import behaviour ────────────────────────────────────────────

    #[test]
    fn adds_use_import_in_namespaced_file() {
        let backend = crate::Backend::defaults();
        let uri = "file:///test.php";
        let content = r#"<?php
namespace App\Http\Controllers;

class Child extends Base {
    public function foo(): void {}
}
"#;
        backend.update_ast(uri, content);
        backend
            .open_files
            .write()
            .insert(uri.to_string(), std::sync::Arc::new(content.to_string()));

        let diag = Diagnostic {
            range: Range {
                start: Position::new(4, 0),
                end: Position::new(4, 80),
            },
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(MISSING_OVERRIDE_ID.to_string())),
            source: Some("PHPStan".to_string()),
            message: "Method App\\Http\\Controllers\\Child::foo() overrides method App\\Http\\Controllers\\Base::foo() but is missing the #[Override] attribute.".to_string(),
            ..Default::default()
        };
        {
            let mut cache = backend.phpstan_last_diags().lock();
            cache.entry(uri.to_string()).or_default().push(diag);
        }

        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range {
                start: Position::new(4, 4),
                end: Position::new(4, 4),
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams {
                work_done_token: None,
            },
            partial_result_params: PartialResultParams {
                partial_result_token: None,
            },
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let action = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("#[Override]") => Some(ca),
                _ => None,
            })
            .expect("should offer action");

        // Phase 1: no edit yet.
        assert!(action.edit.is_none(), "Phase 1 should not have edit");

        // Phase 2: resolve to get the edit.
        let (resolved, _) = backend.resolve_code_action(action.clone());
        let edit = resolved.edit.as_ref().expect("resolve should produce edit");
        let changes = edit.changes.as_ref().unwrap();
        let edits: Vec<&TextEdit> = changes.values().flat_map(|v| v.iter()).collect();

        // Should have two edits: the attribute insertion and the use import.
        assert_eq!(edits.len(), 2, "should have attribute + use import edits");

        let has_attr = edits.iter().any(|e| e.new_text.contains("#[Override]"));
        let has_import = edits.iter().any(|e| e.new_text.contains("use Override;"));

        assert!(has_attr, "should insert #[Override] attribute");
        assert!(has_import, "should add `use Override;` import");

        // The attribute should use the short form, not FQN.
        assert!(
            !edits.iter().any(|e| e.new_text.contains("#[\\Override]")),
            "should use short form #[Override], not FQN"
        );
    }

    #[test]
    fn no_import_in_non_namespaced_file() {
        let backend = crate::Backend::defaults();
        let uri = "file:///test.php";
        let content = r#"<?php
class Child extends Base {
    public function foo(): void {}
}
"#;
        backend.update_ast(uri, content);
        backend
            .open_files
            .write()
            .insert(uri.to_string(), std::sync::Arc::new(content.to_string()));

        let diag = Diagnostic {
            range: Range {
                start: Position::new(2, 0),
                end: Position::new(2, 80),
            },
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(MISSING_OVERRIDE_ID.to_string())),
            source: Some("PHPStan".to_string()),
            message: "Method Child::foo() overrides method Base::foo() but is missing the #[Override] attribute.".to_string(),
            ..Default::default()
        };
        {
            let mut cache = backend.phpstan_last_diags().lock();
            cache.entry(uri.to_string()).or_default().push(diag);
        }

        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range {
                start: Position::new(2, 4),
                end: Position::new(2, 4),
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams {
                work_done_token: None,
            },
            partial_result_params: PartialResultParams {
                partial_result_token: None,
            },
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let action = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("#[Override]") => Some(ca),
                _ => None,
            })
            .expect("should offer action");

        // Phase 1: no edit yet.
        assert!(action.edit.is_none(), "Phase 1 should not have edit");

        // Phase 2: resolve to get the edit.
        let (resolved, _) = backend.resolve_code_action(action.clone());
        let edit = resolved.edit.as_ref().expect("resolve should produce edit");
        let changes = edit.changes.as_ref().unwrap();
        let edits: Vec<&TextEdit> = changes.values().flat_map(|v| v.iter()).collect();

        // No namespace → only the attribute edit, no import.
        assert_eq!(edits.len(), 1, "should have only attribute edit, no import");
        assert!(edits[0].new_text.contains("#[Override]"));
        assert!(
            !edits.iter().any(|e| e.new_text.contains("use Override;")),
            "should NOT add use import in non-namespaced file"
        );
    }

    #[test]
    fn no_duplicate_import_when_already_imported() {
        let backend = crate::Backend::defaults();
        let uri = "file:///test.php";
        let content = r#"<?php
namespace App\Controllers;

use Override;

class Child extends Base {
    public function foo(): void {}
}
"#;
        backend.update_ast(uri, content);
        backend
            .open_files
            .write()
            .insert(uri.to_string(), std::sync::Arc::new(content.to_string()));

        let diag = Diagnostic {
            range: Range {
                start: Position::new(6, 0),
                end: Position::new(6, 80),
            },
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(MISSING_OVERRIDE_ID.to_string())),
            source: Some("PHPStan".to_string()),
            message: "Method App\\Controllers\\Child::foo() overrides method App\\Controllers\\Base::foo() but is missing the #[Override] attribute.".to_string(),
            ..Default::default()
        };
        {
            let mut cache = backend.phpstan_last_diags().lock();
            cache.entry(uri.to_string()).or_default().push(diag);
        }

        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range {
                start: Position::new(6, 4),
                end: Position::new(6, 4),
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams {
                work_done_token: None,
            },
            partial_result_params: PartialResultParams {
                partial_result_token: None,
            },
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let action = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("#[Override]") => Some(ca),
                _ => None,
            })
            .expect("should offer action");

        // Phase 1: no edit yet.
        assert!(action.edit.is_none(), "Phase 1 should not have edit");

        // Phase 2: resolve to get the edit.
        let (resolved, _) = backend.resolve_code_action(action.clone());
        let edit = resolved.edit.as_ref().expect("resolve should produce edit");
        let changes = edit.changes.as_ref().unwrap();
        let edits: Vec<&TextEdit> = changes.values().flat_map(|v| v.iter()).collect();

        // Already imported → only the attribute edit, no duplicate import.
        assert_eq!(
            edits.len(),
            1,
            "should have only attribute edit when already imported"
        );
        assert!(
            !edits.iter().any(|e| e.new_text.contains("use Override;")),
            "should NOT duplicate existing use import"
        );
    }
}
