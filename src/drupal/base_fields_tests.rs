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
    ] {
        assert!(!before.declares_as(&sources(&changed)), "{changed}");
    }
}

/// What a field storage file holding `content` gives the field index.
fn storage_sources(content: &str) -> FieldSources {
    let map = crate::symbol_map::SymbolMap {
        drupal_field_storage: crate::drupal::field_storage::read_field_storage(content)
            .map(Box::new),
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

/// A storage that does not count gives nothing.
#[test]
fn a_storage_that_does_not_count_gives_nothing() {
    let map = crate::symbol_map::SymbolMap {
        drupal_field_storage: crate::drupal::field_storage::read_field_storage(STORAGE)
            .map(Box::new),
        ..Default::default()
    };
    assert!(FieldSources::of("file:///f.yml", &map, || true, || false).is_none());
}
