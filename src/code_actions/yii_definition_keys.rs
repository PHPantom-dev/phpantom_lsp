//! Quick-fix for a Yii DI definition key that names a member without
//! saying which kind: add the `()` a method key needs or the `$` a
//! property key needs (see [`crate::diagnostics::yii_definitions`]).

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::diagnostics::helpers::{FileDiagnosticContext, make_diagnostic};
use crate::diagnostics::yii_definitions::INVALID_YII_DEFINITION_KEY_CODE;
use crate::text_position::ranges_overlap;

impl Backend {
    pub(crate) fn collect_yii_definition_key_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionResponse>,
    ) {
        if !self.is_yii_project() {
            return;
        }
        let ctx = FileDiagnosticContext::gather(self, uri);
        if ctx.symbol_map.yii_definition_bodies.is_empty() {
            return;
        }
        let Some(doc_uri) = crate::util::parse_url(uri) else {
            return;
        };
        for invalid in self.invalid_yii_definition_keys(&ctx, content) {
            let Some(suggestion) = &invalid.suggestion else {
                continue;
            };
            // A key written with an escape is not the text it spells, so
            // rewriting the text would change more than the kind.
            if content.get(invalid.start as usize..invalid.end as usize) != Some(&*invalid.key) {
                continue;
            }
            let Some(range) = self.offset_range_to_lsp_range(
                uri,
                content,
                invalid.start as usize,
                invalid.end as usize,
            ) else {
                continue;
            };
            // A bare cursor touching either end of the key is on it.
            let touches = if params.range.start == params.range.end {
                range.start <= params.range.start && params.range.start <= range.end
            } else {
                ranges_overlap(&range, &params.range)
            };
            if !touches {
                continue;
            }
            // `$` starts a variable inside double quotes.
            let double_quoted = invalid
                .start
                .checked_sub(1)
                .and_then(|quote| content.as_bytes().get(quote as usize))
                == Some(&b'"');
            let new_text = if double_quoted {
                suggestion.replace('$', "\\$")
            } else {
                suggestion.clone()
            };
            let diagnostic = make_diagnostic(
                range,
                DiagnosticSeverity::Error,
                INVALID_YII_DEFINITION_KEY_CODE,
                invalid.message(),
            );
            out.push(CodeActionResponse::CodeAction(CodeAction {
                title: format!("Change key to '{suggestion}'"),
                kind: Some(CodeActionKind::QuickFix),
                diagnostics: Some(vec![diagnostic]),
                edit: Some(super::single_edit(doc_uri.clone(), range, new_text)),
                is_preferred: Some(true),
                ..Default::default()
            }));
        }
    }
}
