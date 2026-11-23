use super::FieldSources;

const NOTE: &str = r#"<?php
namespace Drupal\acme\Entity;

use Drupal\Core\Entity\Attribute\ContentEntityType;
use Drupal\Core\Entity\ContentEntityBase;
use Drupal\Core\Entity\EntityTypeInterface;
use Drupal\Core\Field\BaseFieldDefinition;

#[ContentEntityType(id: 'acme_note', entity_keys: ['id' => 'id'])]
class Note extends ContentEntityBase {
    public static function baseFieldDefinitions(EntityTypeInterface $entity_type) {
        $fields = parent::baseFieldDefinitions($entity_type);
        $fields['body'] = BaseFieldDefinition::create('string');
        return $fields;
    }
}
"#;

/// What a custom module's file holding `content` gives the field index.
fn sources(content: &str) -> FieldSources {
    let dir = tempfile::tempdir().unwrap();
    let backend = crate::Backend::new_test_with_workspace(dir.path().to_path_buf(), vec![]);
    backend.mark_drupal_project();
    let uri = crate::util::path_to_uri(&dir.path().join("web/modules/custom/acme/src/Note.php"));
    backend.update_ast(&uri, content);
    let map = backend.symbol_map_for(&uri).expect("the file's map");
    // Compared across backends, so the URI must not differ.
    FieldSources {
        uri: std::sync::Arc::from("file:///note.php"),
        ..FieldSources::of(&uri, &map, || true, || true).expect("the file declares fields")
    }
}

/// Text moved around a declaration changes where its fields are written,
/// not what they are, so the classes that have them stay resolved.
#[test]
fn moved_declarations_declare_the_same_fields() {
    let before = sources(NOTE);
    let moved = sources(&NOTE.replace("class Note", "\n\nclass Note"));
    assert_ne!(before, moved);
    assert!(before.declares_as(&moved));
}

#[test]
fn a_renamed_field_or_a_changed_key_declares_other_fields() {
    let before = sources(NOTE);
    for changed in [
        NOTE.replace("$fields['body']", "$fields['summary']"),
        NOTE.replace("create('string')", "create('integer')"),
        NOTE.replace("'id' => 'id'", "'id' => 'nid'"),
        NOTE.replace(
            "$fields['body']",
            "unset($fields['id']);\n        $fields['body']",
        ),
    ] {
        assert!(!before.declares_as(&sources(&changed)), "{changed}");
    }
}

/// What a field storage file holding `content` gives the field index.
fn storage_sources(content: &str) -> FieldSources {
    let map = crate::symbol_map::SymbolMap {
        drupal_fields: crate::symbol_map::DrupalFieldSites {
            storage: crate::drupal::field_storage::read_field_storage(content),
            ..Default::default()
        }
        .boxed(),
        ..Default::default()
    };
    FieldSources::of(
        "file:///field.storage.node.field_tags.yml",
        &map,
        || true,
        || true,
    )
    .expect("the file declares a field")
}

const STORAGE: &str = "id: node.field_tags\nfield_name: field_tags\nentity_type: node\ntype: entity_reference\nsettings:\n  target_type: taxonomy_term\n";

/// Editing a field storage file anywhere but the field it declares, as
/// an exported file's `uuid` or `dependencies` change, keeps the classes
/// that have the field resolved.
#[test]
fn a_storage_edited_around_its_field_declares_the_same_field() {
    let before = storage_sources(STORAGE);
    let moved = storage_sources(&format!("uuid: 0d4c5b0e\nlangcode: en\n{STORAGE}"));
    assert_ne!(before, moved);
    assert!(before.declares_as(&moved));
    for changed in [
        STORAGE.replace("type: entity_reference", "type: string"),
        STORAGE.replace("taxonomy_term", "user"),
        STORAGE.replace("entity_type: node", "entity_type: user"),
        STORAGE.replace("field_name: field_tags", "field_name: field_topics"),
    ] {
        assert!(!before.declares_as(&storage_sources(&changed)), "{changed}");
    }
}

/// The field storage files one publish brings, as indexing publishes a
/// folder of them, drop the fields once.
#[test]
fn a_batch_of_field_changes_drops_the_fields_once() {
    let dir = tempfile::tempdir().unwrap();
    let backend = crate::Backend::new_test_with_workspace(dir.path().to_path_buf(), vec![]);
    backend.mark_drupal_project();
    let maps: Vec<(String, std::sync::Arc<crate::symbol_map::SymbolMap>)> = ["tags", "topics"]
        .iter()
        .map(|name| {
            let content = STORAGE.replace("field_tags", &format!("field_{name}"));
            let path = dir
                .path()
                .join(format!("config/sync/field.storage.node.field_{name}.yml"));
            let map = crate::symbol_map::SymbolMap {
                drupal_fields: crate::symbol_map::DrupalFieldSites {
                    storage: crate::drupal::field_storage::read_field_storage(&content),
                    ..Default::default()
                }
                .boxed(),
                ..Default::default()
            };
            (crate::util::path_to_uri(&path), std::sync::Arc::new(map))
        })
        .collect();
    let before = backend.drupal.fields.generation();
    backend.publish_symbol_maps(maps);
    assert_eq!(backend.drupal.fields.generation(), before + 1);
}

/// A storage that does not count gives nothing.
#[test]
fn a_storage_that_does_not_count_gives_nothing() {
    let map = crate::symbol_map::SymbolMap {
        drupal_fields: crate::symbol_map::DrupalFieldSites {
            storage: crate::drupal::field_storage::read_field_storage(STORAGE),
            ..Default::default()
        }
        .boxed(),
        ..Default::default()
    };
    assert!(FieldSources::of("file:///f.yml", &map, || true, || false).is_none());
}

/// A target written after the parent's settings is the one the parent's
/// leave in place: the furthest ancestor's wins, and none is no target.
#[test]
fn a_target_after_the_parent_s_settings_is_used_when_the_parent_has_none() {
    use crate::symbol_map::{DefaultTarget, FieldItemSite};
    let class = |name: &str, parent: Option<&str>| {
        std::sync::Arc::new(crate::types::ClassInfo {
            name: crate::atom::atom(name),
            parent_class: parent.map(crate::atom::atom),
            ..Default::default()
        })
    };
    let classes = [
        class("PlainItem", None),
        class("FileItem", Some("PlainItem")),
        class("MediaItem", Some("FileItem")),
        class("OtherItem", Some("PlainItem")),
    ];
    let loader = |name: crate::class_lookup::Fqn<'_>| {
        classes
            .iter()
            .find(|class| class.name == name.as_str())
            .cloned()
    };
    let site = |owner: &str, target: Option<&str>| {
        (
            owner.to_ascii_lowercase(),
            FieldItemSite {
                owner: crate::atom::atom(owner),
                property_definitions: None,
                default_target: Some(DefaultTarget::FromParent {
                    otherwise: target.map(|target| Some(target.to_string())),
                }),
            },
        )
    };
    let items: std::collections::HashMap<String, FieldItemSite> = [
        site("FileItem", Some("file")),
        site("MediaItem", Some("media")),
        site("OtherItem", None),
    ]
    .into_iter()
    .collect();
    let target = |item: &str| super::default_target(&items, crate::atom::atom(item), &loader);
    assert_eq!(target("MediaItem").as_deref(), Some("file"));
    assert_eq!(target("FileItem").as_deref(), Some("file"));
    assert_eq!(target("OtherItem"), None);
}

const FIELD_ITEM_INTERFACE: &str = "<?php\nnamespace Drupal\\Core\\Field;\ninterface FieldItemInterface {\n    public function __get($name);\n}\n";
const EXCERPT_ITEM: &str = r#"<?php
namespace Drupal\acme\Plugin\Field\FieldType;
use Drupal\Core\TypedData\DataDefinition;
class ExcerptItem implements \Drupal\Core\Field\FieldItemInterface {
    public function __get($name) {}
    public static function propertyDefinitions($field_definition) {
        $properties['value'] = DataDefinition::create('string');
        return $properties;
    }
}
"#;

/// An item class resolved between the change of its property definitions
/// and the end of the publish that brought it takes its properties from the
/// index that change drops, so it goes with that index.
#[test]
fn an_item_class_resolved_before_the_fields_drop_goes_with_them() {
    let dir = tempfile::tempdir().unwrap();
    // A workspace root of its own would leave the providers off.
    let backend = crate::Backend::new_test();
    backend.mark_drupal_project();
    let interface = crate::util::path_to_uri(&dir.path().join("FieldItemInterface.php"));
    backend.update_ast(&interface, FIELD_ITEM_INTERFACE);
    let uri = crate::util::path_to_uri(
        &dir.path()
            .join("web/modules/custom/acme/src/Plugin/Field/FieldType/ExcerptItem.php"),
    );
    backend.update_ast(&uri, EXCERPT_ITEM);
    let properties = || -> Vec<String> {
        let loader = backend.resolved_class_loader_with(&[]);
        let class = loader(crate::class_lookup::Fqn::new(
            "Drupal\\acme\\Plugin\\Field\\FieldType\\ExcerptItem",
        ))
        .expect("the item class");
        crate::virtual_members::resolve_class_fully_cached(
            &class,
            &loader,
            &backend.resolved_class_cache,
        )
        .properties
        .iter()
        .map(|property| property.name.to_string())
        .collect()
    };
    assert!(properties().contains(&"value".to_string()));

    // What another backend reads from the edited file, noted without the
    // publish that would drop the fields at once.
    let edited = EXCERPT_ITEM.replace("$properties['value']", "$properties['summary']");
    let other = crate::Backend::new_test();
    other.mark_drupal_project();
    other.update_ast(&uri, &edited);
    let map = other.symbol_map_for(&uri).expect("the file's map");
    backend.note_drupal_field_sources(&uri, Some(&map));
    // A request resolves the item class before the publish ends.
    properties();
    backend.flush_drupal_field_changes();

    let after = properties();
    assert!(after.contains(&"summary".to_string()), "{after:?}");
}

/// While the fields are being read, as during indexing, a publish that
/// declares classes the read has not looked at leaves the read alone, and
/// one that changes a class it has looked at reads them again.
#[test]
fn a_read_in_progress_restarts_only_for_the_classes_it_looked_at() {
    let backend = crate::Backend::new_test();
    backend.mark_drupal_project();
    for (changed, expected_reads) in [
        ("Drupal\\acme\\Plugin\\Unrelated", 1),
        ("Drupal\\acme\\Entity\\Note", 2),
    ] {
        let reads = std::cell::Cell::new(0);
        backend.drupal.fields.get_or_read(|| {
            reads.set(reads.get() + 1);
            backend
                .drupal
                .fields
                .note_read_class("Drupal\\acme\\Entity\\Note");
            if reads.get() == 1 {
                backend.note_drupal_class_graph_changes(&[changed]);
                backend.flush_drupal_field_changes();
            }
            crate::drupal::fields::FieldIndex::default()
        });
        assert_eq!(reads.get(), expected_reads, "{changed}");
        backend.drupal.fields.invalidate();
    }
}
