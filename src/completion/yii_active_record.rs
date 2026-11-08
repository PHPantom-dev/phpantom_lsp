//! Completion of the attributes and relations a Yii Active Record call
//! names: `$post->get('|')` offers the record's attributes, and
//! `$post->relation('|')` and `Post::query()->with('author.|')` the
//! relations of the record the path has reached.  What each call takes
//! comes from [`crate::yii::active_record`], the same rules the symbol
//! map's spans follow.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::completion::source::code_context::CodeContext;
use crate::completion::source::string_argument::{StringArgumentPosition, string_argument};
use crate::text_position::{offset_to_position, position_to_offset};
use crate::type_engine::resolver::resolve_target_classes;
use crate::types::{AccessKind, FileContext};
use crate::util::short_name;
use crate::yii::active_record::{
    NamedArgument, attribute_names, is_record, named_argument, record_relations,
};
use crate::yii::relations::relation_owners;

use super::eloquent_string::{extract_identifier_backwards, extract_receiver_subject};

impl Backend {
    /// Complete the attribute or relation an Active Record call's string
    /// argument is being typed as.
    pub(crate) fn try_yii_active_record_completion(
        &self,
        content: &str,
        position: Position,
        ctx: &FileContext,
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        let (quote, _) = code.open_string?;
        let cursor_offset = position_to_offset(content, position) as usize;
        let call = code.open_brackets.iter().rev().find(|bracket| {
            bracket.byte == b'('
                && !extract_identifier_backwards(&content[..bracket.code_before])
                    .is_some_and(|(name, _)| name.eq_ignore_ascii_case("array"))
        })?;
        let operator = call.callee_operator.as_ref()?;
        if operator.is_static {
            return None;
        }
        let (method, _) = extract_identifier_backwards(&content[..call.code_before])?;
        named_argument(method, 0)?;
        let argument = string_argument(content, cursor_offset, code, call)?;
        if argument.name.is_some() {
            return None;
        }
        let kind = named_argument(method, argument.index)?;
        let in_array = matches!(
            argument.position,
            StringArgumentPosition::ArrayKey | StringArgumentPosition::ArrayElement
        );
        if !(argument.position == StringArgumentPosition::Direct
            || kind == NamedArgument::QueryRelations && in_array)
        {
            return None;
        }

        let typed = content.get(quote + 1..cursor_offset)?;
        let (path, prefix) = match kind {
            NamedArgument::QueryRelations => typed.rsplit_once('.').unwrap_or(("", typed)),
            _ => ("", typed),
        };
        let subject = extract_receiver_subject(content, operator)?;
        let loaders = self.file_loaders(ctx);
        let class_loader = loaders.resolved_class_loader();
        let rctx = loaders.resolution_ctx_at(content, cursor_offset as u32);

        let mut names: Vec<(String, Option<String>, CompletionItemKind)> = Vec::new();
        let mut push = |name: String, detail: Option<String>, kind: CompletionItemKind| {
            if !names.iter().any(|(known, _, _)| *known == name) {
                names.push((name, detail, kind));
            }
        };
        match kind {
            NamedArgument::Attribute => {
                for resolved in resolve_target_classes(&subject, AccessKind::Arrow, &rctx) {
                    let Some(record) = resolved
                        .class_info
                        .filter(|class| is_record(class, class_loader))
                    else {
                        continue;
                    };
                    let attributes = attribute_names(&record, class_loader);
                    if attributes.is_empty() {
                        let Some(columns) = self.yii_record_columns(&record, class_loader) else {
                            continue;
                        };
                        for column in columns.columns() {
                            push(
                                column.name.clone(),
                                column.kind.clone(),
                                CompletionItemKind::Field,
                            );
                        }
                    }
                    for (name, ty) in attributes {
                        push(
                            name.to_string(),
                            ty.map(|ty| ty.to_string()),
                            CompletionItemKind::Property,
                        );
                    }
                }
            }
            NamedArgument::RecordRelation | NamedArgument::QueryRelations => {
                let on_query = kind == NamedArgument::QueryRelations;
                for owner in relation_owners(&subject, on_query, path, &rctx, class_loader) {
                    for relation in record_relations(&owner, class_loader).unwrap_or_default() {
                        push(
                            relation.name.to_string(),
                            relation
                                .related
                                .map(|related| short_name(&related).to_string()),
                            CompletionItemKind::Field,
                        );
                    }
                }
            }
        }
        if names.is_empty() {
            return None;
        }

        let start = cursor_offset - prefix.len();
        let edit_range = Range {
            start: offset_to_position(content, start),
            end: position,
        };
        let prefix_lower = prefix.to_lowercase();
        let items: Vec<CompletionItem> = names
            .into_iter()
            .filter(|(name, _, _)| {
                name.to_lowercase().starts_with(&prefix_lower)
                    && !argument.used_keys.iter().any(|used| used == name)
            })
            .enumerate()
            .map(|(i, (name, detail, kind))| CompletionItem {
                label: name.clone(),
                kind: Some(kind),
                detail,
                sort_text: Some(format!("{i:05}")),
                filter_text: Some(name.clone()),
                text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
                    range: edit_range,
                    new_text: name,
                })),
                ..Default::default()
            })
            .collect();
        (!items.is_empty()).then_some(CompletionResponse::CompletionItemList(items))
    }
}
