//! Yii event listeners: the method of an `[Handler::class, 'method']`
//! listener in an events file navigates, completes, renames, and is
//! checked, a listener whose parameters the injector cannot fill is
//! reported, and an event class goes to the listeners the project and the
//! installed packages attach to it.
use crate::common::{
    completion_response, edits_for_uri, initialize_with_options, message_text, offset_position,
    open_document, rename, response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": {
        "yiisoft/config": "^1.6",
        "yiisoft/yii-event": "^2.1"
    },
    "autoload": { "psr-4": { "App\\": "src/" } },
    "extra": { "config-plugin-file": "config/configuration.php" }
}"#;

const CONFIGURATION_PHP: &str = r#"<?php
return [
    'config-plugin' => [
        'events' => 'common/events.php',
        'events-web' => ['$events'],
    ],
    'config-plugin-options' => [
        'source-directory' => 'config',
    ],
];
"#;

const EVENT_PHP: &str = r#"<?php
namespace App\Event;

final class UserRegistered
{
}
"#;

const MAILER_PHP: &str = r#"<?php
namespace App\Mail;

use App\Event\UserRegistered;

final class WelcomeMailer
{
    public function send(UserRegistered $event): void {}

    public function greet(string $name): void {}

    public function maybe(?string $name, int $times = 1, object $event = null): void {}

    private function secret(UserRegistered $event): void {}
}
"#;

const EVENTS_PHP: &str = r#"<?php
use App\Event\UserRegistered;
use App\Mail\WelcomeMailer;

return [
    UserRegistered::class => [
        [WelcomeMailer::class, 'send'],
        [WelcomeMailer::class, 'sned'],
        [WelcomeMailer::class, 'secret'],
        [WelcomeMailer::class, 'greet'],
        [WelcomeMailer::class, 'maybe'],
        static fn (WelcomeMailer $mailer, UserRegistered $event) => null,
        static fn ($event) => null,
    ],
];
"#;

/// Not an events file, so its pairs may be data.
const OTHER_PHP: &str = r#"<?php
use App\Mail\WelcomeMailer;

return [[WelcomeMailer::class, 'nope']];
"#;

const INSTALLED_JSON: &str = r#"{
    "packages": [
        {
            "name": "acme/audit",
            "install-path": "../acme/audit",
            "extra": {
                "config-plugin-options": { "source-directory": "config" },
                "config-plugin": { "events-web": "events-web.php" }
            }
        }
    ]
}"#;

const PACKAGE_EVENTS_PHP: &str = r#"<?php
return [
    \App\Event\UserRegistered::class => [
        static function (\App\Event\UserRegistered $event): void {},
    ],
];
"#;

fn files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("composer.json", COMPOSER),
        ("config/configuration.php", CONFIGURATION_PHP),
        ("config/common/events.php", EVENTS_PHP),
        ("config/common/other.php", OTHER_PHP),
        ("src/Event/UserRegistered.php", EVENT_PHP),
        ("src/Mail/WelcomeMailer.php", MAILER_PHP),
        ("vendor/composer/installed.json", INSTALLED_JSON),
        ("vendor/acme/audit/composer.json", "{}"),
        (
            "vendor/acme/audit/config/events-web.php",
            PACKAGE_EVENTS_PHP,
        ),
    ]
}

/// A session over the workspace with its PHP files open.
async fn workspace() -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in files() {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, content).unwrap();
    }
    let backend = Backend::new_test();
    initialize_with_options(&backend, dir.path(), None).await;
    for (path, content) in files() {
        if path.ends_with(".php") && !path.starts_with("vendor/") {
            open_document(&backend, &uri_for(&dir, path), "php", content).await;
        }
    }
    (backend, dir)
}

fn uri_for(dir: &tempfile::TempDir, rel: &str) -> Uri {
    Uri::from_file_path(dunce::canonicalize(dir.path()).unwrap().join(rel)).unwrap()
}

/// The position `inside` bytes into the first occurrence of `needle`.
fn position_in(content: &str, needle: &str, inside: usize) -> Position {
    offset_position(
        content,
        content.find(needle).expect("needle should exist") + inside,
    )
}

fn text_document_position(uri: &Uri, position: Position) -> TextDocumentPositionParams {
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        position,
    }
}

/// The diagnostics with `code` the file at `rel` gets, as their messages
/// and the text they cover.
fn reported(
    backend: &Backend,
    dir: &tempfile::TempDir,
    rel: &str,
    content: &str,
    code: &str,
) -> Vec<(String, String)> {
    let uri = uri_for(dir, rel);
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), content, &mut diagnostics);
    let lines: Vec<&str> = content.lines().collect();
    diagnostics
        .iter()
        .filter(|diagnostic| matches!(&diagnostic.code, Some(Code::String(found)) if found == code))
        .map(|diagnostic| {
            let range = diagnostic.range;
            assert_eq!(range.start.line, range.end.line);
            let line = lines[range.start.line as usize];
            let text = &line[range.start.character as usize..range.end.character as usize];
            (
                message_text(&diagnostic.message).to_string(),
                text.to_string(),
            )
        })
        .collect()
}

#[tokio::test]
async fn a_listener_method_navigates_completes_and_renames() {
    let (backend, dir) = workspace().await;
    let events = uri_for(&dir, "config/common/events.php");
    let mailer = uri_for(&dir, "src/Mail/WelcomeMailer.php");

    let response = backend
        .goto_definition(DefinitionParams {
            text_document_position_params: text_document_position(
                &events,
                position_in(EVENTS_PHP, "'send'", 2),
            ),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
    let Some(DefinitionResponse::Definition(Definition::Location(location))) = response else {
        panic!("expected one location, got {response:?}");
    };
    assert_eq!(location.uri, mailer);
    assert_eq!(location.range.start, position_in(MAILER_PHP, "send(", 0));

    // The public methods complete; the private one cannot be called.
    let typing = EVENTS_PHP.replace("'sned'", "''");
    open_document(&backend, &events, "php", &typing).await;
    let at = position_in(&typing, "''", 1);
    let labels =
        response_labels(completion_response(&backend, &events, at.line, at.character, None).await);
    for public in ["send", "greet", "maybe"] {
        assert!(
            labels
                .iter()
                .any(|label| label.starts_with(&format!("{public}("))),
            "{labels:?}"
        );
    }
    assert!(
        !labels.iter().any(|label| label.starts_with("secret(")),
        "{labels:?}"
    );
    open_document(&backend, &events, "php", EVENTS_PHP).await;

    let at = position_in(MAILER_PHP, "send(", 1);
    let edit = rename(&backend, &mailer, at.line, at.character, "deliver")
        .await
        .expect("rename should produce an edit");
    let edits = edits_for_uri(&edit, &events);
    assert_eq!(edits.len(), 1, "{edits:?}");
    assert_eq!(edits[0].range.start, position_in(EVENTS_PHP, "'send'", 1));
    assert_eq!(edits[0].new_text, "deliver");
}

#[tokio::test]
async fn listeners_the_dispatcher_cannot_call_are_reported() {
    let (backend, dir) = workspace().await;

    let unknown = reported(
        &backend,
        &dir,
        "config/common/events.php",
        EVENTS_PHP,
        "unknown_member",
    );
    assert_eq!(unknown.len(), 1, "{unknown:?}");
    assert_eq!(unknown[0].1, "sned");

    let private = reported(
        &backend,
        &dir,
        "config/common/events.php",
        EVENTS_PHP,
        "inaccessible_member",
    );
    assert_eq!(private.len(), 1, "{private:?}");
    assert_eq!(private[0].1, "secret");

    let unfillable = reported(
        &backend,
        &dir,
        "config/common/events.php",
        EVENTS_PHP,
        "invalid_yii_event_listener",
    );
    assert_eq!(
        unfillable,
        vec![
            (
                "Parameter $name of App\\Mail\\WelcomeMailer::greet() cannot be filled when the event is dispatched: a listener's parameters take the event or a service by their class type".to_string(),
                "greet".to_string(),
            ),
            (
                "Parameter $event cannot be filled when the event is dispatched: a listener's parameters take the event or a service by their class type".to_string(),
                "$event".to_string(),
            ),
        ]
    );

    // Outside the events files a pair may be data.
    assert_eq!(
        reported(
            &backend,
            &dir,
            "config/common/other.php",
            OTHER_PHP,
            "unknown_member"
        ),
        Vec::new()
    );
}

#[tokio::test]
async fn an_event_goes_to_its_listeners() {
    let (backend, dir) = workspace().await;
    let event = uri_for(&dir, "src/Event/UserRegistered.php");
    let events = uri_for(&dir, "config/common/events.php");
    let package = uri_for(&dir, "vendor/acme/audit/config/events-web.php");

    let response = backend
        .goto_implementation(ImplementationParams {
            text_document_position_params: text_document_position(
                &event,
                position_in(EVENT_PHP, "UserRegistered", 2),
            ),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
    let locations = match response {
        Some(ImplementationResponse::Definition(Definition::LocationList(locations))) => locations,
        Some(ImplementationResponse::Definition(Definition::Location(location))) => vec![location],
        other => panic!("expected locations, got {other:?}"),
    };
    let found: Vec<(Uri, u32)> = locations
        .into_iter()
        .map(|location| (location.uri, location.range.start.line))
        .collect();
    let mut expected: Vec<(Uri, u32)> = EVENTS_PHP
        .lines()
        .enumerate()
        .filter(|(_, line)| line.trim_start().starts_with("[Welcome") || line.contains("fn ("))
        .map(|(line, _)| (events.clone(), line as u32))
        .collect();
    expected.push((
        package,
        position_in(PACKAGE_EVENTS_PHP, "static function", 0).line,
    ));
    assert_eq!(found, expected);

    let lenses = backend
        .handle_code_lens(event.as_str(), EVENT_PHP)
        .unwrap_or_default();
    let titles: Vec<String> = lenses
        .iter()
        .filter_map(|lens| lens.command.as_ref().map(|command| command.title.clone()))
        .collect();
    assert!(titles.contains(&"8 listeners".to_string()), "{titles:?}");
}
