//! Strategy: Blade directive-name completion.
//!
//! Always short-circuits: [`crate::blade::directive_completion::directive_prefix_at`]
//! only returns `Some` when the cursor sits in an HTML/directive position
//! of a Blade template, and any `@` typed there is a directive name being
//! written, not a member/variable/class reference — so even a prefix that
//! matches no known directive returns an empty list rather than falling
//! through to another strategy.

use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, CompletionResponse, Position};

use crate::Backend;
use crate::blade::directives::DIRECTIVE_COMPLETIONS;
use crate::text_position::position_to_byte_offset;

impl Backend {
    /// The directive-name prefix already typed after an `@` in `uri`'s raw
    /// Blade source at `position`, when that `@` is in an HTML/directive
    /// position. `uri`'s raw content (not the preprocessed virtual PHP) is
    /// fetched fresh here since the incomplete directive name a user is
    /// mid-typing never survives Blade preprocessing (an unrecognised
    /// directive is masked as inert HTML — see
    /// `src/blade/directive_completion.rs`).
    pub(super) fn blade_directive_prefix_at(
        &self,
        uri: &str,
        position: Position,
    ) -> Option<String> {
        let content = self.get_file_content(uri)?;
        let offset = position_to_byte_offset(&content, position);
        crate::blade::directive_completion::directive_prefix_at(&content, offset)
            .map(|s| s.to_string())
    }

    /// Complete the section or stack name the cursor is writing in `uri`'s
    /// raw Blade source, against the templates that render it.
    ///
    /// Reads the raw buffer for the same reason directive-name completion
    /// does: the name is usually in a string literal the user has not
    /// closed yet, which Blade preprocessing does not preserve, and the
    /// edit it produces has to be in the template's own coordinates rather
    /// than the virtual PHP's.
    pub(super) fn blade_block_name_completion(
        &self,
        uri: &str,
        position: Position,
    ) -> Option<CompletionResponse> {
        let content = self.get_file_content(uri)?;
        let offset = position_to_byte_offset(&content, position);
        let ctx = crate::blade::blocks::block_name_at(&content, offset)?;
        let prefix = content.get(ctx.name_start..offset)?.to_lowercase();

        let edit_range = tower_lsp::lsp_types::Range {
            start: crate::text_position::offset_to_position(&content, ctx.name_start),
            end: position,
        };
        let items = self
            .blade_block_name_candidates(uri, &content, ctx.kind, ctx.role)
            .into_iter()
            .filter(|name| prefix.is_empty() || name.to_lowercase().starts_with(&prefix))
            .enumerate()
            .map(|(index, name)| CompletionItem {
                label: name.clone(),
                kind: Some(CompletionItemKind::VALUE),
                detail: Some(format!("Blade {}", ctx.kind.label())),
                sort_text: Some(format!("{index:05}")),
                filter_text: Some(name.clone()),
                text_edit: Some(tower_lsp::lsp_types::CompletionTextEdit::Edit(
                    tower_lsp::lsp_types::TextEdit {
                        range: edit_range,
                        new_text: name,
                    },
                )),
                ..CompletionItem::default()
            })
            .collect();
        // Always short-circuits: inside a section or stack name nothing
        // else completion offers applies, so an empty list beats falling
        // through to class and variable names.
        Some(CompletionResponse::Array(items))
    }

    /// Build the directive-name completion list for an already-typed
    /// `prefix` (the text after `@`, matched case-insensitively).
    ///
    /// The directives the project's own service providers registered with
    /// `Blade::directive()` / `Blade::if()` are offered alongside Blade's
    /// own, told apart by their detail line rather than by position, since
    /// the client is what orders the list.
    pub(super) fn complete_blade_directive(&self, prefix: &str) -> CompletionResponse {
        let prefix_lower = prefix.to_lowercase();
        let matches_prefix = |name: &str| name.to_lowercase().starts_with(&prefix_lower);

        let mut items: Vec<CompletionItem> = DIRECTIVE_COMPLETIONS
            .iter()
            .filter(|completion| matches_prefix(completion.name))
            .map(|completion| {
                directive_item(
                    completion.name,
                    completion.insert_text.to_string(),
                    completion.is_snippet,
                    "Blade directive",
                )
            })
            .collect();

        let custom = self.blade_custom_directives.read();
        items.extend(
            custom
                .completions()
                .filter(|completion| matches_prefix(completion.name))
                .map(|completion| {
                    directive_item(
                        completion.name,
                        completion.insert_text,
                        completion.is_snippet,
                        "Registered Blade directive",
                    )
                }),
        );
        CompletionResponse::Array(items)
    }
}

/// One directive-name completion item. `insert_text` never carries the
/// leading `@`, which the trigger character already put in the buffer.
fn directive_item(
    name: &str,
    insert_text: String,
    is_snippet: bool,
    detail: &str,
) -> CompletionItem {
    CompletionItem {
        label: format!("@{name}"),
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some(detail.to_string()),
        filter_text: Some(format!("@{name}")),
        insert_text: Some(insert_text),
        insert_text_format: Some(if is_snippet {
            tower_lsp::lsp_types::InsertTextFormat::SNIPPET
        } else {
            tower_lsp::lsp_types::InsertTextFormat::PLAIN_TEXT
        }),
        ..CompletionItem::default()
    }
}
