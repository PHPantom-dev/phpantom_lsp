//! Drupal's entity types: the IDs an entity class's attribute or annotation
//! declares, read by the entity type manager.

use crate::common::{completion_response, response_labels};
use crate::drupal_services::{
    definitions_at, position_in, references_at, uri_for, workspace, workspace_in,
};

const NOTE: &str = r#"<?php
namespace Drupal\acme\Entity;

use Drupal\Core\Entity\Attribute\ContentEntityType;

#[ContentEntityType(
    id: 'acme_note',
)]
class Note {}
"#;

const NOTE_PATH: &str = "web/modules/custom/acme/src/Entity/Note.php";

const MANAGER: &str = "<?php\nnamespace Drupal\\Core\\Entity;\ninterface EntityTypeManagerInterface {\n    public function getStorage(string $entity_type_id): EntityStorageInterface;\n    public function getDefinition(string $entity_type_id, bool $exception_on_invalid = true): ?object;\n}\n";

const MANAGER_PATH: &str = "web/core/lib/Drupal/Core/Entity/EntityTypeManagerInterface.php";

const CONSUMER: &str = r#"<?php
namespace Drupal\acme;

use Drupal\Core\Entity\EntityTypeManagerInterface;

final class Notes {
    public function __construct(private EntityTypeManagerInterface $manager) {}

    public function run(): void {
        $this->manager->getStorage('acme_note');
        $this->manager->getDefinition('acme_note');
    }
}
"#;

const CONSUMER_PATH: &str = "web/modules/custom/acme/src/Notes.php";

#[tokio::test]
async fn an_entity_type_id_goes_to_its_declaration_and_lists_its_readers() {
    let (backend, dir) = workspace(&[
        (MANAGER_PATH, MANAGER),
        (NOTE_PATH, NOTE),
        (CONSUMER_PATH, CONSUMER),
    ])
    .await;
    let php = uri_for(&dir, CONSUMER_PATH);
    for needle in ["getStorage('acme_note')", "getDefinition('acme_note')"] {
        let inside = needle.find('\'').unwrap() + 2;
        let found = definitions_at(&backend, &php, position_in(CONSUMER, needle, inside)).await;
        assert_eq!(found.len(), 1, "{needle}: {found:?}");
        assert_eq!(
            (&found[0].uri, found[0].range.start.line),
            (&uri_for(&dir, NOTE_PATH), 6)
        );
    }

    let found = references_at(
        &backend,
        &uri_for(&dir, NOTE_PATH),
        position_in(NOTE, "acme_note", 2),
    )
    .await;
    assert_eq!(
        found,
        vec![(php.clone(), 9), (php, 10), (uri_for(&dir, NOTE_PATH), 6)]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
    );
}

/// Core's entity classes sit in a folder the project's `.gitignore` leaves
/// out, and its test modules declare entity types no real site has.
#[tokio::test]
async fn core_entity_types_complete_and_test_modules_do_not() {
    let consumer = CONSUMER.replace("getStorage('acme_note')", "getStorage('')");
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in [
        (".git/HEAD", "ref: refs/heads/main\n"),
        (".gitignore", "/web/core/\n"),
        (
            "web/core/modules/node/src/Entity/Node.php",
            "<?php\nnamespace Drupal\\node\\Entity;\n/**\n * @ContentEntityType(\n *   id = \"node\",\n * )\n */\nclass Node {}\n",
        ),
        (
            "web/core/modules/system/tests/modules/entity_test/src/Entity/EntityTest.php",
            "<?php\nnamespace Drupal\\entity_test\\Entity;\n/**\n * @ContentEntityType(\n *   id = \"entity_test\",\n * )\n */\nclass EntityTest {}\n",
        ),
    ] {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    let (backend, dir) = workspace_in(
        dir,
        &[
            (MANAGER_PATH, MANAGER),
            (NOTE_PATH, NOTE),
            (CONSUMER_PATH, consumer.as_str()),
        ],
    )
    .await;
    let php = uri_for(&dir, CONSUMER_PATH);
    // Finding references indexes the workspace, core's entity classes
    // included.
    references_at(
        &backend,
        &php,
        position_in(&consumer, "getDefinition('acme_note')", 16),
    )
    .await;

    let position = position_in(&consumer, "getStorage('')", 12);
    let labels = response_labels(
        completion_response(&backend, &php, position.line, position.character, None).await,
    );
    assert!(labels.contains(&"acme_note".to_string()), "{labels:?}");
    assert!(labels.contains(&"node".to_string()), "{labels:?}");
    assert!(!labels.contains(&"entity_test".to_string()), "{labels:?}");
}

const STORAGE_STUBS: &[(&str, &str)] = &[
    (
        "web/core/lib/Drupal/Core/Entity/EntityInterface.php",
        "<?php\nnamespace Drupal\\Core\\Entity;\ninterface EntityInterface {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Entity/EntityStorageInterface.php",
        "<?php\nnamespace Drupal\\Core\\Entity;\ninterface EntityStorageInterface {\n    public function load($id): ?EntityInterface;\n    public function loadMultiple(?array $ids = null): array;\n    public function create(array $values = []): EntityInterface;\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Entity/Sql/SqlContentEntityStorage.php",
        "<?php\nnamespace Drupal\\Core\\Entity\\Sql;\nclass SqlContentEntityStorage implements \\Drupal\\Core\\Entity\\EntityStorageInterface {\n    public function load($id): ?\\Drupal\\Core\\Entity\\EntityInterface {}\n    public function loadMultiple(?array $ids = null): array {}\n    public function create(array $values = []): \\Drupal\\Core\\Entity\\EntityInterface {}\n}\n",
    ),
];

const LOADER: &str = r#"<?php
namespace Drupal\acme;

use Drupal\Core\Entity\EntityTypeManagerInterface;

final class Loader {
    public function __construct(private EntityTypeManagerInterface $manager) {}

    public function run(): void {
        $storage = $this->manager->getStorage('acme_note');
        $storage;
        $note = $storage->load(1);
        $note;
        $notes = $storage->loadMultiple();
        $notes;
        $created = $this->manager->getStorage('acme_note')->create([]);
        $created;
        $unknown = $this->manager->getStorage('nope')->load(1);
        $unknown;
    }
}
"#;

const LOADER_PATH: &str = "web/modules/custom/acme/src/Loader.php";

#[tokio::test]
async fn storage_and_loads_are_typed_by_the_entity_type() {
    let mut files: Vec<(&str, &str)> = STORAGE_STUBS.to_vec();
    files.extend([
        (MANAGER_PATH, MANAGER),
        (NOTE_PATH, NOTE),
        (LOADER_PATH, LOADER),
    ]);
    let (backend, dir) = workspace(&files).await;
    let php = uri_for(&dir, LOADER_PATH);
    let hover = |needle: &'static str| {
        let position = position_in(LOADER, needle, 2);
        let backend = &backend;
        let php = php.clone();
        async move {
            crate::common::hover_text_at(backend, &php, position.line, position.character)
                .await
                .unwrap_or_default()
        }
    };

    let storage = hover("$storage;").await;
    assert!(
        storage.contains("SqlContentEntityStorage<Note>"),
        "{storage}"
    );
    let note = hover("$note;").await;
    assert!(
        note.contains("?Note") || note.contains("Note|null"),
        "{note}"
    );
    let notes = hover("$notes;").await;
    assert!(notes.contains("array<int|string, Note>"), "{notes}");
    let created = hover("$created;").await;
    assert!(created.contains("= Note"), "{created}");
    let unknown = hover("$unknown;").await;
    assert!(unknown.contains("EntityInterface"), "{unknown}");
}
