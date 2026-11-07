//! Drupal's permission names: the keys a permissions file declares, read by
//! the access checks of accounts and roles, the access result helpers, a
//! route's `_permission` requirement, and an entity type's
//! `admin_permission`.

use crate::common::{completion_response, hover_text_at, initialize_with_options, response_labels};
use crate::drupal_services::{
    definitions_at, position_in, references_at, uri_for, workspace, workspace_in,
};
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

/// Just enough of core's access API, where Composer would put core.
const STUBS: &[(&str, &str)] = &[
    (
        "web/core/lib/Drupal/Core/Session/AccountInterface.php",
        "<?php\nnamespace Drupal\\Core\\Session;\ninterface AccountInterface {\n    public function hasPermission(string $permission);\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Session/PermissionCheckerInterface.php",
        "<?php\nnamespace Drupal\\Core\\Session;\ninterface PermissionCheckerInterface {\n    public function hasPermission(string $permission, AccountInterface $account): bool;\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Session/CalculatedPermissionsItemInterface.php",
        "<?php\nnamespace Drupal\\Core\\Session;\ninterface CalculatedPermissionsItemInterface {\n    public function hasPermission(string $permission): bool;\n}\n",
    ),
    (
        "web/core/modules/user/src/RoleInterface.php",
        "<?php\nnamespace Drupal\\user;\ninterface RoleInterface {\n    public function hasPermission($permission);\n    public function grantPermission($permission);\n    public function revokePermission($permission);\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Access/AccessResult.php",
        "<?php\nnamespace Drupal\\Core\\Access;\nuse Drupal\\Core\\Session\\AccountInterface;\nclass AccessResult {\n    public static function allowedIfHasPermission(AccountInterface $account, $permission) { return new static(); }\n    public static function allowedIfHasPermissions(AccountInterface $account, array $permissions, $conjunction = 'AND') { return new static(); }\n}\n",
    ),
    (
        "web/core/lib/Symfony/Routing/RouteClass.php",
        "<?php\nnamespace Symfony\\Component\\Routing;\nclass Route {\n    public function __construct(string $path, array $defaults = [], array $requirements = []) {}\n    public function setRequirement(string $key, string $regex): static { return $this; }\n    public function setRequirements(array $requirements): static { return $this; }\n}\n",
    ),
    (
        "web/core/lib/Symfony/Routing/RouteAttribute.php",
        "<?php\nnamespace Symfony\\Component\\Routing\\Attribute;\n#[\\Attribute]\nclass Route {\n    public function __construct(?string $path = null, ?string $name = null, array $requirements = []) {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Entity/Attribute/ContentEntityType.php",
        "<?php\nnamespace Drupal\\Core\\Entity\\Attribute;\n#[\\Attribute]\nclass ContentEntityType {\n    public function __construct(string $id, ?string $admin_permission = null) {}\n}\n",
    ),
];

const PERMISSIONS: &str = "administer acme:\n  title: 'Administer acme'\n  description: 'Change every acme setting.'\n  restrict access: true\nview acme notes:\n  title: 'View acme notes'\npermission_callbacks:\n  - '\\Drupal\\acme\\AcmePermissions::permissions'\n";

const PERMISSIONS_PATH: &str = "web/modules/custom/acme/acme.permissions.yml";

const CONSUMER: &str = r#"<?php
namespace Drupal\acme;

use Drupal\Core\Access\AccessResult;
use Drupal\Core\Session\AccountInterface;
use Drupal\Core\Session\CalculatedPermissionsItemInterface;
use Drupal\Core\Session\PermissionCheckerInterface;
use Drupal\user\RoleInterface;
use Symfony\Component\Routing\Route;

final class Access {
    public function __construct(private readonly PermissionCheckerInterface $checker) {}

    public function run(AccountInterface $account, RoleInterface $role, CalculatedPermissionsItemInterface $item): void {
        $account->hasPermission('administer acme');
        $role->hasPermission('administer acme');
        $this->checker->hasPermission('administer acme', $account);
        $item->hasPermission('administer acme');
        $role->grantPermission('administer acme');
        $role->revokePermission('administer acme');
        AccessResult::allowedIfHasPermission($account, 'administer acme');
        AccessResult::allowedIfHasPermissions($account, ['view acme notes', 'administer acme']);
        user_role_grant_permissions('editor', ['administer acme']);
        \user_role_revoke_permissions('editor', ['administer acme']);
        $route = new Route('/acme', [], ['_permission' => 'view acme notes+administer acme']);
        $route->setRequirement('_permission', 'administer acme');
        $route->setRequirements(['_permission' => 'view acme notes,administer acme']);
    }
}
"#;

const CONSUMER_PATH: &str = "web/modules/custom/acme/src/Access.php";

const CONTROLLER: &str = r#"<?php
namespace Drupal\acme\Controller;

use Symfony\Component\Routing\Attribute\Route;

final class NotesController {
    #[Route(path: '/notes', name: 'acme.notes', requirements: ['_permission' => 'administer acme'])]
    public function list(): array { return []; }
}
"#;

const CONTROLLER_PATH: &str = "web/modules/custom/acme/src/Controller/NotesController.php";

const ENTITY: &str = r#"<?php
namespace Drupal\acme\Entity;

use Drupal\Core\Entity\Attribute\ContentEntityType;

#[ContentEntityType(
    id: 'acme_note',
    admin_permission: 'administer acme',
)]
class Note {}

/**
 * @ContentEntityType(
 *   id = "acme_old_note",
 *   admin_permission = "administer acme",
 * )
 */
class OldNote {}
"#;

const ENTITY_PATH: &str = "web/modules/custom/acme/src/Entity/Note.php";

const ROUTING: &str = "acme.settings:\n  path: '/acme/settings'\n  requirements:\n    _permission: 'access content+administer acme'\n";

const ROUTING_PATH: &str = "web/modules/custom/acme/acme.routing.yml";

fn files<'a>(extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    STUBS.iter().copied().chain(extra.iter().copied()).collect()
}

/// The 0-based lines of `content` holding `needle`.
fn lines_of(content: &str, needle: &str) -> Vec<u32> {
    content
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains(needle))
        .map(|(at, _)| at as u32)
        .collect()
}

#[tokio::test]
async fn a_permission_goes_to_its_permissions_file_and_shows_its_title() {
    let (backend, dir) = workspace(&files(&[
        (PERMISSIONS_PATH, PERMISSIONS),
        (CONSUMER_PATH, CONSUMER),
    ]))
    .await;
    let php = uri_for(&dir, CONSUMER_PATH);
    let yaml = uri_for(&dir, PERMISSIONS_PATH);

    let found = definitions_at(
        &backend,
        &php,
        position_in(CONSUMER, "$account->hasPermission('administer acme')", 26),
    )
    .await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!((&found[0].uri, found[0].range.start.line), (&yaml, 0));

    let position = position_in(CONSUMER, "allowedIfHasPermission($account, 'administer", 35);
    let hover = hover_text_at(&backend, &php, position.line, position.character)
        .await
        .unwrap_or_default();
    assert!(hover.contains("Administer acme"), "{hover}");
    assert!(hover.contains("Change every acme setting."), "{hover}");
    assert!(hover.contains("Restricted"), "{hover}");
    assert!(hover.contains("acme.permissions.yml"), "{hover}");

    let position = position_in(PERMISSIONS, "view acme notes", 2);
    let hover = hover_text_at(&backend, &yaml, position.line, position.character)
        .await
        .unwrap_or_default();
    assert!(hover.contains("View acme notes"), "{hover}");
    assert!(!hover.contains("Restricted"), "{hover}");
}

/// Every place that names a permission counts among its references.
#[tokio::test]
async fn every_place_that_names_a_permission_reads_it() {
    let (backend, dir) = workspace(&files(&[
        (PERMISSIONS_PATH, PERMISSIONS),
        (CONSUMER_PATH, CONSUMER),
        (CONTROLLER_PATH, CONTROLLER),
        (ENTITY_PATH, ENTITY),
        (ROUTING_PATH, ROUTING),
    ]))
    .await;
    let yaml = uri_for(&dir, PERMISSIONS_PATH);
    let found = references_at(&backend, &yaml, Position::new(0, 3)).await;

    let mut expected = vec![(yaml.clone(), 0)];
    for (path, content) in [
        (CONSUMER_PATH, CONSUMER),
        (CONTROLLER_PATH, CONTROLLER),
        (ENTITY_PATH, ENTITY),
        (ROUTING_PATH, ROUTING),
    ] {
        let uri = uri_for(&dir, path);
        expected.extend(
            lines_of(content, "administer acme")
                .into_iter()
                .map(|line| (uri.clone(), line)),
        );
    }
    expected.sort();
    assert_eq!(found, expected);

    let found = references_at(&backend, &yaml, Position::new(4, 3)).await;
    let php = uri_for(&dir, CONSUMER_PATH);
    let mut expected = vec![(yaml, 4)];
    expected.extend(
        lines_of(CONSUMER, "view acme notes")
            .into_iter()
            .map(|line| (php.clone(), line)),
    );
    expected.sort();
    assert_eq!(found, expected);
}

/// Core's site, with its permissions files where Composer puts them, and a
/// test module's beside them.
fn core_site() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in [
        (".git/HEAD", "ref: refs/heads/main\n"),
        (".gitignore", "/web/core/\n"),
        (
            "web/core/modules/system/system.permissions.yml",
            "access content:\n  title: 'View published content'\n",
        ),
        (SYSTEM_TEST_PATH, "system test only:\n  title: 'Test'\n"),
    ] {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    dir
}

const SYSTEM_TEST_PATH: &str =
    "web/core/modules/system/tests/modules/system_test/system_test.permissions.yml";

/// Every place that names a permission goes to its declaration and shows
/// it on hover, a custom module's and core's alike.
#[tokio::test]
async fn every_place_goes_to_the_permission_and_shows_it() {
    let core_reader = "<?php\nnamespace Drupal\\acme;\nuse Drupal\\Core\\Session\\AccountInterface;\nfinal class Front {\n    public function run(AccountInterface $account): void {\n        $account->hasPermission('access content');\n    }\n}\n";
    let core_reader_path = "web/modules/custom/acme/src/Front.php";
    let (backend, dir) = workspace_in(
        core_site(),
        &files(&[
            (PERMISSIONS_PATH, PERMISSIONS),
            (CONSUMER_PATH, CONSUMER),
            (CONTROLLER_PATH, CONTROLLER),
            (ENTITY_PATH, ENTITY),
            (ROUTING_PATH, ROUTING),
            (core_reader_path, core_reader),
        ]),
    )
    .await;
    // Finding references indexes the workspace, core's permissions files
    // included.
    references_at(
        &backend,
        &uri_for(&dir, PERMISSIONS_PATH),
        Position::new(0, 3),
    )
    .await;

    let yaml = uri_for(&dir, PERMISSIONS_PATH);
    let system = uri_for(&dir, "web/core/modules/system/system.permissions.yml");
    let mut places = Vec::new();
    for (path, content) in [
        (CONSUMER_PATH, CONSUMER),
        (CONTROLLER_PATH, CONTROLLER),
        (ENTITY_PATH, ENTITY),
        (ROUTING_PATH, ROUTING),
    ] {
        for (at, _) in content.match_indices("administer acme") {
            places.push((path, content, at, &yaml, "Administer acme"));
        }
    }
    for (path, content) in [(ROUTING_PATH, ROUTING), (core_reader_path, core_reader)] {
        let at = content.find("access content").unwrap();
        places.push((path, content, at, &system, "View published content"));
    }
    assert_eq!(places.len(), 19);
    for (path, content, at, declared, title) in places {
        let uri = uri_for(&dir, path);
        let position = crate::common::offset_position(content, at + 2);
        let found = definitions_at(&backend, &uri, position).await;
        assert_eq!(
            found
                .iter()
                .map(|location| (&location.uri, location.range.start.line))
                .collect::<Vec<_>>(),
            [(declared, 0)],
            "{path}:{position:?}"
        );
        let hover = hover_text_at(&backend, &uri, position.line, position.character)
            .await
            .unwrap_or_default();
        assert!(hover.contains(title), "{path}:{position:?}: {hover}");
    }
}

/// A test module's permissions count while its file is open.
#[tokio::test]
async fn an_open_test_module_offers_its_permissions() {
    let consumer = CONSUMER.replace(
        "$account->hasPermission('administer acme')",
        "$account->hasPermission('')",
    );
    let fixture = "system test only:\n  title: 'Test'\n";
    let (backend, dir) = workspace_in(
        core_site(),
        &files(&[
            (CONSUMER_PATH, consumer.as_str()),
            (SYSTEM_TEST_PATH, fixture),
        ]),
    )
    .await;
    let position = position_in(&consumer, "hasPermission('')", 15);
    let labels = response_labels(
        completion_response(
            &backend,
            &uri_for(&dir, CONSUMER_PATH),
            position.line,
            position.character,
            None,
        )
        .await,
    );
    assert!(
        labels.contains(&"system test only".to_string()),
        "{labels:?}"
    );
}

/// The same short names, of classes that are not Drupal's, read nothing.
#[tokio::test]
async fn look_alike_classes_read_no_permission() {
    let consumer = r#"<?php
namespace Drupal\acme;

final class AccessResult {
    public static function allowedIfHasPermission($account, $permission) {}
}

final class Route {
    public function __construct(string $path, array $defaults = [], array $requirements = []) {}
}

final class Gate {
    public function hasPermission(string $permission): bool { return true; }
}

final class Elsewhere {
    public function run(Gate $gate): void {
        AccessResult::allowedIfHasPermission(null, 'administer acme');
        new Route('/acme', [], ['_permission' => 'administer acme']);
        $gate->hasPermission('administer acme');
        acme_grant_permissions('editor', ['administer acme']);
    }
}
"#;
    let path = "web/modules/custom/acme/src/Elsewhere.php";
    let (backend, dir) =
        workspace(&files(&[(PERMISSIONS_PATH, PERMISSIONS), (path, consumer)])).await;
    let yaml = uri_for(&dir, PERMISSIONS_PATH);
    let found = references_at(&backend, &yaml, Position::new(0, 3)).await;
    assert_eq!(found, vec![(yaml, 0)]);
}

#[tokio::test]
async fn core_permissions_complete_and_test_modules_do_not() {
    let consumer = r#"<?php
namespace Drupal\acme;

use Drupal\Core\Access\AccessResult;
use Drupal\Core\Session\AccountInterface;
use Drupal\user\RoleInterface;
use Symfony\Component\Routing\Route;

final class Access {
    public function run(AccountInterface $account, RoleInterface $role, Route $route): void {
        $account->hasPermission('');
        $role->grantPermission('');
        AccessResult::allowedIfHasPermission($account, '');
        AccessResult::allowedIfHasPermissions($account, ['view acme notes', '']);
        user_role_revoke_permissions('editor', ['']);
        new Route('/a', [], ['_permission' => 'view acme notes+']);
        $route->setRequirement('_permission', '');
    }
}
"#;
    let controller =
        CONTROLLER.replace("'_permission' => 'administer acme'", "'_permission' => ''");
    let entity = ENTITY
        .replace(
            "admin_permission: 'administer acme'",
            "admin_permission: ''",
        )
        .replace(
            "admin_permission = \"administer acme\"",
            "admin_permission = \"\"",
        );
    let routing = ROUTING.replace("'access content+administer acme'", "'access content+'");
    let (backend, dir) = workspace_in(
        core_site(),
        &files(&[
            (PERMISSIONS_PATH, PERMISSIONS),
            (CONSUMER_PATH, consumer),
            (CONTROLLER_PATH, controller.as_str()),
            (ENTITY_PATH, entity.as_str()),
            (ROUTING_PATH, routing.as_str()),
        ]),
    )
    .await;
    // Finding references indexes the workspace, core's permissions files
    // included.
    references_at(
        &backend,
        &uri_for(&dir, PERMISSIONS_PATH),
        Position::new(0, 3),
    )
    .await;

    let mut places: Vec<(&str, String, Position)> = [
        "hasPermission('')",
        "grantPermission('')",
        "$account, '')",
        "'view acme notes', '']",
        "['']",
        "acme notes+'",
        "'_permission', '')",
    ]
    .iter()
    .map(|needle| {
        let inside = needle.rfind('\'').unwrap();
        (
            CONSUMER_PATH,
            needle.to_string(),
            position_in(consumer, needle, inside),
        )
    })
    .collect();
    places.push((
        CONTROLLER_PATH,
        "requirements".to_string(),
        position_in(&controller, "'_permission' => ''", 18),
    ));
    places.push((
        ENTITY_PATH,
        "attribute".to_string(),
        position_in(&entity, "admin_permission: ''", 19),
    ));
    places.push((
        ENTITY_PATH,
        "annotation".to_string(),
        position_in(&entity, "admin_permission = \"\"", 20),
    ));
    places.push((
        ROUTING_PATH,
        "routing".to_string(),
        position_in(&routing, "access content+'", 15),
    ));
    for (path, label, position) in places {
        let labels = response_labels(
            completion_response(
                &backend,
                &uri_for(&dir, path),
                position.line,
                position.character,
                None,
            )
            .await,
        );
        for declared in ["administer acme", "view acme notes", "access content"] {
            assert!(
                labels.contains(&declared.to_string()),
                "{label}: {labels:?}"
            );
        }
        assert!(
            !labels.contains(&"system test only".to_string()),
            "{label}: {labels:?}"
        );
        assert!(
            !labels.contains(&"permission_callbacks".to_string()),
            "{label}: {labels:?}"
        );
    }
}

/// A project that is not a Drupal site has no permissions to offer, and
/// reads no `hasPermission()` as one.
#[tokio::test]
async fn a_project_that_is_not_drupal_reads_no_permission() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("composer.json"),
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
    )
    .unwrap();
    let files = [
        (
            "src/Account.php",
            "<?php\nnamespace App;\ninterface Account {\n    public function hasPermission(string $permission): bool;\n}\n",
        ),
        (
            "src/acme.permissions.yml",
            "administer acme:\n  title: 'Administer acme'\n",
        ),
        (
            "src/Check.php",
            "<?php\nnamespace App;\nfinal class Check {\n    public function run(Account $account): void {\n        $account->hasPermission('');\n    }\n}\n",
        ),
    ];
    for (path, content) in files {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    let backend = phpantom_lsp::Backend::new_test();
    initialize_with_options(&backend, dir.path(), None).await;
    let uri = uri_for(&dir, "src/Check.php");
    crate::common::open_document(&backend, &uri, "php", files[2].1).await;
    let labels = response_labels(completion_response(&backend, &uri, 4, 33, None).await);
    assert!(
        !labels.contains(&"administer acme".to_string()),
        "{labels:?}"
    );
}

async fn notify(backend: &phpantom_lsp::Backend, uri: &Uri, kind: FileChangeType) {
    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: uri.clone(),
                kind,
            }],
        })
        .await;
}

/// Adding, editing, or deleting a custom module's permissions file on disk
/// refreshes the names offered.
#[tokio::test]
async fn a_permissions_file_changed_on_disk_refreshes_the_names() {
    let consumer = CONSUMER.replace(
        "$account->hasPermission('administer acme')",
        "$account->hasPermission('')",
    );
    let (backend, dir) = workspace(&files(&[(CONSUMER_PATH, consumer.as_str())])).await;
    let php = uri_for(&dir, CONSUMER_PATH);
    let position = position_in(&consumer, "hasPermission('')", 15);
    let offered = || async {
        response_labels(
            completion_response(&backend, &php, position.line, position.character, None).await,
        )
    };
    assert!(!offered().await.contains(&"administer acme".to_string()));

    let path = dir.path().join(PERMISSIONS_PATH);
    let yaml = uri_for(&dir, PERMISSIONS_PATH);
    std::fs::write(&path, PERMISSIONS).unwrap();
    notify(&backend, &yaml, FileChangeType::Created).await;
    let labels = offered().await;
    assert!(
        labels.contains(&"administer acme".to_string()),
        "{labels:?}"
    );

    std::fs::write(&path, "administer acme notes:\n  title: 'Administer'\n").unwrap();
    notify(&backend, &yaml, FileChangeType::Changed).await;
    let labels = offered().await;
    assert!(
        labels.contains(&"administer acme notes".to_string()),
        "{labels:?}"
    );
    assert!(
        !labels.contains(&"administer acme".to_string()),
        "{labels:?}"
    );

    std::fs::remove_file(&path).unwrap();
    notify(&backend, &yaml, FileChangeType::Deleted).await;
    let labels = offered().await;
    assert!(
        !labels.contains(&"administer acme notes".to_string()),
        "{labels:?}"
    );
}

/// A comment just above the string, holding quotes and backticks of its
/// own, changes nothing, and the same text inside the comment is no
/// string to complete: the first `hasPermission('` of the file is the
/// comment's.
#[tokio::test]
async fn a_comment_with_quotes_before_the_string_changes_nothing() {
    let consumer = r#"<?php
namespace Drupal\acme;

use Drupal\Core\Session\AccountInterface;

final class Access {
    public function run(AccountInterface $account): bool {
        // Try: complete a permission inside `hasPermission('')`; core's are
        // offered too.
        return $account->hasPermission('');
    }
}
"#;
    let entity = r#"<?php
namespace Drupal\acme\Entity;

use Drupal\Core\Entity\Attribute\ContentEntityType;

#[ContentEntityType(
    id: 'acme_note',
    // Try: complete a permission inside `admin_permission: ''`.
    admin_permission: '',
)]
class Note {}
"#;
    let (backend, dir) = workspace(&files(&[
        (PERMISSIONS_PATH, PERMISSIONS),
        (CONSUMER_PATH, consumer),
        (ENTITY_PATH, entity),
    ]))
    .await;
    for (path, content, needle) in [
        (CONSUMER_PATH, consumer, "return $account->hasPermission(''"),
        (ENTITY_PATH, entity, "    admin_permission: ''"),
    ] {
        let at = content.find(needle).unwrap() + needle.len() - 1;
        let position = crate::common::offset_position(content, at);
        let labels = response_labels(
            completion_response(
                &backend,
                &uri_for(&dir, path),
                position.line,
                position.character,
                None,
            )
            .await,
        );
        assert!(
            labels.contains(&"administer acme".to_string()),
            "{path}: {labels:?}"
        );

        let in_comment = content
            .find(needle.trim_start().trim_start_matches("return $account->"))
            .unwrap();
        let in_comment = in_comment
            + needle
                .trim_start()
                .trim_start_matches("return $account->")
                .len()
            - 1;
        assert!(
            content[..in_comment]
                .rsplit('\n')
                .next()
                .unwrap()
                .contains("//")
        );
        let position = crate::common::offset_position(content, in_comment);
        let labels = response_labels(
            completion_response(
                &backend,
                &uri_for(&dir, path),
                position.line,
                position.character,
                None,
            )
            .await,
        );
        assert!(labels.is_empty(), "{path}, in the comment: {labels:?}");
    }
}
