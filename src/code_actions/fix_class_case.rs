use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::diagnostics::class_case_mismatch::CLASS_CASE_MISMATCH_CODE;
use crate::diagnostics::helpers::make_diagnostic;

impl Backend {
    /// Offer a quick fix that rewrites a mis-cased class reference to its
    /// canonical casing so it autoloads on case-sensitive filesystems.
    pub(crate) fn collect_fix_class_case_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let mismatches = self.class_case_mismatches(uri, content);
        if mismatches.is_empty() {
            return;
        }
        let parsed_uri = match Url::parse(uri) {
            Ok(u) => u,
            Err(_) => return,
        };

        for m in mismatches {
            if !ranges_overlap(&m.range, &params.range) {
                continue;
            }

            let diag = make_diagnostic(
                m.range,
                DiagnosticSeverity::WARNING,
                CLASS_CASE_MISMATCH_CODE,
                m.message.clone(),
            );

            let mut changes = HashMap::new();
            changes.insert(
                parsed_uri.clone(),
                vec![TextEdit {
                    range: m.range,
                    new_text: m.corrected.clone(),
                }],
            );

            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: format!("Fix case to `{}`", m.corrected),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![diag]),
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..Default::default()
                }),
                is_preferred: Some(true),
                ..Default::default()
            }));
        }
    }
}

/// Whether two ranges intersect (touching counts as overlapping so a
/// zero-width cursor at either edge of the reference still matches).
fn ranges_overlap(a: &Range, b: &Range) -> bool {
    !(position_before(&a.end, &b.start) || position_before(&b.end, &a.start))
}

fn position_before(a: &Position, b: &Position) -> bool {
    a.line < b.line || (a.line == b.line && a.character < b.character)
}
