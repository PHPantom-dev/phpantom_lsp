//! A Drupal tree opened without a `composer.json` of its own, with a module
//! outside `contrib` and `custom` that the tree's `.gitignore` leaves out:
//! the workspace is a Drupal site, and the module's services name its
//! classes.

use crate::common::{
    completion_response, hover_text_at, initialize_with_options, offset_position, open_document,
    response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

const DRUPAL_CLASS: &str =
    "<?php\nclass Drupal {\n    public static function service(string $id): object {}\n}\n";

const CORE_COMPOSER: &str = r#"{ "name": "drupal/core" }"#;

const MODULE_COMPOSER: &str = r#"{ "name": "acme/acme_greeter", "type": "drupal-module" }"#;

const GREETER: &str = "<?php\nnamespace Drupal\\acme_greeter;\nclass Greeter {\n    public function greet(): string { return ''; }\n}\n";

const SERVICES: &str =
    "services:\n  acme_greeter.greeter:\n    class: Drupal\\acme_greeter\\Greeter\n";

const CONSUMER: &str = r#"<?php
namespace Drupal\acme_greeter;

final class StaticConsumer {
    public function run(): void {
        $greeter = \Drupal::service('acme_greeter.greeter');
        $greeter;
        \Drupal::service('acme_greeter.');
    }
}
"#;

const MODULE: &str = "modules/acme/acme_greeter";

/// An initialized workspace holding a Drupal tree with its web root at
/// `web_root` (relative to the workspace, `""` for the workspace itself),
/// in a git checkout whose `.gitignore` leaves out every module, and with
/// the services file and the consumer open, the way the other Drupal tests
/// open their declarations.  The `Greeter` class is not open.
async fn drupal_tree(web_root: &str) -> (Backend, tempfile::TempDir, Uri) {
    let dir = tempfile::tempdir().unwrap();
    let web = dir.path().join(web_root);
    let files = [
        ("core/lib/Drupal.php".to_string(), DRUPAL_CLASS),
        ("core/composer.json".to_string(), CORE_COMPOSER),
        (format!("{MODULE}/composer.json"), MODULE_COMPOSER),
        (format!("{MODULE}/src/Greeter.php"), GREETER),
        (format!("{MODULE}/acme_greeter.services.yml"), SERVICES),
        (format!("{MODULE}/src/StaticConsumer.php"), CONSUMER),
        (".gitignore".to_string(), "/modules/\n"),
    ];
    for (path, content) in &files {
        let full = web.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();

    let backend = Backend::new_test();
    initialize_with_options(&backend, dir.path(), None).await;
    let services =
        Uri::from_file_path(web.join(format!("{MODULE}/acme_greeter.services.yml"))).unwrap();
    open_document(&backend, &services, "yaml", SERVICES).await;
    let consumer =
        Uri::from_file_path(web.join(format!("{MODULE}/src/StaticConsumer.php"))).unwrap();
    open_document(&backend, &consumer, "php", CONSUMER).await;
    (backend, dir, consumer)
}

/// The position `inside` bytes into the first occurrence of `needle`.
fn position_in(needle: &str, inside: usize) -> Position {
    offset_position(
        CONSUMER,
        CONSUMER.find(needle).expect("needle should exist") + inside,
    )
}

async fn assert_services_resolve(backend: &Backend, consumer: &Uri) {
    let at = position_in("$greeter;", 2);
    let hover = hover_text_at(backend, consumer, at.line, at.character)
        .await
        .unwrap_or_default();
    assert!(hover.contains("Greeter"), "{hover}");

    let at = position_in("service('acme_greeter.')", 22);
    let labels =
        response_labels(completion_response(backend, consumer, at.line, at.character, None).await);
    assert!(
        labels.contains(&"acme_greeter.greeter".to_string()),
        "{labels:?}"
    );
}

#[tokio::test]
async fn services_resolve_in_a_drupal_tree_without_composer_json() {
    let (backend, _dir, consumer) = drupal_tree("").await;
    assert_services_resolve(&backend, &consumer).await;
}

#[tokio::test]
async fn services_resolve_in_a_drupal_tree_under_web_without_composer_json() {
    let (backend, _dir, consumer) = drupal_tree("web").await;
    assert_services_resolve(&backend, &consumer).await;
}
