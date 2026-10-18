//! "Add `#[\ReturnTypeWillChange]`" code action for PHPStan
//! `method.tentativeReturnType`.
//!
//! When PHPStan reports that a method has a tentative return type and
//! suggests using `#[\ReturnTypeWillChange]` to suppress the error,
//! this code action offers to insert the attribute on the line above
//! the method declaration, with correct indentation.
//!
//! **Trigger:** A PHPStan diagnostic with identifier
//! `method.tentativeReturnType` overlaps the cursor.
//!
//! **Code action kind:** `quickfix`.
//!
//! ## Two-phase resolve
//!
//! Phase 1 (`collect_add_return_type_will_change_actions`) validates
//! and emits a lightweight `CodeAction` with a `data` payload but no
//! `edit`.  Phase 2 (`resolve_add_return_type_will_change`) computes
//! the workspace edit on demand when the user picks the action.

use tower_lsp::lsp_types::*;

use super::attributes::attributed_member_at;
use crate::Backend;
use crate::code_actions::phpstan::phpstan_identifier;
use crate::code_actions::{CodeActionData, make_code_action_data};
use crate::text_position::{line_ending, offset_to_position};

/// The PHPStan identifier we match on.
const TENTATIVE_RETURN_TYPE_ID: &str = "method.tentativeReturnType";

/// The attribute to insert (always FQN — `ReturnTypeWillChange` lives
/// in the global namespace and has no short-form import convention).
const ATTRIBUTE_TEXT: &str = "#[\\ReturnTypeWillChange]";

/// The attribute's name, however it is spelled on the method.
const ATTRIBUTE_NAME: &str = "ReturnTypeWillChange";

impl Backend {
    /// Collect "Add `#[\\ReturnTypeWillChange]`" code actions for
    /// PHPStan `method.tentativeReturnType` diagnostics.
    ///
    /// **Phase 1**: validates the action is applicable and emits a
    /// lightweight `CodeAction` with a `data` payload but **no `edit`**.
    /// The edit is computed lazily in
    /// [`resolve_add_return_type_will_change`](Self::resolve_add_return_type_will_change).
    pub(crate) fn collect_add_return_type_will_change_actions(
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

            if identifier != TENTATIVE_RETURN_TYPE_ID {
                continue;
            }

            let diag_line = diag.range.start.line as usize;
            let Some(method) = attributed_member_at(content, diag_line) else {
                continue;
            };

            // If the attribute is already present (user added it
            // manually since PHPStan last ran), skip.
            if method.has_attribute(ATTRIBUTE_NAME) {
                continue;
            }

            let method_name = extract_method_name(&diag.message).unwrap_or("method");
            let title = format!("Add {} to {}", ATTRIBUTE_TEXT, method_name);

            let extra = serde_json::json!({
                "diagnostic_message": diag.message,
                "diagnostic_line": diag.range.start.line,
                "diagnostic_code": TENTATIVE_RETURN_TYPE_ID,
            });

            let data =
                make_code_action_data("phpstan.addReturnTypeWillChange", uri, &params.range, extra);

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

    /// Resolve the "Add `#[\\ReturnTypeWillChange]`" code action by
    /// computing the full workspace edit.
    ///
    /// **Phase 2**: called from
    /// [`resolve_code_action`](Self::resolve_code_action) when the user
    /// picks this action.
    pub(crate) fn resolve_add_return_type_will_change(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let uri = &data.uri;
        let diag_line = data.extra.get("diagnostic_line")?.as_u64()? as usize;

        let method = attributed_member_at(content, diag_line)?;

        // If the attribute was added since the action was offered, bail.
        if method.has_attribute(ATTRIBUTE_NAME) {
            return None;
        }

        // Build the text edit: insert `<indent>#[\ReturnTypeWillChange]` on
        // a line of its own at the insertion point (before any existing
        // attributes or modifiers).
        let insert_text = format!(
            "{}{}{}",
            method.indent,
            ATTRIBUTE_TEXT,
            line_ending(content)
        );
        let insert_pos = offset_to_position(content, method.line_start);

        let edits = vec![TextEdit {
            range: Range {
                start: insert_pos,
                end: insert_pos,
            },
            new_text: insert_text,
        }];

        let doc_uri: Url = uri.parse().ok()?;
        Some(crate::code_actions::single_file_edit(doc_uri, edits))
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Extract the method name from a PHPStan `method.tentativeReturnType`
/// message.
///
/// Expected format:
/// - `"Return type (int) of method Foo::count() should be covariant
///    with return type (int) of method Countable::count()"`
///
/// We extract just the short method name from the first `::name()`
/// occurrence.
fn extract_method_name(message: &str) -> Option<&str> {
    // Find `method <Class>::<name>()` — the first occurrence is the
    // overriding method.
    let marker = "method ";
    let pos = message.find(marker)?;
    let after = &message[pos + marker.len()..];
    let paren_pos = after.find('(')?;
    let class_and_name = &after[..paren_pos];
    let name = class_and_name.rsplit("::").next()?;
    if name.is_empty() {
        return None;
    }
    Some(name)
}

/// Check whether a `method.tentativeReturnType` diagnostic is stale
/// by verifying that the `#[\ReturnTypeWillChange]` attribute is now
/// written on the method.
pub(crate) fn is_add_return_type_will_change_stale(content: &str, diag_line: usize) -> bool {
    attributed_member_at(content, diag_line)
        .is_some_and(|method| method.has_attribute(ATTRIBUTE_NAME))
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_method_name ─────────────────────────────────────────

    #[test]
    fn extracts_method_name_from_tentative_message() {
        let msg = "Return type (int) of method Foo::count() should be covariant with return type (int) of method Countable::count()";
        assert_eq!(extract_method_name(msg), Some("count"));
    }

    #[test]
    fn extracts_method_name_with_namespace() {
        let msg = "Return type (array) of method App\\Collection::toArray() should be covariant with return type (array) of method ArrayAccess::toArray()";
        assert_eq!(extract_method_name(msg), Some("toArray"));
    }

    #[test]
    fn returns_none_for_unrelated_message() {
        let msg = "Some other PHPStan error about something.";
        assert_eq!(extract_method_name(msg), None);
    }
}
