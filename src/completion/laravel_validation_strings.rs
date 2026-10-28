//! Completion for rule names and the declarations their parameters reference.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::completion::source::code_context::CodeContext;
use crate::text_position::{offset_to_position, position_to_offset};
use crate::types::FileContext;
use crate::virtual_members::laravel::validation_strings as validation;

impl Backend {
    /// Complete a confirmed validation value or message key.
    pub(crate) fn try_validation_string_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        ctx: &FileContext,
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        let cursor = position_to_offset(content, position) as usize;
        if !validation::might_contain_site(content, cursor, ctx, code) {
            return None;
        }
        let site = validation::at_cursor(content, cursor, code)?;
        if !validation::confirmed(self, &site, content, ctx) {
            return None;
        }
        let catalog = self.cached_validation_rules();
        let selection = validation::selection_at(&site, content, cursor, &catalog)?;
        let decoded_prefix = validation::selection_value(&site, &selection, content, cursor)?;
        let prefix = decoded_prefix.to_ascii_lowercase();
        let source_prefix = content.get(selection.start..cursor)?;
        let has_parameters = matches!(selection.kind, validation::SelectionKind::Rule)
            && validation::token_at(&site, content, cursor)
                .is_some_and(|token| token.parameters_start.is_some());
        let range = Range {
            start: offset_to_position(content, selection.start),
            end: offset_to_position(content, selection.end),
        };
        let mut items: Vec<_> =
            validation::candidates_at(self, uri, content, ctx, &site, &selection)
                .into_iter()
                .filter(|candidate| {
                    if matches!(selection.kind, validation::SelectionKind::Rule) {
                        validation::rule_prefix_matches(&candidate.name, &prefix)
                    } else {
                        candidate.name.to_ascii_lowercase().starts_with(&prefix)
                    }
                })
                .filter_map(|candidate| {
                    let parameters = matches!(selection.kind, validation::SelectionKind::Rule)
                        && !has_parameters
                        && catalog
                            .rules
                            .iter()
                            .any(|rule| rule.name == candidate.name && rule.takes_parameters);
                    let quote = code.open_string.map(|(_, quote)| quote).unwrap_or('\'');
                    let text = if parameters {
                        format!("{}:${{1}}", candidate.name)
                    } else if candidate.kind == CompletionItemKind::EnumMember {
                        if site.kind == validation::SiteKind::Rule {
                            if !site.array_rule && candidate.name.contains('|') {
                                return None;
                            }
                            escape_literal_value(
                                &validation::csv_parameter(&candidate.name)?,
                                quote,
                            )
                        } else {
                            escape_literal_value(&candidate.name, quote)
                        }
                    } else {
                        escape_literal_value(&candidate.name, quote)
                    };
                    // Editors filter against the source the user typed, which may
                    // contain PHP escapes or a CSV enclosure absent from the value.
                    let filter_text = if source_prefix != decoded_prefix.as_ref() {
                        format!(
                            "{source_prefix}{}",
                            candidate
                                .name
                                .get(decoded_prefix.len()..)
                                .unwrap_or(&candidate.name)
                        )
                    } else {
                        candidate.name.clone()
                    };
                    Some(CompletionItem {
                        label: candidate.name.clone(),
                        kind: Some(candidate.kind),
                        detail: Some(candidate.detail.clone()),
                        documentation: Some(Documentation::String(candidate.detail)),
                        filter_text: Some(filter_text),
                        insert_text_format: parameters.then_some(InsertTextFormat::Snippet),
                        text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
                            range,
                            new_text: text,
                        })),
                        ..Default::default()
                    })
                })
                .collect();
        if site.array_rule && selection.start == site.start && selection.end == site.end {
            for class in &catalog.classes {
                let name = crate::util::short_name(&class.fqn);
                if !name.to_ascii_lowercase().starts_with(&prefix)
                    && !class.fqn.to_ascii_lowercase().starts_with(&prefix)
                {
                    continue;
                }
                items.push(CompletionItem {
                    label: name.to_string(),
                    kind: Some(CompletionItemKind::Class),
                    detail: Some(class.fqn.clone()),
                    documentation: Some(Documentation::String(class.description.clone())),
                    filter_text: Some(name.to_string()),
                    text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
                        range: Range {
                            start: offset_to_position(content, site.start - 1),
                            end: offset_to_position(
                                content,
                                site.end
                                    + usize::from(matches!(
                                        content.as_bytes().get(site.end),
                                        Some(b'\'' | b'"')
                                    )),
                            ),
                        },
                        new_text: format!("new \\{}()", class.fqn),
                    })),
                    ..Default::default()
                });
            }
        }
        Some(CompletionResponse::CompletionItemList(items))
    }
}

fn escape_literal_value(value: &str, quote: char) -> String {
    let mut text = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch == '\\' || ch == quote || (quote == '"' && ch == '$') {
            text.push('\\');
        }
        text.push(ch);
    }
    text
}
