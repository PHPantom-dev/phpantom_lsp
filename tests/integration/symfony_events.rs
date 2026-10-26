//! Symfony event names, from the listeners that subscribe to them to the
//! dispatchers that dispatch them, and Messenger buses and handlers.

use crate::common::message_text;
use crate::common::{
    completion_response, create_psr4_workspace, offset_position, open_document, response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": { "symfony/framework-bundle": "^7.3" },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Symfony\\Component\\EventDispatcher\\": "stubs/event-dispatcher/",
        "Symfony\\Contracts\\EventDispatcher\\": "stubs/event-dispatcher-contracts/",
        "Symfony\\Component\\Messenger\\": "stubs/messenger/"
    } }
}"#;

/// Just enough of the event dispatcher and Messenger for the type engine
/// to tell a dispatcher from any other receiver.
const STUBS: &[(&str, &str)] = &[
    (
        "stubs/event-dispatcher-contracts/EventDispatcherInterface.php",
        "<?php\nnamespace Symfony\\Contracts\\EventDispatcher;\ninterface EventDispatcherInterface {\n    public function dispatch(object $event, ?string $eventName = null): object;\n}\n",
    ),
    (
        "stubs/event-dispatcher/EventDispatcherInterface.php",
        "<?php\nnamespace Symfony\\Component\\EventDispatcher;\ninterface EventDispatcherInterface extends \\Symfony\\Contracts\\EventDispatcher\\EventDispatcherInterface {\n    public function addListener(string $eventName, callable $listener, int $priority = 0): void;\n    public function hasListeners(?string $eventName = null): bool;\n}\n",
    ),
    (
        "stubs/event-dispatcher/Attribute/AsEventListener.php",
        "<?php\nnamespace Symfony\\Component\\EventDispatcher\\Attribute;\n#[\\Attribute(\\Attribute::TARGET_CLASS | \\Attribute::TARGET_METHOD | \\Attribute::IS_REPEATABLE)]\nclass AsEventListener {\n    public function __construct(?string $event = null, ?string $method = null, int $priority = 0, ?string $dispatcher = null) {}\n}\n",
    ),
    (
        "stubs/messenger/Attribute/AsMessageHandler.php",
        "<?php\nnamespace Symfony\\Component\\Messenger\\Attribute;\n#[\\Attribute(\\Attribute::TARGET_CLASS | \\Attribute::TARGET_METHOD)]\nclass AsMessageHandler {\n    public function __construct(?string $bus = null, ?string $fromTransport = null, ?string $handles = null, ?string $method = null, int $priority = 0) {}\n}\n",
    ),
    (
        "stubs/messenger/Stamp/BusNameStamp.php",
        "<?php\nnamespace Symfony\\Component\\Messenger\\Stamp;\nfinal class BusNameStamp {\n    public function __construct(private string $busName) {}\n}\n",
    ),
];

const LISTENER_PHP: &str = r#"<?php
namespace App\EventListener;

use Symfony\Component\EventDispatcher\Attribute\AsEventListener;

#[AsEventListener(event: 'app.order.placed', method: 'onPlaced')]
final class OrderListener
{
    public function onPlaced(object $event): void {}

    #[AsEventListener(event: 'app.order.shipped')]
    public function onShipped(object $event): void {}
}
"#;

const SUBSCRIBER_PHP: &str = r#"<?php
namespace App\EventSubscriber;

final class OrderSubscriber
{
    public static function getSubscribedEvents(): array
    {
        return ['app.order.placed' => ['onPlaced', 10]];
    }

    public function onPlaced(object $event): void {}
}
"#;

const SERVICES_YAML: &str = "services:
    App\\EventListener\\AuditListener:
        tags:
            - { name: kernel.event_listener, event: app.order.placed, method: audit }
";

const DISPATCHER_PHP: &str = r#"<?php
namespace App\Service;

use Symfony\Component\EventDispatcher\EventDispatcherInterface;

final class Checkout
{
    public function __construct(private EventDispatcherInterface $events) {}

    public function place(object $order, \ArrayObject $other): void
    {
        $this->events->dispatch($order, 'app.order.placed');
        $this->events->hasListeners('app.order.shipped');
        $other->offsetSet($order, 'app.order.placed');
    }
}
"#;

const MESSENGER_YAML: &str = "framework:
    messenger:
        default_bus: command.bus
        buses:
            command.bus: ~
            query.bus: ~
";

const MESSAGE_PHP: &str = r#"<?php
namespace App\Message;

final class PlaceOrder {}
"#;

const HANDLER_PHP: &str = r#"<?php
namespace App\MessageHandler;

use App\Message\PlaceOrder;
use Symfony\Component\Messenger\Attribute\AsMessageHandler;
use Symfony\Component\Messenger\Stamp\BusNameStamp;

#[AsMessageHandler(bus: 'command.bus')]
final class PlaceOrderHandler
{
    public function __invoke(PlaceOrder $message): void
    {
        new BusNameStamp('query.bus');
        new BusNameStamp('event.bus');
    }
}
"#;

/// A workspace holding the stubs and `files`, every one of them open.
async fn workspace(files: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let all: Vec<(&str, &str)> = STUBS.iter().chain(files).copied().collect();
    let (backend, dir) = create_psr4_workspace(COMPOSER, &all);
    for (path, content) in files {
        let language = if path.ends_with(".php") {
            "php"
        } else {
            "yaml"
        };
        open_document(&backend, &uri_for(&dir, path), language, content).await;
    }
    (backend, dir)
}

fn uri_for(dir: &tempfile::TempDir, rel: &str) -> Uri {
    Uri::from_file_path(dir.path().join(rel)).unwrap()
}

/// The position `inside` bytes into the first occurrence of `needle`.
fn position_in(content: &str, needle: &str, inside: usize) -> Position {
    offset_position(
        content,
        content.find(needle).expect("needle should exist") + inside,
    )
}

async fn definitions(backend: &Backend, uri: &Uri, position: Position) -> Vec<(Uri, u32)> {
    let response = backend
        .goto_definition(DefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
    let mut locations: Vec<(Uri, u32)> = match response {
        None => Vec::new(),
        Some(DefinitionResponse::Definition(Definition::Location(location))) => vec![location],
        Some(DefinitionResponse::Definition(Definition::LocationList(locations))) => locations,
        Some(DefinitionResponse::DefinitionLinkList(_)) => panic!("unexpected location links"),
    }
    .into_iter()
    .map(|location| (location.uri, location.range.start.line))
    .collect();
    locations.sort();
    locations
}

async fn references(backend: &Backend, uri: &Uri, position: Position) -> Vec<(Uri, u32)> {
    let mut found: Vec<(Uri, u32)> = backend
        .references(ReferenceParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await
        .unwrap()
        .unwrap_or_default()
        .into_iter()
        .map(|location| (location.uri, location.range.start.line))
        .collect();
    found.sort();
    found
}

fn sorted(mut locations: Vec<(Uri, u32)>) -> Vec<(Uri, u32)> {
    locations.sort();
    locations
}

async fn completion_labels(backend: &Backend, uri: &Uri, position: Position) -> Vec<String> {
    response_labels(
        completion_response(backend, uri, position.line, position.character, None).await,
    )
}

#[tokio::test]
async fn a_dispatched_event_goes_to_every_listener_that_subscribes_to_it() {
    let (backend, dir) = workspace(&[
        ("src/EventListener/OrderListener.php", LISTENER_PHP),
        ("src/EventSubscriber/OrderSubscriber.php", SUBSCRIBER_PHP),
        ("config/services.yaml", SERVICES_YAML),
        ("src/Service/Checkout.php", DISPATCHER_PHP),
    ])
    .await;
    let listener = uri_for(&dir, "src/EventListener/OrderListener.php");
    let subscriber = uri_for(&dir, "src/EventSubscriber/OrderSubscriber.php");
    let yaml = uri_for(&dir, "config/services.yaml");
    let checkout = uri_for(&dir, "src/Service/Checkout.php");

    let found = definitions(
        &backend,
        &checkout,
        position_in(DISPATCHER_PHP, "'app.order.placed'", 3),
    )
    .await;
    assert_eq!(
        found,
        sorted(vec![
            (listener.clone(), 5),
            (subscriber.clone(), 7),
            (yaml.clone(), 3)
        ])
    );

    let found = definitions(
        &backend,
        &checkout,
        position_in(DISPATCHER_PHP, "'app.order.shipped'", 3),
    )
    .await;
    assert_eq!(found, vec![(listener.clone(), 10)]);

    // `offsetSet()` on something that is not a dispatcher names nothing.
    let found = definitions(
        &backend,
        &checkout,
        position_in(DISPATCHER_PHP, "$order, 'app.order.placed');\n    }", 10),
    )
    .await;
    assert!(found.is_empty(), "{found:?}");

    let found = references(
        &backend,
        &yaml,
        position_in(SERVICES_YAML, "app.order.placed", 2),
    )
    .await;
    assert_eq!(
        found,
        sorted(vec![
            (checkout.clone(), 11),
            (listener.clone(), 5),
            (subscriber.clone(), 7),
            (yaml.clone(), 3),
        ])
    );
}

#[tokio::test]
async fn a_listener_method_goes_to_the_method_and_an_unknown_one_is_reported() {
    let broken = LISTENER_PHP.replace("method: 'onPlaced'", "method: 'onMissing'");
    let (backend, dir) = workspace(&[
        ("src/EventListener/OrderListener.php", LISTENER_PHP),
        ("src/EventSubscriber/OrderSubscriber.php", SUBSCRIBER_PHP),
        ("src/EventListener/BrokenListener.php", &broken),
    ])
    .await;
    let listener = uri_for(&dir, "src/EventListener/OrderListener.php");
    let subscriber = uri_for(&dir, "src/EventSubscriber/OrderSubscriber.php");

    let found = definitions(
        &backend,
        &listener,
        position_in(LISTENER_PHP, "'onPlaced'", 3),
    )
    .await;
    assert_eq!(found, vec![(listener.clone(), 8)]);
    let found = definitions(
        &backend,
        &subscriber,
        position_in(SUBSCRIBER_PHP, "'onPlaced'", 3),
    )
    .await;
    assert_eq!(found, vec![(subscriber.clone(), 10)]);

    let broken_uri = uri_for(&dir, "src/EventListener/BrokenListener.php");
    let mut diagnostics = Vec::new();
    backend.collect_unknown_member_diagnostics(broken_uri.as_str(), &broken, &mut diagnostics);
    let reported: Vec<&str> = diagnostics
        .iter()
        .map(|diagnostic| message_text(&diagnostic.message))
        .collect();
    assert_eq!(reported.len(), 1, "{reported:?}");
    assert!(reported[0].contains("onMissing"), "{reported:?}");
}

#[tokio::test]
async fn event_names_complete_where_listeners_and_dispatchers_take_them() {
    let subscription = r#"<?php
namespace App\Service;

use Symfony\Component\EventDispatcher\Attribute\AsEventListener;

#[AsEventListener(event: 'app.')]
final class Subscription {}
"#;
    let dispatch = r#"<?php
namespace App\Service;

use Symfony\Contracts\EventDispatcher\EventDispatcherInterface;

function run(EventDispatcherInterface $events, object $event): void
{
    $events->dispatch($event, 'app.');
}
"#;
    let tag = "services:
    App\\Listener:
        tags:
            - { name: kernel.event_listener, event: app. }
";
    let expected = vec![
        "app.order.placed".to_string(),
        "app.order.shipped".to_string(),
    ];

    // Each in a workspace of its own: a subscription being typed declares
    // the name typed so far, which every other file would be offered.
    for (path, content, needle) in [
        ("src/Service/Subscription.php", subscription, "event: 'app."),
        ("src/Service/dispatch.php", dispatch, "$event, 'app."),
        ("config/services.yaml", tag, "event: app."),
    ] {
        let (backend, dir) = workspace(&[
            ("src/EventListener/OrderListener.php", LISTENER_PHP),
            (path, content),
        ])
        .await;
        let labels = completion_labels(
            &backend,
            &uri_for(&dir, path),
            position_in(content, needle, needle.len()),
        )
        .await;
        assert_eq!(labels, expected, "in {path}");
    }
}

#[tokio::test]
async fn buses_navigate_complete_and_report_unknown_names() {
    let consumer = r#"<?php
namespace App\MessageHandler;

use Symfony\Component\Messenger\Attribute\AsMessageHandler;

#[AsMessageHandler(bus: '')]
final class Other
{
    public function __invoke(object $message): void {}
}
"#;
    let (backend, dir) = workspace(&[
        ("config/packages/messenger.yaml", MESSENGER_YAML),
        ("src/Message/PlaceOrder.php", MESSAGE_PHP),
        ("src/MessageHandler/PlaceOrderHandler.php", HANDLER_PHP),
        ("src/MessageHandler/Other.php", consumer),
    ])
    .await;
    let yaml = uri_for(&dir, "config/packages/messenger.yaml");
    let handler = uri_for(&dir, "src/MessageHandler/PlaceOrderHandler.php");
    let consumer_uri = uri_for(&dir, "src/MessageHandler/Other.php");

    let found = definitions(
        &backend,
        &handler,
        position_in(HANDLER_PHP, "'command.bus'", 3),
    )
    .await;
    assert_eq!(found, vec![(yaml.clone(), 4)]);
    let found = definitions(
        &backend,
        &handler,
        position_in(HANDLER_PHP, "'query.bus'", 3),
    )
    .await;
    assert_eq!(found, vec![(yaml.clone(), 5)]);

    let found = references(
        &backend,
        &yaml,
        position_in(MESSENGER_YAML, "command.bus: ~", 2),
    )
    .await;
    assert_eq!(
        found,
        sorted(vec![
            (handler.clone(), 7),
            (yaml.clone(), 2),
            (yaml.clone(), 4)
        ])
    );

    let labels =
        completion_labels(&backend, &consumer_uri, position_in(consumer, "bus: ''", 6)).await;
    assert_eq!(
        labels,
        vec!["command.bus".to_string(), "query.bus".to_string()]
    );

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(handler.as_str(), HANDLER_PHP, &mut diagnostics);
    let reported: Vec<String> = diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                &diagnostic.code,
                Some(Code::String(code)) if code == "invalid_symfony_messenger_bus"
            )
        })
        .map(|diagnostic| message_text(&diagnostic.message).to_string())
        .collect();
    assert_eq!(
        reported,
        vec!["Unknown Messenger bus: 'event.bus'".to_string()]
    );
}

#[tokio::test]
async fn code_lenses_link_a_message_and_its_handler() {
    let (backend, dir) = workspace(&[
        ("src/Message/PlaceOrder.php", MESSAGE_PHP),
        ("src/MessageHandler/PlaceOrderHandler.php", HANDLER_PHP),
    ])
    .await;
    let message = uri_for(&dir, "src/Message/PlaceOrder.php");
    let handler = uri_for(&dir, "src/MessageHandler/PlaceOrderHandler.php");

    let lens = |uri: &Uri, content: &str, prefix: &str| {
        backend
            .handle_code_lens(uri.as_str(), content)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|lens| {
                let command = lens.command?;
                command
                    .title
                    .starts_with(prefix)
                    .then_some((lens.range.start.line, command.title))
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        lens(&message, MESSAGE_PHP, "Messenger"),
        vec![(3, "Messenger handler: PlaceOrderHandler".to_string())]
    );
    assert_eq!(
        lens(&handler, HANDLER_PHP, "Messenger"),
        vec![(10, "Messenger message: PlaceOrder".to_string())]
    );
}
