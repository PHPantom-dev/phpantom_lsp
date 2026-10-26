//! PHP symbol navigation from non-PHP resource files.
//!
//! YAML and XML often carry fully-qualified PHP class names in arbitrary
//! keys, values, attributes, and text. This module recognises those names
//! without knowing the schema of the file that contains them, then delegates
//! declaration lookup to the normal PHP class loader.
//!
//! Twig templates and XLIFF catalogues are resource documents too, read
//! only for the template names, and in a Symfony project the route names
//! and translation keys, they hold (see [`crate::twig`],
//! [`crate::symfony::routes`], and [`crate::symfony::translations`]).  A
//! template whose extension does not say so (`.html`) is one only under a
//! discovered root, so [`Backend::is_resource_uri`] has the last word.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::{Location, Position};

use crate::Backend;
use crate::atom::{AtomMap, atom};
use crate::symbol_map::{ClassRefContext, SubjectText, SymbolKind, SymbolMap, SymbolSpan};

#[derive(Debug, PartialEq, Eq)]
enum ResourceSymbol {
    Class(String),
    Member {
        class_fqn: String,
        member_name: String,
    },
}

#[derive(Debug, PartialEq, Eq)]
struct ScannedResourceSymbol {
    class_fqn: String,
    class_start: usize,
    class_end: usize,
    member: Option<(String, usize, usize)>,
}

/// Whether `uri` names a YAML or XML document that can carry PHP symbols,
/// a Twig template, or an XLIFF translation catalogue.
pub(crate) fn is_resource_document(uri: &str) -> bool {
    let path = uri
        .split(['?', '#'])
        .next()
        .unwrap_or(uri)
        .to_ascii_lowercase();

    [
        ".yaml",
        ".yml",
        ".xml",
        ".yaml.dist",
        ".yml.dist",
        ".xml.dist",
        ".twig",
        ".xlf",
        ".xliff",
    ]
    .iter()
    .any(|suffix| path.ends_with(suffix))
}

/// Whether `uri` is a resource document that holds no PHP class names: a
/// Twig template or an XLIFF catalogue.
pub(crate) fn is_symfony_only_document(uri: &str) -> bool {
    crate::symfony::routes::is_twig_uri(uri) || crate::symfony::translations::is_xliff_uri(uri)
}

/// Whether a filesystem path is a resource document.
pub(crate) fn is_resource_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_resource_document)
}

impl Backend {
    /// Resolve the fully-qualified PHP class or `Class::member` under the
    /// cursor in a YAML/XML document.
    pub(crate) fn resolve_resource_symbol_definition(
        &self,
        content: &str,
        position: Position,
    ) -> Option<Location> {
        match symbol_at(content, position)? {
            ResourceSymbol::Class(fqn) => self.class_declaration_location(&fqn),
            ResourceSymbol::Member {
                class_fqn,
                member_name,
            } => self.class_member_declaration_location(&class_fqn, &member_name, false),
        }
    }

    /// Go-to-definition in a YAML/XML document: the PHP class or member
    /// under the cursor, or else what the file's symbol map records there:
    /// the declarations of a string key (a Symfony service ID or container
    /// parameter), a property a validation mapping names, or a constraint
    /// class it names by its short name.
    pub(crate) fn resolve_resource_definition(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Vec<Location> {
        if let Some(location) = self.resolve_resource_symbol_definition(content, position) {
            return vec![location];
        }
        let Some(span) = self.lookup_symbol_at_position(uri, content, position) else {
            return Vec::new();
        };
        match span.kind {
            SymbolKind::StringKey { kind, key, .. } if self.string_key_kind_enabled(kind) => {
                crate::virtual_members::laravel::resolve_laravel_string_key(self, &kind, &key, uri)
            }
            SymbolKind::MemberAccess {
                subject_text: SubjectText::Owned(class_fqn),
                member_name,
                is_method_call,
                ..
            } => self
                .class_member_declaration_location(&class_fqn, &member_name, !is_method_call)
                .into_iter()
                .collect(),
            SymbolKind::ClassReference {
                name, is_fqn: true, ..
            } => self.class_declaration_location(&name).into_iter().collect(),
            _ => Vec::new(),
        }
    }

    /// Replace one resource document's synthetic symbol map and reference
    /// contributions.  A file that can be a Twig template is remembered for
    /// root discovery first, which may make it a template or stop it being
    /// one.
    pub(crate) fn update_resource_symbol_index(&self, uri: &str, content: &str) {
        self.note_twig_source(uri, content);
        if self.is_resource_uri(uri) {
            self.publish_resource_symbol_index(uri, content);
        } else if self.symbol_map_for(uri).is_some() {
            // It stopped being a template.
            if self.open_files.read().contains_key(uri) {
                self.update_ast(uri, content);
            } else {
                self.clear_file_maps(uri);
            }
        }
    }

    /// Replace one resource document's synthetic symbol map and reference
    /// contributions, leaving root discovery alone.
    pub(crate) fn publish_resource_symbol_index(&self, uri: &str, content: &str) {
        let symbol_map = Arc::new(self.resource_symbol_map(uri, content));
        self.publish_symbol_maps([(uri.to_string(), Arc::clone(&symbol_map))]);
        self.reindex_references_for_symbol_maps_batch(vec![(uri.to_string(), symbol_map)]);
        self.index_framework_uri_content(uri, content);
    }

    /// Index resource files discovered during the workspace walk.
    ///
    /// The files that can be Twig templates are read first, all of them, so
    /// that the roots are discovered before any template is named.
    pub(crate) fn index_resource_paths_batch(&self, files: &[(String, PathBuf)]) {
        // An XLIFF catalogue holds nothing but Symfony names, and a template
        // nothing but Twig's, so neither is even read in another project.
        let is_symfony = self.is_symfony_project();
        let is_twig = self.is_twig_project();
        let (candidates, others): (Vec<_>, Vec<_>) = files
            .iter()
            .filter(|(uri, _)| {
                (is_symfony || !crate::symfony::translations::is_xliff_uri(uri))
                    && (is_twig || !crate::symfony::routes::is_twig_uri(uri))
            })
            .partition(|(uri, _)| crate::twig::roots::is_candidate_uri(uri));
        // An open file is read from its buffer.
        let candidates: Vec<(&String, Arc<String>)> = if is_twig {
            candidates
                .into_iter()
                .filter_map(|(uri, path)| {
                    let open = self.open_files.read().get(uri).cloned();
                    let content = match open {
                        Some(content) => content,
                        None => Arc::new(std::fs::read_to_string(path).ok()?),
                    };
                    Some((uri, content))
                })
                .collect()
        } else {
            Vec::new()
        };
        self.note_twig_sources(
            candidates
                .iter()
                .map(|(uri, content)| (uri.as_str(), content.as_str())),
        );

        let candidate_maps = candidates
            .iter()
            .filter(|(uri, _)| self.is_resource_uri(uri))
            .map(|(uri, content)| {
                self.index_framework_uri_content(uri, content);
                (
                    (*uri).clone(),
                    Arc::new(self.resource_symbol_map(uri, content)),
                )
            });
        let other_maps = others.into_iter().filter_map(|(uri, path)| {
            let content = std::fs::read_to_string(path).ok()?;
            self.index_framework_uri_content(uri, &content);
            Some((
                uri.clone(),
                Arc::new(self.resource_symbol_map(uri, &content)),
            ))
        });
        let maps: Vec<(String, Arc<SymbolMap>)> = candidate_maps.chain(other_maps).collect();
        if maps.is_empty() {
            return;
        }

        self.publish_symbol_maps(maps.iter().cloned());
        self.reindex_references_for_symbol_maps_batch(maps);
    }

    fn resource_symbol_map(&self, uri: &str, content: &str) -> SymbolMap {
        let is_symfony = self.is_symfony_project();
        let is_template = self.is_twig_template_uri(uri);
        if is_template || crate::symfony::translations::is_xliff_uri(uri) {
            let spans = if is_template {
                if self.is_twig_project() {
                    self.twig_template_spans(uri, content, is_symfony)
                } else {
                    Vec::new()
                }
            } else if is_symfony {
                crate::symfony::translations::catalogue_spans(uri, content)
            } else {
                Vec::new()
            };
            return SymbolMap {
                spans,
                source_len: u32::try_from(content.len()).unwrap_or(u32::MAX),
                ..SymbolMap::default()
            };
        }

        let mut spans = Vec::new();
        let mut container_values = Vec::new();
        if is_symfony && crate::symfony::resources::is_container_config_uri(uri) {
            // A bus is declared by a key under `config/packages/`, which is
            // a configuration key too; the bus is the one that means
            // something, and the first of two spans over the same text is
            // the one kept.
            let mut slots = crate::symfony::tags::tag_slots(uri, content);
            slots.extend(crate::symfony::messenger::bus_slots(uri, content));
            spans = crate::symfony::slot_spans(content, slots);
            let (container_spans, values) =
                crate::symfony::resources::container_declarations(uri, content);
            spans.extend(container_spans);
            container_values = values;
            if crate::symfony::routes::is_routing_config_uri(uri) {
                spans.extend(crate::symfony::routes::routing_key_spans(uri, content));
            }
        }
        if is_symfony {
            spans.extend(crate::symfony::translations::catalogue_spans(uri, content));
            if crate::symfony::validation::is_validation_mapping_uri(uri) {
                spans.extend(crate::symfony::validation::validation_spans(uri, content));
            }
            if crate::symfony::config_tree::is_bundle_config_uri(uri) {
                spans.extend(crate::symfony::config_tree::config_key_spans(content));
            }
        }
        for symbol in scan_symbols(content) {
            spans.push(SymbolSpan {
                start: symbol.class_start as u32,
                end: symbol.class_end as u32,
                kind: SymbolKind::ClassReference {
                    name: atom(&symbol.class_fqn),
                    is_fqn: true,
                    context: ClassRefContext::Other,
                },
            });

            if let Some((member_name, member_start, member_end)) = symbol.member {
                spans.push(SymbolSpan {
                    start: member_start as u32,
                    end: member_end as u32,
                    kind: SymbolKind::MemberAccess {
                        subject_text: SubjectText::owned(symbol.class_fqn),
                        member_name: atom(&member_name),
                        is_static: false,
                        is_method_call: true,
                        docblock_ref: crate::symbol_map::DocblockMemberRef::No,
                        is_array_callable: false,
                        is_nullsafe: false,
                    },
                });
            }
        }
        spans.sort_by_key(|span| span.start);
        spans.dedup_by(|b, a| a.start == b.start && a.end == b.end);

        let mut member_access_indices = AtomMap::default();
        for (index, span) in spans.iter().enumerate() {
            if let SymbolKind::MemberAccess { member_name, .. } = &span.kind {
                member_access_indices
                    .entry(*member_name)
                    .or_insert_with(Vec::new)
                    .push(index);
            }
        }

        SymbolMap {
            spans,
            member_access_indices,
            container_values,
            source_len: u32::try_from(content.len()).unwrap_or(u32::MAX),
            ..SymbolMap::default()
        }
    }
}

impl Backend {
    /// The spans of a Twig template: the names it is loaded by, the
    /// templates it names, and in a Symfony project the route names and
    /// translation keys it reads.
    fn twig_template_spans(&self, uri: &str, content: &str, is_symfony: bool) -> Vec<SymbolSpan> {
        let roots = self.twig_roots();
        let names = crate::util::uri_to_path(uri)
            .map(|path| roots.template_names(&path))
            .unwrap_or_default();
        let mut spans = crate::twig::templates::template_declaration_spans(names);
        if is_symfony {
            spans.extend(crate::symfony::routes::twig_route_spans(content));
            spans.extend(crate::symfony::translations::twig_translation_spans(
                content,
            ));
        }
        spans.extend(crate::twig::templates::twig_template_spans(
            content,
            |name| roots.is_template_name(name),
        ));
        spans.sort_by_key(|span| span.start);
        spans
    }
}

fn symbol_at(content: &str, position: Position) -> Option<ResourceSymbol> {
    let offset = crate::text_position::position_to_offset(content, position) as usize;
    let previous_offset = offset.checked_sub(1);

    for symbol in scan_symbols(content) {
        if contains_cursor(
            symbol.class_start,
            symbol.class_end,
            offset,
            previous_offset,
        ) {
            return Some(ResourceSymbol::Class(symbol.class_fqn));
        }
        if let Some((member_name, member_start, member_end)) = symbol.member
            && contains_cursor(member_start, member_end, offset, previous_offset)
        {
            return Some(ResourceSymbol::Member {
                class_fqn: symbol.class_fqn,
                member_name,
            });
        }
    }
    None
}

fn scan_symbols(content: &str) -> Vec<ScannedResourceSymbol> {
    let bytes = content.as_bytes();
    let mut cursor = 0usize;
    let mut symbols = Vec::new();

    while cursor < bytes.len() {
        if !is_name_start(bytes[cursor]) || (cursor > 0 && is_name_char(bytes[cursor - 1])) {
            cursor += 1;
            continue;
        }

        let class_start = cursor;
        let mut class_end = cursor + 1;
        while class_end < bytes.len() && is_name_char(bytes[class_end]) {
            class_end += 1;
        }

        let raw_name = &content[class_start..class_end];
        if raw_name.contains('\\') && !raw_name.ends_with('\\') {
            let fqn = normalize_fqn(raw_name);
            if is_class_fqn(&fqn) {
                let member = if bytes.get(class_end) == Some(&b':')
                    && bytes.get(class_end + 1) == Some(&b':')
                {
                    let member_start = class_end + 2;
                    let member_end = scan_identifier(bytes, member_start);
                    if member_end > member_start {
                        Some((
                            content[member_start..member_end].to_string(),
                            member_start,
                            member_end,
                        ))
                    } else {
                        None
                    }
                } else {
                    None
                };
                symbols.push(ScannedResourceSymbol {
                    class_fqn: fqn,
                    class_start,
                    class_end,
                    member,
                });
            }
        }

        cursor = class_end;
    }

    symbols
}

fn contains_cursor(start: usize, end: usize, offset: usize, previous: Option<usize>) -> bool {
    (start..end).contains(&offset) || previous.is_some_and(|offset| (start..end).contains(&offset))
}

fn is_name_start(byte: u8) -> bool {
    byte == b'\\' || byte == b'_' || byte.is_ascii_alphabetic() || !byte.is_ascii()
}

fn is_name_char(byte: u8) -> bool {
    byte == b'\\' || byte == b'_' || byte.is_ascii_alphanumeric() || !byte.is_ascii()
}

fn scan_identifier(bytes: &[u8], start: usize) -> usize {
    if !bytes
        .get(start)
        .is_some_and(|byte| *byte == b'_' || byte.is_ascii_alphabetic() || !byte.is_ascii())
    {
        return start;
    }

    let mut end = start + 1;
    while end < bytes.len()
        && (bytes[end] == b'_' || bytes[end].is_ascii_alphanumeric() || !bytes[end].is_ascii())
    {
        end += 1;
    }
    end
}

fn normalize_fqn(raw: &str) -> String {
    let mut normalized = String::with_capacity(raw.len());
    let mut previous_was_separator = false;

    for character in raw.trim_matches('\\').chars() {
        if character == '\\' {
            if !previous_was_separator {
                normalized.push(character);
            }
            previous_was_separator = true;
        } else {
            normalized.push(character);
            previous_was_separator = false;
        }
    }

    normalized
}

fn is_class_fqn(name: &str) -> bool {
    let mut segments = name.split('\\');
    let Some(first) = segments.next() else {
        return false;
    };
    if !is_identifier(first) {
        return false;
    }

    let mut has_namespace = false;
    for segment in segments {
        has_namespace = true;
        if !is_identifier(segment) {
            return false;
        }
    }
    has_namespace
}

fn is_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (first == '_' || first.is_alphabetic())
        && characters.all(|character| character == '_' || character.is_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_classes_without_knowing_yaml_keys() {
        let content = "anything: App\\UseCase\\Run\n";
        assert_eq!(
            symbol_at(content, Position::new(0, 18)),
            Some(ResourceSymbol::Class("App\\UseCase\\Run".to_string()))
        );
    }

    #[test]
    fn scans_xml_text_and_attributes() {
        let content = r#"<item handler="App\Handler\Run">App\Handler\Fallback</item>"#;
        assert_eq!(
            symbol_at(content, Position::new(0, 21)),
            Some(ResourceSymbol::Class("App\\Handler\\Run".to_string()))
        );
        assert_eq!(
            symbol_at(content, Position::new(0, 39)),
            Some(ResourceSymbol::Class("App\\Handler\\Fallback".to_string()))
        );
    }

    #[test]
    fn scans_class_members_and_yaml_escaped_names() {
        let content = r#"callback: "App\\Handler\\Run::handle""#;
        assert_eq!(
            symbol_at(content, Position::new(0, 32)),
            Some(ResourceSymbol::Member {
                class_fqn: "App\\Handler\\Run".to_string(),
                member_name: "handle".to_string(),
            })
        );
    }

    #[test]
    fn ignores_short_and_malformed_names() {
        assert_eq!(symbol_at("handler: Run", Position::new(0, 10)), None);
        assert_eq!(symbol_at("path: folder\\-file", Position::new(0, 9)), None);
        assert_eq!(
            symbol_at("prefix: App\\Handler\\", Position::new(0, 14)),
            None
        );
    }
}
