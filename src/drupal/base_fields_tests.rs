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
        ..FieldSources::of(&uri, &map).expect("the file declares fields")
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
