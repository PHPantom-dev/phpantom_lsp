//! Completion for the arguments a Yii route's path declares.
//!
//! The arguments array of `$urls->generate('blog/post', ['|' => …])`, and of
//! `generateAbsolute()`, is keyed by the placeholders of the named route's
//! path, which the route side tables keep beside each name (see
//! [`crate::symbol_map::RouteSite`]).  The call is recognised by its
//! receiver, as its route name is.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::completion::eloquent_string::detect_string_call_context;
use crate::completion::source::code_context::CodeContext;
use crate::completion::source::helpers::enclosing_array_key_call;
use crate::symbol_map::StringKeyKind;
use crate::text_position::{offset_to_position, position_to_offset};
use crate::types::FileContext;

impl Backend {
    /// Complete a key of the arguments array of a URL generator call.
    ///
    /// `None` when the cursor is not on such a key, or when the route's
    /// path declares no arguments.
    pub(crate) fn try_yii_route_param_completion(
        &self,
        content: &str,
        position: Position,
        code: &CodeContext<'_>,
        ctx: &FileContext,
    ) -> Option<CompletionResponse> {
        let cursor = position_to_offset(content, position) as usize;
        let (quote, _) = code.open_string?;
        if !matches!(code.last_code_byte(), Some(b'[' | b',')) {
            return None;
        }
        // The array is the call's second argument, right inside it.
        code.nested_pair(b'[', b'(')?;
        let array_call = enclosing_array_key_call(content, code)?;
        if !matches!(array_call.callee.as_str(), "generate" | "generateabsolute") {
            return None;
        }
        let call = detect_string_call_context(content, cursor, code)?;
        if call.is_static || call.arg_index != 1 {
            return None;
        }
        let kind = self.receiver_call_key(
            call.subject.as_deref()?,
            &call.method_name,
            0,
            content,
            cursor,
            ctx,
        )?;
        if kind != StringKeyKind::YiiRoute {
            return None;
        }

        let prefix = content.get(quote + 1..cursor)?.as_bytes();
        let edit_range = Range {
            start: offset_to_position(content, quote + 1),
            end: position,
        };
        let mut items: Vec<CompletionItem> = Vec::new();
        for path in self.yii_route_paths(&array_call.name) {
            for name in crate::yii::routes::path_placeholder_names(&path) {
                let matches = name
                    .as_bytes()
                    .get(..prefix.len())
                    .is_some_and(|start| start.eq_ignore_ascii_case(prefix));
                if !matches || items.iter().any(|item| item.label == name) {
                    continue;
                }
                items.push(CompletionItem {
                    label: name.to_string(),
                    kind: Some(CompletionItemKind::Field),
                    detail: Some(path.clone()),
                    sort_text: Some(format!("{:05}", items.len())),
                    filter_text: Some(name.to_string()),
                    text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
                        range: edit_range,
                        new_text: name.to_string(),
                    })),
                    ..Default::default()
                });
            }
        }
        (!items.is_empty()).then_some(CompletionResponse::CompletionItemList(items))
    }
}
