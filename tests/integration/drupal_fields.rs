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
        r#"<?php
namespace Drupal\Core\Entity;
interface FieldableEntityInterface extends EntityInterface {
    public function hasField($field_name);
    /** @return \Drupal\Core\Field\FieldDefinitionInterface|null */
    public function getFieldDefinition($name);
    /** @return \Drupal\Core\Field\FieldItemListInterface */
    public function get($field_name);
    /** @return $this */
    public function set($field_name, $value, $notify = TRUE);
}
"#,
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
        if ($entity_type->hasKey('bundle')) {
            if ($bundle_entity_type_id = $entity_type->getBundleEntityType()) {
                $fields[$entity_type->getKey('bundle')] = BaseFieldDefinition::create('entity_reference')
                    ->setSetting('target_type', $bundle_entity_type_id);
            }
            else {
                $fields[$entity_type->getKey('bundle')] = BaseFieldDefinition::create('string');
            }
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
        r#"<?php
namespace Drupal\Core\Field;
/**
 * @template T of FieldItemInterface
 * @property mixed $value
 */
interface FieldItemListInterface {
    /**
     * @return \Drupal\Core\TypedData\TypedDataInterface|null
     * @phpstan-return ?T
     */
    public function first();
    public function __get($property_name);
    public function __set($property_name, $value);
}
"#,
    ),
    (
        "web/core/lib/Drupal/Core/Field/FieldItemList.php",
        r#"<?php
namespace Drupal\Core\Field;
/**
 * @template T of FieldItemInterface
 * @implements FieldItemListInterface<T>
 * @implements \IteratorAggregate<int, T>
 */
class FieldItemList implements \IteratorAggregate, FieldItemListInterface {
    protected $langcode;
    /** {@inheritdoc} */
    public function first() {}
    public function getIterator(): \ArrayIterator {}
    public function __get($property_name) {}
    public function __set($property_name, $value) {}
}
"#,
    ),
    (
        "web/core/lib/Drupal/Core/Field/EntityReferenceFieldItemList.php",
        r#"<?php
namespace Drupal\Core\Field;
class EntityReferenceFieldItemList extends FieldItemList implements EntityReferenceFieldItemListInterface {
    /**
     * @return \Drupal\Core\Entity\EntityInterface[]
     */
    public function referencedEntities() {}
}
"#,
    ),
    (
        "web/core/lib/Drupal/Core/Field/EntityReferenceFieldItemListInterface.php",
        "<?php\nnamespace Drupal\\Core\\Field;\ninterface EntityReferenceFieldItemListInterface extends FieldItemListInterface {\n    /** @return \\Drupal\\Core\\Entity\\EntityInterface[] */\n    public function referencedEntities();\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Field/FieldItemInterface.php",
        "<?php\nnamespace Drupal\\Core\\Field;\ninterface FieldItemInterface {\n    /** @return mixed */\n    public function __get($property_name);\n    public function __set($property_name, $value);\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Field/FieldItemBase.php",
        "<?php\nnamespace Drupal\\Core\\Field;\nabstract class FieldItemBase implements FieldItemInterface {\n    public function __get($name) {}\n    public function __set($name, $value) {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Language/LanguageInterface.php",
        "<?php\nnamespace Drupal\\Core\\Language;\ninterface LanguageInterface {\n    public function getId();\n}\n",
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
        r#"<?php
namespace Drupal\Core\Field\Plugin\Field\FieldType;
use Drupal\Core\Field\Attribute\FieldType;
use Drupal\Core\Field\FieldItemBase;
use Drupal\Core\TypedData\DataDefinition;
use Drupal\Core\TypedData\DataReferenceDefinition;
#[FieldType(id: 'language')]
class LanguageItem extends FieldItemBase {
    public static function propertyDefinitions($field_definition) {
        $properties['value'] = DataDefinition::create('string');
        $properties['language'] = DataReferenceDefinition::create('language')
            ->setComputed(TRUE);
        return $properties;
    }
}
"#,
    ),
    (
        "web/core/lib/Drupal/Core/Field/Plugin/Field/FieldType/StringItem.php",
        r#"<?php
namespace Drupal\Core\Field\Plugin\Field\FieldType;
use Drupal\Core\Field\Attribute\FieldType;
use Drupal\Core\Field\FieldItemBase;
use Drupal\Core\TypedData\DataDefinition;
#[FieldType(id: 'string')]
class StringItem extends FieldItemBase {
    public static function propertyDefinitions($field_definition) {
        $properties['value'] = DataDefinition::create('string');
        return $properties;
    }
}
"#,
    ),
    (
        "web/core/lib/Drupal/Core/Field/Plugin/Field/FieldType/BooleanItem.php",
        "<?php\nnamespace Drupal\\Core\\Field\\Plugin\\Field\\FieldType;\nuse Drupal\\Core\\Field\\Attribute\\FieldType;\n#[FieldType(id: 'boolean')]\nclass BooleanItem {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Field/Plugin/Field/FieldType/EntityReferenceItem.php",
        r#"<?php
namespace Drupal\Core\Field\Plugin\Field\FieldType;
use Drupal\Core\Field\Attribute\FieldType;
use Drupal\Core\Field\EntityReferenceFieldItemList;
use Drupal\Core\Field\FieldItemBase;
use Drupal\Core\TypedData\DataReferenceDefinition;
use Drupal\Core\TypedData\DataReferenceTargetDefinition;
#[FieldType(id: 'entity_reference', list_class: EntityReferenceFieldItemList::class)]
class EntityReferenceItem extends FieldItemBase {
    public static function defaultStorageSettings() {
        return [
            'target_type' => \Drupal::moduleHandler()->moduleExists('node') ? 'node' : 'user',
        ] + parent::defaultStorageSettings();
    }
    public static function propertyDefinitions($field_definition) {
        $target_id_definition = DataReferenceTargetDefinition::create('integer');
        $properties['target_id'] = $target_id_definition;
        $properties['entity'] = DataReferenceDefinition::create('entity')
            ->setComputed(TRUE)
            ->setReadOnly(FALSE);
        return $properties;
    }
}
"#,
    ),
    (
        "web/core/modules/node/src/NodeInterface.php",
        "<?php\nnamespace Drupal\\node;\ninterface NodeInterface extends \\Drupal\\Core\\Entity\\ContentEntityInterface {}\n",
    ),
    (NODE_PATH, NODE),
    (
        "web/core/modules/node/src/Entity/NodeType.php",
        "<?php\nnamespace Drupal\\node\\Entity;\nuse Drupal\\Core\\Entity\\Attribute\\ConfigEntityType;\n#[ConfigEntityType(id: 'node_type')]\nclass NodeType {}\n",
    ),
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
  bundle_entity_type: 'node_type',
  entity_keys: [
    'id' => 'nid',
    'revision' => 'vid',
    'bundle' => 'type',
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
    site_in(tempfile::tempdir().unwrap(), files).await
}

/// [`site`] in `dir`, which may hold files already.
async fn site_in(dir: tempfile::TempDir, files: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
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

/// A custom entity type whose base field method is a trait's, which takes
/// fields from its parent and from another trait of the class.
const CARD: &[(&str, &str)] = &[
    (
        "web/modules/custom/acme/src/Entity/StampedTrait.php",
        r#"<?php
namespace Drupal\acme\Entity;

use Drupal\Core\Entity\EntityTypeInterface;
use Drupal\Core\Field\BaseFieldDefinition;

trait StampedTrait {
    public static function baseFieldDefinitions(EntityTypeInterface $entity_type) {
        $fields = parent::baseFieldDefinitions($entity_type);
        $fields += self::stampBaseFieldDefinitions($entity_type);
        $fields['label'] = BaseFieldDefinition::create('string');
        return $fields;
    }
}
"#,
    ),
    (
        "web/modules/custom/acme/src/Entity/StampTrait.php",
        r#"<?php
namespace Drupal\acme\Entity;

use Drupal\Core\Entity\EntityTypeInterface;
use Drupal\Core\Field\BaseFieldDefinition;

trait StampTrait {
    public static function stampBaseFieldDefinitions(EntityTypeInterface $entity_type) {
        $fields['stamp'] = BaseFieldDefinition::create('integer');
        return $fields;
    }
}
"#,
    ),
    (
        "web/modules/custom/acme/src/Entity/Card.php",
        r#"<?php
namespace Drupal\acme\Entity;

use Drupal\Core\Entity\Attribute\ContentEntityType;
use Drupal\Core\Entity\ContentEntityBase;

#[ContentEntityType(id: 'acme_card', entity_keys: ['id' => 'id', 'uuid' => 'uuid'])]
class Card extends ContentEntityBase {
    use StampedTrait;
    use StampTrait;
}
"#,
    ),
];

/// `parent::` and `self::` in a trait's base field method are the using
/// class's, as PHP binds them.
#[tokio::test]
async fn a_trait_s_base_field_method_follows_the_class_that_uses_it() {
    let (backend, dir) = site_with(CARD, &[]).await;
    let labels = members_of(&backend, &dir, "Drupal\\acme\\Entity\\Card").await;
    for field in ["id", "uuid", "stamp", "label"] {
        assert!(offers(&labels, field), "{field}: {labels:?}");
    }
}

/// A field a base field method unsets is gone, though the method it takes
/// it from creates it.
#[tokio::test]
async fn a_field_a_base_field_method_unsets_is_gone() {
    let unset = NOTE.replace(
        "$fields['body']",
        "unset($fields['uuid']);\n        $fields['body']",
    );
    let (backend, dir) = site(&[(NOTE_PATH, &unset)]).await;
    for receiver in [
        "Drupal\\acme\\Entity\\NoteInterface",
        "Drupal\\acme\\Entity\\Note",
    ] {
        let labels = members_of(&backend, &dir, receiver).await;
        assert!(offers(&labels, "body"), "{receiver}: {labels:?}");
        assert!(!offers(&labels, "uuid"), "{receiver}: {labels:?}");
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

/// A base class between the entity class and `ContentEntityBase` that
/// adds a field, and a trait that gives one.
const EDITORIAL: &[(&str, &str)] = &[
    (
        "web/core/lib/Drupal/Core/Entity/EditorialContentEntityBase.php",
        "<?php\nnamespace Drupal\\Core\\Entity;\nuse Drupal\\Core\\Field\\BaseFieldDefinition;\nabstract class EditorialContentEntityBase extends ContentEntityBase {\n    public static function baseFieldDefinitions(EntityTypeInterface $entity_type) {\n        $fields = parent::baseFieldDefinitions($entity_type);\n        $fields['status'] = BaseFieldDefinition::create('boolean');\n        return $fields;\n    }\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Entity/EntityPublishedTrait.php",
        "<?php\nnamespace Drupal\\Core\\Entity;\nuse Drupal\\Core\\Field\\BaseFieldDefinition;\ntrait EntityPublishedTrait {\n    public static function publishedBaseFieldDefinitions(EntityTypeInterface $entity_type) {\n        return ['published' => BaseFieldDefinition::create('boolean')];\n    }\n}\n",
    ),
];

/// Changing the parent, the traits, or the interfaces of an entity class
/// refreshes its fields, on the class and on its interface, though its base
/// field methods stay the same.
#[tokio::test]
async fn editing_an_entity_class_s_parent_traits_or_interfaces_refreshes_its_fields() {
    let imported = NOTE.replace(
        "use Drupal\\user\\EntityOwnerTrait;",
        "use Drupal\\user\\EntityOwnerTrait;\nuse Drupal\\Core\\Entity\\EditorialContentEntityBase;\nuse Drupal\\Core\\Entity\\EntityPublishedTrait;",
    );
    let publishing = imported.replace(
        "$fields['body']",
        "$fields += static::publishedBaseFieldDefinitions($entity_type);\n        $fields['body']",
    );
    let unimplemented = imported.replace(" implements NoteInterface", "");
    // Each edit: the text before and after it, and the field it gives.
    for (before, after, field) in [
        (
            imported.clone(),
            imported.replace(
                "extends ContentEntityBase",
                "extends EditorialContentEntityBase",
            ),
            "status",
        ),
        (
            publishing.clone(),
            publishing.replace(
                "use EntityOwnerTrait;",
                "use EntityOwnerTrait;\n    use EntityPublishedTrait;",
            ),
            "published",
        ),
        (unimplemented.clone(), imported.clone(), "body"),
    ] {
        let (backend, dir) = site_with(EDITORIAL, &[(NOTE_PATH, &before)]).await;
        let receivers = [
            "Drupal\\acme\\Entity\\NoteInterface",
            "Drupal\\acme\\Entity\\Note",
        ];
        let receivers = if field == "body" {
            &receivers[..1]
        } else {
            &receivers[..]
        };
        for receiver in receivers {
            let labels = members_of(&backend, &dir, receiver).await;
            assert!(!offers(&labels, field), "{receiver} before: {labels:?}");
        }
        edit(&backend, &uri_for(&dir, NOTE_PATH), &after).await;
        for receiver in receivers {
            let labels = members_of(&backend, &dir, receiver).await;
            assert!(offers(&labels, field), "{receiver} after: {labels:?}");
        }
    }
}

/// A base class an entity class names before it exists gives its fields
/// once it is created.
#[tokio::test]
async fn a_base_class_created_after_the_fields_are_read_gives_its_fields() {
    let based = NOTE.replace(
        "extends ContentEntityBase",
        "extends \\Drupal\\acme\\Entity\\NoteBase",
    );
    let (backend, dir) = site_with(EDITORIAL, &[(NOTE_PATH, &based)]).await;
    let receivers = [
        "Drupal\\acme\\Entity\\NoteInterface",
        "Drupal\\acme\\Entity\\Note",
    ];
    for receiver in receivers {
        let labels = members_of(&backend, &dir, receiver).await;
        assert!(!offers(&labels, "status"), "{receiver} before: {labels:?}");
    }
    open_document(
        &backend,
        &uri_for(&dir, "web/modules/custom/acme/src/Entity/NoteBase.php"),
        "php",
        "<?php\nnamespace Drupal\\acme\\Entity;\nabstract class NoteBase extends \\Drupal\\Core\\Entity\\EditorialContentEntityBase {}\n",
    )
    .await;
    for receiver in receivers {
        let labels = members_of(&backend, &dir, receiver).await;
        assert!(offers(&labels, "status"), "{receiver} after: {labels:?}");
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

// ─── Configurable fields ────────────────────────────────────────────────────

/// Core's taxonomy term entity type, which reference fields point to.
const TERM: (&str, &str) = (
    "web/core/modules/taxonomy/src/Entity/Term.php",
    "<?php\nnamespace Drupal\\taxonomy\\Entity;\nuse Drupal\\Core\\Entity\\Attribute\\ContentEntityType;\nuse Drupal\\Core\\Entity\\ContentEntityBase;\n#[ContentEntityType(id: 'taxonomy_term', entity_keys: ['id' => 'tid', 'uuid' => 'uuid'])]\nclass Term extends ContentEntityBase {}\n",
);

/// A field storage file for the field `name` of `entity_type`, of the field
/// type `field_type`, pointing to `target` when given.
fn storage(entity_type: &str, name: &str, field_type: &str, target: Option<&str>) -> String {
    let settings = match target {
        Some(target) => format!("settings:\n  target_type: {target}\n"),
        None => "settings: {  }\n".to_string(),
    };
    format!(
        "uuid: 0d4c5b0e-0000-4000-8000-000000000000\nlangcode: en\nstatus: true\ndependencies:\n  module:\n    - {entity_type}\nid: {entity_type}.{name}\nfield_name: {name}\nentity_type: {entity_type}\ntype: {field_type}\n{settings}module: core\nlocked: false\ncardinality: -1\ntranslatable: true\n"
    )
}

/// A Drupal site with [`CORE`], the custom entity type, the term entity
/// type, and `disk` on disk, `open` open, indexed.
async fn site_with(disk: &[(&str, &str)], open: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in disk.iter().chain([&TERM]) {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    site_in(dir, open).await
}

/// Configurable fields come from an extension's default configuration,
/// core's included, and from the site's exported configuration beside the
/// web root, each on its own entity type only.
#[tokio::test]
async fn configurable_fields_come_from_extensions_and_exported_configuration() {
    let tags = storage(
        "node",
        "field_tags",
        "entity_reference",
        Some("taxonomy_term"),
    );
    let rating = storage("acme_note", "field_rating", "integer", None);
    let picture = storage("user", "user_picture", "string", None);
    let (backend, dir) = site_with(
        &[
            ("config/sync/field.storage.node.field_tags.yml", &tags),
            (
                "web/modules/custom/acme/config/install/field.storage.acme_note.field_rating.yml",
                &rating,
            ),
            (
                "web/core/profiles/standard/config/install/field.storage.user.user_picture.yml",
                &picture,
            ),
        ],
        &[],
    )
    .await;
    let node = members_of(&backend, &dir, "Drupal\\node\\NodeInterface").await;
    let user = members_of(&backend, &dir, "Drupal\\user\\UserInterface").await;
    let note = members_of(&backend, &dir, "Drupal\\acme\\Entity\\Note").await;
    let shared = members_of(
        &backend,
        &dir,
        "Drupal\\Core\\Entity\\ContentEntityInterface",
    )
    .await;
    for (labels, own) in [
        (&node, "field_tags"),
        (&user, "user_picture"),
        (&note, "field_rating"),
    ] {
        for field in ["field_tags", "user_picture", "field_rating"] {
            assert_eq!(offers(labels, field), field == own, "{field}: {labels:?}");
        }
    }
    for field in ["field_tags", "user_picture", "field_rating"] {
        assert!(!offers(&shared, field), "{field}: {shared:?}");
    }
}

const CONFIGURED: &str = r#"<?php
namespace Drupal\acme;

use Drupal\acme\Entity\NoteInterface;
use Drupal\node\NodeInterface;

final class Configured {
    public function read(NodeInterface $node, NoteInterface $note): void {
        $tags = $node->field_tags;
        $rating = $note->field_rating;
    }
}
"#;
const CONFIGURED_PATH: &str = "web/modules/custom/acme/src/Configured.php";
const SYNC_TAGS: &str = "config/sync/field.storage.node.field_tags.yml";
const MODULE_TAGS: &str =
    "web/modules/custom/acme/config/install/field.storage.node.field_tags.yml";

/// A configurable field is its field type's item list, and a reference
/// field names the class of the entity type its `target_type` gives.
#[tokio::test]
async fn a_configurable_field_is_typed_by_its_field_type() {
    let tags = storage(
        "node",
        "field_tags",
        "entity_reference",
        Some("taxonomy_term"),
    );
    let rating = storage("acme_note", "field_rating", "integer", None);
    let (backend, dir) = site_with(
        &[
            (SYNC_TAGS, &tags),
            (
                "config/sync/field.storage.acme_note.field_rating.yml",
                &rating,
            ),
        ],
        &[(CONFIGURED_PATH, CONFIGURED)],
    )
    .await;
    let uri = uri_for(&dir, CONFIGURED_PATH);
    let tags = hovered_type(&backend, uri.as_str(), CONFIGURED, "$tags");
    assert!(
        tags.contains("EntityReferenceFieldItemList<EntityReferenceItem, Term>"),
        "{tags}"
    );
    let rating = hovered_type(&backend, uri.as_str(), CONFIGURED, "$rating");
    assert!(rating.contains("FieldItemList<IntegerItem>"), "{rating}");
}

/// A field a module installs and the site exports is one field, offered
/// once, that goes to both files.
#[tokio::test]
async fn a_field_declared_twice_is_one_field_with_both_files() {
    let tags = storage(
        "node",
        "field_tags",
        "entity_reference",
        Some("taxonomy_term"),
    );
    let (backend, dir) = site_with(
        &[(SYNC_TAGS, &tags), (MODULE_TAGS, &tags)],
        &[(CONFIGURED_PATH, CONFIGURED)],
    )
    .await;
    let labels = members_of(&backend, &dir, "Drupal\\node\\NodeInterface").await;
    assert_eq!(
        labels.iter().filter(|label| *label == "field_tags").count(),
        1,
        "{labels:?}"
    );
    let uri = uri_for(&dir, CONFIGURED_PATH);
    let found = definitions_at(
        &backend,
        &uri,
        position_in(CONFIGURED, "$node->field_tags", "$node->field".len()),
    )
    .await;
    let mut targets: Vec<_> = found.iter().map(|location| location.uri.clone()).collect();
    targets.sort_by_key(|uri| uri.to_string());
    let mut expected = vec![uri_for(&dir, SYNC_TAGS), uri_for(&dir, MODULE_TAGS)];
    expected.sort_by_key(|uri| uri.to_string());
    assert_eq!(targets, expected);
    for location in &found {
        assert_eq!(
            location.range.start,
            position_in(&tags, "field_name: field_tags", "field_name: ".len())
        );
    }
}

/// Files under `vendor/` and in recipes are no source, and a test module's
/// counts only while it is open.
#[tokio::test]
async fn recipes_dependencies_and_closed_test_modules_declare_no_field() {
    const TEST_MODULE: &str = "web/modules/custom/acme/tests/modules/acme_test/config/install/field.storage.node.field_test.yml";
    const VENDOR: &str = "vendor/example/fields/config/install/field.storage.node.field_vendor.yml";
    let recipe = storage("node", "field_recipe", "string", None);
    let core_recipe = storage("node", "field_core_recipe", "string", None);
    let test = storage("node", "field_test", "string", None);
    let vendor = storage("node", "field_vendor", "string", None);
    let (backend, dir) = site_with(
        &[
            (
                "recipes/example/config/field.storage.node.field_recipe.yml",
                &recipe,
            ),
            (
                "web/core/recipes/example/config/field.storage.node.field_core_recipe.yml",
                &core_recipe,
            ),
            (TEST_MODULE, &test),
            (VENDOR, &vendor),
        ],
        &[(VENDOR, &vendor)],
    )
    .await;
    let labels = members_of(&backend, &dir, "Drupal\\node\\NodeInterface").await;
    for field in [
        "field_recipe",
        "field_core_recipe",
        "field_test",
        "field_vendor",
    ] {
        assert!(!offers(&labels, field), "{field}: {labels:?}");
    }

    let test_uri = uri_for(&dir, TEST_MODULE);
    open_document(&backend, &test_uri, "yaml", &test).await;
    let labels = members_of(&backend, &dir, "Drupal\\node\\NodeInterface").await;
    assert!(offers(&labels, "field_test"), "{labels:?}");

    close(&backend, &test_uri).await;
    let labels = members_of(&backend, &dir, "Drupal\\node\\NodeInterface").await;
    assert!(!offers(&labels, "field_test"), "{labels:?}");
}

async fn close(backend: &Backend, uri: &tower_lsp_server::gen_lsp_types::Uri) {
    use tower_lsp_server::LanguageServer;
    use tower_lsp_server::gen_lsp_types::*;
    backend
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
        })
        .await;
}

async fn notify(
    backend: &Backend,
    uri: &tower_lsp_server::gen_lsp_types::Uri,
    kind: tower_lsp_server::gen_lsp_types::FileChangeType,
) {
    use tower_lsp_server::LanguageServer;
    use tower_lsp_server::gen_lsp_types::*;
    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: uri.clone(),
                kind,
            }],
        })
        .await;
}

/// Adding, editing, and deleting a field storage file refreshes the fields
/// of its entity type, on the class and on its interface.
#[tokio::test]
async fn field_storage_changes_refresh_the_fields() {
    use tower_lsp_server::gen_lsp_types::FileChangeType;
    let tags = storage(
        "node",
        "field_tags",
        "entity_reference",
        Some("taxonomy_term"),
    );
    let (backend, dir) = site_with(&[(SYNC_TAGS, &tags)], &[]).await;
    let receivers = ["Drupal\\node\\NodeInterface", "Drupal\\node\\Entity\\Node"];

    let topics_path = "config/sync/field.storage.node.field_topics.yml";
    let topics = dir.path().join(topics_path);
    std::fs::write(&topics, storage("node", "field_topics", "string", None)).unwrap();
    let topics_uri = uri_for(&dir, topics_path);
    notify(&backend, &topics_uri, FileChangeType::Created).await;
    for receiver in receivers {
        let labels = members_of(&backend, &dir, receiver).await;
        assert!(offers(&labels, "field_topics"), "{receiver}: {labels:?}");
    }

    let tags_uri = uri_for(&dir, SYNC_TAGS);
    open_document(&backend, &tags_uri, "yaml", &tags).await;
    edit(
        &backend,
        &tags_uri,
        &tags.replace("field_name: field_tags", "field_name: field_labels"),
    )
    .await;
    for receiver in receivers {
        let labels = members_of(&backend, &dir, receiver).await;
        assert!(offers(&labels, "field_labels"), "{receiver}: {labels:?}");
        assert!(!offers(&labels, "field_tags"), "{receiver}: {labels:?}");
    }

    // An export or a checkout rewrites a closed file on disk.
    std::fs::write(&topics, storage("node", "field_subjects", "string", None)).unwrap();
    notify(&backend, &topics_uri, FileChangeType::Changed).await;
    for receiver in receivers {
        let labels = members_of(&backend, &dir, receiver).await;
        assert!(offers(&labels, "field_subjects"), "{receiver}: {labels:?}");
        assert!(!offers(&labels, "field_topics"), "{receiver}: {labels:?}");
    }

    std::fs::remove_file(&topics).unwrap();
    notify(&backend, &topics_uri, FileChangeType::Deleted).await;
    for receiver in receivers {
        let labels = members_of(&backend, &dir, receiver).await;
        assert!(!offers(&labels, "field_subjects"), "{receiver}: {labels:?}");
    }
}

/// An exported field storage file the editor restores while the workspace
/// is read gives its field once it is.
#[tokio::test]
async fn a_storage_opened_before_indexing_ends_gives_its_field() {
    let tags = storage(
        "node",
        "field_tags",
        "entity_reference",
        Some("taxonomy_term"),
    );
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in CORE.iter().chain(&[
        (".git/HEAD", "ref: refs/heads/main\n"),
        (".gitignore", "/web/core/\n"),
        TERM,
    ]) {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    let (backend, dir) = workspace_opened_early(dir, &[(SYNC_TAGS, &tags)]).await;
    backend.ensure_workspace_indexed();
    let labels = members_of(&backend, &dir, "Drupal\\node\\NodeInterface").await;
    assert!(offers(&labels, "field_tags"), "{labels:?}");
}

// ─── Field properties ───────────────────────────────────────────────────────

/// A custom field item that takes the reference item's properties and adds
/// two.
const ATTACHMENT_ITEM_PATH: &str =
    "web/modules/custom/acme/src/Plugin/Field/FieldType/AttachmentItem.php";
const ATTACHMENT_ITEM: &str = r#"<?php
namespace Drupal\acme\Plugin\Field\FieldType;

use Drupal\Core\Field\Plugin\Field\FieldType\EntityReferenceItem;
use Drupal\Core\TypedData\DataDefinition;

class AttachmentItem extends EntityReferenceItem {
    public static function defaultStorageSettings() {
        return ['target_type' => 'acme_note'] + parent::defaultStorageSettings();
    }
    public static function propertyDefinitions($field_definition) {
        $properties = parent::propertyDefinitions($field_definition);
        $properties['display'] = DataDefinition::create('boolean');
        $properties['description'] = DataDefinition::create('string');
        $properties['langcode'] = DataDefinition::create('string');
        return $properties;
    }
}
"#;

/// A field type whose item takes the attachment's properties, drops two,
/// and adds its own.
const PICTURE_ITEM_PATH: &str =
    "web/modules/custom/acme/src/Plugin/Field/FieldType/PictureItem.php";
const PICTURE_ITEM: &str = r#"<?php
namespace Drupal\acme\Plugin\Field\FieldType;

use Drupal\Core\Field\Attribute\FieldType;
use Drupal\Core\Field\EntityReferenceFieldItemList;
use Drupal\Core\TypedData\DataDefinition;

#[FieldType(id: 'acme_picture', list_class: EntityReferenceFieldItemList::class)]
class PictureItem extends AttachmentItem {
    public static function defaultStorageSettings() {
        return ['default_image' => ['alt' => '']] + parent::defaultStorageSettings();
    }
    public static function propertyDefinitions($field_definition) {
        $properties = parent::propertyDefinitions($field_definition);
        unset($properties['display']);
        unset($properties['description']);
        $properties['alt'] = DataDefinition::create('string');
        return $properties;
    }
}
"#;

const PICTURE: &str = "Drupal\\acme\\Plugin\\Field\\FieldType\\PictureItem";

/// A Drupal site with the custom item classes and three configurable node
/// fields: tags pointing to terms, a picture pointing to notes, and a
/// reference that names no target; `open` open, indexed.
async fn property_site(open: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let tags = storage(
        "node",
        "field_tags",
        "entity_reference",
        Some("taxonomy_term"),
    );
    // The picture field names no target: its field type's default one.
    let picture = storage("node", "field_picture", "acme_picture", None);
    let any = storage("node", "field_any", "entity_reference", None);
    site_with(
        &[
            (ATTACHMENT_ITEM_PATH, ATTACHMENT_ITEM),
            (PICTURE_ITEM_PATH, PICTURE_ITEM),
            (SYNC_TAGS, &tags),
            ("config/sync/field.storage.node.field_picture.yml", &picture),
            ("config/sync/field.storage.node.field_any.yml", &any),
        ],
        open,
    )
    .await
}

/// The members completion offers after `{subject}->` in a function that
/// takes `\Drupal\node\NodeInterface $node`.
async fn members_after(backend: &Backend, dir: &tempfile::TempDir, subject: &str) -> Vec<String> {
    members_after_on(backend, dir, "\\Drupal\\node\\NodeInterface $node", subject).await
}

/// The members completion offers after `{subject}->` in a function that
/// takes `parameter`.
async fn members_after_on(
    backend: &Backend,
    dir: &tempfile::TempDir,
    parameter: &str,
    subject: &str,
) -> Vec<String> {
    let probe = format!(
        "<?php\nnamespace Drupal\\acme;\nfunction probe({parameter}): void {{\n    {subject}->\n}}\n"
    );
    let uri = uri_for(dir, PROBE_PATH);
    open_document(backend, &uri, "php", &probe).await;
    let typed = format!("{subject}->\n");
    let position = position_in(&probe, &typed, typed.len() - 1);
    crate::common::response_labels(
        crate::common::completion_response(backend, &uri, position.line, position.character, None)
            .await,
    )
}

/// An item offers the properties its `propertyDefinitions()` declares, with
/// those it takes from its parent's and without those it unsets, and so do
/// the lists that hold it and their first item.
#[tokio::test]
async fn items_and_their_lists_offer_the_item_properties() {
    let (backend, dir) = property_site(&[]).await;
    let attachment = members_of(
        &backend,
        &dir,
        "Drupal\\acme\\Plugin\\Field\\FieldType\\AttachmentItem",
    )
    .await;
    for property in ["target_id", "entity", "display", "description"] {
        assert!(offers(&attachment, property), "{property}: {attachment:?}");
    }
    let picture = members_of(&backend, &dir, PICTURE).await;
    let list = members_after(&backend, &dir, "$node->field_picture").await;
    let first = members_after(&backend, &dir, "$node->field_picture->first()").await;
    for labels in [&picture, &list, &first] {
        for property in ["alt", "target_id", "entity"] {
            assert!(offers(labels, property), "{property}: {labels:?}");
        }
        for unset in ["display", "description"] {
            assert!(!offers(labels, unset), "{unset}: {labels:?}");
        }
    }
    let title = members_after(&backend, &dir, "$node->title").await;
    assert!(offers(&title, "value"), "{title:?}");
    assert!(!offers(&title, "entity"), "{title:?}");
}

const PROPERTIES: &str = r#"<?php
namespace Drupal\acme;

use Drupal\Core\Field\FieldItemListInterface;
use Drupal\node\NodeInterface;

final class Properties {
    public function read(NodeInterface $node, FieldItemListInterface $list): void {
        $value = $node->title->value;
        $first = $node->title->first()->value;
        $owner = $node->uid->entity;
        $owner_id = $node->uid->target_id;
        $item_owner = $node->uid->first()->entity;
        $tag = $node->field_tags->entity;
        $tags = $node->field_tags->referencedEntities();
        $any = $node->field_any->entity;
        $picture = $node->field_picture->entity;
        $alt = $node->field_picture->alt;
        $picture_langcode = $node->field_picture->langcode;
        $language = $node->langcode->language;
        $bundle = $node->type->entity;
        $bundle_list = $node->type;
        foreach ($node->title as $item) {
            $looped = $item->value;
        }
        $untyped = $list->value;
        $node->field_tags->entity->id();
        $node->langcode->language->getId();
        $list->anything;
        $written = $node->title->first();
        $written->value = 'Example';
        $after = $written->value;
    }
}
"#;
const PROPERTIES_PATH: &str = "web/modules/custom/acme/src/Properties.php";

/// A property read on a list is its first item's; a reference's `entity`
/// is its target class, or any entity when the field names none, and null
/// when the field is empty; the stored values stay `mixed`, written or not.
#[tokio::test]
async fn item_properties_are_typed_on_items_and_lists() {
    let (backend, dir) = property_site(&[(PROPERTIES_PATH, PROPERTIES)]).await;
    let uri = uri_for(&dir, PROPERTIES_PATH);
    let hover = |var: &str| hovered_type(&backend, uri.as_str(), PROPERTIES, var);
    for var in [
        "$value",
        "$first",
        "$owner_id",
        "$alt",
        "$picture_langcode",
        "$looped",
        "$untyped",
        "$after",
    ] {
        let text = hover(var);
        assert!(text.contains("mixed"), "{var}: {text}");
    }
    for (var, class) in [
        ("$owner", "User"),
        ("$item_owner", "User"),
        ("$tag", "Term"),
        ("$any", "EntityInterface"),
        ("$picture", "Note"),
        ("$language", "LanguageInterface"),
        ("$bundle", "NodeType"),
    ] {
        let text = hover(var);
        assert!(text.contains(&format!("?{class}")), "{var}: {text}");
    }
    let text = hover("$tags");
    assert!(text.contains("array<Term>"), "{text}");
    // The bundle field is a reference only, since node names a bundle
    // entity type.
    let text = hover("$bundle_list");
    assert!(
        text.contains("EntityReferenceFieldItemList<EntityReferenceItem, NodeType>")
            && !text.contains("StringItem"),
        "{text}"
    );

    let mut messages = Vec::new();
    backend.update_ast(uri.as_str(), PROPERTIES);
    backend.collect_slow_diagnostics(uri.as_str(), PROPERTIES, &mut messages);
    let messages: Vec<String> = messages
        .iter()
        .map(|diagnostic| format!("{:?}", diagnostic.message))
        .collect();
    assert!(messages.is_empty(), "{messages:?}");
}

const PAYLOAD_PATH: &str = "web/modules/custom/acme/src/Plugin/Field/FieldType/PayloadItem.php";
const PAYLOAD: &str = r#"<?php
namespace Drupal\acme\Plugin\Field\FieldType;

use Drupal\Core\Field\FieldItemBase;

/**
 * @template T
 */
class PayloadItem extends FieldItemBase {
    /** @return T */
    public function payload() {}
}
"#;
const PAYLOADS_PATH: &str = "web/modules/custom/acme/src/Payloads.php";
const PAYLOADS: &str = r#"<?php
namespace Drupal\acme;

use Drupal\acme\Entity\Note;
use Drupal\acme\Plugin\Field\FieldType\PayloadItem;
use Drupal\Core\Field\FieldItemList;

final class Payloads {
    /**
     * @param PayloadItem<Note> $item
     * @param FieldItemList<PayloadItem<Note>> $list
     */
    public function read($item, $list): void {
        $from_item = $item->payload();
        $from_list = $list->first()->payload();
    }
}
"#;

/// A field item class with templates of its own keeps the type arguments a
/// type hint gives it, alone or in a list.
#[tokio::test]
async fn a_templated_item_keeps_its_type_arguments() {
    let (backend, dir) = site(&[(PAYLOAD_PATH, PAYLOAD), (PAYLOADS_PATH, PAYLOADS)]).await;
    let uri = uri_for(&dir, PAYLOADS_PATH);
    for var in ["$from_item", "$from_list"] {
        let text = hovered_type(&backend, uri.as_str(), PAYLOADS, var);
        assert!(text.contains("Note"), "{var}: {text}");
    }
}

const RENAMED_PATH: &str = "web/modules/custom/acme/src/Renamed.php";
const RENAMED: &str = r#"<?php
namespace Drupal\acme;

use Drupal\acme\Entity\NoteInterface;
use Drupal\node\NodeInterface;

final class Renamed {
    public function read(NodeInterface $node, NoteInterface $note): void {
        $note->body;
        $node->field_tags;
        $node->field_picture->alt;
    }
}
"#;

/// A field or an item property is named in places a property rename does
/// not edit (`get()` calls, field storage files, their names), so neither
/// is offered for renaming.
#[tokio::test]
async fn fields_and_item_properties_are_not_renamed() {
    let (backend, dir) = property_site(&[(RENAMED_PATH, RENAMED)]).await;
    let uri = uri_for(&dir, RENAMED_PATH);
    for access in ["$note->body", "$node->field_tags", "->alt"] {
        let at = position_in(RENAMED, access, access.len() - 1);
        assert!(
            crate::common::prepare_rename(&backend, &uri, at.line, at.character)
                .await
                .is_none(),
            "{access}"
        );
        assert!(
            crate::common::rename(&backend, &uri, at.line, at.character, "renamed")
                .await
                .is_none(),
            "{access}"
        );
    }
}

/// Editing an item's `propertyDefinitions()` refreshes its properties on
/// the item and on the lists that hold it.
#[tokio::test]
async fn editing_property_definitions_refreshes_items_and_lists() {
    let (backend, dir) = property_site(&[(PICTURE_ITEM_PATH, PICTURE_ITEM)]).await;
    for labels in [
        members_of(&backend, &dir, PICTURE).await,
        members_after(&backend, &dir, "$node->field_picture").await,
    ] {
        assert!(offers(&labels, "alt"), "{labels:?}");
        assert!(!offers(&labels, "caption"), "{labels:?}");
    }

    let edited = PICTURE_ITEM.replace("$properties['alt']", "$properties['caption']");
    edit(&backend, &uri_for(&dir, PICTURE_ITEM_PATH), &edited).await;

    for labels in [
        members_of(&backend, &dir, PICTURE).await,
        members_after(&backend, &dir, "$node->field_picture").await,
    ] {
        assert!(offers(&labels, "caption"), "{labels:?}");
        assert!(!offers(&labels, "alt"), "{labels:?}");
    }
}

/// Outside a Drupal project an item class has only what it declares.
#[tokio::test]
async fn items_have_no_properties_outside_drupal() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("composer.json"), "{}").unwrap();
    for (path, content) in CORE.iter().chain(&[
        (ATTACHMENT_ITEM_PATH, ATTACHMENT_ITEM),
        (PICTURE_ITEM_PATH, PICTURE_ITEM),
    ]) {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    let backend = Backend::new_test();
    initialize_with_options(&backend, dir.path(), None).await;
    let labels = members_of(&backend, &dir, PICTURE).await;
    assert!(!offers(&labels, "alt"), "{labels:?}");
}

/// An item class the editor restores while the workspace is read gives its
/// properties to the fields that hold it once it is.
#[tokio::test]
async fn item_files_opened_before_indexing_ends_give_their_properties() {
    let picture = storage("node", "field_picture", "acme_picture", None);
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in CORE.iter().chain(&[
        (".git/HEAD", "ref: refs/heads/main\n"),
        (".gitignore", "/web/core/\n"),
        ("config/sync/field.storage.node.field_picture.yml", &picture),
    ]) {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    let (backend, dir) = workspace_opened_early(
        dir,
        &[
            (ATTACHMENT_ITEM_PATH, ATTACHMENT_ITEM),
            (PICTURE_ITEM_PATH, PICTURE_ITEM),
        ],
    )
    .await;
    backend.ensure_workspace_indexed();
    let labels = members_after(&backend, &dir, "$node->field_picture").await;
    assert!(offers(&labels, "alt"), "{labels:?}");
    assert!(!offers(&labels, "display"), "{labels:?}");
}

/// A custom entity type reached through two interfaces of its own.
const MEMO_PATH: &str = "web/modules/custom/acme/src/Entity/Memo.php";
const MEMO: &str = r#"<?php
namespace Drupal\acme\Entity;

use Drupal\Core\Entity\Attribute\ContentEntityType;
use Drupal\Core\Entity\ContentEntityBase;
use Drupal\Core\Entity\EntityTypeInterface;
use Drupal\Core\Field\BaseFieldDefinition;

#[ContentEntityType(
  id: 'acme_memo',
  entity_keys: ['id' => 'id', 'bundle' => 'kind'],
)]
class Memo extends ContentEntityBase implements MemoInterface {
    public static function baseFieldDefinitions(EntityTypeInterface $entity_type) {
        $fields = parent::baseFieldDefinitions($entity_type);
        $fields['subject'] = BaseFieldDefinition::create('string');
        return $fields;
    }
}
"#;

/// An interface whose fields come through an interface nothing else
/// resolves refreshes when its entity class changes, and an entity type
/// with a bundle key but no bundle entity type has a string bundle field.
#[tokio::test]
async fn editing_base_fields_refreshes_interfaces_further_down() {
    let (backend, dir) = site_with(
        &[
            (
                "web/modules/custom/acme/src/Entity/MemoBaseInterface.php",
                "<?php\nnamespace Drupal\\acme\\Entity;\ninterface MemoBaseInterface extends \\Drupal\\Core\\Entity\\ContentEntityInterface {}\n",
            ),
            (
                "web/modules/custom/acme/src/Entity/MemoInterface.php",
                "<?php\nnamespace Drupal\\acme\\Entity;\ninterface MemoInterface extends MemoBaseInterface {}\n",
            ),
            (MEMO_PATH, MEMO),
        ],
        &[(MEMO_PATH, MEMO)],
    )
    .await;
    let before = members_of(&backend, &dir, "Drupal\\acme\\Entity\\MemoInterface").await;
    assert!(offers(&before, "subject"), "{before:?}");
    let kind = members_after_on(
        &backend,
        &dir,
        "\\Drupal\\acme\\Entity\\MemoInterface $memo",
        "$memo->kind",
    )
    .await;
    assert!(offers(&kind, "value"), "{kind:?}");
    assert!(!offers(&kind, "entity"), "{kind:?}");

    let edited = MEMO.replace("$fields['subject']", "$fields['topic']");
    edit(&backend, &uri_for(&dir, MEMO_PATH), &edited).await;

    let after = members_of(&backend, &dir, "Drupal\\acme\\Entity\\MemoInterface").await;
    assert!(offers(&after, "topic"), "{after:?}");
    assert!(!offers(&after, "subject"), "{after:?}");
}

// ─── Field names in code ────────────────────────────────────────────────────

const NAMED_PATH: &str = "web/modules/custom/acme/src/Named.php";

/// The calls that take a field name, each written with the arguments after
/// the name.
const FIELD_NAME_CALLS: &[(&str, &str)] = &[
    ("get", ""),
    ("set", ", 'Example'"),
    ("hasField", ""),
    ("getFieldDefinition", ""),
];

/// Every call that takes a field name, on an interface receiver and on a
/// class receiver, for a base field and a configurable field, one per
/// line, then the fields read as properties, and the same calls on
/// receivers that are not fieldable.
fn named() -> String {
    let mut body = String::new();
    for field in ["title", "field_tags"] {
        for receiver in ["$node", "$entity"] {
            for (method, rest) in FIELD_NAME_CALLS {
                body.push_str(&format!("        {receiver}->{method}('{field}'{rest});\n"));
            }
        }
    }
    format!(
        r#"<?php
namespace Drupal\acme;

use Drupal\Core\Entity\EntityInterface;
use Drupal\node\Entity\Node;
use Drupal\node\NodeInterface;

final class Named {{
    public function read(NodeInterface $node, Node $entity, Bag $bag, Fake $fake, EntityInterface $plain, string $name): void {{
{body}        $node->title;
        $entity->field_tags;
        $bag->get('title');
        $fake->get('title');
        $plain->get('title');
        $node->get($name);
    }}
}}

final class Bag {{
    public function get($key) {{}}
}}

final class Fake implements \FieldableEntityInterface {{
    public function get($field_name) {{}}
}}
"#
    )
}

/// An interface named like core's, outside core's namespace.
const GLOBAL_FIELDABLE: (&str, &str) = (
    "web/modules/custom/acme/src/FieldableEntityInterface.php",
    "<?php\ninterface FieldableEntityInterface {\n    public function get($field_name);\n}\n",
);

fn tags_storage() -> String {
    storage(
        "node",
        "field_tags",
        "entity_reference",
        Some("taxonomy_term"),
    )
}

/// A site with node's `field_tags` exported, and `open` open.
async fn named_site(open: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let tags = tags_storage();
    site_with(&[(SYNC_TAGS, &tags), GLOBAL_FIELDABLE], open).await
}

/// The position in the string the first `call` in `content` opens,
/// `inside` characters past its quote.
fn named_site_at(
    content: &str,
    call: &str,
    inside: usize,
) -> tower_lsp_server::gen_lsp_types::Position {
    position_in(content, call, call.find('\'').unwrap() + 1 + inside)
}

/// The line of `call` in `content`.
fn line_of(content: &str, call: &str) -> u32 {
    position_in(content, call, 0).line
}

/// Completion, go-to-definition, hover, and references work on each call
/// that takes a field name, for a base field and a configurable field, on
/// an interface receiver and on a class receiver.
#[tokio::test]
async fn field_names_complete_navigate_and_hover_at_every_call() {
    let named = named();
    let (backend, dir) = named_site(&[(NAMED_PATH, &named)]).await;
    let uri = uri_for(&dir, NAMED_PATH);
    let tags = tags_storage();
    // Each field: where it is declared, what its hover says, and the lines
    // that read it as a property.
    let title_declared = (
        uri_for(&dir, NODE_PATH),
        position_in(NODE, "$fields['title']", 9),
    );
    let tags_declared = (
        uri_for(&dir, SYNC_TAGS),
        position_in(&tags, "field_name: field_tags", "field_name: ".len()),
    );
    let fields = [
        (
            "title",
            &title_declared,
            [
                "Field type `string`",
                "FieldItemList<StringItem>",
                "Node.php",
            ],
            "$node->title;",
        ),
        (
            "field_tags",
            &tags_declared,
            [
                "Field type `entity_reference`",
                "EntityReferenceFieldItemList<EntityReferenceItem, Term>",
                "field.storage.node.field_tags.yml",
            ],
            "$entity->field_tags;",
        ),
    ];
    for (field, (declared_uri, declared_at), hover_parts, property_read) in fields {
        // Every line that names the field, and the file that declares it.
        let mut expected: Vec<(tower_lsp_server::gen_lsp_types::Uri, u32)> = Vec::new();
        for receiver in ["$node", "$entity"] {
            for (method, _) in FIELD_NAME_CALLS {
                let call = format!("{receiver}->{method}('{field}'");
                expected.push((uri.clone(), line_of(&named, &call)));
            }
        }
        expected.push((uri.clone(), line_of(&named, property_read)));
        expected.push((declared_uri.clone(), declared_at.line));
        expected.sort();
        for receiver in ["$node", "$entity"] {
            for (method, _) in FIELD_NAME_CALLS {
                let call = format!("{receiver}->{method}('{field}'");
                let at = named_site_at(&named, &call, 1);

                let found = definitions_at(&backend, &uri, at).await;
                assert_eq!(found.len(), 1, "{call}: {found:?}");
                assert_eq!(&found[0].uri, declared_uri, "{call}");
                assert_eq!(found[0].range.start, *declared_at, "{call}");

                let hover = hover_at(&backend, uri.as_str(), &named, at.line, at.character)
                    .unwrap_or_else(|| panic!("no hover on {call}"));
                let text = hover_text(&hover);
                for part in hover_parts.iter().chain(&["Entity type `node`"]) {
                    assert!(text.contains(part), "{call}: {part} not in {text}");
                }

                let found = crate::drupal_services::references_at(&backend, &uri, at).await;
                assert_eq!(found, expected, "{call}");

                let at = named_site_at(&named, &call, 0);
                let labels = crate::common::response_labels(
                    crate::common::completion_response(&backend, &uri, at.line, at.character, None)
                        .await,
                );
                for offered in ["title", "field_tags", "nid", "uid"] {
                    assert!(offers(&labels, offered), "{call}: {offered} in {labels:?}");
                }
                for not_offered in ["name", "body", "acme.settings"] {
                    assert!(!offers(&labels, not_offered), "{call}: {not_offered}");
                }
            }
        }
    }
}

/// A receiver that is not fieldable, core's interface's namesake outside
/// its namespace included, and a name built at runtime name no field.
#[tokio::test]
async fn other_receivers_and_runtime_names_name_no_field() {
    let named = named();
    let (backend, dir) = named_site(&[(NAMED_PATH, &named)]).await;
    let uri = uri_for(&dir, NAMED_PATH);
    for call in [
        "$bag->get('title'",
        "$fake->get('title'",
        "$plain->get('title'",
    ] {
        let at = named_site_at(&named, call, 1);
        assert!(
            definitions_at(&backend, &uri, at).await.is_empty(),
            "{call}"
        );
        let hover = hover_at(&backend, uri.as_str(), &named, at.line, at.character)
            .map(|hover| hover_text(&hover).to_string())
            .unwrap_or_default();
        assert!(!hover.contains("Entity type"), "{call}: {hover}");
        let at = named_site_at(&named, call, 0);
        let labels = crate::common::response_labels(
            crate::common::completion_response(&backend, &uri, at.line, at.character, None).await,
        );
        assert!(!offers(&labels, "title"), "{call}: {labels:?}");
    }
    let at = position_in(&named, "get($name)", 5);
    let hover = hover_at(&backend, uri.as_str(), &named, at.line, at.character)
        .map(|hover| hover_text(&hover).to_string())
        .unwrap_or_default();
    assert!(!hover.contains("Entity type"), "{hover}");
}

const SHARED_PATH: &str = "web/modules/custom/acme/src/Shared.php";
const SHARED: &str = r#"<?php
namespace Drupal\acme;

use Drupal\Core\Entity\ContentEntityInterface;
use Drupal\node\NodeInterface;

final class Shared {
    public function read(Authored $authored, ContentEntityInterface $content, NodeInterface $node): void {
        $authored->get('uid');
        $authored->get('title');
        $content->get('uuid');
        $content->get('title');
        $node->get('uid');
        $node->get('title');
    }
}
"#;

/// An interface only node's and user's entity classes implement.
const AUTHORED: (&str, &str) = (
    "web/modules/custom/acme/src/Authored.php",
    "<?php\nnamespace Drupal\\acme;\ninterface Authored extends \\Drupal\\Core\\Entity\\ContentEntityInterface {}\n",
);

/// A field name on a receiver that holds several entity types is the field
/// all of them have, declared wherever each declares it, as the property
/// is; a name some of them lack is no field there.
#[tokio::test]
async fn a_field_name_on_several_entity_types_is_the_field_they_share() {
    const USER_PATH: &str = "web/core/modules/user/src/Entity/User.php";
    let user = CORE
        .iter()
        .find(|(path, _)| *path == USER_PATH)
        .map(|(_, content)| {
            content.replace(
                "implements UserInterface",
                "implements UserInterface, \\Drupal\\acme\\Authored",
            )
        })
        .unwrap();
    let node = NODE.replace(
        "implements NodeInterface",
        "implements NodeInterface, \\Drupal\\acme\\Authored",
    );
    let (backend, dir) = site(&[
        AUTHORED,
        (NODE_PATH, &node),
        (USER_PATH, &user),
        (SHARED_PATH, SHARED),
    ])
    .await;
    let uri = uri_for(&dir, SHARED_PATH);
    let hover = |call: &str| {
        let at = named_site_at(SHARED, call, 1);
        hover_at(&backend, uri.as_str(), SHARED, at.line, at.character)
            .map(|hover| hover_text(&hover).to_string())
            .unwrap_or_default()
    };

    // `uid` is the owner on node and the ID on user.
    let at = named_site_at(SHARED, "$authored->get('uid'", 1);
    let mut found: Vec<_> = definitions_at(&backend, &uri, at)
        .await
        .into_iter()
        .map(|location| location.uri)
        .collect();
    found.sort();
    let mut expected = vec![
        uri_for(
            &dir,
            "web/core/lib/Drupal/Core/Entity/ContentEntityBase.php",
        ),
        uri_for(&dir, "web/core/modules/user/src/EntityOwnerTrait.php"),
    ];
    expected.sort();
    assert_eq!(found, expected);
    let text = hover("$authored->get('uid'");
    assert!(text.contains("Entity types `node`, `user`"), "{text}");
    assert!(text.contains("EntityOwnerTrait.php"), "{text}");
    assert!(text.contains("ContentEntityBase.php"), "{text}");
    let found = crate::drupal_services::references_at(&backend, &uri, at).await;
    for call in ["$authored->get('uid'", "$node->get('uid'"] {
        assert!(
            found.contains(&(uri.clone(), line_of(SHARED, call))),
            "{call}: {found:?}"
        );
    }

    // Every content entity type has a UUID.
    let at = named_site_at(SHARED, "$content->get('uuid'", 1);
    assert_eq!(definitions_at(&backend, &uri, at).await.len(), 1);
    assert!(!hover("$content->get('uuid'").contains("Not a"));

    // Only node has a title.
    for call in ["$authored->get('title'", "$content->get('title'"] {
        let at = named_site_at(SHARED, call, 1);
        assert!(
            definitions_at(&backend, &uri, at).await.is_empty(),
            "{call}"
        );
        let text = hover(call);
        assert!(!text.contains("Field type"), "{call}: {text}");
    }
    let at = named_site_at(SHARED, "$node->get('title'", 1);
    let found = crate::drupal_services::references_at(&backend, &uri, at).await;
    for call in ["$authored->get('title'", "$content->get('title'"] {
        assert!(
            !found.contains(&(uri.clone(), line_of(SHARED, call))),
            "{call}: {found:?}"
        );
    }
}

/// Outside a Drupal project the same calls name nothing.
#[tokio::test]
async fn field_names_are_plain_strings_outside_drupal() {
    let named = named();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("composer.json"), "{}").unwrap();
    let tags = tags_storage();
    for (path, content) in CORE
        .iter()
        .copied()
        .chain([(NAMED_PATH, named.as_str()), (SYNC_TAGS, tags.as_str())])
    {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    let backend = Backend::new_test();
    initialize_with_options(&backend, dir.path(), None).await;
    let uri = uri_for(&dir, NAMED_PATH);
    open_document(&backend, &uri, "php", &named).await;
    let at = named_site_at(&named, "$node->get('title'", 1);
    assert!(definitions_at(&backend, &uri, at).await.is_empty());
    let at = named_site_at(&named, "$node->get('title'", 0);
    let labels = crate::common::response_labels(
        crate::common::completion_response(&backend, &uri, at.line, at.character, None).await,
    );
    assert!(!offers(&labels, "title"), "{labels:?}");
}

const TYPED_NAMES: &str = r#"<?php
namespace Drupal\acme;

use Drupal\node\Entity\Node;
use Drupal\node\NodeInterface;

final class TypedNames {
    public function read(NodeInterface $node, Node $entity, string $name): void {
        $title = $node->get('title');
        $class_title = $entity->get('title');
        $tags = $node->get('field_tags');
        $unknown = $node->get('field_missing');
        $dynamic = $node->get($name);
        $first_tag = $node->get('field_tags')->entity;
        $class_first_tag = $entity->get('field_tags')->entity;
    }
}
"#;
const TYPED_NAMES_PATH: &str = "web/modules/custom/acme/src/TypedNames.php";

/// `get('name')` is the field `->name` reads, assigned or chained, on an
/// interface and on a class; an unknown or a runtime name keeps the
/// declared item list interface.
#[tokio::test]
async fn get_with_a_field_name_is_typed_as_the_field() {
    let (backend, dir) = named_site(&[(TYPED_NAMES_PATH, TYPED_NAMES)]).await;
    let uri = uri_for(&dir, TYPED_NAMES_PATH);
    let hover = |var: &str| hovered_type(&backend, uri.as_str(), TYPED_NAMES, var);
    for var in ["$title", "$class_title"] {
        let text = hover(var);
        assert!(text.contains("FieldItemList<StringItem>"), "{var}: {text}");
    }
    let text = hover("$tags");
    assert!(
        text.contains("EntityReferenceFieldItemList<EntityReferenceItem, Term>"),
        "{text}"
    );
    for var in ["$unknown", "$dynamic"] {
        let text = hover(var);
        assert!(text.contains("FieldItemListInterface"), "{var}: {text}");
        assert!(!text.contains("StringItem"), "{var}: {text}");
    }
    for var in ["$first_tag", "$class_first_tag"] {
        let text = hover(var);
        assert!(text.contains("Term"), "{var}: {text}");
    }
}
