//! Editing a service provider re-scans what it registers.
//!
//! The full provider scan runs once, at startup. These tests cover the
//! single-file refresh that keeps its table current while providers are
//! edited: a container binding written now resolves now, one deleted stops
//! resolving, and a key two providers bind ends up with whichever of them the
//! container would let win once the edit has landed.

use crate::common::{APP_HELPERS_PHP, consumer_class, create_psr4_workspace};
use phpantom_lsp::Backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "Illuminate\\Foundation\\": "vendor/illuminate/Foundation/",
            "Acme\\": "vendor/acme/"
        }
    }
}"#;

/// Both providers are listed by the application, so neither outranks the
/// other: the later registration wins the keys they share.
const PROVIDERS_PHP: &str = "<?php\nreturn [\n    App\\AppServiceProvider::class,\n    Acme\\AcmeServiceProvider::class,\n];\n";

const APP_PROVIDER: &str = r#"<?php
namespace App;

use App\Support\Clock;

class AppServiceProvider
{
    public function register(): void
    {
        $this->app->bind('clock', Clock::class);
    }
}
"#;

const ACME_PROVIDER: &str = r#"<?php
namespace Acme;

class AcmeServiceProvider
{
    public function register(): void
    {
        $this->app->bind('clock', Stopwatch::class);
    }
}
"#;

const CLOCK_PHP: &str = r#"<?php
namespace App\Support;
class Clock
{
    public function now(): string { return ''; }
}
"#;

const STOPWATCH_PHP: &str = r#"<?php
namespace Acme;
class Stopwatch
{
    public function elapsed(): int { return 0; }
}
"#;

const MAILER_PHP: &str = r#"<?php
namespace App\Support;
class Mailer
{
    public function send(string $to): bool { return true; }
}
"#;

const APPLICATION_PHP: &str = r#"<?php
namespace Illuminate\Foundation;
class Application
{
    /**
     * @template TClass
     * @param string|class-string<TClass> $abstract
     * @return ($abstract is class-string<TClass> ? TClass : mixed)
     */
    public function make($abstract, array $parameters = [])
    {
        return new $abstract();
    }
}
"#;

/// The consumer resolves `$x` from a container key, so its hover text names
/// whatever class the provider scan currently has behind that key.
fn consumer(key: &str) -> String {
    consumer_class(&format!("app('{key}')"))
}

fn base_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("bootstrap/providers.php", PROVIDERS_PHP),
        ("src/AppServiceProvider.php", APP_PROVIDER),
        ("src/Support/Clock.php", CLOCK_PHP),
        ("src/Support/Mailer.php", MAILER_PHP),
        ("src/helpers.php", APP_HELPERS_PHP),
        ("vendor/acme/AcmeServiceProvider.php", ACME_PROVIDER),
        ("vendor/acme/Stopwatch.php", STOPWATCH_PHP),
        (
            "vendor/illuminate/Foundation/Application.php",
            APPLICATION_PHP,
        ),
    ]
}

/// Build the workspace and run the startup scan the refresh has to keep
/// current.
async fn start(extra: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let mut files = base_files();
    files.extend_from_slice(extra);
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, &files);
    backend.initialized(InitializedParams {}).await;
    (backend, dir)
}

/// Edit a file the way an editor does: open it, then replace its contents.
async fn edit(backend: &Backend, path: std::path::PathBuf, before: &str, after: &str) {
    let uri = Url::from_file_path(&path).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: before.to_string(),
            },
        })
        .await;
    backend
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier { uri, version: 2 },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: after.to_string(),
            }],
        })
        .await;
}

/// Hover the `$x;` statement of a consumer that resolves `key`.
async fn hover_key(backend: &Backend, dir: &tempfile::TempDir, key: &str) -> String {
    let text = consumer(key);
    let path = dir.path().join("src/Consumer.php");
    std::fs::write(&path, &text).unwrap();
    let uri = Url::from_file_path(&path).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text.clone(),
            },
        })
        .await;

    let idx = text.rfind("$x;").expect("consumer should use $x");
    let prefix = &text[..idx + 1];
    let line = prefix.bytes().filter(|b| *b == b'\n').count() as u32;
    let character = prefix.rsplit('\n').next().unwrap().len() as u32 - 1;
    match backend.handle_hover(uri.as_str(), &text, Position { line, character }) {
        Some(hover) => match &hover.contents {
            HoverContents::Markup(markup) => markup.value.clone(),
            _ => panic!("expected MarkupContent"),
        },
        None => String::new(),
    }
}

/// A key no provider binds falls through to the helper's declared return
/// type, so "unbound" is the absence of the class rather than empty hover.
fn assert_unbound(text: &str) {
    assert!(
        !text.contains("Mailer"),
        "expected the key to be unbound before the edit, got: {text}"
    );
}

/// A binding added to a provider resolves without restarting the server.
#[tokio::test]
async fn a_binding_added_to_a_provider_resolves_after_the_edit() {
    let (backend, dir) = start(&[]).await;
    assert_unbound(&hover_key(&backend, &dir, "mailer").await);

    let edited = APP_PROVIDER.replace(
        "$this->app->bind('clock', Clock::class);",
        "$this->app->bind('clock', Clock::class);\n        $this->app->bind('mailer', Support\\Mailer::class);",
    );
    edit(
        &backend,
        dir.path().join("src/AppServiceProvider.php"),
        APP_PROVIDER,
        &edited,
    )
    .await;

    let text = hover_key(&backend, &dir, "mailer").await;
    assert!(
        text.contains("Mailer"),
        "expected the new 'mailer' binding to resolve to Mailer, got: {text}"
    );
}

/// Deleting the registration takes the key with it: the scan must not keep
/// serving a binding the provider no longer makes.
#[tokio::test]
async fn a_binding_removed_from_a_provider_stops_resolving() {
    let (backend, dir) = start(&[]).await;
    assert!(
        hover_key(&backend, &dir, "clock")
            .await
            .contains("Stopwatch")
    );

    let both_dropped = ACME_PROVIDER.replace("$this->app->bind('clock', Stopwatch::class);", "");
    edit(
        &backend,
        dir.path().join("vendor/acme/AcmeServiceProvider.php"),
        ACME_PROVIDER,
        &both_dropped,
    )
    .await;
    let app_dropped = APP_PROVIDER.replace("$this->app->bind('clock', Clock::class);", "");
    edit(
        &backend,
        dir.path().join("src/AppServiceProvider.php"),
        APP_PROVIDER,
        &app_dropped,
    )
    .await;

    let text = hover_key(&backend, &dir, "clock").await;
    assert!(
        !text.contains("Clock") && !text.contains("Stopwatch"),
        "expected the deleted 'clock' binding to resolve to nothing, got: {text}"
    );
}

/// The key both providers bind falls back to the other provider's binding
/// when the winner gives it up, which only a rebuilt merge can produce: the
/// losing registration was never in the published table to begin with.
#[tokio::test]
async fn the_other_provider_takes_over_a_key_the_edit_gave_up() {
    let (backend, dir) = start(&[]).await;
    let text = hover_key(&backend, &dir, "clock").await;
    assert!(
        text.contains("Stopwatch"),
        "expected the later-registered provider to win 'clock', got: {text}"
    );

    let edited = ACME_PROVIDER.replace("$this->app->bind('clock', Stopwatch::class);", "");
    edit(
        &backend,
        dir.path().join("vendor/acme/AcmeServiceProvider.php"),
        ACME_PROVIDER,
        &edited,
    )
    .await;

    let text = hover_key(&backend, &dir, "clock").await;
    assert!(
        text.contains("Clock") && !text.contains("Stopwatch"),
        "expected 'clock' to fall back to the app provider's binding, got: {text}"
    );
}

/// Registering another provider is an edit to the provider list, which
/// decides both which providers are scanned and in what order.
#[tokio::test]
async fn a_provider_added_to_the_list_is_scanned() {
    const LATE_PROVIDER: &str = r#"<?php
namespace App;

use App\Support\Mailer;

class LateServiceProvider
{
    public function register(): void
    {
        $this->app->bind('mailer', Mailer::class);
    }
}
"#;
    let (backend, dir) = start(&[("src/LateServiceProvider.php", LATE_PROVIDER)]).await;
    assert_unbound(&hover_key(&backend, &dir, "mailer").await);

    let edited = PROVIDERS_PHP.replace("];", "    App\\LateServiceProvider::class,\n];");
    edit(
        &backend,
        dir.path().join("bootstrap/providers.php"),
        PROVIDERS_PHP,
        &edited,
    )
    .await;

    let text = hover_key(&backend, &dir, "mailer").await;
    assert!(
        text.contains("Mailer"),
        "expected the newly listed provider's binding to resolve, got: {text}"
    );
}

/// A provider can be listed before it is written, in which case the startup
/// scan has no file to read.  Writing it is what makes it scannable.
#[tokio::test]
async fn a_provider_written_after_the_list_names_it_is_picked_up() {
    const LISTED_FIRST: &str = "<?php\nreturn [\n    App\\AppServiceProvider::class,\n    App\\UnwrittenServiceProvider::class,\n];\n";
    const UNWRITTEN_PROVIDER: &str = r#"<?php
namespace App;

use App\Support\Mailer;

class UnwrittenServiceProvider
{
    public function register(): void
    {
        $this->app->bind('mailer', Mailer::class);
    }
}
"#;
    let (backend, dir) = start(&[]).await;
    std::fs::write(dir.path().join("bootstrap/providers.php"), LISTED_FIRST).unwrap();
    edit(
        &backend,
        dir.path().join("bootstrap/providers.php"),
        PROVIDERS_PHP,
        LISTED_FIRST,
    )
    .await;
    assert_unbound(&hover_key(&backend, &dir, "mailer").await);

    let path = dir.path().join("src/UnwrittenServiceProvider.php");
    std::fs::write(&path, UNWRITTEN_PROVIDER).unwrap();
    edit(&backend, path, "<?php\n", UNWRITTEN_PROVIDER).await;

    let text = hover_key(&backend, &dir, "mailer").await;
    assert!(
        text.contains("Mailer"),
        "expected the newly written provider's binding to resolve, got: {text}"
    );
}
