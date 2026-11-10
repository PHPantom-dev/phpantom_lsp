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
