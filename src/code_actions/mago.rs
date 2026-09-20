//! Mago quick-fix code actions.
//!
//! Converts fix edits attached to Mago diagnostics (`"mago-lint"` / `"mago-analyze"`)
//! into LSP quick-fix code actions.

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::code_actions::single_file_edit;

/// The safety level of a Mago fix edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Safety {
    /// The fix is always correct.
    Safe,
    /// The fix might change semantics in edge cases.
    PotentiallyUnsafe,
    /// The fix is likely to change semantics.
    Unsafe,
}

impl Safety {
    /// Parse a safety string from the diagnostic data JSON.
    fn from_str(s: &str) -> Self {
        match s {
            "PotentiallyUnsafe" => Self::PotentiallyUnsafe,
            "Unsafe" => Self::Unsafe,
            _ => Self::Safe,
        }
    }
}

impl Backend {
    /// Collect quick-fix code actions from Mago diagnostics that carry fix edits.
    ///
    /// For each diagnostic whose source is `"mago-lint"` or `"mago-analyze"`, this
    /// method checks for an `"edits"` array in the diagnostic's `data` field and
    /// converts byte-offset edits into LSP `TextEdit`s wrapped in a `CodeAction`.
    pub(crate) fn collect_mago_fix_actions(
        &self,
        _uri: &str,
        content: &str,
        params: &CodeActionParams,
        actions: &mut Vec<CodeActionOrCommand>,
    ) {
        let document_uri = &params.text_document.uri;

        for diag in &params.context.diagnostics {
            let source = match &diag.source {
                Some(s) => s.as_str(),
                None => continue,
            };
            if source != "mago-lint" && source != "mago-analyze" {
                continue;
            }

            let data = match &diag.data {
                Some(d) => d,
                None => continue,
            };

            let edits_json = match data.get("mago_edits") {
                Some(serde_json::Value::Array(arr)) => arr,
                _ => continue,
            };

            let mut text_edits = Vec::new();
            let mut worst_safety = Safety::Safe;

            for edit_val in edits_json {
                let start = match edit_val.get("start").and_then(|v| v.as_u64()) {
                    Some(v) => v as usize,
                    None => continue,
                };
                let end = match edit_val.get("end").and_then(|v| v.as_u64()) {
                    Some(v) => v as usize,
                    None => continue,
                };
                let new_text = match edit_val.get("new_text").and_then(|v| v.as_str()) {
                    Some(s) => s.to_string(),
                    None => continue,
                };
                let safety = edit_val
                    .get("safety")
                    .and_then(|v| v.as_str())
                    .map(Safety::from_str)
                    .unwrap_or(Safety::Safe);

                if safety > worst_safety {
                    worst_safety = safety;
                }

                let start_pos = crate::text_position::offset_to_position(content, start);
                let end_pos = crate::text_position::offset_to_position(content, end);

                text_edits.push(TextEdit {
                    range: Range::new(start_pos, end_pos),
                    new_text,
                });
            }

            if text_edits.is_empty() {
                continue;
            }

            let code_label = diag.code.as_ref().map(|c| match c {
                NumberOrString::Number(n) => n.to_string(),
                NumberOrString::String(s) => s.clone(),
            });

            let mut title = match &code_label {
                Some(code) => format!("Mago fix: {code}"),
                None => "Mago fix".to_string(),
            };

            let is_preferred = match worst_safety {
                Safety::Safe => Some(true),
                Safety::PotentiallyUnsafe => {
                    title.push_str(" (potentially unsafe)");
                    None
                }
                Safety::Unsafe => {
                    title.push_str(" (unsafe)");
                    None
                }
            };

            let action = CodeAction {
                title,
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![diag.clone()]),
                edit: Some(single_file_edit(document_uri.clone(), text_edits)),
                is_preferred,
                ..Default::default()
            };

            actions.push(CodeActionOrCommand::CodeAction(action));
        }
    }
}
