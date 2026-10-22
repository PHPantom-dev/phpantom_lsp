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

use tower_lsp::lsp_types::*;

use super::attributes::attributed_member_at;
use crate::Backend;
use crate::code_actions::phpstan::{phpstan_identifier, phpstan_member_name};
use crate::code_actions::{CodeActionData, deferred_quickfix};

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
        let phpstan_diags = self.phpstan_diags_overlapping(uri, &params.range);

        // Group matching diagnostics by line so that overlapping
        // identifiers (e.g. `property.override` and
        // `property.overrideAttribute` on the same property) produce
        // a single code action that clears all of them at once.
        let mut by_line: std::collections::BTreeMap<u32, Vec<&Diagnostic>> =
            std::collections::BTreeMap::new();

        for diag in &phpstan_diags {
            let Some(identifier) = phpstan_identifier(diag) else {
                continue;
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

            // Check that the member still carries `#[Override]`. If the
            // user already removed it manually, don't offer the action.
            if !attributed_member_at(content, diag_line)
                .is_some_and(|member| member.has_attribute("Override"))
            {
                continue;
            }

            // Try to extract a readable member name from any of the
            // grouped diagnostics (some identifiers like
            // `property.overrideAttribute` don't include the name).
            let member_name = diags.iter().find_map(|d| phpstan_member_name(&d.message));

            let title = match member_name {
                Some(name) => format!("Remove #[Override] from {}", name),
                None => "Remove #[Override]".to_string(),
            };

            out.push(deferred_quickfix(
                title,
                "phpstan.removeOverride",
                uri,
                &params.range,
                diags.iter().map(|d| (*d).clone()).collect(),
                true,
                serde_json::Value::Null,
            ));
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

        let edit =
            attributed_member_at(content, diag_line)?.remove_attribute_edit(content, "Override")?;

        let doc_uri: Url = uri.parse().ok()?;
        Some(crate::code_actions::single_file_edit(doc_uri, vec![edit]))
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Check whether a `method.override` / `property.override` diagnostic
/// is stale by verifying that the member still carries `#[Override]`.
pub(crate) fn is_remove_override_stale(content: &str, diag_line: usize) -> bool {
    !attributed_member_at(content, diag_line).is_some_and(|member| member.has_attribute("Override"))
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
}
