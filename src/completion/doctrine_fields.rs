//! Completion of the entity fields a Doctrine repository call or a DQL
//! variable names.
//!
//! Inside `$users->findBy(['|' => …])`, `->findOneBy()`, `->count()`, and
//! the `$orderBy` of the first two, offer the fields the repository's
//! entity maps.  Which calls take field names, and which entity a
//! repository manages, come from [`crate::doctrine`], the same rules the
//! symbol map's spans follow.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::completion::source::code_context::CodeContext;
use crate::completion::source::string_argument::{StringArgumentPosition, string_argument};
use crate::text_position::{offset_to_position, position_to_offset};
use crate::type_engine::resolver::resolve_target_classes_expr;
use crate::type_engine::subject_expr::SubjectExpr;
use crate::types::{AccessKind, FileContext, ResolvedType};

use super::eloquent_string::{extract_identifier_backwards, extract_receiver_subject};

impl Backend {
    /// Try entity field-name completion in a repository call's criteria or
    /// order.
    pub(crate) fn try_entity_field_completion(
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
        if !crate::doctrine::takes_field_names(method, 0) {
            return None;
        }
        let argument = string_argument(content, cursor_offset, code, call)?;
        let takes_fields = match &argument.name {
            Some(name) => crate::doctrine::field_keyed_parameter(method, name),
            None => crate::doctrine::takes_field_names(method, argument.index),
        };
        // A string standing alone in the array becomes a key once its
        // `=>` is typed.
        if !takes_fields
            || !matches!(
                argument.position,
                StringArgumentPosition::ArrayKey | StringArgumentPosition::ArrayElement
            )
        {
            return None;
        }

        let subject = extract_receiver_subject(content, operator)?;
        let subject = SubjectExpr::parse(&subject);
        let cursor = cursor_offset as u32;
        let loaders = self.file_loaders(ctx);
        let class_loader = loaders.resolved_class_loader();
        let resolved = ResolvedType::into_arced_classes(resolve_target_classes_expr(
            &subject,
            AccessKind::Arrow,
            &loaders.resolution_ctx_at(content, cursor),
        ));
        let prefix = content.get(quote + 1..cursor_offset)?;
        let edit_range = Range {
            start: offset_to_position(content, quote + 1),
            end: position,
        };
        self.entity_field_items(
            resolved,
            class_loader,
            prefix,
            edit_range,
            &argument.used_keys,
        )
    }

    /// Complete a field name in DQL after the variable it belongs to:
    /// `'… WHERE u.|'` in a query or a query builder fragment.  What the
    /// variable stands for is what the file's symbol map records for it.
    pub(crate) fn try_dql_field_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        ctx: &FileContext,
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        let (quote, _) = code.open_string?;
        let cursor_offset = position_to_offset(content, position) as usize;
        let text = content.get(quote + 1..cursor_offset)?;
        let (alias, field_start) = crate::doctrine::dql::path_before(text, text.len())?;
        let map = self.symbol_map_for(uri)?;
        let cursor = cursor_offset as u32;
        let subject = SubjectExpr::parse(map.dql_alias_subject(alias, cursor)?);
        let loaders = self.file_loaders(ctx);
        let class_loader = loaders.resolved_class_loader();
        let resolved = ResolvedType::into_arced_classes(resolve_target_classes_expr(
            &subject,
            AccessKind::Arrow,
            &loaders.resolution_ctx_at(content, cursor),
        ));
        let start = quote + 1 + field_start;
        let edit_range = Range {
            start: offset_to_position(content, start),
            end: position,
        };
        self.entity_field_items(
            resolved,
            class_loader,
            &text[field_start..],
            edit_range,
            &[],
        )
    }

    /// Complete an entity class name in DQL where a range or a join names
    /// one: `'SELECT u FROM App\Entity\U|'`.  DQL names classes by their
    /// fully-qualified name, so that is what is inserted, and only classes
    /// mapped as entities are offered.
    pub(crate) fn try_dql_entity_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        if !self.is_doctrine_project() {
            return None;
        }
        let (quote, _) = code.open_string?;
        let cursor_offset = position_to_offset(content, position) as usize;
        let text = content.get(quote + 1..cursor_offset)?;
        let prefix_start = text
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '\\'))
            .map_or(0, |i| i + 1);
        let prefix = &text[prefix_start..];
        let before = text[..prefix_start].trim_end();
        if before.len() == text[..prefix_start].len() {
            return None;
        }
        let keyword_start = before
            .rfind(|c: char| !c.is_ascii_alphabetic())
            .map_or(0, |i| i + 1);
        let keyword = &before[keyword_start..];
        if !["FROM", "JOIN", "UPDATE", "DELETE"]
            .iter()
            .any(|k| k.eq_ignore_ascii_case(keyword))
        {
            return None;
        }

        let no_imports = std::collections::HashMap::new();
        let (items, _) = self.build_class_name_completions(
            crate::completion::context::class_completion::ClassCompletionParams {
                file_use_map: &no_imports,
                file_namespace: &None,
                prefix,
                content,
                context: crate::completion::context::class_completion::ClassNameContext::UseImport,
                position,
                affinity_table_override: None,
                uri,
            },
        );
        let state = self.resolved_class_cache.read().doctrine().clone();
        let items: Vec<CompletionItem> = items
            .into_iter()
            .filter(|item| {
                item.detail
                    .as_deref()
                    .and_then(|fqn| self.find_or_load_class(fqn))
                    .is_some_and(|class| crate::doctrine::is_entity(&class, &state))
            })
            .collect();
        (!items.is_empty()).then_some(CompletionResponse::CompletionList(CompletionList {
            is_incomplete: true,
            items,
            apply_kind: None,
            item_defaults: None,
        }))
    }

    /// The fields every entity `receivers` stands for maps, starting with
    /// `prefix` and not among `used`, as completion items replacing
    /// `edit_range`.
    fn entity_field_items(
        &self,
        receivers: Vec<std::sync::Arc<crate::types::ClassInfo>>,
        class_loader: &crate::class_lookup::ResolvedClassLoader<'_>,
        prefix: &str,
        edit_range: Range,
        used: &[String],
    ) -> Option<CompletionResponse> {
        let entities = crate::doctrine::managed_entity_classes(
            receivers,
            class_loader,
            &self.resolved_class_cache,
        );
        if entities.is_empty() {
            return None;
        }
        let state = self.resolved_class_cache.read().doctrine().clone();
        // Every entity the repository may be for has to map the field.
        let mut fields: Option<Vec<crate::atom::Atom>> = None;
        for entity in &entities {
            let mapped = crate::doctrine::entity_fields(entity, class_loader, &state)?;
            fields = Some(match fields {
                None => mapped,
                Some(known) => known.into_iter().filter(|f| mapped.contains(f)).collect(),
            });
        }
        let entity = &entities[0];
        let resolved_entity = crate::virtual_members::resolve_class_fully_maybe_cached(
            entity,
            class_loader,
            Some(&self.resolved_class_cache),
        );

        let prefix_lower = prefix.to_lowercase();
        let items: Vec<CompletionItem> = fields?
            .into_iter()
            .filter(|field| {
                field.to_lowercase().starts_with(&prefix_lower)
                    && !used.iter().any(|used| used == field.as_str())
            })
            .enumerate()
            .map(|(i, field)| {
                let detail = resolved_entity
                    .properties
                    .iter()
                    .find(|p| p.name == field)
                    .and_then(|p| p.type_hint.as_ref())
                    .map(|ty| ty.to_string());
                CompletionItem {
                    label: field.to_string(),
                    kind: Some(CompletionItemKind::Field),
                    detail,
                    sort_text: Some(format!("{i:05}")),
                    filter_text: Some(field.to_string()),
                    text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
                        range: edit_range,
                        new_text: field.to_string(),
                    })),
                    ..Default::default()
                }
            })
            .collect();
        (!items.is_empty()).then_some(CompletionResponse::CompletionItemList(items))
    }
}
