//! The entity types a Drupal entity class declares, by attribute or by the
//! annotation older code uses (see [`crate::drupal`]).

use super::*;
use crate::symbol_map::{EntityTypeSite, StringKeyKind};
use crate::virtual_members::laravel::helpers::resolved_string_literal;

const CONTENT_STORAGE: &str = "Drupal\\Core\\Entity\\Sql\\SqlContentEntityStorage";
const CONFIG_STORAGE: &str = "Drupal\\Core\\Config\\Entity\\ConfigEntityStorage";

/// An entity type attribute and the annotation older code writes instead,
/// with the storage a declaration that names none gets.
struct EntityTypeKind {
    attribute: &'static str,
    /// The annotation's opening, matched by short name, the way Drupal's
    /// annotation discovery reads it.
    annotation: &'static str,
    default_storage: Option<&'static str>,
}

const ENTITY_TYPE_KINDS: [EntityTypeKind; 3] = [
    EntityTypeKind {
        attribute: "Drupal\\Core\\Entity\\Attribute\\ContentEntityType",
        annotation: "@ContentEntityType(",
        default_storage: Some(CONTENT_STORAGE),
    },
    EntityTypeKind {
        attribute: "Drupal\\Core\\Entity\\Attribute\\ConfigEntityType",
        annotation: "@ConfigEntityType(",
        default_storage: Some(CONFIG_STORAGE),
    },
    EntityTypeKind {
        attribute: "Drupal\\Core\\Entity\\Attribute\\EntityType",
        annotation: "@EntityType(",
        default_storage: None,
    },
];

/// Record the entity type `class` declares: a span over its ID, and the
/// class and storage the ID stands for.
pub(in crate::symbol_map::extraction) fn emit_drupal_entity_type_spans(
    class: &Class<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal {
        return;
    }
    let mut found = Vec::new();
    for kind in &ENTITY_TYPE_KINDS {
        for attribute in attributes_named(&class.attribute_lists, kind.attribute, ctx) {
            let mut id = None;
            let mut storage = kind.default_storage.map(str::to_string);
            for (parameter, value) in attribute_arguments(attribute, &["id"]) {
                match parameter {
                    "id" => id = resolved_string_literal(value, ctx.content),
                    "handlers" => {
                        if let Some(named) = handler_class(value, "storage", ctx) {
                            storage = Some(named);
                        }
                    }
                    _ => {}
                }
            }
            if let Some((key, start, end)) = id.filter(|(key, _, _)| !key.is_empty()) {
                found.push((key.to_string(), start as u32, end as u32, storage));
            }
        }
    }
    if found.is_empty()
        && let Some((doc, offset)) = get_docblock_text_with_offset(ctx.trivias, ctx.content, class)
        && doc.contains("EntityType(")
    {
        found.extend(annotation_entity_type(doc, offset));
    }
    if found.is_empty() {
        return;
    }
    let Some(fqn) = ctx
        .resolved_name_at(class.name.span.start.offset)
        .map(|fqn| crate::atom::atom(fqn.trim_start_matches('\\')))
    else {
        return;
    };
    for (key, start, end, storage) in found {
        ctx.spans.push(SymbolSpan {
            start,
            end,
            kind: SymbolKind::StringKey {
                kind: StringKeyKind::DrupalEntityType,
                key,
                is_write: true,
                is_optional: false,
            },
        });
        ctx.entity_types.push(EntityTypeSite {
            start,
            class: fqn,
            storage: storage.map(|storage| crate::atom::atom(&storage)),
        });
    }
}

/// The class a `handlers` array gives the handler `name`, as a class
/// constant or a string.
fn handler_class(handlers: &Expression<'_>, name: &str, ctx: &ExtractionCtx<'_>) -> Option<String> {
    let entries = crate::parser::array_literal_elements(handlers)?;
    entries.iter().find_map(|entry| {
        let ArrayElement::KeyValue(entry) = entry else {
            return None;
        };
        let (key, _, _) = resolved_string_literal(entry.key, ctx.content)?;
        (key == name)
            .then(|| message_class(entry.value, ctx))
            .flatten()
    })
}

/// The entity type an `@ContentEntityType(…)`-family annotation in the
/// docblock `doc`, which starts at `offset` in the file, declares: its ID
/// with the ID's range in the file, and its storage.
fn annotation_entity_type(doc: &str, offset: u32) -> Option<(String, u32, u32, Option<String>)> {
    let (body, default_storage) = ENTITY_TYPE_KINDS.iter().find_map(|kind| {
        let at = doc.find(kind.annotation)?;
        Some((at + kind.annotation.len(), kind.default_storage))
    })?;
    let mut id = None;
    let mut storage = default_storage.map(str::to_string);
    for entry in annotation_entries(doc, body) {
        match (entry.path.as_slice(), entry.key) {
            ([], "id") => id = Some((entry.value_start, entry.value_end)),
            (["handlers"], "storage") => {
                storage = Some(
                    doc[entry.value_start..entry.value_end]
                        .trim_start_matches('\\')
                        .to_string(),
                );
            }
            _ => {}
        }
    }
    let (start, end) = id?;
    let key = &doc[start..end];
    (!key.is_empty()).then(|| {
        (
            key.to_string(),
            offset + start as u32,
            offset + end as u32,
            storage,
        )
    })
}

/// A `key = "value"` pair of an annotation, with the keys of the `{…}`
/// blocks around it.
struct AnnotationEntry<'d> {
    path: Vec<&'d str>,
    key: &'d str,
    value_start: usize,
    value_end: usize,
}

/// Every `key = "value"` pair of the annotation whose arguments start at
/// `from` in `doc`, up to its closing parenthesis.  Keys may be quoted.
fn annotation_entries(doc: &str, from: usize) -> Vec<AnnotationEntry<'_>> {
    let bytes = doc.as_bytes();
    let mut entries = Vec::new();
    let mut path: Vec<&str> = Vec::new();
    // The key read last, waiting for its `=`, and whether the `=` came.
    let mut key: Option<&str> = None;
    let mut assigned = false;
    let mut parens = 0usize;
    let mut i = from;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                let start = i + 1;
                let Some(len) = doc[start..].find('"') else {
                    break;
                };
                let end = start + len;
                if assigned {
                    if let Some(name) = key.take() {
                        entries.push(AnnotationEntry {
                            path: path.clone(),
                            key: name,
                            value_start: start,
                            value_end: end,
                        });
                    }
                    assigned = false;
                } else {
                    key = Some(&doc[start..end]);
                }
                i = end + 1;
                continue;
            }
            b'=' => assigned = key.is_some(),
            b'{' => {
                if assigned && let Some(name) = key.take() {
                    path.push(name);
                } else {
                    path.push("");
                }
                assigned = false;
            }
            b'}' => {
                path.pop();
                key = None;
                assigned = false;
            }
            // A nested annotation's own pairs (`@Translation(context = …)`)
            // are not the entity type's.
            b'(' => {
                parens += 1;
                path.push("(");
                key = None;
                assigned = false;
            }
            b')' => {
                if parens == 0 {
                    break;
                }
                parens -= 1;
                path.pop();
                key = None;
                assigned = false;
            }
            b',' => {
                key = None;
                assigned = false;
            }
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                if !assigned {
                    key = Some(&doc[start..i]);
                } else {
                    // A bare value (`TRUE`, a constant): nothing to read.
                    key = None;
                    assigned = false;
                }
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::OwnedResolvedNames;

    /// The entity type IDs, classes, and storages `php` declares.
    fn declared(php: &str) -> Vec<(String, String, Option<String>)> {
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"test.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
        let resolver = mago_names::resolver::NameResolver::new(&arena);
        let names = OwnedResolvedNames::from_resolved(&resolver.resolve(program));
        let map = crate::symbol_map::extraction::extract_symbol_map_for_index(
            program,
            php,
            &names,
            crate::symbol_map::extraction::Frameworks {
                symfony: false,
                service_container: false,
                drupal: true,
                twig: false,
            },
            None,
        );
        map.entity_types
            .iter()
            .map(|site| {
                let id = map
                    .spans
                    .iter()
                    .find_map(|span| match &span.kind {
                        SymbolKind::StringKey {
                            kind: StringKeyKind::DrupalEntityType,
                            key,
                            is_write: true,
                            ..
                        } if span.start == site.start => Some(key.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                (
                    id,
                    site.class.to_string(),
                    site.storage.as_ref().map(ToString::to_string),
                )
            })
            .collect()
    }

    #[test]
    fn an_attribute_declares_the_id_class_and_storage() {
        let php = "<?php\nnamespace Drupal\\node\\Entity;\nuse Drupal\\Core\\Entity\\Attribute\\ContentEntityType;\nuse Drupal\\node\\NodeStorage;\n#[ContentEntityType(\n  id: 'node',\n  handlers: ['storage' => NodeStorage::class, 'form' => ['default' => 'X']],\n)]\nclass Node {}\n";
        assert_eq!(
            declared(php),
            vec![(
                "node".to_string(),
                "Drupal\\node\\Entity\\Node".to_string(),
                Some("Drupal\\node\\NodeStorage".to_string())
            )]
        );
    }

    #[test]
    fn a_config_attribute_without_handlers_gets_config_storage() {
        let php = "<?php\nnamespace Drupal\\acme\\Entity;\nuse Drupal\\Core\\Entity\\Attribute\\ConfigEntityType;\n#[ConfigEntityType(id: 'acme_preset')]\nclass Preset {}\n";
        assert_eq!(
            declared(php),
            vec![(
                "acme_preset".to_string(),
                "Drupal\\acme\\Entity\\Preset".to_string(),
                Some(CONFIG_STORAGE.to_string())
            )]
        );
    }

    #[test]
    fn an_annotation_declares_too_and_another_attribute_does_not() {
        let php = "<?php\nnamespace Drupal\\flag\\Entity;\n/**\n * @ConfigEntityType(\n *   id = \"flag\",\n * )\n */\nclass Flag {}\n#[\\Other\\ContentEntityType(id: 'nope')]\nclass Other {}\n";
        assert_eq!(
            declared(php),
            vec![(
                "flag".to_string(),
                "Drupal\\flag\\Entity\\Flag".to_string(),
                Some(CONFIG_STORAGE.to_string())
            )]
        );
    }

    #[test]
    fn annotations_give_the_id_and_the_storage_handler() {
        let doc = "/**\n * @ContentEntityType(\n *   id = \"redirect\",\n *   label = @Translation(\"Redirect\"),\n *   handlers = {\n *     \"list_builder\" = \"Drupal\\redirect\\RedirectListBuilder\",\n *     \"storage\" = \"\\Drupal\\redirect\\RedirectStorage\",\n *     \"form\" = {\n *       \"id\" = \"nested\"\n *     }\n *   },\n *   entity_keys = {\n *     \"id\" = \"rid\"\n *   }\n * )\n */";
        let (id, start, end, storage) = annotation_entity_type(doc, 100).unwrap();
        assert_eq!(id, "redirect");
        assert_eq!(
            &doc[(start - 100) as usize..(end - 100) as usize],
            "redirect"
        );
        assert_eq!(
            storage.as_deref(),
            Some("Drupal\\redirect\\RedirectStorage")
        );
    }

    #[test]
    fn an_annotation_without_handlers_gets_its_kinds_storage() {
        let doc = "/**\n * @ConfigEntityType(id = \"flag\", label = @Translation(\"Flag\"))\n */";
        let (id, _, _, storage) = annotation_entity_type(doc, 0).unwrap();
        assert_eq!(id, "flag");
        assert_eq!(storage.as_deref(), Some(CONFIG_STORAGE));
    }

    #[test]
    fn a_nested_annotations_pairs_are_its_own() {
        let doc = "/**\n * @ContentEntityType(\n *   label = @Translation(\"Note\", id = \"other\"),\n *   id = \"note\",\n *   extra = @Bar(handlers = {\"storage\" = \"Wrong\"}),\n * )\n */";
        let (id, _, _, storage) = annotation_entity_type(doc, 0).unwrap();
        assert_eq!(id, "note");
        assert_eq!(storage.as_deref(), Some(CONTENT_STORAGE));
    }

    #[test]
    fn a_doc_comment_without_an_entity_type_declares_none() {
        assert!(annotation_entity_type("/** @see EntityType(id = \"x\") */", 0).is_none());
    }
}
