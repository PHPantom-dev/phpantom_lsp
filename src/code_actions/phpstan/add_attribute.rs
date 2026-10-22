//! "Add `#[Override]`" and "Add `#[\ReturnTypeWillChange]`" code actions
//! for PHPStan `method.missingOverride` and `method.tentativeReturnType`.
//!
//! When PHPStan reports that a method overrides a parent/interface method
//! but is missing the `#[\Override]` attribute (PHP 8.3+), or that it has
//! a tentative return type `#[\ReturnTypeWillChange]` would silence, this
//! code action offers to insert the attribute on the line above the
//! method declaration, with correct indentation.
//!
//! **Trigger:** A PHPStan diagnostic with one of the identifiers in
//! [`MISSING_ATTRIBUTES`] overlaps the cursor.
//!
//! **Code action kind:** `quickfix`.
//!
//! ## Two-phase resolve
//!
//! Phase 1 (`collect_add_attribute_actions`) performs all validation and
//! emits a lightweight `CodeAction` with a `data` payload but no `edit`.
//! Phase 2 (`resolve_add_attribute`) recomputes the workspace edit on
//! demand when the user picks the action.

use tower_lsp::lsp_types::*;

use super::attributes::attributed_member_at;
use crate::Backend;
use crate::code_actions::phpstan::{phpstan_identifier, phpstan_member_name};
use crate::code_actions::{CodeActionData, deferred_quickfix};
use crate::completion::use_edit::{
    analyze_use_block_in, build_use_edit, short_name_needs_import, use_import_conflicts,
};
use crate::text_position::{line_ending, offset_to_position};

/// An attribute a PHPStan diagnostic asks to have added to a method.
struct MissingAttribute {
    /// The PHPStan identifier that asks for it.
    identifier: &'static str,
    /// The `action_kind` of the deferred action.
    action_kind: &'static str,
    /// The attribute's class name, which lives in the global namespace.
    name: &'static str,
    /// Whether a namespaced file imports the attribute with `use` and
    /// writes the short name, the way it would any other global class.
    /// Otherwise it is always written fully qualified.
    import: bool,
}

const MISSING_ATTRIBUTES: &[MissingAttribute] = &[
    MissingAttribute {
        identifier: "method.missingOverride",
        action_kind: "phpstan.addOverride",
        name: "Override",
        import: true,
    },
    MissingAttribute {
        identifier: "method.tentativeReturnType",
        action_kind: "phpstan.addReturnTypeWillChange",
        name: "ReturnTypeWillChange",
        import: false,
    },
];

impl MissingAttribute {
    fn for_identifier(identifier: &str) -> Option<&'static Self> {
        MISSING_ATTRIBUTES
            .iter()
            .find(|attribute| attribute.identifier == identifier)
    }

    fn for_action_kind(action_kind: &str) -> Option<&'static Self> {
        MISSING_ATTRIBUTES
            .iter()
            .find(|attribute| attribute.action_kind == action_kind)
    }
}

impl Backend {
    /// Collect "Add `#[Override]`" and "Add `#[\ReturnTypeWillChange]`"
    /// code actions for the PHPStan diagnostics that ask for them.
    ///
    /// **Phase 1**: validates the action is applicable and emits a
    /// lightweight `CodeAction` with a `data` payload but **no `edit`**.
    /// The edit is computed lazily in [`resolve_add_attribute`](Self::resolve_add_attribute).
    pub(crate) fn collect_add_attribute_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let phpstan_diags = self.phpstan_diags_overlapping(uri, &params.range);

        for diag in &phpstan_diags {
            let Some(attribute) =
                phpstan_identifier(diag).and_then(MissingAttribute::for_identifier)
            else {
                continue;
            };

            // The diagnostic range covers the method signature line.
            let diag_line = diag.range.start.line as usize;
            let Some(method) = attributed_member_at(content, diag_line) else {
                continue;
            };

            // The attribute may have been added by hand since PHPStan
            // last ran.
            if method.has_attribute(attribute.name) {
                continue;
            }

            // ── Phase 1: emit lightweight action with data ──────────
            let method_name = phpstan_member_name(&diag.message).unwrap_or("method");
            let written = if attribute.import { "" } else { "\\" };
            let title = format!("Add #[{written}{}] to {method_name}", attribute.name);

            out.push(deferred_quickfix(
                title,
                attribute.action_kind,
                uri,
                &params.range,
                vec![diag.clone()],
                true,
                serde_json::Value::Null,
            ));
        }
    }

    /// Resolve an "Add `#[Override]`" or "Add `#[\ReturnTypeWillChange]`"
    /// code action by computing the full workspace edit.
    ///
    /// **Phase 2**: called from
    /// [`resolve_code_action`](Self::resolve_code_action) when the user
    /// picks this action.  Recomputes the attribute insertion edit and
    /// (optionally) the import edit from the data payload.
    pub(crate) fn resolve_add_attribute(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let attribute = MissingAttribute::for_action_kind(&data.action_kind)?;
        let uri = &data.uri;

        let diag_line = data.extra.get("diagnostic_line")?.as_u64()? as usize;

        let method = attributed_member_at(content, diag_line)?;

        // If the attribute was added since the action was offered, bail out.
        if method.has_attribute(attribute.name) {
            return None;
        }

        let block = self.import_block_at(uri, method.line_start);
        let file_use_map = &block.use_map;
        let file_namespace = &block.namespace;

        // Decide whether to use the short form `#[Override]` with a
        // `use Override;` import, or the FQN `#[\Override]`.
        //
        // The attribute lives in the global namespace.  When the file
        // declares a namespace we need a `use Override;` import (just
        // like any other global class), unless a different class of the
        // same short name is already imported.  When the file has no
        // namespace, no import is needed.
        let needs_import = attribute.import
            && short_name_needs_import(attribute.name, file_use_map, file_namespace);
        let use_fqn = !attribute.import
            || (needs_import && use_import_conflicts(attribute.name, file_use_map));
        let written = if use_fqn { "\\" } else { "" };

        // Build the text edit: insert `<indent>#[Override]` on a line of
        // its own at the start of the method declaration line (before any
        // existing attributes or modifiers).
        let insert_text = format!(
            "{}#[{written}{}]{}",
            method.indent,
            attribute.name,
            line_ending(content)
        );

        let insert_pos = offset_to_position(content, method.line_start);

        let mut edits = vec![TextEdit {
            range: Range {
                start: insert_pos,
                end: insert_pos,
            },
            new_text: insert_text,
        }];

        if needs_import && !use_fqn {
            let use_block = analyze_use_block_in(content, block.range);
            if let Some(import_edits) = build_use_edit(attribute.name, &use_block, file_namespace) {
                edits.extend(import_edits);
            }
        }

        let doc_uri: Url = uri.parse().ok()?;
        Some(crate::code_actions::single_file_edit(doc_uri, edits))
    }
}

/// Check whether a diagnostic asking for an attribute is stale because
/// the attribute is now written on the method.  `None` when `identifier`
/// asks for no attribute.
pub(crate) fn is_add_attribute_stale(
    content: &str,
    diag_line: usize,
    identifier: &str,
) -> Option<bool> {
    let attribute = MissingAttribute::for_identifier(identifier)?;
    Some(
        attributed_member_at(content, diag_line)
            .is_some_and(|method| method.has_attribute(attribute.name)),
    )
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_once_the_attribute_is_written() {
        let content = "<?php\nclass Foo extends Base {\n    #[\\Override]\n    public function bar(): void {}\n}\n";
        assert_eq!(
            is_add_attribute_stale(content, 3, "method.missingOverride"),
            Some(true)
        );
        assert_eq!(
            is_add_attribute_stale(content, 3, "method.tentativeReturnType"),
            Some(false)
        );
        assert_eq!(is_add_attribute_stale(content, 3, "return.type"), None);
    }
}
