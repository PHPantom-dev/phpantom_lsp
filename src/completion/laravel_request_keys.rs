//! Request input key completion driven by validation rules.
//!
//! Inside `$request->input('|')`, `->string('|')`, `->has('|')`,
//! `->validated('|')`, `->safe()->only(['|'])`, `$request['|']` and their
//! siblings, the acceptable field names are exactly the keys of the rules
//! array that validates the request — a `FormRequest`'s `rules()` method or
//! a `validate()` / `Validator::make()` call earlier in the same function.
//!
//! The field lookup itself lives in
//! [`crate::virtual_members::laravel::request_fields`], which go-to-definition
//! shares.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::completion::source::code_context::CodeContext;
use crate::types::FileContext;
use crate::virtual_members::laravel::request_fields_at_position;

impl Backend {
    /// Try completing a request input field name from the validation rules in
    /// scope.
    ///
    /// Returns `None` when the cursor is not inside a recognised request
    /// input-key string, or when no rules describe the request.
    pub(crate) fn try_request_input_key_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        ctx: &FileContext,
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        let (field_ctx, _, fields) =
            request_fields_at_position(self, uri, content, position, ctx, code)?;

        // Replace the whole typed prefix so dotted names survive the editor's
        // word-based filtering.
        let edit_range = Range {
            start: crate::text_position::offset_to_position(content, field_ctx.content_start),
            end: position,
        };
        let prefix_lower = field_ctx.prefix.to_lowercase();

        let items: Vec<CompletionItem> = fields
            .into_iter()
            .filter(|field| {
                prefix_lower.is_empty() || field.name.to_lowercase().starts_with(&prefix_lower)
            })
            .enumerate()
            .map(|(i, field)| CompletionItem {
                label: field.name.clone(),
                kind: Some(CompletionItemKind::Field),
                detail: (!field.rules.is_empty()).then(|| field.rules.clone()),
                sort_text: Some(format!("{:05}", i)),
                filter_text: Some(field.name.clone()),
                text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
                    range: edit_range,
                    new_text: field.name,
                })),
                ..Default::default()
            })
            .collect();

        if items.is_empty() {
            None
        } else {
            Some(CompletionResponse::CompletionItemList(items))
        }
    }
}
