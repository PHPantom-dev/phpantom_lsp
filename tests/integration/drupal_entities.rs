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

/// A load or a service lookup called straight on the call before it, with
/// no variable in between, is typed the same way.
#[tokio::test]
async fn chained_loads_and_service_lookups_are_typed() {
    let note = NOTE.replace(
        "class Note {}",
        "class Note {\n    public function noteOnly(): void {}\n}",
    );
    let chained = r#"<?php
namespace Drupal\acme;

use Drupal\Core\Entity\EntityTypeManagerInterface;

final class Chained {
    public function run(EntityTypeManagerInterface $manager): void {
        $manager->getStorage('acme_note')->load(1)?->noteOnly();
        $manager->getStorage('acme_note')->create([])->noteOnly();
        \Drupal::service('acme.greeter')->greet();
    }
}
"#;
    let path = "web/modules/custom/acme/src/Chained.php";
    let services = "services:\n  acme.greeter:\n    class: Drupal\\acme\\Greeter\n";
    let mut files: Vec<(&str, &str)> = STORAGE_STUBS.to_vec();
    files.extend([
        (MANAGER_PATH, MANAGER),
        (NOTE_PATH, note.as_str()),
        ("web/modules/custom/acme/acme.services.yml", services),
        (path, chained),
    ]);
    let (backend, dir) = workspace(&files).await;
    let php = uri_for(&dir, path);
    for (needle, target) in [
        ("load(1)?->noteOnly", NOTE_PATH),
        ("create([])->noteOnly", NOTE_PATH),
        ("->greet()", "web/modules/custom/acme/src/Greeter.php"),
    ] {
        let inside = needle.rfind('>').unwrap() + 2;
        let found = definitions_at(&backend, &php, position_in(chained, needle, inside)).await;
        assert_eq!(found.len(), 1, "{needle}: {found:?}");
        assert_eq!(found[0].uri, uri_for(&dir, target), "{needle}");
    }
}

/// `getStorage(entity_type_id: 'x')` names the same entity type as the
/// positional form.
#[tokio::test]
async fn a_named_entity_type_id_types_the_storage() {
    let named = LOADER.replace(
        "$storage = $this->manager->getStorage('acme_note');",
        "$storage = $this->manager->getStorage(entity_type_id: 'acme_note');",
    );
    let mut files: Vec<(&str, &str)> = STORAGE_STUBS.to_vec();
    files.extend([
        (MANAGER_PATH, MANAGER),
        (NOTE_PATH, NOTE),
        (LOADER_PATH, named.as_str()),
    ]);
    let (backend, dir) = workspace(&files).await;
    let position = position_in(&named, "$storage;", 2);
    let hover = crate::common::hover_text_at(
        &backend,
        &uri_for(&dir, LOADER_PATH),
        position.line,
        position.character,
    )
    .await
    .unwrap_or_default();
    assert!(hover.contains("SqlContentEntityStorage<Note>"), "{hover}");
}

/// Just enough of core's entity queries: `\Drupal`'s query starters, the
/// query interfaces, and a storage that starts one.
const QUERY_STUBS: &[(&str, &str)] = &[
    (
        "web/core/lib/Drupal.php",
        "<?php\nclass Drupal {\n    public static function service(string $id): object {}\n    /** @return \\Drupal\\Core\\Entity\\Query\\QueryInterface */\n    public static function entityQuery($entity_type, $conjunction = 'AND') {}\n    /** @return \\Drupal\\Core\\Entity\\Query\\QueryAggregateInterface */\n    public static function entityQueryAggregate($entity_type, $conjunction = 'AND') {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Entity/Query/QueryInterface.php",
        "<?php\nnamespace Drupal\\Core\\Entity\\Query;\ninterface QueryInterface {\n    /** @return $this */\n    public function condition($field, $value = NULL, $operator = NULL, $langcode = NULL);\n    /** @return $this */\n    public function accessCheck($access_check = TRUE);\n    /** @return $this */\n    public function count();\n    /** @return int|array */\n    public function execute();\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Entity/Query/QueryAggregateInterface.php",
        "<?php\nnamespace Drupal\\Core\\Entity\\Query;\ninterface QueryAggregateInterface extends QueryInterface {\n    /** @return array */\n    public function execute();\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Entity/EntityInterface.php",
        "<?php\nnamespace Drupal\\Core\\Entity;\ninterface EntityInterface {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Entity/EntityStorageInterface.php",
        "<?php\nnamespace Drupal\\Core\\Entity;\ninterface EntityStorageInterface {\n    public function load($id): ?EntityInterface;\n    /** @return \\Drupal\\Core\\Entity\\Query\\QueryInterface */\n    public function getQuery($conjunction = 'AND');\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Entity/Sql/SqlContentEntityStorage.php",
        "<?php\nnamespace Drupal\\Core\\Entity\\Sql;\nclass SqlContentEntityStorage implements \\Drupal\\Core\\Entity\\EntityStorageInterface {\n    public function load($id): ?\\Drupal\\Core\\Entity\\EntityInterface {}\n    /** @return \\Drupal\\Core\\Entity\\Query\\QueryInterface */\n    public function getQuery($conjunction = 'AND') {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Config/Entity/ConfigEntityInterface.php",
        "<?php\nnamespace Drupal\\Core\\Config\\Entity;\ninterface ConfigEntityInterface extends \\Drupal\\Core\\Entity\\EntityInterface {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Entity/ContentEntityInterface.php",
        "<?php\nnamespace Drupal\\Core\\Entity;\ninterface ContentEntityInterface extends EntityInterface {}\n",
    ),
];

const CONTENT_NOTE: &str = r#"<?php
namespace Drupal\acme\Entity;

use Drupal\Core\Entity\Attribute\ContentEntityType;
use Drupal\Core\Entity\ContentEntityInterface;

#[ContentEntityType(
    id: 'acme_note',
)]
class Note implements ContentEntityInterface {}
"#;

/// An entity class that is neither a content nor a configuration entity.
const PLAIN: &str = r#"<?php
namespace Drupal\acme\Entity;

use Drupal\Core\Entity\Attribute\EntityType;

#[EntityType(
    id: 'acme_plain',
)]
class Plain {}
"#;

const PLAIN_PATH: &str = "web/modules/custom/acme/src/Entity/Plain.php";

const STYLE: &str = r#"<?php
namespace Drupal\acme\Entity;

use Drupal\Core\Config\Entity\ConfigEntityInterface;
use Drupal\Core\Entity\Attribute\ConfigEntityType;

#[ConfigEntityType(
    id: 'acme_style',
)]
class Style implements ConfigEntityInterface {}
"#;

const STYLE_PATH: &str = "web/modules/custom/acme/src/Entity/Style.php";

const QUERIES: &str = r#"<?php
namespace Drupal\acme;

use Drupal\Core\Entity\EntityTypeManagerInterface;

final class Queries {
    public function run(EntityTypeManagerInterface $manager): void {
        $query = \Drupal::entityQuery('acme_note');
        $query->condition('title', 'Ada');
        $ids = $query->accessCheck(TRUE)->execute();
        $ids;
        $count = \Drupal::entityQuery('acme_note')->accessCheck(TRUE)->count()->execute();
        $count;
        $counting = $manager->getStorage('acme_note')->getQuery()->count();
        $counted = $counting->condition('title', 'Ada')->execute();
        $counted;
        $styles = \Drupal::entityQuery('acme_style')->accessCheck(TRUE)->execute();
        $styles;
        $rows = \Drupal::entityQueryAggregate('acme_note')->execute();
        $rows;
        $plain = \Drupal::entityQuery('acme_plain')->accessCheck(TRUE)->execute();
        $plain;
    }
}
"#;

const QUERIES_PATH: &str = "web/modules/custom/acme/src/Queries.php";

/// The files of a site with a content and a configuration entity type and
/// `consumer` at [`QUERIES_PATH`].
fn query_files(consumer: &str) -> Vec<(&str, &str)> {
    let mut files: Vec<(&str, &str)> = QUERY_STUBS.to_vec();
    files.extend([
        (MANAGER_PATH, MANAGER),
        (NOTE_PATH, CONTENT_NOTE),
        (STYLE_PATH, STYLE),
        (PLAIN_PATH, PLAIN),
        (QUERIES_PATH, consumer),
    ]);
    files
}

/// An entity query's `execute()` gives the IDs of the entity type the query
/// was started on, keyed by revision ID for a content entity and by ID for a
/// configuration entity, or their number once the query counts.  The query
/// keeps what it was started on through a variable and through a chain of
/// the methods that return it.  An entity class of neither kind keeps
/// core's `int|array`.
#[tokio::test]
async fn entity_queries_are_typed_by_the_entity_type() {
    let (backend, dir) = workspace(&query_files(QUERIES)).await;
    let php = uri_for(&dir, QUERIES_PATH);
    let hover = |needle: &'static str| {
        let position = position_in(QUERIES, needle, 2);
        let backend = &backend;
        let php = php.clone();
        async move {
            crate::common::hover_text_at(backend, &php, position.line, position.character)
                .await
                .unwrap_or_default()
        }
    };

    let ids = hover("$ids;").await;
    assert!(ids.contains("array<int, string>"), "{ids}");
    let count = hover("$count;").await;
    assert!(count.contains("int<0, max>"), "{count}");
    let counted = hover("$counted;").await;
    assert!(counted.contains("int<0, max>"), "{counted}");
    let styles = hover("$styles;").await;
    assert!(styles.contains("array<string, string>"), "{styles}");
    // An aggregate query gives rows, as core declares, not IDs.
    let rows = hover("$rows;").await;
    assert!(rows.contains("$rows = array\n"), "{rows}");
    let plain = hover("$plain;").await;
    assert!(plain.contains("$plain = int|array\n"), "{plain}");
}

/// The entity type ID `\Drupal::entityQuery()` and
/// `\Drupal::entityQueryAggregate()` start a query on goes to its
/// declaration and completes.
#[tokio::test]
async fn entity_query_ids_go_to_their_declaration_and_complete() {
    let (backend, dir) = workspace(&query_files(QUERIES)).await;
    let php = uri_for(&dir, QUERIES_PATH);
    let id_line = CONTENT_NOTE
        .lines()
        .position(|line| line.contains("id: 'acme_note'"))
        .unwrap() as u32;
    for needle in [
        "entityQuery('acme_note')",
        "entityQueryAggregate('acme_note')",
    ] {
        let inside = needle.find('\'').unwrap() + 2;
        let found = definitions_at(&backend, &php, position_in(QUERIES, needle, inside)).await;
        assert_eq!(found.len(), 1, "{needle}: {found:?}");
        assert_eq!(
            (&found[0].uri, found[0].range.start.line),
            (&uri_for(&dir, NOTE_PATH), id_line),
            "{needle}"
        );
    }

    let typing = QUERIES.replace(
        "\\Drupal::entityQuery('acme_style')",
        "\\Drupal::entityQuery('acme_')",
    );
    let (backend, dir) = workspace(&query_files(typing.as_str())).await;
    let position = position_in(&typing, "entityQuery('acme_')", 18);
    let labels = response_labels(
        completion_response(
            &backend,
            &uri_for(&dir, QUERIES_PATH),
            position.line,
            position.character,
            None,
        )
        .await,
    );
    assert!(labels.contains(&"acme_note".to_string()), "{labels:?}");
    assert!(labels.contains(&"acme_style".to_string()), "{labels:?}");
}
