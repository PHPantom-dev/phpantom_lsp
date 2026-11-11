//! Drupal's event subscribers and event names: the listener methods a
//! subscriber's `getSubscribedEvents()` names, whether it returns the map
//! or builds it by assignment, and the event names its keys declare for
//! the dispatcher calls that name them.

use crate::common::{completion_response, hover_text_at, message_text, rename, response_labels};
use crate::drupal_services::{definitions_at, position_in, references_at, uri_for, workspace_in};
use tower_lsp_server::gen_lsp_types::*;

/// Just enough of Symfony's event dispatcher, where the Drupal walk reads
/// core, core's `event_dispatcher` service, and a core module that
/// subscribes by constant: files nobody has open.
const STUBS: &[(&str, &str)] = &[
    (
        "web/core/lib/Symfony/EventDispatcher/EventSubscriberInterface.php",
        "<?php\nnamespace Symfony\\Component\\EventDispatcher;\ninterface EventSubscriberInterface {\n    public static function getSubscribedEvents();\n}\n",
    ),
    (
        "web/core/lib/Symfony/EventDispatcher/Attribute/AsEventListener.php",
        "<?php\nnamespace Symfony\\Component\\EventDispatcher\\Attribute;\n#[\\Attribute(\\Attribute::TARGET_CLASS | \\Attribute::TARGET_METHOD | \\Attribute::IS_REPEATABLE)]\nclass AsEventListener {\n    public function __construct(?string $event = null, ?string $method = null, int $priority = 0) {}\n}\n",
    ),
    (
        "web/core/lib/Symfony/EventDispatcherContracts/EventDispatcherInterface.php",
        "<?php\nnamespace Symfony\\Contracts\\EventDispatcher;\ninterface EventDispatcherInterface {\n    public function dispatch(object $event, ?string $eventName = null): object;\n}\n",
    ),
    (
        "web/core/lib/Symfony/EventDispatcher/EventDispatcherInterface.php",
        "<?php\nnamespace Symfony\\Component\\EventDispatcher;\ninterface EventDispatcherInterface extends \\Symfony\\Contracts\\EventDispatcher\\EventDispatcherInterface {\n    public function addListener(string $eventName, callable|array $listener, int $priority = 0): void;\n    public function removeListener(string $eventName, callable|array $listener): void;\n    public function hasListeners(?string $eventName = null): bool;\n    public function getListeners(?string $eventName = null): array;\n    public function getListenerPriority(string $eventName, callable|array $listener): ?int;\n}\n",
    ),
    (
        "web/core/lib/Symfony/EventDispatcher/EventDispatcher.php",
        "<?php\nnamespace Symfony\\Component\\EventDispatcher;\nclass EventDispatcher implements EventDispatcherInterface {\n    public function dispatch(object $event, ?string $eventName = null): object { return $event; }\n    public function addListener(string $eventName, callable|array $listener, int $priority = 0): void {}\n    public function removeListener(string $eventName, callable|array $listener): void {}\n    public function hasListeners(?string $eventName = null): bool { return false; }\n    public function getListeners(?string $eventName = null): array { return []; }\n    public function getListenerPriority(string $eventName, callable|array $listener): ?int { return null; }\n}\n",
    ),
    (
        "web/core/core.services.yml",
        "services:\n  event_dispatcher:\n    class: Symfony\\Component\\EventDispatcher\\EventDispatcher\n",
    ),
    (
        "web/core/modules/shop/src/ShopEventsInterface.php",
        "<?php\nnamespace Drupal\\shop;\ninterface ShopEventsInterface {\n    const CLOSED = 'shop.closed';\n}\n",
    ),
    (
        "web/core/modules/shop/src/ShopEvents.php",
        "<?php\nnamespace Drupal\\shop;\nfinal class ShopEvents implements ShopEventsInterface {\n    const OPENED = 'shop.opened';\n    const TESTED = 'shop.tested';\n}\n",
    ),
    (SHOP_SUBSCRIBER_PATH, SHOP_SUBSCRIBER),
    // A test module's subscriber, which declares nothing while it is closed.
    (
        "web/core/modules/shop/tests/modules/shop_test/src/EventSubscriber/TestSubscriber.php",
        "<?php\nnamespace Drupal\\shop_test\\EventSubscriber;\nuse Drupal\\shop\\ShopEvents;\nfinal class TestSubscriber {\n    public static function getSubscribedEvents(): array {\n        return [ShopEvents::TESTED => 'onTested'];\n    }\n    public function onTested(object $event): void {}\n}\n",
    ),
];

/// A core subscriber keyed only by constants, one of them inherited from
/// an interface.
const SHOP_SUBSCRIBER: &str = "<?php
namespace Drupal\\shop\\EventSubscriber;

use Drupal\\shop\\ShopEvents;
use Symfony\\Component\\EventDispatcher\\EventSubscriberInterface;

final class ShopSubscriber implements EventSubscriberInterface {
    public static function getSubscribedEvents(): array {
        $events[ShopEvents::OPENED][] = ['onOpened', 100];
        $events[ShopEvents::CLOSED][] = ['onClosed'];
        return $events;
    }

    public function onOpened(object $event): void {}

    public function onClosed(object $event): void {}
}
";

const SHOP_SUBSCRIBER_PATH: &str = "web/core/modules/shop/src/EventSubscriber/ShopSubscriber.php";

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

/// A Drupal site holding the dispatcher stubs and `files`, `files` open
/// and the rest read by the workspace index.
async fn site(files: &[(&str, &str)]) -> (phpantom_lsp::Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in STUBS {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    let (backend, dir) = workspace_in(dir, files).await;
    // A test backend has no client, so `initialized` leaves the index out.
    backend.ensure_workspace_indexed();
    (backend, dir)
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

/// A module's calls on each of the dispatcher's typings, and on something
/// that is not a dispatcher.
const CHECKOUT: &str = r#"<?php
namespace Drupal\acme;

use Symfony\Component\EventDispatcher\EventDispatcherInterface;
use Symfony\Contracts\EventDispatcher\EventDispatcherInterface as ContractsDispatcher;

final class Checkout {
    public function __construct(
        private EventDispatcherInterface $events,
        private ContractsDispatcher $contracts,
    ) {}

    public function place(object $order, \ArrayObject $other): void {
        $this->contracts->dispatch($order, 'acme.order_placed');
        $this->events->hasListeners('shop.opened');
        \Drupal::service('event_dispatcher')->dispatch($order, 'shop.closed');
        $this->events->addListener('acme.order_paid', [$this, 'place']);
        $other->offsetSet($order, 'acme.order_placed');
    }
}
"#;

const CHECKOUT_PATH: &str = "web/modules/custom/acme/src/Checkout.php";

#[tokio::test]
async fn a_dispatched_event_goes_to_every_subscription_on_each_dispatcher_typing() {
    let (backend, dir) = site(&[(ORDER_PATH, ORDER_SUBSCRIBER), (CHECKOUT_PATH, CHECKOUT)]).await;
    let checkout = uri_for(&dir, CHECKOUT_PATH);
    let order = uri_for(&dir, ORDER_PATH);
    let shop = uri_for(&dir, SHOP_SUBSCRIBER_PATH);

    for (needle, uri, line) in [
        (
            "'acme.order_placed'",
            &order,
            line_of(ORDER_SUBSCRIBER, "'acme.order_placed'"),
        ),
        (
            "'shop.opened'",
            &shop,
            line_of(SHOP_SUBSCRIBER, "ShopEvents::OPENED"),
        ),
        (
            "'shop.closed'",
            &shop,
            line_of(SHOP_SUBSCRIBER, "ShopEvents::CLOSED"),
        ),
    ] {
        let found = definitions_at(&backend, &checkout, position_in(CHECKOUT, needle, 2)).await;
        let found: Vec<(&Uri, u32)> = found
            .iter()
            .map(|location| (&location.uri, location.range.start.line))
            .collect();
        assert_eq!(found, vec![(uri, line)], "{needle}");

        let position = position_in(CHECKOUT, needle, 2);
        let hover = hover_text_at(&backend, &checkout, position.line, position.character)
            .await
            .unwrap_or_default();
        assert!(
            hover.contains("1 listener in the project"),
            "{needle}: {hover}"
        );
    }

    // `offsetSet()` on something that is not a dispatcher names nothing.
    let position = position_in(CHECKOUT, "$order, 'acme.order_placed');\n    }", 10);
    let found = definitions_at(&backend, &checkout, position).await;
    assert!(found.is_empty(), "{found:?}");
}

#[tokio::test]
async fn references_of_an_event_name_include_the_subscriptions_keyed_by_a_constant() {
    let (backend, dir) = site(&[(ORDER_PATH, ORDER_SUBSCRIBER), (CHECKOUT_PATH, CHECKOUT)]).await;
    let checkout = uri_for(&dir, CHECKOUT_PATH);
    let shop = uri_for(&dir, SHOP_SUBSCRIBER_PATH);

    let found = references_at(
        &backend,
        &checkout,
        position_in(CHECKOUT, "'shop.opened'", 2),
    )
    .await;
    let mut expected = vec![
        (checkout.clone(), line_of(CHECKOUT, "'shop.opened'")),
        (shop, line_of(SHOP_SUBSCRIBER, "ShopEvents::OPENED")),
    ];
    expected.sort();
    assert_eq!(found, expected);
}

#[tokio::test]
async fn a_listener_added_in_code_completes_but_declares_nothing() {
    let (backend, dir) = site(&[(ORDER_PATH, ORDER_SUBSCRIBER), (CHECKOUT_PATH, CHECKOUT)]).await;
    let checkout = uri_for(&dir, CHECKOUT_PATH);

    // Drupal records no event a call declares, so the name the call adds
    // a listener for neither navigates nor hovers.
    let position = position_in(CHECKOUT, "'acme.order_paid'", 2);
    let found = definitions_at(&backend, &checkout, position).await;
    assert!(found.is_empty(), "{found:?}");
    let hover = hover_text_at(&backend, &checkout, position.line, position.character).await;
    assert!(hover.is_none(), "{hover:?}");
}

#[tokio::test]
async fn event_names_complete_in_each_dispatcher_call() {
    let typing = CHECKOUT
        .replace(
            "'acme.order_placed');\n        $this->events",
            "'');\n        $this->events",
        )
        .replace("'shop.opened'", "''")
        .replace("'shop.closed'", "''")
        .replace("'acme.order_paid'", "''");
    let (backend, dir) = site(&[
        (ORDER_PATH, ORDER_SUBSCRIBER),
        (CHECKOUT_PATH, typing.as_str()),
    ])
    .await;
    let checkout = uri_for(&dir, CHECKOUT_PATH);
    let expected = vec![
        "acme.order_paid".to_string(),
        "acme.order_placed".to_string(),
        "shop.closed".to_string(),
        "shop.opened".to_string(),
    ];
    for needle in [
        "dispatch($order, '",
        "hasListeners('",
        "->dispatch($order, '');\n        $this->events->addListener",
        "addListener('",
    ] {
        let at = typing.find(needle).expect("needle should exist");
        let cursor = at + needle.find('\'').unwrap() + 1;
        let position = crate::common::offset_position(&typing, cursor);
        let labels = response_labels(
            completion_response(&backend, &checkout, position.line, position.character, None).await,
        );
        assert_eq!(labels, expected, "{needle}");
    }
}
