//! "Remove `#[Override]`" code action for PHPStan `method.override` /
//! `property.override` / `property.overrideAttribute`.
//!
//! When PHPStan reports that a method or property has `#[\Override]` but
//! does not actually override anything, or that the attribute cannot be
//! used on properties in the current PHP version, this code action offers
//! to remove the attribute.
//!
//! **Trigger:** A PHPStan diagnostic with identifier `method.override`,
//! `property.override`, or `property.overrideAttribute` overlaps the cursor.
//!
//! **Code action kind:** `quickfix`.
//!
//! ## Two-phase resolve
//!
//! Phase 1 (`collect_remove_override_actions`) validates and emits a
//! lightweight `CodeAction` with a `data` payload but no `edit`.
//! Phase 2 (`resolve_remove_override`) computes the workspace edit on
//! demand when the user picks the action.

use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::code_actions::phpstan::contains_php_attribute;
use crate::code_actions::{CodeActionData, make_code_action_data};
use crate::text_position::{offset_to_position, ranges_overlap};
use crate::util::strip_fqn_prefix;

/// PHPStan identifiers we match on.
const METHOD_OVERRIDE_ID: &str = "method.override";
const PROPERTY_OVERRIDE_ID: &str = "property.override";
const PROPERTY_OVERRIDE_ATTR_ID: &str = "property.overrideAttribute";

impl Backend {
    /// Collect "Remove `#[Override]`" code actions for PHPStan
    /// `method.override` / `property.override` /
    /// `property.overrideAttribute` diagnostics.
    ///
    /// **Phase 1**: validates the action is applicable and emits a
    /// lightweight `CodeAction` with a `data` payload but **no `edit`**.
    /// The edit is computed lazily in
    /// [`resolve_remove_override`](Self::resolve_remove_override).
    pub(crate) fn collect_remove_override_actions(
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

        // Group matching diagnostics by line so that overlapping
        // identifiers (e.g. `property.override` and
        // `property.overrideAttribute` on the same property) produce
        // a single code action that clears all of them at once.
        let mut by_line: std::collections::BTreeMap<u32, Vec<&Diagnostic>> =
            std::collections::BTreeMap::new();

        for diag in &phpstan_diags {
            if !ranges_overlap(&diag.range, &params.range) {
                continue;
            }

            let identifier = match &diag.code {
                Some(NumberOrString::String(s)) => s.as_str(),
                _ => continue,
            };

            if identifier != METHOD_OVERRIDE_ID
                && identifier != PROPERTY_OVERRIDE_ID
                && identifier != PROPERTY_OVERRIDE_ATTR_ID
            {
                continue;
            }

            by_line.entry(diag.range.start.line).or_default().push(diag);
        }

        for diags in by_line.values() {
            let first = diags[0];
            let diag_line = first.range.start.line as usize;

            // Check that there actually is an `#[Override]` attribute
            // near the diagnostic line. If the user already removed it
            // manually, don't offer the action.
            if find_override_attribute_line(content, diag_line).is_none() {
                continue;
            }

            // Try to extract a readable member name from any of the
            // grouped diagnostics (some identifiers like
            // `property.overrideAttribute` don't include the name).
            let member_name = diags.iter().find_map(|d| {
                let id = match &d.code {
                    Some(NumberOrString::String(s)) => s.as_str(),
                    _ => return None,
                };
                extract_member_name(&d.message, id)
            });

            let title = match member_name {
                Some(name) => format!("Remove #[Override] from {}", name),
                None => "Remove #[Override]".to_string(),
            };

            let extra = serde_json::json!({
                "diagnostic_message": first.message,
                "diagnostic_line": first.range.start.line,
                "diagnostic_code": match &first.code {
                    Some(NumberOrString::String(s)) => s.as_str(),
                    _ => "",
                },
            });

            let data = make_code_action_data("phpstan.removeOverride", uri, &params.range, extra);

            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title,
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(diags.iter().map(|d| (*d).clone()).collect()),
                edit: None,
                command: None,
                is_preferred: Some(true),
                disabled: None,
                data: Some(data),
            }));
        }
    }

    /// Resolve the "Remove `#[Override]`" code action by computing the
    /// full workspace edit.
    ///
    /// **Phase 2**: called from
    /// [`resolve_code_action`](Self::resolve_code_action) when the user
    /// picks this action.
    pub(crate) fn resolve_remove_override(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let uri = &data.uri;
        let diag_line = data.extra.get("diagnostic_line")?.as_u64()? as usize;

        let attr_line = find_override_attribute_line(content, diag_line)?;
        let edit = build_remove_override_edit(content, attr_line)?;

        let doc_uri: Url = uri.parse().ok()?;
        let mut changes = HashMap::new();
        changes.insert(doc_uri, vec![edit]);

        Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        })
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Extract a member name from a PHPStan `method.override`,
/// `property.override`, or `property.overrideAttribute` message.
///
/// Expected formats:
/// - `"Method Foo::bar() has #[\Override] attribute but does not override any method."`
/// - `"Property Foo::$baz has #[\Override] attribute but does not override any property."`
/// - `"Attribute class Override can be used with properties only on PHP 8.5 and later."`
///   (no member name extractable — returns `None`)
fn extract_member_name<'a>(message: &'a str, identifier: &str) -> Option<&'a str> {
    if identifier == METHOD_OVERRIDE_ID {
        let after = message.strip_prefix("Method ")?;
        let paren_pos = after.find('(')?;
        let class_and_name = &after[..paren_pos];
        let name = class_and_name.rsplit("::").next()?;
        if name.is_empty() {
            return None;
        }
        Some(name)
    } else if identifier == PROPERTY_OVERRIDE_ID {
        let after = message.strip_prefix("Property ")?;
        let has_pos = after.find(" has ")?;
        let class_and_name = &after[..has_pos];
        let name = class_and_name.rsplit("::").next()?;
        if name.is_empty() {
            return None;
        }
        Some(name)
    } else {
        // property.overrideAttribute message doesn't contain the
        // property name, so we return None and the title will be
        // the generic "Remove #[Override]".
        None
    }
}

/// Search for a line containing `#[Override]` or `#[\Override]` near
/// `diag_line`.
///
/// Walks backward from `diag_line` (inclusive) up to 10 lines to find
/// the attribute. Returns the 0-based line number of the attribute
/// line, or `None` if not found.
fn find_override_attribute_line(content: &str, diag_line: usize) -> Option<usize> {
    let lines: Vec<&str> = content.lines().collect();
    if diag_line >= lines.len() {
        return None;
    }

    let search_start = diag_line.saturating_sub(10);
    for i in (search_start..=diag_line).rev() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("#[") && contains_php_attribute(trimmed, b"Override") {
            return Some(i);
        }
    }
    None
}

/// Build a `TextEdit` that removes the `#[Override]` attribute from
/// line `attr_line`.
///
/// If `Override` is the only attribute on the line (the common case),
/// the entire line including trailing newline is removed. If the line
/// has multiple attributes (e.g. `#[Override, SomeOther]`), only the
/// `Override` token (with its leading `\` prefix and surrounding
/// comma/space) is removed.
fn build_remove_override_edit(content: &str, attr_line: usize) -> Option<TextEdit> {
    let lines: Vec<&str> = content.lines().collect();
    if attr_line >= lines.len() {
        return None;
    }

    let line_text = lines[attr_line];
    let trimmed = line_text.trim();

    // Check if this is the sole attribute on the line.
    if is_sole_override_attribute(trimmed) {
        // Remove the entire line (including newline).
        let start = line_byte_offset(content, attr_line);
        let end = if attr_line + 1 < lines.len() {
            line_byte_offset(content, attr_line + 1)
        } else {
            content.len()
        };

        let start_pos = offset_to_position(content, start);
        let end_pos = offset_to_position(content, end);

        Some(TextEdit {
            range: Range {
                start: start_pos,
                end: end_pos,
            },
            new_text: String::new(),
        })
    } else {
        // Multiple attributes on the line. Remove just the Override
        // token (with optional `\` prefix and surrounding comma/space).
        let new_line = remove_override_from_attribute_list(trimmed)?;

        // Preserve original indentation.
        let indent: String = line_text
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect();
        let replacement = format!("{}{}", indent, new_line);

        let start = line_byte_offset(content, attr_line);
        let end = start + line_text.len();

        let start_pos = offset_to_position(content, start);
        let end_pos = offset_to_position(content, end);

        Some(TextEdit {
            range: Range {
                start: start_pos,
                end: end_pos,
            },
            new_text: replacement,
        })
    }
}

/// Check whether the trimmed attribute line contains only `Override`
/// (possibly with a leading backslash) and no other attributes.
fn is_sole_override_attribute(trimmed: &str) -> bool {
    // Matches: #[Override], #[\Override], #[Override()], #[\Override()]
    let inner = trimmed.strip_prefix("#[").and_then(|s| s.strip_suffix(']'));
    let Some(inner) = inner else {
        return false;
    };
    let inner = inner.trim();
    let inner = strip_fqn_prefix(inner);
    // After stripping optional `\`, should be `Override` optionally
    // followed by `(...)`.
    if let Some(rest) = inner.strip_prefix("Override") {
        let rest = rest.trim();
        rest.is_empty() || (rest.starts_with('(') && rest.ends_with(')'))
    } else {
        false
    }
}

/// Remove `Override` (or `\Override`) from a multi-attribute line like
/// `#[Override, Deprecated]` → `#[Deprecated]`.
fn remove_override_from_attribute_list(trimmed: &str) -> Option<String> {
    let inner = trimmed
        .strip_prefix("#[")
        .and_then(|s| s.strip_suffix(']'))?;

    // Split on commas, preserving order.
    let parts: Vec<&str> = inner.split(',').collect();
    let mut kept: Vec<String> = Vec::new();

    for part in &parts {
        let p = part.trim();
        let without_backslash = strip_fqn_prefix(p);
        // Check if this part is `Override` optionally followed by `(...)`.
        let is_override = if let Some(rest) = without_backslash.strip_prefix("Override") {
            let rest = rest.trim();
            rest.is_empty() || (rest.starts_with('(') && rest.ends_with(')'))
        } else {
            false
        };

        if !is_override {
            kept.push(p.to_string());
        }
    }

    if kept.is_empty() {
        // All attributes were Override — shouldn't happen normally, but
        // handle gracefully.
        return None;
    }

    Some(format!("#[{}]", kept.join(", ")))
}

/// Compute the byte offset of the start of the given line number
/// (0-based).
fn line_byte_offset(content: &str, line: usize) -> usize {
    let mut offset = 0;
    for (i, l) in content.lines().enumerate() {
        if i == line {
            return offset;
        }
        offset += l.len() + 1; // +1 for newline
    }
    content.len()
}

/// Check whether a `method.override` / `property.override` diagnostic
/// is stale by verifying that the `#[Override]` attribute is still
/// present near the diagnostic line.
pub(crate) fn is_remove_override_stale(content: &str, diag_line: usize) -> bool {
    find_override_attribute_line(content, diag_line).is_none()
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_member_name ─────────────────────────────────────────

    #[test]
    fn extracts_method_name_from_method_override_message() {
        let msg =
            "Method App\\Foo::bar() has #[\\Override] attribute but does not override any method.";
        assert_eq!(extract_member_name(msg, METHOD_OVERRIDE_ID), Some("bar"));
    }

    #[test]
    fn extracts_property_name_from_property_override_message() {
        let msg = "Property App\\Foo::$baz has #[\\Override] attribute but does not override any property.";
        assert_eq!(extract_member_name(msg, PROPERTY_OVERRIDE_ID), Some("$baz"));
    }

    #[test]
    fn returns_none_for_unrelated_message() {
        let msg = "Some other PHPStan error.";
        assert_eq!(extract_member_name(msg, METHOD_OVERRIDE_ID), None);
    }

    #[test]
    fn returns_none_for_override_attribute_message() {
        let msg = "Attribute class Override can be used with properties only on PHP 8.5 and later.";
        assert_eq!(extract_member_name(msg, PROPERTY_OVERRIDE_ATTR_ID), None);
    }

    #[test]
    fn extracts_constructor_name() {
        let msg = "Method App\\Foo::__construct() has #[\\Override] attribute but does not override any method.";
        assert_eq!(
            extract_member_name(msg, METHOD_OVERRIDE_ID),
            Some("__construct")
        );
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

    // ── is_sole_override_attribute ──────────────────────────────────

    #[test]
    fn detects_sole_override() {
        assert!(is_sole_override_attribute("#[Override]"));
        assert!(is_sole_override_attribute("#[\\Override]"));
        assert!(is_sole_override_attribute("#[Override()]"));
        assert!(is_sole_override_attribute("#[\\Override()]"));
    }

    #[test]
    fn rejects_multi_attribute_as_sole() {
        assert!(!is_sole_override_attribute("#[Override, Deprecated]"));
        assert!(!is_sole_override_attribute("#[Deprecated, Override]"));
    }

    #[test]
    fn rejects_non_override_as_sole() {
        assert!(!is_sole_override_attribute("#[Deprecated]"));
        assert!(!is_sole_override_attribute("#[Route('/foo')]"));
    }

    // ── remove_override_from_attribute_list ──────────────────────────

    #[test]
    fn removes_override_first_in_list() {
        let result = remove_override_from_attribute_list("#[Override, Deprecated]");
        assert_eq!(result, Some("#[Deprecated]".to_string()));
    }

    #[test]
    fn removes_override_last_in_list() {
        let result = remove_override_from_attribute_list("#[Deprecated, Override]");
        assert_eq!(result, Some("#[Deprecated]".to_string()));
    }

    #[test]
    fn removes_backslash_override_from_list() {
        let result = remove_override_from_attribute_list("#[\\Override, Deprecated]");
        assert_eq!(result, Some("#[Deprecated]".to_string()));
    }

    #[test]
    fn removes_override_middle_of_list() {
        let result = remove_override_from_attribute_list("#[Route('/foo'), Override, Deprecated]");
        assert_eq!(result, Some("#[Route('/foo'), Deprecated]".to_string()));
    }

    #[test]
    fn returns_none_when_only_override() {
        let result = remove_override_from_attribute_list("#[Override]");
        assert_eq!(result, None);
    }

    // ── find_override_attribute_line ─────────────────────────────────

    #[test]
    fn finds_override_line_directly_above() {
        let content =
            "<?php\nclass Foo {\n    #[\\Override]\n    public function bar(): void {}\n}\n";
        // Diagnostic is on line 3 (the function line).
        assert_eq!(find_override_attribute_line(content, 3), Some(2));
    }

    #[test]
    fn finds_override_line_on_diag_line() {
        // Edge case: attribute is on the same line as reported diagnostic.
        let content = "<?php\n#[Override]\n";
        assert_eq!(find_override_attribute_line(content, 1), Some(1));
    }

    #[test]
    fn returns_none_when_no_override() {
        let content = "<?php\nclass Foo {\n    public function bar(): void {}\n}\n";
        assert_eq!(find_override_attribute_line(content, 2), None);
    }

    #[test]
    fn finds_override_with_other_attrs_between() {
        let content = "<?php\nclass Foo {\n    #[\\Override]\n    #[Route('/bar')]\n    public function bar(): void {}\n}\n";
        // Diagnostic on line 4, Override on line 2.
        assert_eq!(find_override_attribute_line(content, 4), Some(2));
    }

    // ── build_remove_override_edit ──────────────────────────────────

    #[test]
    fn removes_entire_line_for_sole_override() {
        let content =
            "<?php\nclass Foo {\n    #[\\Override]\n    public function bar(): void {}\n}\n";
        let edit = build_remove_override_edit(content, 2).unwrap();
        assert_eq!(edit.new_text, "");
        // The range should cover the entire `    #[\Override]\n` line.
        assert_eq!(edit.range.start.line, 2);
        assert_eq!(edit.range.start.character, 0);
        assert_eq!(edit.range.end.line, 3);
        assert_eq!(edit.range.end.character, 0);
    }

    #[test]
    fn removes_override_from_multi_attr_line() {
        let content = "<?php\nclass Foo {\n    #[Override, Deprecated]\n    public function bar(): void {}\n}\n";
        let edit = build_remove_override_edit(content, 2).unwrap();
        assert_eq!(edit.new_text, "    #[Deprecated]");
        assert_eq!(edit.range.start.line, 2);
        assert_eq!(edit.range.end.line, 2);
    }

    #[test]
    fn removes_backslash_override_from_multi_attr_line() {
        let content = "<?php\nclass Foo {\n    #[\\Override, Deprecated]\n    public function bar(): void {}\n}\n";
        let edit = build_remove_override_edit(content, 2).unwrap();
        assert_eq!(edit.new_text, "    #[Deprecated]");
    }

    // ── is_remove_override_stale ────────────────────────────────────

    #[test]
    fn stale_when_override_removed() {
        let content = "<?php\nclass Foo {\n    public function bar(): void {}\n}\n";
        assert!(is_remove_override_stale(content, 2));
    }

    #[test]
    fn not_stale_when_override_still_present() {
        let content =
            "<?php\nclass Foo {\n    #[\\Override]\n    public function bar(): void {}\n}\n";
        assert!(!is_remove_override_stale(content, 3));
    }

    // ── Integration: full code action via Backend ───────────────────

    #[test]
    fn offers_remove_override_action_for_method() {
        let backend = crate::Backend::defaults();
        let uri = "file:///test.php";
        let content = r#"<?php
class Foo {
    #[\Override]
    public function bar(): void {}
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
            code: Some(NumberOrString::String(METHOD_OVERRIDE_ID.to_string())),
            source: Some("PHPStan".to_string()),
            message:
                "Method Foo::bar() has #[\\Override] attribute but does not override any method."
                    .to_string(),
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
        let remove_action = actions.iter().find_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Remove #[Override]") => {
                Some(ca)
            }
            _ => None,
        });

        assert!(
            remove_action.is_some(),
            "should offer Remove #[Override] action"
        );

        let action = remove_action.unwrap();
        assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
        assert_eq!(action.is_preferred, Some(true));
        assert!(
            action.title.contains("bar"),
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
        // Should remove the entire `#[\Override]` line.
        assert_eq!(edits[0].new_text, "");
    }

    #[test]
    fn offers_remove_override_action_for_property() {
        let backend = crate::Backend::defaults();
        let uri = "file:///test.php";
        let content = r#"<?php
class Foo {
    #[\Override]
    public string $baz = '';
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
            code: Some(NumberOrString::String(PROPERTY_OVERRIDE_ID.to_string())),
            source: Some("PHPStan".to_string()),
            message:
                "Property Foo::$baz has #[\\Override] attribute but does not override any property."
                    .to_string(),
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
        let remove_action = actions.iter().find_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Remove #[Override]") => {
                Some(ca)
            }
            _ => None,
        });

        assert!(
            remove_action.is_some(),
            "should offer Remove #[Override] action for property"
        );

        let action = remove_action.unwrap();
        assert!(
            action.title.contains("$baz"),
            "title should mention property name: {}",
            action.title
        );

        // Phase 2: resolve the action.
        let (resolved, _) = backend.resolve_code_action(action.clone());
        let edit = resolved.edit.as_ref().expect("resolve should produce edit");
        let changes = edit.changes.as_ref().unwrap();
        let edits: Vec<&TextEdit> = changes.values().flat_map(|v| v.iter()).collect();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "");
    }

    #[test]
    fn offers_remove_override_action_for_override_attribute_on_property() {
        let backend = crate::Backend::defaults();
        let uri = "file:///test.php";
        let content = r#"<?php
class Foo {
    #[\Override]
    public string $baz = '';
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
            code: Some(NumberOrString::String(
                PROPERTY_OVERRIDE_ATTR_ID.to_string(),
            )),
            source: Some("PHPStan".to_string()),
            message:
                "Attribute class Override can be used with properties only on PHP 8.5 and later."
                    .to_string(),
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
        let remove_actions: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Remove #[Override]") => {
                    Some(ca)
                }
                _ => None,
            })
            .collect();

        assert_eq!(
            remove_actions.len(),
            1,
            "should offer exactly one Remove #[Override] action"
        );

        let action = remove_actions[0];
        // Even though the overrideAttribute message doesn't contain
        // the property name, the title should still be generic when
        // it's the only diagnostic.
        assert_eq!(action.title, "Remove #[Override]");
        assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
        assert_eq!(action.is_preferred, Some(true));

        // Phase 2: resolve the action.
        let (resolved, _) = backend.resolve_code_action(action.clone());
        let edit = resolved.edit.as_ref().expect("resolve should produce edit");
        let changes = edit.changes.as_ref().unwrap();
        let edits: Vec<&TextEdit> = changes.values().flat_map(|v| v.iter()).collect();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "");
    }

    #[test]
    fn deduplicates_property_override_and_override_attribute() {
        let backend = crate::Backend::defaults();
        let uri = "file:///test.php";
        let content = r#"<?php
class Foo {
    #[\Override]
    public string $baz = '';
}
"#;
        backend.update_ast(uri, content);
        backend
            .open_files
            .write()
            .insert(uri.to_string(), std::sync::Arc::new(content.to_string()));

        // PHPStan reports both identifiers on the same line.
        let diag1 = Diagnostic {
            range: Range {
                start: Position::new(3, 0),
                end: Position::new(3, 80),
            },
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(PROPERTY_OVERRIDE_ID.to_string())),
            source: Some("PHPStan".to_string()),
            message:
                "Property Foo::$baz has #[\\Override] attribute but does not override any property."
                    .to_string(),
            ..Default::default()
        };
        let diag2 = Diagnostic {
            range: Range {
                start: Position::new(3, 0),
                end: Position::new(3, 80),
            },
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(
                PROPERTY_OVERRIDE_ATTR_ID.to_string(),
            )),
            source: Some("PHPStan".to_string()),
            message:
                "Attribute class Override can be used with properties only on PHP 8.5 and later."
                    .to_string(),
            ..Default::default()
        };
        {
            let mut cache = backend.phpstan_last_diags().lock();
            let entry = cache.entry(uri.to_string()).or_default();
            entry.push(diag1);
            entry.push(diag2);
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
        let remove_actions: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Remove #[Override]") => {
                    Some(ca)
                }
                _ => None,
            })
            .collect();

        // Should produce exactly ONE action, not two.
        assert_eq!(
            remove_actions.len(),
            1,
            "should deduplicate into a single action, got: {:?}",
            remove_actions.iter().map(|a| &a.title).collect::<Vec<_>>()
        );

        let action = remove_actions[0];
        // Title should include the property name extracted from the
        // property.override diagnostic.
        assert!(
            action.title.contains("$baz"),
            "title should mention property name: {}",
            action.title
        );

        // Both diagnostics should be attached.
        let attached = action.diagnostics.as_ref().unwrap();
        assert_eq!(
            attached.len(),
            2,
            "should attach both diagnostics to the action"
        );

        // Phase 2: resolve clears both.
        let (resolved, _) = backend.resolve_code_action(action.clone());
        let edit = resolved.edit.as_ref().expect("resolve should produce edit");
        let changes = edit.changes.as_ref().unwrap();
        let edits: Vec<&TextEdit> = changes.values().flat_map(|v| v.iter()).collect();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "");
    }

    #[test]
    fn no_action_when_override_already_removed() {
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
            code: Some(NumberOrString::String(METHOD_OVERRIDE_ID.to_string())),
            source: Some("PHPStan".to_string()),
            message:
                "Method Foo::bar() has #[\\Override] attribute but does not override any method."
                    .to_string(),
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
        let remove_action = actions.iter().find_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Remove #[Override]") => {
                Some(ca)
            }
            _ => None,
        });

        assert!(
            remove_action.is_none(),
            "should NOT offer action when #[Override] already removed"
        );
    }

    #[test]
    fn no_action_for_other_identifiers() {
        let backend = crate::Backend::defaults();
        let uri = "file:///test.php";
        let content = r#"<?php
class Foo {
    #[\Override]
    public function bar(): void {}
}
"#;
        backend.update_ast(uri, content);

        let diag = Diagnostic {
            range: Range {
                start: Position::new(3, 0),
                end: Position::new(3, 80),
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
        let remove_action = actions.iter().find_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Remove #[Override]") => {
                Some(ca)
            }
            _ => None,
        });

        assert!(
            remove_action.is_none(),
            "should NOT offer action for non-override identifiers"
        );
    }

    #[test]
    fn removes_override_from_shared_attribute_line() {
        let backend = crate::Backend::defaults();
        let uri = "file:///test.php";
        let content = r#"<?php
class Foo {
    #[Override, Deprecated]
    public function bar(): void {}
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
            code: Some(NumberOrString::String(METHOD_OVERRIDE_ID.to_string())),
            source: Some("PHPStan".to_string()),
            message:
                "Method Foo::bar() has #[\\Override] attribute but does not override any method."
                    .to_string(),
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
                CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Remove #[Override]") => {
                    Some(ca)
                }
                _ => None,
            })
            .expect("should offer action");

        // Phase 2: resolve.
        let (resolved, _) = backend.resolve_code_action(action.clone());
        let edit = resolved.edit.as_ref().expect("resolve should produce edit");
        let changes = edit.changes.as_ref().unwrap();
        let edits: Vec<&TextEdit> = changes.values().flat_map(|v| v.iter()).collect();

        assert_eq!(edits.len(), 1);
        // Should keep the other attribute but remove Override.
        assert_eq!(edits[0].new_text, "    #[Deprecated]");
    }
}
