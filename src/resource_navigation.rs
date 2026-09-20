//! PHP symbol navigation from non-PHP resource files.
//!
//! YAML and XML often carry fully-qualified PHP class names in arbitrary
//! keys, values, attributes, and text. This module recognises those names
//! without knowing the schema of the file that contains them, then delegates
//! declaration lookup to the normal PHP class loader.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tower_lsp::lsp_types::{Location, Position};

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

/// Whether `uri` names a YAML or XML document that can carry PHP symbols.
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
    ]
    .iter()
    .any(|suffix| path.ends_with(suffix))
}

/// Whether a filesystem path is a YAML/XML resource document.
pub(crate) fn is_resource_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_resource_document)
}

impl Backend {
    /// Resolve the fully-qualified PHP class or `Class::member` under the
    /// cursor in a YAML/XML document.
    pub(crate) fn resolve_resource_definition(
        &self,
        content: &str,
        position: Position,
    ) -> Option<Location> {
        match symbol_at(content, position)? {
            ResourceSymbol::Class(fqn) => self.class_declaration_location(&fqn),
            ResourceSymbol::Member {
                class_fqn,
                member_name,
            } => self.class_member_declaration_location(&class_fqn, &member_name),
        }
    }

    /// Replace one resource document's synthetic symbol map and reference
    /// contributions.
    pub(crate) fn update_resource_symbol_index(&self, uri: &str, content: &str) {
        let symbol_map = Arc::new(self.resource_symbol_map(content));
        self.symbol_maps
            .write()
            .insert(uri.to_string(), Arc::clone(&symbol_map));
        self.reindex_references_for_symbol_maps_batch(vec![(uri.to_string(), symbol_map)]);
    }

    /// Index resource files discovered during the workspace walk.
    pub(crate) fn index_resource_paths_batch(&self, files: &[(String, PathBuf)]) {
        let maps: Vec<(String, Arc<SymbolMap>)> = files
            .iter()
            .filter_map(|(uri, path)| {
                let content = std::fs::read_to_string(path).ok()?;
                Some((uri.clone(), Arc::new(self.resource_symbol_map(&content))))
            })
            .collect();
        if maps.is_empty() {
            return;
        }

        {
            let mut symbol_maps = self.symbol_maps.write();
            for (uri, map) in &maps {
                symbol_maps.insert(uri.clone(), Arc::clone(map));
            }
        }
        self.reindex_references_for_symbol_maps_batch(maps);
    }

    fn resource_symbol_map(&self, content: &str) -> SymbolMap {
        let mut spans = Vec::new();
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
            source_len: u32::try_from(content.len()).unwrap_or(u32::MAX),
            ..SymbolMap::default()
        }
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
