//! Quick-fix for a private member nothing uses: remove its declaration,
//! docblock and attributes included.
//!
//! Offered on the `unused_property`, `unused_method`, and `unused_constant`
//! diagnostics when removing the member leaves working code: not for a
//! property the class still writes, or one a constructor promotes.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::diagnostics::helpers::FileDiagnosticContext;
use crate::diagnostics::unused_private_members::MemberKind;
use crate::text_position::{offset_to_position, ranges_overlap};

impl Backend {
    pub(crate) fn collect_remove_unused_member_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionResponse>,
    ) {
        // A file without a private member has nothing to offer, and the
        // scan below walks the whole file.
        if !content.contains("private") {
            return;
        }
        let Ok(doc_uri) = uri.parse::<Uri>() else {
            return;
        };

        let ctx = FileDiagnosticContext::gather(self, uri);
        for member in self.unused_private_members(&ctx, uri, content) {
            let Some((from, to)) = member.removal else {
                continue;
            };
            let Some(diagnostic) = self.unused_member_diagnostic(uri, content, &member) else {
                continue;
            };
            let range = diagnostic.range;
            // A bare cursor touching either end of the name is on it.
            let touches = if params.range.start == params.range.end {
                range.start <= params.range.start && params.range.start <= range.end
            } else {
                ranges_overlap(&range, &params.range)
            };
            if !touches {
                continue;
            }
            let what = match member.kind {
                MemberKind::Property => "property",
                MemberKind::Method => "method",
                MemberKind::Constant => "constant",
            };
            let edit = TextEdit {
                range: Range {
                    start: offset_to_position(content, from as usize),
                    end: offset_to_position(content, to as usize),
                },
                new_text: String::new(),
            };
            out.push(CodeActionResponse::CodeAction(CodeAction {
                title: format!("Remove unused {what} {}", member.label),
                kind: Some(CodeActionKind::QuickFix),
                diagnostics: Some(vec![diagnostic]),
                edit: Some(super::single_file_edit(doc_uri.clone(), vec![edit])),
                command: None,
                is_preferred: Some(true),
                disabled: None,
                data: None,
                tags: None,
            }));
        }
    }
}
