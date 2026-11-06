//! Component tags and directive names in the template's own coordinates.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::text_position::{byte_range_to_lsp_range, position_to_offset};

use super::syntax::{Tag, Token, tokens};

const DIRECTIVES: [&str; 8] = [
    ":if", ":elseif", ":else", ":isset", ":foreach", ":forelse", ":apply", ":as",
];
const BUILTINS: [&str; 3] = ["x-slot", "x-template", "x-component"];

impl Backend {
    /// Resolve either end of a component tag to the discovered view file.
    pub(crate) fn tempest_component_definition(
        &self,
        uri: &str,
        position: Position,
    ) -> Option<Location> {
        if !super::is_view(uri) {
            return None;
        }
        let source = self.get_file_content_arc(uri)?;
        let offset = position_to_offset(&source, position) as usize;
        let tag = tag_at(&source, offset)?;
        let name = if tag.name_span.start <= offset && offset <= tag.name_span.end {
            tag.name
        } else if tag.name == "x-component" {
            let attr = tag.attributes.iter().find(|attr| attr.name == "is")?;
            let value = attr.value.as_ref()?;
            if !value.contains(&offset) {
                return None;
            }
            &source[value.clone()]
        } else {
            return None;
        };
        let components = self.tempest_components();
        let path = components.get(name)?;
        Some(Location {
            uri: crate::util::parse_url(&crate::util::path_to_uri(path))?,
            range: Range::default(),
        })
    }

    /// Complete component and directive names before entering virtual PHP.
    pub(crate) fn tempest_source_completion(
        &self,
        uri: &str,
        position: Position,
    ) -> Option<CompletionResponse> {
        if !super::is_view(uri) {
            return None;
        }
        let source = self.get_file_content_arc(uri)?;
        let offset = position_to_offset(&source, position) as usize;
        let empty = || Some(CompletionResponse::CompletionItemList(Vec::new()));
        if self
            .template_source_map(uri)
            .and_then(|map| map.copied.as_ref().and_then(|spans| spans.to_php(position)))
            .is_some()
        {
            return None;
        }
        let Some(tag) = tag_at(&source, offset) else {
            return empty();
        };
        if tag.name_span.start <= offset && offset <= tag.name_span.end {
            let prefix = &source[tag.name_span.start..offset];
            if !prefix.starts_with('x') {
                return empty();
            }
            let components = self.tempest_components();
            let mut names: Vec<_> = components
                .keys()
                .map(String::as_str)
                .chain(BUILTINS)
                .collect();
            names.sort_unstable();
            names.dedup();
            let range = byte_range_to_lsp_range(&source, tag.name_span.start, tag.name_span.end);
            return Some(CompletionResponse::CompletionItemList(
                names
                    .into_iter()
                    .filter(|name| name.starts_with(prefix))
                    .map(|name| {
                        item(
                            name,
                            "Tempest view component",
                            range,
                            CompletionItemKind::Class,
                        )
                    })
                    .collect(),
            ));
        }
        if tag.closing {
            return empty();
        }
        if tag.attributes.iter().any(|attr| {
            attr.value
                .as_ref()
                .is_some_and(|value| value.start <= offset && offset <= value.end)
        }) {
            return empty();
        }
        let start = source[..offset]
            .rfind(|ch: char| ch.is_whitespace() || ch == '<')
            .map_or(offset, |at| at + 1);
        let prefix = &source[start..offset];
        if prefix.starts_with("::") {
            return empty();
        }
        let end = tag
            .attributes
            .iter()
            .find(|attr| attr.span.start == start)
            .map_or(offset, |attr| start + attr.name.len());
        let range = byte_range_to_lsp_range(&source, start, end);
        let mut items: Vec<_> = DIRECTIVES
            .into_iter()
            .filter(|name| name.starts_with(prefix))
            .filter(|name| {
                !tag.attributes
                    .iter()
                    .any(|attr| attr.name == *name && attr.span.start != start)
            })
            .map(|name| {
                item(
                    name,
                    "Tempest directive",
                    range,
                    CompletionItemKind::Keyword,
                )
            })
            .collect();
        if tag.name.starts_with("x-") {
            let components = self.tempest_components();
            if let Some(path) = components.get(tag.name)
                && let Some(component) = self.get_file_content_arc(&crate::util::path_to_uri(path))
            {
                for (name, _) in crate::blade::signature::declarations(&component) {
                    if matches!(name.as_str(), "this" | "attributes" | "slots") {
                        continue;
                    }
                    let name = crate::blade::component_tags::kebab_case(&name);
                    let name = if prefix.starts_with(':') {
                        format!(":{name}")
                    } else {
                        name
                    };
                    if name.starts_with(prefix)
                        && !tag.attributes.iter().any(|attr| {
                            attr.name.trim_start_matches(':') == name.trim_start_matches(':')
                                && attr.span.start != start
                        })
                    {
                        items.push(item(
                            &name,
                            "Tempest component attribute",
                            range,
                            CompletionItemKind::Property,
                        ));
                    }
                }
            }
        }
        Some(CompletionResponse::CompletionItemList(items))
    }
}

fn tag_at(source: &str, offset: usize) -> Option<Tag<'_>> {
    tokens(source).into_iter().find_map(|token| match token {
        Token::Tag(tag) if tag.span.start < offset && offset <= tag.span.end => Some(tag),
        _ => None,
    })
}

fn item(name: &str, detail: &str, range: Range, kind: CompletionItemKind) -> CompletionItem {
    CompletionItem {
        label: name.to_string(),
        detail: Some(detail.to_string()),
        kind: Some(kind),
        text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
            range,
            new_text: name.to_string(),
        })),
        ..Default::default()
    }
}
