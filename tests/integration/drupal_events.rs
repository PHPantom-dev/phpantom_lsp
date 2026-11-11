//! Drupal's event subscribers: the listener methods a subscriber's
//! `getSubscribedEvents()` names, whether it returns the map or builds it
//! by assignment.

use crate::common::{hover_text_at, message_text, rename};
use crate::drupal_services::{definitions_at, position_in, references_at, uri_for, workspace_in};
use tower_lsp_server::gen_lsp_types::*;

/// Just enough of Symfony's event dispatcher, where Composer would put it
/// in a Drupal site.
const STUBS: &[(&str, &str)] = &[
    (
        "vendor/symfony/event-dispatcher/EventSubscriberInterface.php",
        "<?php\nnamespace Symfony\\Component\\EventDispatcher;\ninterface EventSubscriberInterface {\n    public static function getSubscribedEvents();\n}\n",
    ),
    (
        "vendor/symfony/event-dispatcher/Attribute/AsEventListener.php",
        "<?php\nnamespace Symfony\\Component\\EventDispatcher\\Attribute;\n#[\\Attribute(\\Attribute::TARGET_CLASS | \\Attribute::TARGET_METHOD | \\Attribute::IS_REPEATABLE)]\nclass AsEventListener {\n    public function __construct(?string $event = null, ?string $method = null, int $priority = 0) {}\n}\n",
    ),
];

/// A subscriber that builds its map by assignment.
const ORDER_SUBSCRIBER: &str = r#"<?php
namespace Drupal\acme\EventSubscriber;

use Symfony\Component\EventDispatcher\EventSubscriberInterface;

final class OrderSubscriber implements EventSubscriberInterface {
    public static function getSubscribedEvents(): array {
        $events['acme.order_placed'][] = ['onPlaced', 10];
        $events['acme.order_paid'][] = ['onPaid'];
        return $events;
    }

    public function onPlaced(object $event): void {}

    public function onPaid(object $event): void {}
}
"#;

/// A subscriber that returns its map.
const AUDIT_SUBSCRIBER: &str = r#"<?php
namespace Drupal\acme\EventSubscriber;

use Symfony\Component\EventDispatcher\EventSubscriberInterface;

final class AuditSubscriber implements EventSubscriberInterface {
    public static function getSubscribedEvents(): array {
        return ['acme.order_placed' => 'audit'];
    }

    public function audit(object $event): void {}
}
"#;

const ORDER_PATH: &str = "web/modules/custom/acme/src/EventSubscriber/OrderSubscriber.php";
const AUDIT_PATH: &str = "web/modules/custom/acme/src/EventSubscriber/AuditSubscriber.php";

/// A Drupal site holding the dispatcher stubs and `files`, all open.
async fn site(files: &[(&str, &str)]) -> (phpantom_lsp::Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in STUBS {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    workspace_in(dir, files).await
}

/// The line `needle` first appears on.
fn line_of(content: &str, needle: &str) -> u32 {
    position_in(content, needle, 0).line
}

/// The messages of the unknown-member warnings in `content`.
fn unknown_members(backend: &phpantom_lsp::Backend, uri: &Uri, content: &str) -> Vec<String> {
    let mut diagnostics = Vec::new();
    backend.collect_unknown_member_diagnostics(uri.as_str(), content, &mut diagnostics);
    diagnostics
        .iter()
        .map(|diagnostic| message_text(&diagnostic.message).to_string())
        .collect()
}

#[tokio::test]
async fn a_listener_method_goes_to_its_definition_in_both_map_shapes() {
    let (backend, dir) = site(&[
        (ORDER_PATH, ORDER_SUBSCRIBER),
        (AUDIT_PATH, AUDIT_SUBSCRIBER),
    ])
    .await;
    let order = uri_for(&dir, ORDER_PATH);
    let audit = uri_for(&dir, AUDIT_PATH);

    for (uri, content, needle, method) in [
        (&order, ORDER_SUBSCRIBER, "'onPlaced'", "function onPlaced"),
        (&order, ORDER_SUBSCRIBER, "'onPaid'", "function onPaid"),
        (&audit, AUDIT_SUBSCRIBER, "'audit'", "function audit"),
    ] {
        let found = definitions_at(&backend, uri, position_in(content, needle, 2)).await;
        assert_eq!(found.len(), 1, "{needle}: {found:?}");
        assert_eq!(&found[0].uri, uri, "{needle}");
        assert_eq!(
            found[0].range.start.line,
            line_of(content, method),
            "{needle}"
        );
    }
}

#[tokio::test]
async fn a_listener_method_hovers_and_is_referenced_from_the_map() {
    let (backend, dir) = site(&[(ORDER_PATH, ORDER_SUBSCRIBER)]).await;
    let order = uri_for(&dir, ORDER_PATH);

    let position = position_in(ORDER_SUBSCRIBER, "'onPlaced'", 2);
    let hover = hover_text_at(&backend, &order, position.line, position.character)
        .await
        .unwrap_or_default();
    assert!(hover.contains("onPlaced"), "{hover}");

    let found = references_at(
        &backend,
        &order,
        position_in(ORDER_SUBSCRIBER, "function onPlaced", 10),
    )
    .await;
    assert_eq!(
        found,
        vec![
            (order.clone(), line_of(ORDER_SUBSCRIBER, "'onPlaced'")),
            (
                order.clone(),
                line_of(ORDER_SUBSCRIBER, "function onPlaced")
            ),
        ]
    );
}

#[tokio::test]
async fn renaming_a_listener_method_rewrites_the_map() {
    let (backend, dir) = site(&[
        (ORDER_PATH, ORDER_SUBSCRIBER),
        (AUDIT_PATH, AUDIT_SUBSCRIBER),
    ])
    .await;
    let order = uri_for(&dir, ORDER_PATH);
    let audit = uri_for(&dir, AUDIT_PATH);

    for (uri, content, needle, method) in [
        (&order, ORDER_SUBSCRIBER, "function onPlaced", "'onPlaced'"),
        (&audit, AUDIT_SUBSCRIBER, "function audit", "'audit'"),
    ] {
        let position = position_in(content, needle, 10);
        let edit = rename(&backend, uri, position.line, position.character, "renamed")
            .await
            .expect("the rename should produce edits");
        let changes = edit.changes.expect("a change map");
        let mut lines: Vec<u32> = changes
            .get(uri)
            .into_iter()
            .flatten()
            .inspect(|edit| assert_eq!(edit.new_text, "renamed"))
            .map(|edit| edit.range.start.line)
            .collect();
        lines.sort_unstable();
        assert_eq!(
            lines,
            vec![line_of(content, method), line_of(content, needle)],
            "{needle}"
        );
    }
}

#[tokio::test]
async fn a_missing_listener_method_is_reported_and_an_inherited_one_is_not() {
    let missing = ORDER_SUBSCRIBER.replace("['onPaid']", "['onRefunded']");
    let inherited = r#"<?php
namespace Drupal\acme\EventSubscriber;

final class ChildSubscriber extends BaseSubscriber {
    public static function getSubscribedEvents(): array {
        $events['acme.order_placed'][] = ['onBase'];
        return $events;
    }
}
"#;
    let base = r#"<?php
namespace Drupal\acme\EventSubscriber;

use Symfony\Component\EventDispatcher\EventSubscriberInterface;

abstract class BaseSubscriber implements EventSubscriberInterface {
    public static function getSubscribedEvents(): array {
        $events['acme.order_paid'][] = ['onAlter', 100];
        return $events;
    }

    abstract public function onAlter(object $event): void;

    public function onBase(object $event): void {}
}
"#;
    let child_path = "web/modules/custom/acme/src/EventSubscriber/ChildSubscriber.php";
    let base_path = "web/modules/custom/acme/src/EventSubscriber/BaseSubscriber.php";
    let (backend, dir) = site(&[
        (ORDER_PATH, missing.as_str()),
        (child_path, inherited),
        (base_path, base),
    ])
    .await;

    let reported = unknown_members(&backend, &uri_for(&dir, ORDER_PATH), &missing);
    assert_eq!(reported.len(), 1, "{reported:?}");
    assert!(reported[0].contains("onRefunded"), "{reported:?}");
    for (path, content) in [(child_path, inherited), (base_path, base)] {
        let reported = unknown_members(&backend, &uri_for(&dir, path), content);
        assert!(reported.is_empty(), "{path}: {reported:?}");
    }
}

#[tokio::test]
async fn drupal_does_not_read_event_listener_attributes() {
    let listener = r#"<?php
namespace Drupal\acme\EventListener;

use Symfony\Component\EventDispatcher\Attribute\AsEventListener;

#[AsEventListener(event: 'acme.order_placed', method: 'onMissing')]
final class OrderListener {
    #[AsEventListener(event: 'acme.')]
    public function onPlaced(object $event): void {}
}
"#;
    let path = "web/modules/custom/acme/src/EventListener/OrderListener.php";
    let (backend, dir) = site(&[(path, listener), (ORDER_PATH, ORDER_SUBSCRIBER)]).await;
    let uri = uri_for(&dir, path);

    assert!(unknown_members(&backend, &uri, listener).is_empty());
    let found = definitions_at(&backend, &uri, position_in(listener, "'onMissing'", 2)).await;
    assert!(found.is_empty(), "{found:?}");
    let position = position_in(listener, "event: 'acme.'", 13);
    let labels = crate::common::response_labels(
        crate::common::completion_response(&backend, &uri, position.line, position.character, None)
            .await,
    );
    assert!(
        !labels.iter().any(|label| label.starts_with("acme.")),
        "{labels:?}"
    );
}

#[tokio::test]
async fn nothing_is_read_outside_a_project_with_a_service_container() {
    let content = ORDER_SUBSCRIBER.replace("onPaid'", "onMissing'");
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("composer.json"),
        r#"{ "require": { "psr/log": "^3" } }"#,
    )
    .unwrap();
    let path = "src/OrderSubscriber.php";
    let full = dir.path().join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(&full, &content).unwrap();
    let backend = phpantom_lsp::Backend::new_test();
    crate::common::initialize_with_options(&backend, dir.path(), None).await;
    let uri = uri_for(&dir, path);
    crate::common::open_document(&backend, &uri, "php", &content).await;
    let reported = unknown_members(&backend, &uri, &content);
    assert!(reported.is_empty(), "{reported:?}");
}
