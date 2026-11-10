//! Drupal's entity fields: what reading a property on a content entity
//! gives, on the entity class and on the interfaces it implements.

use crate::common::{hover_at, hover_text, initialize_with_options, open_document};
use crate::drupal_services::{
    definitions_at, position_in, uri_for, workspace, workspace_opened_early,
};
use phpantom_lsp::Backend;

/// Just enough of core's entity interfaces and classes.
const ENTITY_STUBS: &[(&str, &str)] = &[
    (
        "web/core/lib/Drupal/Core/Entity/EntityInterface.php",
        "<?php\nnamespace Drupal\\Core\\Entity;\ninterface EntityInterface {\n    public function id();\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Entity/RevisionableInterface.php",
        "<?php\nnamespace Drupal\\Core\\Entity;\ninterface RevisionableInterface extends EntityInterface {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Entity/FieldableEntityInterface.php",
        "<?php\nnamespace Drupal\\Core\\Entity;\ninterface FieldableEntityInterface extends EntityInterface {\n    public function get($field_name);\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Entity/ContentEntityInterface.php",
        "<?php\nnamespace Drupal\\Core\\Entity;\ninterface ContentEntityInterface extends FieldableEntityInterface, RevisionableInterface {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Config/Entity/ConfigEntityInterface.php",
        "<?php\nnamespace Drupal\\Core\\Config\\Entity;\ninterface ConfigEntityInterface extends \\Drupal\\Core\\Entity\\EntityInterface {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Entity/ContentEntityBase.php",
        "<?php\nnamespace Drupal\\Core\\Entity;\nabstract class ContentEntityBase implements ContentEntityInterface {\n    public function id() {}\n    public function get($field_name) {}\n    public function &__get($name) {}\n}\n",
    ),
    (
        "web/core/modules/node/src/NodeInterface.php",
        "<?php\nnamespace Drupal\\node;\ninterface NodeInterface extends \\Drupal\\Core\\Entity\\ContentEntityInterface {}\n",
    ),
    (
        "web/core/modules/node/src/Entity/Node.php",
        "<?php\nnamespace Drupal\\node\\Entity;\nuse Drupal\\Core\\Entity\\ContentEntityBase;\nuse Drupal\\node\\NodeInterface;\n/**\n * @ContentEntityType(\n *   id = \"node\",\n * )\n */\nclass Node extends ContentEntityBase implements NodeInterface {}\n",
    ),
    (
        "web/core/modules/user/src/UserInterface.php",
        "<?php\nnamespace Drupal\\user;\ninterface UserInterface extends \\Drupal\\Core\\Entity\\ContentEntityInterface {}\n",
    ),
];

const READER: &str = r#"<?php
namespace Drupal\acme;

use Drupal\Core\Config\Entity\ConfigEntityInterface;
use Drupal\Core\Entity\EntityInterface;
use Drupal\Core\Entity\RevisionableInterface;
use Drupal\node\Entity\Node;
use Drupal\node\NodeInterface;
use Drupal\user\UserInterface;

final class Reader {
    public function fieldable(NodeInterface $node, UserInterface $user, Node $entity): void {
        $body = $node->body->value;
        $raw = $user->passRaw;
        $title = $entity->title;
        $entity->__get('title');
        $node->title = 'Example';
        $user->passRaw = 'example';
    }

    public function plain(EntityInterface $entity, RevisionableInterface $revision, ConfigEntityInterface $config): void {
        $entity->body;
        $revision->body;
        $config->body;
    }
}
"#;

const READER_PATH: &str = "web/modules/custom/acme/src/Reader.php";

/// The unknown-member messages the diagnostics of [`READER`] report.
fn unknown_members(backend: &Backend, uri: &str) -> Vec<String> {
    crate::common::slow_diagnostic_messages(backend, uri, READER, "unknown_member")
}

/// A content entity's interface reads any property through the magic
/// `__get()` every content entity has, the way the entity class does; an
/// interface without fields still reports one it does not declare.
#[tokio::test]
async fn fieldable_interfaces_read_any_property_as_the_class_does() {
    let (backend, dir) = workspace(&[ENTITY_STUBS, &[(READER_PATH, READER)]].concat()).await;
    let uri = uri_for(&dir, READER_PATH);

    let mut messages = unknown_members(&backend, uri.as_str());
    messages.sort();
    assert_eq!(messages.len(), 3, "{messages:?}");
    for (message, receiver) in messages.iter().zip([
        "ConfigEntityInterface",
        "EntityInterface",
        "RevisionableInterface",
    ]) {
        assert!(message.contains(&format!("{receiver}::$body")), "{message}");
    }

    let on_class = variable_hover(&backend, uri.as_str(), "$title");
    assert_eq!(variable_hover(&backend, uri.as_str(), "$raw"), on_class);
    assert_eq!(variable_hover(&backend, uri.as_str(), "$body"), on_class);
}

/// The hover on the variable `var` assigned in [`READER`], with its name
/// left out.
fn variable_hover(backend: &Backend, uri: &str, var: &str) -> String {
    let (line, text) = READER
        .lines()
        .enumerate()
        .find(|(_, line)| line.trim_start().starts_with(&format!("{var} = ")))
        .expect("the variable should be assigned");
    let character = (text.len() - text.trim_start().len() + 1) as u32;
    let hover = hover_at(backend, uri, READER, line as u32, character)
        .unwrap_or_else(|| panic!("no hover on {var}"));
    hover_text(&hover).replace(var, "$_")
}

/// A value written on a content entity's interface goes through the same
/// magic `__set()` the class has, so a later read is not the written value.
#[tokio::test]
async fn a_write_on_a_fieldable_interface_does_not_narrow_the_read() {
    const WRITER: &str = r#"<?php
namespace Drupal\acme;

use Drupal\node\NodeInterface;

final class Writer {
    public function write(NodeInterface $node): void {
        $node->example_state = '';
        $node->example_state->value;
        $node->example_state = NULL;
        $node->example_state->value;
    }
}
"#;
    const WRITER_PATH: &str = "web/modules/custom/acme/src/Writer.php";
    let (backend, dir) = workspace(&[ENTITY_STUBS, &[(WRITER_PATH, WRITER)]].concat()).await;
    let uri = uri_for(&dir, WRITER_PATH);
    let messages = crate::common::slow_diagnostic_messages(
        &backend,
        uri.as_str(),
        WRITER,
        "scalar_member_access",
    );
    assert!(messages.is_empty(), "{messages:?}");
}

/// The entity class keeps the `__get()` it inherits.
#[tokio::test]
async fn an_entity_class_keeps_its_own_magic_get() {
    let (backend, dir) = workspace(&[ENTITY_STUBS, &[(READER_PATH, READER)]].concat()).await;
    let uri = uri_for(&dir, READER_PATH);
    let found = definitions_at(&backend, &uri, position_in(READER, "__get('title')", 2)).await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        found[0].uri,
        uri_for(
            &dir,
            "web/core/lib/Drupal/Core/Entity/ContentEntityBase.php"
        )
    );
}

/// A file the editor restores while the workspace is read sees the same
/// interfaces once it is.
#[tokio::test]
async fn files_opened_before_indexing_ends_read_any_property() {
    let (backend, dir) = workspace_opened_early(
        tempfile::tempdir().unwrap(),
        &[ENTITY_STUBS, &[(READER_PATH, READER)]].concat(),
    )
    .await;
    let uri = uri_for(&dir, READER_PATH);
    let messages = unknown_members(&backend, uri.as_str());
    assert_eq!(messages.len(), 3, "{messages:?}");
}

/// Outside a Drupal project the same interfaces are plain interfaces.
#[tokio::test]
async fn fieldable_interfaces_have_no_magic_outside_drupal() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("composer.json"), "{}").unwrap();
    for (path, content) in ENTITY_STUBS.iter().chain([&(READER_PATH, READER)]) {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    let backend = Backend::new_test();
    initialize_with_options(&backend, dir.path(), None).await;
    let uri = uri_for(&dir, READER_PATH);
    open_document(&backend, &uri, "php", READER).await;

    let messages = unknown_members(&backend, uri.as_str());
    assert!(
        messages
            .iter()
            .any(|message| message.contains("NodeInterface::$body")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("UserInterface::$passRaw")),
        "{messages:?}"
    );
}

// ─── Base fields ────────────────────────────────────────────────────────────

/// Core's entity API, field API, field types, and two entity types: `node`,
/// declared by attribute, translatable and revisionable, and `user`,
/// declared by annotation, translatable only.  Written where Composer puts
/// core, which the workspace's `.gitignore` leaves out.
const CORE: &[(&str, &str)] = &[
    (
        "web/core/lib/Drupal/Core/Entity/EntityInterface.php",
        "<?php\nnamespace Drupal\\Core\\Entity;\ninterface EntityInterface {\n    public function id();\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Entity/FieldableEntityInterface.php",
        "<?php\nnamespace Drupal\\Core\\Entity;\ninterface FieldableEntityInterface extends EntityInterface {\n    public function get($field_name);\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Entity/ContentEntityInterface.php",
        "<?php\nnamespace Drupal\\Core\\Entity;\ninterface ContentEntityInterface extends FieldableEntityInterface {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Entity/ContentEntityBase.php",
        r#"<?php
namespace Drupal\Core\Entity;

use Drupal\Core\Field\BaseFieldDefinition;

abstract class ContentEntityBase implements ContentEntityInterface {
    public function id() {}
    public function get($field_name) {}
    public function &__get($name) {}
    public function __set($name, $value) {}

    public static function baseFieldDefinitions(EntityTypeInterface $entity_type) {
        $fields = [];
        if ($entity_type->hasKey('id')) {
            $fields[$entity_type->getKey('id')] = BaseFieldDefinition::create('integer');
        }
        if ($entity_type->hasKey('uuid')) {
            $fields[$entity_type->getKey('uuid')] = BaseFieldDefinition::create('uuid');
        }
        if ($entity_type->hasKey('revision')) {
            $fields[$entity_type->getKey('revision')] = BaseFieldDefinition::create('integer');
        }
        if ($entity_type->hasKey('langcode')) {
            $fields[$entity_type->getKey('langcode')] = BaseFieldDefinition::create('language');
        }
        return $fields;
    }
}
"#,
    ),
    (
        "web/core/modules/user/src/EntityOwnerTrait.php",
        r#"<?php
namespace Drupal\user;

use Drupal\Core\Entity\EntityTypeInterface;
use Drupal\Core\Field\BaseFieldDefinition;

trait EntityOwnerTrait {
    public static function ownerBaseFieldDefinitions(EntityTypeInterface $entity_type) {
        return [
            $entity_type->getKey('owner') => BaseFieldDefinition::create('entity_reference')
                ->setLabel('Authored by')
                ->setSetting('target_type', 'user'),
        ];
    }
}
"#,
    ),
    (
        "web/core/lib/Drupal/Core/Field/BaseFieldDefinition.php",
        "<?php\nnamespace Drupal\\Core\\Field;\nclass BaseFieldDefinition {\n    public static function create($type): static { return new static(); }\n    public function setLabel($label): static { return $this; }\n    public function setSetting($name, $value): static { return $this; }\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Field/FieldItemListInterface.php",
        "<?php\nnamespace Drupal\\Core\\Field;\n/**\n * @template T of FieldItemInterface\n * @property mixed $value\n */\ninterface FieldItemListInterface {\n    public function __get($property_name);\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Field/FieldItemList.php",
        "<?php\nnamespace Drupal\\Core\\Field;\n/**\n * @template T of FieldItemInterface\n * @implements FieldItemListInterface<T>\n */\nclass FieldItemList implements FieldItemListInterface {\n    public function __get($property_name) {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Field/EntityReferenceFieldItemList.php",
        "<?php\nnamespace Drupal\\Core\\Field;\nclass EntityReferenceFieldItemList extends FieldItemList {}\n",
    ),
    (
        "web/core/lib/Drupal/Component/Plugin/Attribute/AttributeInterface.php",
        "<?php\nnamespace Drupal\\Component\\Plugin\\Attribute;\ninterface AttributeInterface {}\n",
    ),
    (
        "web/core/lib/Drupal/Component/Plugin/Attribute/Plugin.php",
        "<?php\nnamespace Drupal\\Component\\Plugin\\Attribute;\n#[\\Attribute(\\Attribute::TARGET_CLASS)]\nclass Plugin implements AttributeInterface {\n    public function __construct(public readonly string $id) {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Plugin/DefaultPluginManager.php",
        "<?php\nnamespace Drupal\\Core\\Plugin;\nclass DefaultPluginManager {\n    public function __construct($subdir, \\Traversable $namespaces, $module_handler, $plugin_interface = NULL, ?string $plugin_definition_attribute_name = NULL, $plugin_definition_annotation_name = NULL) {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Field/Attribute/FieldType.php",
        "<?php\nnamespace Drupal\\Core\\Field\\Attribute;\nuse Drupal\\Component\\Plugin\\Attribute\\Plugin;\n#[\\Attribute(\\Attribute::TARGET_CLASS)]\nclass FieldType extends Plugin {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Field/FieldTypePluginManager.php",
        "<?php\nnamespace Drupal\\Core\\Field;\nuse Drupal\\Core\\Field\\Attribute\\FieldType;\nuse Drupal\\Core\\Plugin\\DefaultPluginManager;\nclass FieldTypePluginManager extends DefaultPluginManager {\n    public function __construct(\\Traversable $namespaces, $module_handler) {\n        parent::__construct('Plugin/Field/FieldType', $namespaces, $module_handler, FieldItemInterface::class, FieldType::class, 'Drupal\\Core\\Field\\Annotation\\FieldType');\n    }\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Field/Plugin/Field/FieldType/IntegerItem.php",
        "<?php\nnamespace Drupal\\Core\\Field\\Plugin\\Field\\FieldType;\nuse Drupal\\Core\\Field\\Attribute\\FieldType;\n#[FieldType(id: 'integer')]\nclass IntegerItem {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Field/Plugin/Field/FieldType/UuidItem.php",
        "<?php\nnamespace Drupal\\Core\\Field\\Plugin\\Field\\FieldType;\nuse Drupal\\Core\\Field\\Attribute\\FieldType;\n#[FieldType(id: 'uuid')]\nclass UuidItem {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Field/Plugin/Field/FieldType/LanguageItem.php",
        "<?php\nnamespace Drupal\\Core\\Field\\Plugin\\Field\\FieldType;\nuse Drupal\\Core\\Field\\Attribute\\FieldType;\n#[FieldType(id: 'language')]\nclass LanguageItem {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Field/Plugin/Field/FieldType/StringItem.php",
        "<?php\nnamespace Drupal\\Core\\Field\\Plugin\\Field\\FieldType;\nuse Drupal\\Core\\Field\\Attribute\\FieldType;\n#[FieldType(id: 'string')]\nclass StringItem {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Field/Plugin/Field/FieldType/BooleanItem.php",
        "<?php\nnamespace Drupal\\Core\\Field\\Plugin\\Field\\FieldType;\nuse Drupal\\Core\\Field\\Attribute\\FieldType;\n#[FieldType(id: 'boolean')]\nclass BooleanItem {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Field/Plugin/Field/FieldType/EntityReferenceItem.php",
        "<?php\nnamespace Drupal\\Core\\Field\\Plugin\\Field\\FieldType;\nuse Drupal\\Core\\Field\\Attribute\\FieldType;\nuse Drupal\\Core\\Field\\EntityReferenceFieldItemList;\n#[FieldType(id: 'entity_reference', list_class: EntityReferenceFieldItemList::class)]\nclass EntityReferenceItem {}\n",
    ),
    (
        "web/core/modules/node/src/NodeInterface.php",
        "<?php\nnamespace Drupal\\node;\ninterface NodeInterface extends \\Drupal\\Core\\Entity\\ContentEntityInterface {}\n",
    ),
    (NODE_PATH, NODE),
    (
        "web/core/modules/user/src/UserInterface.php",
        "<?php\nnamespace Drupal\\user;\ninterface UserInterface extends \\Drupal\\Core\\Entity\\ContentEntityInterface {}\n",
    ),
    (
        "web/core/modules/user/src/Entity/User.php",
        r#"<?php
namespace Drupal\user\Entity;

use Drupal\Core\Entity\ContentEntityBase;
use Drupal\Core\Entity\EntityTypeInterface;
use Drupal\Core\Field\BaseFieldDefinition;
use Drupal\user\UserInterface;

/**
 * @ContentEntityType(
 *   id = "user",
 *   label = @Translation("User"),
 *   entity_keys = {
 *     "id" = "uid",
 *     "langcode" = "langcode",
 *     "uuid" = "uuid"
 *   },
 *   translatable = TRUE,
 * )
 */
class User extends ContentEntityBase implements UserInterface {
    public static function baseFieldDefinitions(EntityTypeInterface $entity_type) {
        $fields = parent::baseFieldDefinitions($entity_type);
        $fields['name'] = BaseFieldDefinition::create('string');
        return $fields;
    }
}
"#,
    ),
];

const NODE_PATH: &str = "web/core/modules/node/src/Entity/Node.php";
const NODE: &str = r#"<?php
namespace Drupal\node\Entity;

use Drupal\Core\Entity\Attribute\ContentEntityType;
use Drupal\Core\Entity\ContentEntityBase;
use Drupal\Core\Entity\EntityTypeInterface;
use Drupal\Core\Field\BaseFieldDefinition;
use Drupal\node\NodeInterface;
use Drupal\user\EntityOwnerTrait;

#[ContentEntityType(
  id: 'node',
  entity_keys: [
    'id' => 'nid',
    'revision' => 'vid',
    'langcode' => 'langcode',
    'uuid' => 'uuid',
    'owner' => 'uid',
  ],
  translatable: TRUE,
)]
class Node extends ContentEntityBase implements NodeInterface {
    use EntityOwnerTrait;

    public static function baseFieldDefinitions(EntityTypeInterface $entity_type) {
        $fields = parent::baseFieldDefinitions($entity_type);
        $fields += static::ownerBaseFieldDefinitions($entity_type);
        $fields['title'] = BaseFieldDefinition::create('string')
            ->setLabel('Title');
        $fields['uid']->setLabel('Authored by');
        return $fields;
    }
}
"#;

/// A custom entity type, neither translatable nor revisionable, with an
/// interface of its own.
const NOTE_PATH: &str = "web/modules/custom/acme/src/Entity/Note.php";
const NOTE: &str = r#"<?php
namespace Drupal\acme\Entity;

use Drupal\Core\Entity\Attribute\ContentEntityType;
use Drupal\Core\Entity\ContentEntityBase;
use Drupal\Core\Entity\EntityTypeInterface;
use Drupal\Core\Field\BaseFieldDefinition;
use Drupal\user\EntityOwnerTrait;

#[ContentEntityType(
  id: 'acme_note',
  entity_keys: ['id' => 'id', 'uuid' => 'uuid', 'owner' => 'author'],
)]
class Note extends ContentEntityBase implements NoteInterface {
    use EntityOwnerTrait;

    public static function baseFieldDefinitions(EntityTypeInterface $entity_type) {
        $fields = parent::baseFieldDefinitions($entity_type);
        $fields += static::ownerBaseFieldDefinitions($entity_type);
        $fields['body'] = BaseFieldDefinition::create('string');
        $fields['excerpt'] = BaseFieldDefinition::create('acme_excerpt');
        return $fields;
    }
}
"#;
const NOTE_INTERFACE_PATH: &str = "web/modules/custom/acme/src/Entity/NoteInterface.php";
const NOTE_INTERFACE: &str = "<?php\nnamespace Drupal\\acme\\Entity;\ninterface NoteInterface extends \\Drupal\\Core\\Entity\\ContentEntityInterface {}\n";

/// A custom field type, which names no item list class.
const EXCERPT_ITEM_PATH: &str =
    "web/modules/custom/acme/src/Plugin/Field/FieldType/ExcerptItem.php";
const EXCERPT_ITEM: &str = "<?php\nnamespace Drupal\\acme\\Plugin\\Field\\FieldType;\nuse Drupal\\Core\\Field\\Attribute\\FieldType;\n#[FieldType(id: 'acme_excerpt')]\nclass ExcerptItem {}\n";

const PROBE_PATH: &str = "web/modules/custom/acme/src/Probe.php";

/// A Drupal site with [`CORE`] on disk and the custom entity type, `files`
/// open, indexed.
async fn site(files: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in CORE.iter().chain(&[
        (".git/HEAD", "ref: refs/heads/main\n"),
        (".gitignore", "/web/core/\n"),
        (NOTE_PATH, NOTE),
        (NOTE_INTERFACE_PATH, NOTE_INTERFACE),
        (EXCERPT_ITEM_PATH, EXCERPT_ITEM),
    ]) {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    let (backend, dir) = crate::drupal_services::workspace_in(dir, files).await;
    backend.ensure_workspace_indexed();
    (backend, dir)
}

/// The members completion offers after `$receiver->` on a receiver of
/// `class`, written in a file of its own.
async fn members_of(backend: &Backend, dir: &tempfile::TempDir, class: &str) -> Vec<String> {
    let probe = format!(
        "<?php\nnamespace Drupal\\acme;\nfunction probe(\\{class} $receiver): void {{\n    $receiver->\n}}\n"
    );
    let uri = uri_for(dir, PROBE_PATH);
    open_document(backend, &uri, "php", &probe).await;
    let position = position_in(&probe, "$receiver->\n", "$receiver->".len());
    crate::common::response_labels(
        crate::common::completion_response(backend, &uri, position.line, position.character, None)
            .await,
    )
}

fn offers(labels: &[String], name: &str) -> bool {
    labels.iter().any(|label| label == name)
}

/// Node has its own fields, `ContentEntityBase`'s key fields under the
/// names node gives the keys, and the owner trait's field; a key node does
/// not declare drops its field.
#[tokio::test]
async fn an_entity_type_has_its_own_inherited_and_trait_fields() {
    let (backend, dir) = site(&[]).await;
    for receiver in ["Drupal\\node\\NodeInterface", "Drupal\\node\\Entity\\Node"] {
        let labels = members_of(&backend, &dir, receiver).await;
        for field in ["nid", "uuid", "vid", "langcode", "uid", "title"] {
            assert!(
                offers(&labels, field),
                "{receiver} lacks {field}: {labels:?}"
            );
        }
        for not_field in ["id", "owner", "bundle", "name", "body"] {
            assert!(!offers(&labels, not_field), "{receiver} has {not_field}");
        }
    }
}

/// The annotation-declared `user` and the custom `acme_note` have their own.
#[tokio::test]
async fn annotated_and_custom_entity_types_have_their_fields() {
    let (backend, dir) = site(&[]).await;
    let user = members_of(&backend, &dir, "Drupal\\user\\UserInterface").await;
    for field in ["uid", "uuid", "langcode", "name"] {
        assert!(offers(&user, field), "user lacks {field}: {user:?}");
    }
    assert!(!offers(&user, "title"), "{user:?}");
    let note = members_of(&backend, &dir, "Drupal\\acme\\Entity\\NoteInterface").await;
    for field in ["id", "uuid", "author", "body"] {
        assert!(offers(&note, field), "acme_note lacks {field}: {note:?}");
    }
    assert!(!offers(&note, "langcode"), "{note:?}");
}

/// The entity field manager's own fields: all three on a translatable,
/// revisionable type, `default_langcode` alone on a translatable one, none
/// on the rest.
#[tokio::test]
async fn the_field_managers_fields_follow_its_conditions() {
    let (backend, dir) = site(&[]).await;
    let manager_fields = [
        "default_langcode",
        "revision_default",
        "revision_translation_affected",
    ];
    let present = |labels: &[String]| -> Vec<&str> {
        manager_fields
            .iter()
            .copied()
            .filter(|field| offers(labels, field))
            .collect()
    };
    let node = members_of(&backend, &dir, "Drupal\\node\\NodeInterface").await;
    assert_eq!(present(&node), manager_fields);
    let user = members_of(&backend, &dir, "Drupal\\user\\UserInterface").await;
    assert_eq!(present(&user), ["default_langcode"]);
    let note = members_of(&backend, &dir, "Drupal\\acme\\Entity\\NoteInterface").await;
    assert!(present(&note).is_empty(), "{note:?}");
}

/// An interface several entity classes implement has the fields all of
/// them have.
#[tokio::test]
async fn a_shared_interface_has_the_fields_every_entity_type_has() {
    let (backend, dir) = site(&[]).await;
    let labels = members_of(
        &backend,
        &dir,
        "Drupal\\Core\\Entity\\ContentEntityInterface",
    )
    .await;
    assert!(offers(&labels, "uuid"), "{labels:?}");
    for not_shared in [
        "nid",
        "title",
        "name",
        "body",
        "langcode",
        "default_langcode",
    ] {
        assert!(!offers(&labels, not_shared), "{not_shared}: {labels:?}");
    }
}

const TYPED: &str = r#"<?php
namespace Drupal\acme;

use Drupal\node\Entity\Node;
use Drupal\node\NodeInterface;

final class Typed {
    public function read(NodeInterface $node, Node $entity): void {
        $title = $node->title;
        $owner = $node->uid;
        $flag = $node->default_langcode;
        $class_title = $entity->title;
        $class_owner = $entity->uid;
    }
}
"#;
const TYPED_PATH: &str = "web/modules/custom/acme/src/Typed.php";

/// The type the hover on the variable `var`, assigned in `content`, gives.
fn hovered_type(backend: &Backend, uri: &str, content: &str, var: &str) -> String {
    let (line, text) = content
        .lines()
        .enumerate()
        .find(|(_, line)| line.trim_start().starts_with(&format!("{var} = ")))
        .expect("the variable should be assigned");
    let character = (text.len() - text.trim_start().len() + 1) as u32;
    let hover = hover_at(backend, uri, content, line as u32, character)
        .unwrap_or_else(|| panic!("no hover on {var}"));
    hover_text(&hover).to_string()
}

/// A field is its field type's item list, the item class its type
/// argument; a reference field's target class follows.
#[tokio::test]
async fn a_field_is_typed_by_its_field_types_item_list() {
    let (backend, dir) = site(&[(TYPED_PATH, TYPED)]).await;
    let uri = uri_for(&dir, TYPED_PATH);
    let hover = |var: &str| hovered_type(&backend, uri.as_str(), TYPED, var);
    for var in ["$title", "$class_title"] {
        let text = hover(var);
        assert!(text.contains("FieldItemList<StringItem>"), "{var}: {text}");
    }
    for var in ["$owner", "$class_owner"] {
        let text = hover(var);
        assert!(
            text.contains("EntityReferenceFieldItemList<EntityReferenceItem, User>"),
            "{var}: {text}"
        );
    }
    let text = hover("$flag");
    assert!(text.contains("FieldItemList<BooleanItem>"), "{text}");
}

const NAVIGATED: &str = r#"<?php
namespace Drupal\acme;

use Drupal\acme\Entity\NoteInterface;
use Drupal\node\NodeInterface;

final class Navigated {
    public function read(NodeInterface $node, NoteInterface $note): void {
        $node->title;
        $node->nid;
        $node->uid;
        $note->body;
    }
}
"#;
const NAVIGATED_PATH: &str = "web/modules/custom/acme/src/Navigated.php";

/// Go-to-definition on a field goes to the name its declaration creates
/// it under: in the entity class, in `ContentEntityBase` for a key field,
/// in the trait for the owner field, and in a custom entity class.
#[tokio::test]
async fn a_field_goes_to_where_it_is_declared() {
    let (backend, dir) = site(&[(NAVIGATED_PATH, NAVIGATED)]).await;
    let uri = uri_for(&dir, NAVIGATED_PATH);
    // Each access, the file it goes to, and the text the declared name
    // starts in, with how far into that text the name starts.
    for (access, path, declared, inside) in [
        ("$node->title", NODE_PATH, "$fields['title']", 9),
        (
            "$node->nid",
            "web/core/lib/Drupal/Core/Entity/ContentEntityBase.php",
            "$entity_type->getKey('id')",
            0,
        ),
        (
            "$node->uid",
            "web/core/modules/user/src/EntityOwnerTrait.php",
            "$entity_type->getKey('owner')",
            0,
        ),
        ("$note->body", NOTE_PATH, "$fields['body']", 9),
    ] {
        let found = definitions_at(
            &backend,
            &uri,
            position_in(NAVIGATED, access, access.len() - 1),
        )
        .await;
        assert_eq!(found.len(), 1, "{access}: {found:?}");
        assert_eq!(found[0].uri, uri_for(&dir, path), "{access}");
        let content = CORE
            .iter()
            .chain(&[(NOTE_PATH, NOTE)])
            .find(|(file, _)| *file == path)
            .map(|(_, content)| *content)
            .unwrap();
        assert_eq!(
            found[0].range.start,
            position_in(content, declared, inside),
            "{access}"
        );
    }
}

const WRITTEN: &str = r#"<?php
namespace Drupal\acme;

use Drupal\node\Entity\Node;
use Drupal\node\NodeInterface;

final class Written {
    public function write(NodeInterface $node, Node $entity): void {
        $node->title = 'Example';
        $node->title->value;
        $entity->title = 'Example';
        $entity->title->value;
    }
}
"#;
const WRITTEN_PATH: &str = "web/modules/custom/acme/src/Written.php";

/// Writing a field stores the value into it: a later read is still the
/// field, on the class and on its interface.
#[tokio::test]
async fn writing_a_field_keeps_its_type() {
    let (backend, dir) = site(&[(WRITTEN_PATH, WRITTEN)]).await;
    let uri = uri_for(&dir, WRITTEN_PATH);
    let messages = crate::common::slow_diagnostic_messages(
        &backend,
        uri.as_str(),
        WRITTEN,
        "scalar_member_access",
    );
    assert!(messages.is_empty(), "{messages:?}");
}

/// Editing a custom entity's `baseFieldDefinitions()` refreshes its fields
/// on the class and on its interface, whose resolved members do not depend
/// on the class.
#[tokio::test]
async fn editing_base_fields_refreshes_the_class_and_its_interfaces() {
    let (backend, dir) = site(&[(NOTE_PATH, NOTE)]).await;
    let before_interface = members_of(&backend, &dir, "Drupal\\acme\\Entity\\NoteInterface").await;
    let before_class = members_of(&backend, &dir, "Drupal\\acme\\Entity\\Note").await;
    for labels in [&before_interface, &before_class] {
        assert!(offers(labels, "body"), "{labels:?}");
        assert!(!offers(labels, "summary"), "{labels:?}");
    }

    let edited = NOTE.replace("$fields['body']", "$fields['summary']");
    edit(&backend, &uri_for(&dir, NOTE_PATH), &edited).await;

    let after_interface = members_of(&backend, &dir, "Drupal\\acme\\Entity\\NoteInterface").await;
    let after_class = members_of(&backend, &dir, "Drupal\\acme\\Entity\\Note").await;
    for labels in [&after_interface, &after_class] {
        assert!(offers(labels, "summary"), "{labels:?}");
        assert!(!offers(labels, "body"), "{labels:?}");
    }
}

/// Replace the whole text of the open document `uri`.
async fn edit(backend: &Backend, uri: &tower_lsp_server::gen_lsp_types::Uri, text: &str) {
    use tower_lsp_server::LanguageServer;
    use tower_lsp_server::gen_lsp_types::*;
    backend
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                text_document_identifier: TextDocumentIdentifier { uri: uri.clone() },
                version: 2,
            },
            content_changes: vec![
                TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                    TextDocumentContentChangeWholeDocument {
                        text: text.to_string(),
                    },
                ),
            ],
        })
        .await;
}

const EXCERPTED: &str = r#"<?php
namespace Drupal\acme;

use Drupal\acme\Entity\NoteInterface;

final class Excerpted {
    public function read(NoteInterface $note): void {
        $excerpt = $note->excerpt;
    }
}
"#;
const EXCERPTED_PATH: &str = "web/modules/custom/acme/src/Excerpted.php";

/// Giving a field type an item list class retypes the fields of that type.
#[tokio::test]
async fn editing_a_field_type_retypes_its_fields() {
    let (backend, dir) = site(&[
        (EXCERPTED_PATH, EXCERPTED),
        (EXCERPT_ITEM_PATH, EXCERPT_ITEM),
    ])
    .await;
    let uri = uri_for(&dir, EXCERPTED_PATH);
    let before = hovered_type(&backend, uri.as_str(), EXCERPTED, "$excerpt");
    assert!(before.contains("FieldItemList<ExcerptItem>"), "{before}");

    let listed = EXCERPT_ITEM.replace(
        "#[FieldType(id: 'acme_excerpt')]",
        "#[FieldType(id: 'acme_excerpt', list_class: ExcerptList::class)]",
    );
    edit(&backend, &uri_for(&dir, EXCERPT_ITEM_PATH), &listed).await;

    let after = hovered_type(&backend, uri.as_str(), EXCERPTED, "$excerpt");
    assert!(after.contains("ExcerptList<ExcerptItem>"), "{after}");
}

/// The field type manager read after the fields types them once it is.
#[tokio::test]
async fn a_field_type_manager_read_late_types_the_fields() {
    const MANAGER_PATH: &str = "web/core/lib/Drupal/Core/Field/FieldTypePluginManager.php";
    let manager = CORE
        .iter()
        .find(|(path, _)| *path == MANAGER_PATH)
        .map(|(_, content)| *content)
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in CORE
        .iter()
        .filter(|(path, _)| *path != MANAGER_PATH)
        .chain(&[
            (".git/HEAD", "ref: refs/heads/main\n"),
            (".gitignore", "/web/core/\n"),
            (NOTE_PATH, NOTE),
            (NOTE_INTERFACE_PATH, NOTE_INTERFACE),
        ])
    {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    let (backend, dir) = crate::drupal_services::workspace_in(dir, &[(TYPED_PATH, TYPED)]).await;
    backend.ensure_workspace_indexed();
    let uri = uri_for(&dir, TYPED_PATH);
    let before = hovered_type(&backend, uri.as_str(), TYPED, "$title");
    assert!(!before.contains("StringItem"), "{before}");

    open_document(&backend, &uri_for(&dir, MANAGER_PATH), "php", manager).await;

    let after = hovered_type(&backend, uri.as_str(), TYPED, "$title");
    assert!(after.contains("FieldItemList<StringItem>"), "{after}");
}

/// Lines added above a field's declaration move where go-to-definition
/// lands, though the fields stay the same.
#[tokio::test]
async fn a_field_goes_to_its_declaration_after_lines_above_it_move() {
    let (backend, dir) = site(&[(NAVIGATED_PATH, NAVIGATED), (NOTE_PATH, NOTE)]).await;
    let uri = uri_for(&dir, NAVIGATED_PATH);
    let body = position_in(NAVIGATED, "$note->body", "$note->b".len());
    let before = definitions_at(&backend, &uri, body).await;
    assert_eq!(
        before.first().map(|location| location.range.start),
        Some(position_in(NOTE, "$fields['body']", 9))
    );

    let moved = NOTE.replace("class Note", "// A note.\n// Kept short.\nclass Note");
    edit(&backend, &uri_for(&dir, NOTE_PATH), &moved).await;

    let after = definitions_at(&backend, &uri, body).await;
    assert_eq!(
        after.first().map(|location| location.range.start),
        Some(position_in(&moved, "$fields['body']", 9))
    );
}

/// A custom entity's file the editor restores while the workspace is read
/// gives its fields once it is.
#[tokio::test]
async fn an_entity_file_opened_before_indexing_ends_gives_its_fields() {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in CORE.iter().chain(&[
        (".git/HEAD", "ref: refs/heads/main\n"),
        (".gitignore", "/web/core/\n"),
        (NOTE_INTERFACE_PATH, NOTE_INTERFACE),
        (EXCERPT_ITEM_PATH, EXCERPT_ITEM),
    ]) {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    let (backend, dir) = workspace_opened_early(dir, &[(NOTE_PATH, NOTE)]).await;
    backend.ensure_workspace_indexed();
    let labels = members_of(&backend, &dir, "Drupal\\acme\\Entity\\NoteInterface").await;
    for field in ["body", "author", "uuid"] {
        assert!(offers(&labels, field), "{field}: {labels:?}");
    }
}
