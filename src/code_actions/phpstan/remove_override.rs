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
use crate::code_actions::phpstan::phpstan_identifier;
use crate::code_actions::{CodeActionData, make_code_action_data};

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

        let edit =
            attributed_member_at(content, diag_line)?.remove_attribute_edit(content, "Override")?;

        let doc_uri: Url = uri.parse().ok()?;
        Some(crate::code_actions::single_file_edit(doc_uri, vec![edit]))
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

/// Check whether a `method.override` / `property.override` diagnostic
/// is stale by verifying that the member still carries `#[Override]`.
pub(crate) fn is_remove_override_stale(content: &str, diag_line: usize) -> bool {
    !attributed_member_at(content, diag_line).is_some_and(|member| member.has_attribute("Override"))
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
