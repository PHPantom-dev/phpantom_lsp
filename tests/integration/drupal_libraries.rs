//! Drupal's library names: the keys a libraries file declares as
//! `<extension>/<key>`, read by `#attached` in a render array, a template's
//! `attach_library()`, a library's `dependencies`, and a theme's
//! `libraries`.

use crate::common::{
    completion_response, hover_text_at, initialize_with_options, open_document, response_labels,
};
use crate::drupal_services::{definitions_at, position_in, references_at, uri_for};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const DRUPAL: &str = r#"{
    "require": { "drupal/core-recommended": "^11" },
    "extra": { "drupal-scaffold": { "locations": { "web-root": "web/" } } }
}"#;

/// A Drupal site whose root package requires Twig too, so the templates
/// are read the way a Twig project's are.
const DRUPAL_WITH_TWIG: &str = r#"{
    "require": { "drupal/core-recommended": "^11", "twig/twig": "^3" },
    "extra": { "drupal-scaffold": { "locations": { "web-root": "web/" } } }
}"#;

/// Core, its test modules, and the site's `.gitignore`, which leaves core
/// out of the workspace walk.
const CORE: &[(&str, &str)] = &[
    (".git/HEAD", "ref: refs/heads/main\n"),
    (".gitignore", "/web/core/\n"),
    (
        "web/core/core.libraries.yml",
        "drupal:\n  version: VERSION\n  js:\n    misc/drupal.js: {}\nonce:\n  version: \"1.0.1\"\n",
    ),
    (
        "web/core/modules/system/tests/modules/fixture_test/fixture_test.libraries.yml",
        "fixture:\n  js:\n    fixture.js: {}\n",
    ),
    (
        "web/core/themes/claro/claro.info.yml",
        "name: Claro\ntype: theme\nlibraries:\n  - core/drupal\n",
    ),
    (
        "web/core/modules/system/system.info.yml",
        "name: System\ntype: module\nlibraries:\n  - core/once\n",
    ),
];

const LIBRARIES: &str = "greeting:\n  version: 1.x\n  css:\n    component:\n      css/acme-greeting.css: {}\n  dependencies:\n    - core/drupal\n    - core/once\n";
const LIBRARIES_PATH: &str = "web/modules/custom/acme/acme.libraries.yml";

const THEME_INFO: &str =
    "name: Acme Theme\ntype: theme\ncore_version_requirement: ^11\nlibraries:\n  - acme/greeting\n";
const THEME_INFO_PATH: &str = "web/themes/custom/acme_theme/acme_theme.info.yml";

const TEMPLATE: &str = "{{ attach_library('acme/greeting') }}\n<p>{{ greeting }}</p>\n";
const TEMPLATE_PATH: &str = "web/modules/custom/acme/templates/acme-greeting.html.twig";

const CONTROLLER: &str = r#"<?php
namespace Drupal\acme\Controller;

final class GreetingController {
    public function greet(): array {
        $build = [
            '#markup' => 'Hi',
            '#attached' => ['library' => ['acme/greeting', 'core/drupal']],
        ];
        $build['#attached']['library'][] = 'core/once';
        $build['actions']['#attached']['library'] = ['acme/greeting'];
        $build['#attached'] = ['library' => ['core/drupal']];
        $other = ['library' => ['acme/greeting']];
        $other['library'][] = 'acme/greeting';
        return $build;
    }
}
"#;
const CONTROLLER_PATH: &str = "web/modules/custom/acme/src/Controller/GreetingController.php";

/// A project described by `composer`, with core written to disk and
/// `files` open, once the workspace is indexed.
async fn site(composer: &str, files: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("composer.json"), composer).unwrap();
    for (path, content) in CORE.iter().chain(files) {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    let backend = Backend::new_test();
    initialize_with_options(&backend, dir.path(), None).await;
    // A test backend has no client, so `initialized` leaves out the
    // background index that reads core's declarations.
    backend.ensure_workspace_indexed();
    for (path, content) in files {
        let language = if path.ends_with(".php") {
            "php"
        } else if path.ends_with(".twig") {
            "twig"
        } else {
            "yaml"
        };
        open_document(&backend, &uri_for(&dir, path), language, content).await;
    }
    (backend, dir)
}

fn all_files() -> Vec<(&'static str, &'static str)> {
    vec![
        (LIBRARIES_PATH, LIBRARIES),
        (THEME_INFO_PATH, THEME_INFO),
        (TEMPLATE_PATH, TEMPLATE),
        (CONTROLLER_PATH, CONTROLLER),
    ]
}

/// The file and line each definition of the name at `position` lands on.
async fn landing(
    backend: &Backend,
    dir: &tempfile::TempDir,
    path: &str,
    position: Position,
) -> Vec<(Uri, u32)> {
    definitions_at(backend, &uri_for(dir, path), position)
        .await
        .into_iter()
        .map(|location| (location.uri, location.range.start.line))
        .collect()
}

async fn labels_at(
    backend: &Backend,
    dir: &tempfile::TempDir,
    path: &str,
    position: Position,
) -> Vec<String> {
    response_labels(
        completion_response(
            backend,
            &uri_for(dir, path),
            position.line,
            position.character,
            None,
        )
        .await,
    )
}

#[tokio::test]
async fn yaml_and_twig_readers_go_to_the_librarys_key() {
    let (backend, dir) = site(DRUPAL, &all_files()).await;
    let acme = uri_for(&dir, LIBRARIES_PATH);
    let core = uri_for(&dir, "web/core/core.libraries.yml");

    for (path, content, needle, expected) in [
        (LIBRARIES_PATH, LIBRARIES, "core/drupal", (core.clone(), 0)),
        (LIBRARIES_PATH, LIBRARIES, "core/once", (core.clone(), 4)),
        (
            THEME_INFO_PATH,
            THEME_INFO,
            "acme/greeting",
            (acme.clone(), 0),
        ),
        (TEMPLATE_PATH, TEMPLATE, "acme/greeting", (acme.clone(), 0)),
    ] {
        let found = landing(&backend, &dir, path, position_in(content, needle, 2)).await;
        assert_eq!(found, vec![expected], "{path} {needle}");
    }
}

#[tokio::test]
async fn render_arrays_attach_libraries_from_php() {
    let (backend, dir) = site(DRUPAL, &all_files()).await;
    let acme = uri_for(&dir, LIBRARIES_PATH);
    let core = uri_for(&dir, "web/core/core.libraries.yml");

    for (needle, expected) in [
        ("['acme/greeting', ", (acme.clone(), 0)),
        ("'core/drupal']],", (core.clone(), 0)),
        ("'core/once'", (core.clone(), 4)),
        ("['acme/greeting'];", (acme.clone(), 0)),
        ("['core/drupal']];", (core.clone(), 0)),
    ] {
        let found = landing(
            &backend,
            &dir,
            CONTROLLER_PATH,
            position_in(CONTROLLER, needle, 4),
        )
        .await;
        assert_eq!(found, vec![expected], "{needle}");
    }

    // A `library` key outside `#attached` is any array's.
    for needle in ["['library' => ['acme/greeting']];", "[] = 'acme/greeting'"] {
        let position = position_in(CONTROLLER, needle, needle.find("acme").unwrap() + 2);
        assert!(
            landing(&backend, &dir, CONTROLLER_PATH, position)
                .await
                .is_empty(),
            "{needle}"
        );
    }
}

#[tokio::test]
async fn a_librarys_references_list_every_reader() {
    let (backend, dir) = site(DRUPAL, &all_files()).await;
    let acme = uri_for(&dir, LIBRARIES_PATH);
    let found = references_at(&backend, &acme, position_in(LIBRARIES, "greeting", 2)).await;
    let php = uri_for(&dir, CONTROLLER_PATH);
    let mut expected = vec![
        (acme, 0),
        (uri_for(&dir, THEME_INFO_PATH), 4),
        (uri_for(&dir, TEMPLATE_PATH), 0),
        (php.clone(), 7),
        (php, 10),
    ];
    expected.sort();
    assert_eq!(found, expected);
}

/// A closed theme's info file is read for the libraries it attaches; a
/// module's has no such list.
#[tokio::test]
async fn a_core_themes_info_file_is_a_reader() {
    let (backend, dir) = site(DRUPAL, &all_files()).await;
    let acme = uri_for(&dir, LIBRARIES_PATH);
    let readers = |needle: &'static str| {
        let backend = &backend;
        let acme = acme.clone();
        async move {
            references_at(backend, &acme, position_in(LIBRARIES, needle, 2))
                .await
                .into_iter()
                .map(|(uri, _)| uri)
                .collect::<Vec<_>>()
        }
    };
    let claro = uri_for(&dir, "web/core/themes/claro/claro.info.yml");
    let found = readers("core/drupal").await;
    assert!(found.contains(&claro), "{found:?}");
    let system = uri_for(&dir, "web/core/modules/system/system.info.yml");
    let found = readers("core/once").await;
    assert!(!found.contains(&system), "{found:?}");
    assert!(found.contains(&acme), "{found:?}");
}

#[tokio::test]
async fn hover_shows_the_file_and_version() {
    let (backend, dir) = site(DRUPAL, &all_files()).await;
    for (path, content, needle, file, version) in [
        (
            THEME_INFO_PATH,
            THEME_INFO,
            "acme/greeting",
            LIBRARIES_PATH,
            "1.x",
        ),
        (
            LIBRARIES_PATH,
            LIBRARIES,
            "core/drupal",
            "web/core/core.libraries.yml",
            "VERSION",
        ),
        (
            CONTROLLER_PATH,
            CONTROLLER,
            "'core/once'",
            "web/core/core.libraries.yml",
            "1.0.1",
        ),
    ] {
        let position = position_in(content, needle, 3);
        let hover = hover_text_at(
            &backend,
            &uri_for(&dir, path),
            position.line,
            position.character,
        )
        .await
        .unwrap_or_default();
        assert!(hover.contains("**Library**"), "{needle}: {hover}");
        assert!(hover.contains(&format!("`{file}`")), "{needle}: {hover}");
        assert!(
            hover.contains(&format!("Version `{version}`")),
            "{needle}: {hover}"
        );
    }
}

/// Every place a library is read completes the declared names, core's
/// included, and leaves test modules' out.
#[tokio::test]
async fn library_names_complete_where_they_are_read() {
    let cases: [(&str, &str, &str, usize); 8] = [
        (
            LIBRARIES_PATH,
            "greeting:\n  dependencies:\n    - \nother:\n  js: {}\n",
            "- \n",
            2,
        ),
        (
            "web/modules/custom/acme/acme_extra.libraries.yml",
            "extra:\n  dependencies: [core/once, ]\n",
            ", ]",
            2,
        ),
        (
            THEME_INFO_PATH,
            "name: Acme Theme\ntype: theme\nlibraries:\n  - ''\n",
            "''",
            1,
        ),
        (TEMPLATE_PATH, "{{ attach_library('') }}\n", "''", 1),
        (
            CONTROLLER_PATH,
            "<?php\nfunction build(): array {\n    return ['#attached' => ['library' => ['core/once', '']]];\n}\n",
            ", '']",
            3,
        ),
        (
            CONTROLLER_PATH,
            "<?php\nfunction build(array $build): array {\n    $build['#attached']['library'][] = '';\n    return $build;\n}\n",
            "= '';",
            3,
        ),
        (
            CONTROLLER_PATH,
            "<?php\nfunction build(array $build): array {\n    $build['#attached'] = ['library' => [''\n",
            "[''",
            2,
        ),
        (
            CONTROLLER_PATH,
            "<?php\nfunction build(): void {\n    $build = ['#attached' => ['library' => ['core/']]];\n}\n",
            "core/']",
            5,
        ),
    ];
    for (path, content, needle, inside) in cases {
        let mut files = vec![(LIBRARIES_PATH, LIBRARIES)];
        files.retain(|(file, _)| *file != path);
        files.push((path, content));
        let (backend, dir) = site(DRUPAL, &files).await;
        let labels = labels_at(&backend, &dir, path, position_in(content, needle, inside)).await;
        for declared in ["core/drupal", "core/once"] {
            assert!(labels.contains(&declared.to_string()), "{path}: {labels:?}");
        }
        assert!(
            !labels.contains(&"fixture_test/fixture".to_string()),
            "{path}: {labels:?}"
        );
    }
}

/// A `library` list outside `#attached` completes nothing.
#[tokio::test]
async fn other_library_keys_complete_nothing() {
    let content = "<?php\nfunction build(): array {\n    return ['library' => ['']];\n}\n";
    let (backend, dir) = site(
        DRUPAL,
        &[(LIBRARIES_PATH, LIBRARIES), (CONTROLLER_PATH, content)],
    )
    .await;
    let labels = labels_at(
        &backend,
        &dir,
        CONTROLLER_PATH,
        position_in(content, "['']", 2),
    )
    .await;
    assert!(!labels.contains(&"core/drupal".to_string()), "{labels:?}");
}

/// A Drupal site that requires Twig is a Twig project, whose templates are
/// read through the Twig support; `attach_library()` is read there too.
#[tokio::test]
async fn a_twig_project_reads_attach_library() {
    let template = "{{ attach_library('acme/greeting') }}\n{{ attach_library('') }}\n";
    let (backend, dir) = site(
        DRUPAL_WITH_TWIG,
        &[(LIBRARIES_PATH, LIBRARIES), (TEMPLATE_PATH, template)],
    )
    .await;
    let found = landing(
        &backend,
        &dir,
        TEMPLATE_PATH,
        position_in(template, "acme/greeting", 2),
    )
    .await;
    assert_eq!(found, vec![(uri_for(&dir, LIBRARIES_PATH), 0)]);
    let labels = labels_at(
        &backend,
        &dir,
        TEMPLATE_PATH,
        position_in(template, "('')", 2),
    )
    .await;
    assert!(labels.contains(&"acme/greeting".to_string()), "{labels:?}");
}

/// Outside a Drupal project, none of it is read.
#[tokio::test]
async fn a_project_without_drupal_reads_no_libraries() {
    let content = "<?php\nfunction build(): array {\n    return ['#attached' => ['library' => ['acme/greeting', '']]];\n}\n";
    let (backend, dir) = site(
        r#"{ "require": { "php": "^8.3" } }"#,
        &[(LIBRARIES_PATH, LIBRARIES), (CONTROLLER_PATH, content)],
    )
    .await;
    let found = landing(
        &backend,
        &dir,
        CONTROLLER_PATH,
        position_in(content, "acme/greeting", 2),
    )
    .await;
    assert!(found.is_empty(), "{found:?}");
    let labels = labels_at(
        &backend,
        &dir,
        CONTROLLER_PATH,
        position_in(content, ", '']", 3),
    )
    .await;
    assert!(!labels.contains(&"acme/greeting".to_string()), "{labels:?}");
}

/// A libraries file written, edited, or deleted outside the editor
/// changes the names offered.
#[tokio::test]
async fn libraries_files_changed_on_disk_refresh_the_names() {
    let content =
        "<?php\nfunction build(): array {\n    return ['#attached' => ['library' => ['']]];\n}\n";
    let (backend, dir) = site(DRUPAL, &[(CONTROLLER_PATH, content)]).await;
    let position = position_in(content, "['']", 2);
    let libraries = dir
        .path()
        .join("web/themes/custom/acme_theme/acme_theme.libraries.yml");
    let notify = |kind: FileChangeType| DidChangeWatchedFilesParams {
        changes: vec![FileEvent {
            uri: Uri::from_file_path(&libraries).unwrap(),
            kind,
        }],
    };
    let offered = |labels: Vec<String>, name: &str| labels.contains(&name.to_string());

    assert!(!offered(
        labels_at(&backend, &dir, CONTROLLER_PATH, position).await,
        "acme_theme/global"
    ));

    std::fs::create_dir_all(libraries.parent().unwrap()).unwrap();
    std::fs::write(&libraries, "global:\n  css: {}\n").unwrap();
    backend
        .did_change_watched_files(notify(FileChangeType::Created))
        .await;
    assert!(offered(
        labels_at(&backend, &dir, CONTROLLER_PATH, position).await,
        "acme_theme/global"
    ));

    std::fs::write(&libraries, "base:\n  css: {}\n").unwrap();
    backend
        .did_change_watched_files(notify(FileChangeType::Changed))
        .await;
    let labels = labels_at(&backend, &dir, CONTROLLER_PATH, position).await;
    assert!(offered(labels.clone(), "acme_theme/base"), "{labels:?}");
    assert!(!offered(labels, "acme_theme/global"));

    std::fs::remove_file(&libraries).unwrap();
    backend
        .did_change_watched_files(notify(FileChangeType::Deleted))
        .await;
    assert!(!offered(
        labels_at(&backend, &dir, CONTROLLER_PATH, position).await,
        "acme_theme/base"
    ));
}
