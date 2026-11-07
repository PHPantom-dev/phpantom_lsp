//! Drupal's render elements: the names a `#[RenderElement]` or
//! `#[FormElement]` class (or its annotation) declares, read by the `#type`
//! of a render array.

use crate::common::{completion_response, hover_text_at, response_labels};
use crate::drupal_services::{definitions_at, position_in, uri_for, workspace_in};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const DETAILS_PATH: &str = "web/core/lib/Drupal/Core/Render/Element/Details.php";

const DETAILS: &str = "<?php\nnamespace Drupal\\Core\\Render\\Element;\n\nuse Drupal\\Core\\Render\\Attribute\\RenderElement;\n\n#[RenderElement('details')]\nclass Details {}\n";

/// An element of a test module, which no real site enables.
const PROBE_PATH: &str = "web/core/modules/system/tests/modules/element_test/src/Element/Probe.php";

const PROBE: &str = "<?php\nnamespace Drupal\\element_test\\Element;\n\nuse Drupal\\Core\\Render\\Attribute\\RenderElement;\n\n#[RenderElement('element_test_probe')]\nclass Probe {}\n";

const CARD_PATH: &str = "web/modules/custom/acme/src/Element/Card.php";

const CARD: &str = "<?php\nnamespace Drupal\\acme\\Element;\n\nuse Drupal\\Core\\Render\\Attribute\\RenderElement;\n\n#[RenderElement('acme_card')]\nclass Card {}\n";

const BADGE_PATH: &str = "web/modules/custom/acme/src/Element/Badge.php";

const BADGE: &str = "<?php\nnamespace Drupal\\acme\\Element;\n\n/**\n * A badge, declared the way older code does.\n *\n * @FormElement(\"acme_badge\")\n */\nclass Badge {}\n";

const BUILDER_PATH: &str = "web/modules/custom/acme/src/Builder.php";

const BUILDER: &str = r#"<?php
namespace Drupal\acme;

final class Builder {
    public function build(array $form, string $type): array {
        $form['card']['#type'] = 'acme_card';
        $form['badge']['#type'] = 'acme_badge';
        $form['typo']['#type'] = 'acme_crad';
        $form['dynamic']['#type'] = $type;
        $form['probe'] = ['#type' => 'element_test_probe'];
        return [
            '#type' => 'details',
            'empty' => ['#type' => ''],
        ];
    }
}
"#;

/// A Drupal site with core's and a test module's elements behind the
/// site's `.gitignore`, the custom elements, and `files` open, indexed the
/// way the background index reads it.
async fn site(files: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in [
        (".git/HEAD", "ref: refs/heads/main\n"),
        (".gitignore", "/web/core/\n"),
        (DETAILS_PATH, DETAILS),
        (PROBE_PATH, PROBE),
        (CARD_PATH, CARD),
        (BADGE_PATH, BADGE),
    ] {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    let (backend, dir) = workspace_in(dir, files).await;
    backend.ensure_workspace_indexed();
    (backend, dir)
}

/// The labels completion offers at `inside` bytes into `needle` in `php`.
async fn labels_at(
    backend: &Backend,
    uri: &Uri,
    php: &str,
    needle: &str,
    inside: usize,
) -> Vec<String> {
    let position = position_in(php, needle, inside);
    response_labels(
        completion_response(backend, uri, position.line, position.character, None).await,
    )
}

/// The unknown-element messages the diagnostics of `uri` report.
fn unknown_elements(backend: &Backend, uri: &Uri, content: &str) -> Vec<String> {
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), content, &mut diagnostics);
    diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                &diagnostic.code,
                Some(Code::String(code)) if code == "invalid_drupal_render_element"
            )
        })
        .map(|diagnostic| crate::common::message_text(&diagnostic.message).to_string())
        .collect()
}

#[tokio::test]
async fn element_names_complete_from_core_and_custom_elements() {
    let (backend, dir) = site(&[(BUILDER_PATH, BUILDER)]).await;
    let php = uri_for(&dir, BUILDER_PATH);
    let in_literal = labels_at(&backend, &php, BUILDER, "'#type' => ''", 12).await;
    for name in ["details", "acme_card", "acme_badge"] {
        assert!(
            in_literal.contains(&name.to_string()),
            "{name}: {in_literal:?}"
        );
    }
    assert!(
        !in_literal.contains(&"element_test_probe".to_string()),
        "{in_literal:?}"
    );
    let in_assignment = labels_at(&backend, &php, BUILDER, "['#type'] = 'acme_", 18).await;
    assert_eq!(in_assignment, ["acme_badge", "acme_card"]);
}

#[tokio::test]
async fn an_element_name_goes_to_its_class_and_hovers_it() {
    let (backend, dir) = site(&[(BUILDER_PATH, BUILDER)]).await;
    let php = uri_for(&dir, BUILDER_PATH);
    for (needle, target, line) in [
        ("'acme_card'", CARD_PATH, 6),
        ("'acme_badge'", BADGE_PATH, 8),
        ("'details'", DETAILS_PATH, 6),
    ] {
        let found = definitions_at(&backend, &php, position_in(BUILDER, needle, 2)).await;
        assert_eq!(found.len(), 1, "{needle}: {found:?}");
        assert_eq!(
            (&found[0].uri, found[0].range.start.line),
            (&uri_for(&dir, target), line),
            "{needle}"
        );
    }

    let position = position_in(BUILDER, "'acme_card'", 2);
    let hover = hover_text_at(&backend, &php, position.line, position.character)
        .await
        .unwrap_or_default();
    assert!(hover.contains("Render element"), "{hover}");
    assert!(hover.contains("Drupal\\acme\\Element\\Card"), "{hover}");
    assert!(hover.contains(CARD_PATH), "{hover}");
}

#[tokio::test]
async fn an_unknown_element_name_is_reported() {
    let (backend, dir) = site(&[(BUILDER_PATH, BUILDER)]).await;
    let php = uri_for(&dir, BUILDER_PATH);
    assert_eq!(
        unknown_elements(&backend, &php, BUILDER),
        [
            "Unknown render element: 'acme_crad'",
            "Unknown render element: 'element_test_probe'",
        ]
    );

    // A test module's element counts while its file is open.
    crate::common::open_php(&backend, &uri_for(&dir, PROBE_PATH), PROBE).await;
    assert_eq!(
        unknown_elements(&backend, &php, BUILDER),
        ["Unknown render element: 'acme_crad'"]
    );

    // Test code is not judged.
    let form_path = "web/core/modules/system/tests/modules/element_test/src/ProbeForm.php";
    let form = "<?php\nnamespace Drupal\\element_test;\n\nfunction probe_form(): array {\n    return ['#type' => 'element_test_missing'];\n}\n";
    let form_uri = uri_for(&dir, form_path);
    crate::common::open_php(&backend, &form_uri, form).await;
    assert!(unknown_elements(&backend, &form_uri, form).is_empty());
}

#[tokio::test]
async fn elements_are_not_read_outside_drupal() {
    let backend = Backend::new_test();
    let uri = Uri::parse("file:///project/src/Builder.php").unwrap();
    crate::common::open_php(&backend, &uri, BUILDER).await;
    let labels = labels_at(&backend, &uri, BUILDER, "'#type' => ''", 12).await;
    assert!(!labels.contains(&"details".to_string()), "{labels:?}");
    assert!(unknown_elements(&backend, &uri, BUILDER).is_empty());
}

fn watched(path: &std::path::Path, kind: FileChangeType) -> DidChangeWatchedFilesParams {
    DidChangeWatchedFilesParams {
        changes: vec![FileEvent {
            uri: Uri::from_file_path(path).unwrap(),
            kind,
        }],
    }
}

#[tokio::test]
async fn adding_editing_and_deleting_an_element_class_refreshes_the_names() {
    let (backend, dir) = site(&[(BUILDER_PATH, BUILDER)]).await;
    let php = uri_for(&dir, BUILDER_PATH);
    let offered = || labels_at(&backend, &php, BUILDER, "'#type' => ''", 12);
    let path = dir
        .path()
        .join("web/modules/custom/acme/src/Element/Fresh.php");
    let fresh = |id: &str| {
        format!(
            "<?php\nnamespace Drupal\\acme\\Element;\n\nuse Drupal\\Core\\Render\\Attribute\\RenderElement;\n\n#[RenderElement('{id}')]\nclass Fresh {{}}\n"
        )
    };

    std::fs::write(&path, fresh("acme_fresh")).unwrap();
    backend
        .did_change_watched_files(watched(&path, FileChangeType::Created))
        .await;
    assert!(offered().await.contains(&"acme_fresh".to_string()));

    std::fs::write(&path, fresh("acme_renamed")).unwrap();
    backend
        .did_change_watched_files(watched(&path, FileChangeType::Changed))
        .await;
    let labels = offered().await;
    assert!(labels.contains(&"acme_renamed".to_string()), "{labels:?}");
    assert!(!labels.contains(&"acme_fresh".to_string()), "{labels:?}");

    std::fs::remove_file(&path).unwrap();
    backend
        .did_change_watched_files(watched(&path, FileChangeType::Deleted))
        .await;
    assert!(!offered().await.contains(&"acme_renamed".to_string()));
}
