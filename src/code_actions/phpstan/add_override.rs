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

use tower_lsp::lsp_types::*;

use super::attributes::attributed_member_at;
use crate::Backend;
use crate::code_actions::phpstan::phpstan_identifier;
use crate::code_actions::{CodeActionData, make_code_action_data};
use crate::completion::use_edit::{
    analyze_use_block_in, build_use_edit, short_name_needs_import, use_import_conflicts,
};
use crate::text_position::{line_ending, offset_to_position};

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
        let phpstan_diags = self.phpstan_diags_overlapping(uri, &params.range);

        for diag in &phpstan_diags {
            let Some(identifier) = phpstan_identifier(diag) else {
                continue;
            };

            if identifier != MISSING_OVERRIDE_ID {
                continue;
            }

            // The diagnostic range covers the method signature line.
            let diag_line = diag.range.start.line as usize;
            let Some(method) = attributed_member_at(content, diag_line) else {
                continue;
            };

            // `#[Override]` may have been added by hand since PHPStan
            // last ran.
            if method.has_attribute("Override") {
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

        let method = attributed_member_at(content, diag_line)?;

        // If Override was added since the action was offered, bail out.
        if method.has_attribute("Override") {
            return None;
        }

        let block = self.import_block_at(uri, method.line_start);
        let file_use_map = &block.use_map;
        let file_namespace = &block.namespace;

        // Decide whether to use the short form `#[Override]` with a
        // `use Override;` import, or the FQN `#[\Override]`.
        //
        // `Override` lives in the global namespace.  When the file
        // declares a namespace we need a `use Override;` import
        // (just like any other global class).  When the file has no
        // namespace, no import is needed.
        let needs_import = short_name_needs_import("Override", file_use_map, file_namespace);

        // Check for import conflicts (e.g. a different class named
        // `Override` is already imported).
        let use_fqn = needs_import && use_import_conflicts("Override", file_use_map);

        let attr_text = if use_fqn {
            "#[\\Override]"
        } else {
            "#[Override]"
        };

        // Build the text edit: insert `<indent>#[Override]` on a line of
        // its own at the start of the method declaration line (before any
        // existing attributes or modifiers).
        let insert_text = format!("{}{}{}", method.indent, attr_text, line_ending(content));

        let insert_pos = offset_to_position(content, method.line_start);

        let mut edits = vec![TextEdit {
            range: Range {
                start: insert_pos,
                end: insert_pos,
            },
            new_text: insert_text,
        }];

        // Add `use Override;` import when needed and possible.
        if needs_import && !use_fqn {
            let use_block = analyze_use_block_in(content, block.range);
            if let Some(import_edits) = build_use_edit("Override", &use_block, file_namespace) {
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

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
}
