//! Hover and navigation on the same validation declarations completion offers.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::text_position::{offset_to_position, position_to_offset};
use crate::virtual_members::laravel::validation_strings as validation;

impl Backend {
    /// Show the declaration behind a rule, parameter, or message key.
    pub(crate) fn validation_string_hover(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<Hover> {
        let (candidate, range) = self.validation_string_target(uri, content, position)?;
        Some(Hover {
            contents: Contents::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: format!("**`{}`**\n\n{}", candidate.name, candidate.detail),
            }),
            range: Some(range),
        })
    }

    /// Navigate to a rule method, field key, model column, or enum case.
    pub(crate) fn validation_string_definition(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<Location> {
        self.validation_string_target(uri, content, position)?
            .0
            .location
    }

    fn validation_string_target(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<(validation::Candidate, Range)> {
        if !self.resolved_class_cache.read().laravel().is_enabled() {
            return None;
        }
        let cursor = position_to_offset(content, position) as usize;
        let code = crate::completion::source::code_context::code_context_at(content, cursor)?;
        let ctx = self.file_context(uri);
        if !validation::might_contain_site(content, cursor, &ctx, &code) {
            return None;
        }
        let site = validation::at_cursor(content, cursor, &code)?;
        if !validation::confirmed(self, &site, content, &ctx) {
            return None;
        }
        let catalog = self.cached_validation_rules();
        let selection = validation::selection_at(&site, content, cursor, &catalog)?;
        let name = validation::selection_value(&site, &selection, content, selection.end)?;
        let mut candidate = validation::candidates_at(self, uri, content, &ctx, &site, &selection)
            .into_iter()
            .find(|candidate| {
                if matches!(selection.kind, validation::SelectionKind::Rule) {
                    validation::rule_matches(&candidate.name, &name)
                } else {
                    candidate.name == name
                }
            })?;
        if candidate.location.is_none() {
            candidate.location = validation::database_definition(self, &selection, &name);
        }
        Some((
            candidate,
            Range {
                start: offset_to_position(content, selection.start),
                end: offset_to_position(content, selection.end),
            },
        ))
    }
}
