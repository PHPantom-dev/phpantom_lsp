//! Tests that a container binding registered under a *string* key by a
//! service provider resolves to the class it binds.
//!
//! `$this->app->singleton('sentry', fn () => new HubAdapter())` is the only
//! record that `'sentry'` names a class at all: nothing in the string itself
//! points at `HubAdapter`. The provider scan indexes those keys so both the
//! `app('sentry')` helper form and the container's own
//! `app()->make('sentry')` resolve to the bound class.

use crate::common::create_psr4_workspace;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "Illuminate\\Foundation\\": "vendor/illuminate/Foundation/",
            "Sentry\\": "vendor/sentry/"
        }
    }
}"#;

const PROVIDERS_PHP: &str = "<?php\nreturn [\n    App\\AppServiceProvider::class,\n];\n";

/// The three binding shapes a provider writes: a factory closure, a bare
/// `::class`, and a ready-made instance.
const APP_SERVICE_PROVIDER: &str = r#"<?php
namespace App;

use App\Support\Clock;
use Sentry\HubAdapter;

class AppServiceProvider
{
    public function register(): void
    {
        $this->app->singleton('sentry', fn () => new HubAdapter());
        $this->app->bind('clock', Clock::class);
        $this->app->instance('flags', new Support\FeatureFlags());
    }
}
"#;

const HUB_ADAPTER_PHP: &str = r#"<?php
namespace Sentry;
class HubAdapter
{
    public function captureException($exception): string { return ''; }
}
"#;

const CLOCK_PHP: &str = r#"<?php
namespace App\Support;
class Clock
{
    public function now(): string { return ''; }
}
"#;

const FEATURE_FLAGS_PHP: &str = r#"<?php
namespace App\Support;
class FeatureFlags
{
    public function enabled(string $name): bool { return false; }
}
"#;

/// The container, with the argument-dependent return Laravel declares on
/// `make()`.
const APPLICATION_PHP: &str = r#"<?php
namespace Illuminate\Foundation;
class Application
{
    public function registerCoreContainerAliases()
    {
        foreach ([
            'app' => [self::class],
        ] as $key => $aliases) {
            foreach ($aliases as $alias) {
                $this->alias($key, $alias);
            }
        }
    }

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

const HELPERS_PHP: &str = r#"<?php
/**
 * @template TClass
 * @param string|class-string<TClass> $abstract
 * @return ($abstract is class-string<TClass> ? TClass : \Illuminate\Foundation\Application)
 */
function app($abstract = null, array $parameters = [])
{
}
"#;

fn base_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("bootstrap/providers.php", PROVIDERS_PHP),
        ("src/AppServiceProvider.php", APP_SERVICE_PROVIDER),
        ("src/Support/Clock.php", CLOCK_PHP),
        ("src/Support/FeatureFlags.php", FEATURE_FLAGS_PHP),
        ("src/helpers.php", HELPERS_PHP),
        ("vendor/sentry/HubAdapter.php", HUB_ADAPTER_PHP),
        (
            "vendor/illuminate/Foundation/Application.php",
            APPLICATION_PHP,
        ),
    ]
}

/// Open a consumer that assigns `$x` and hover the following `$x;` statement,
/// returning the hover text.
async fn hover_over_x(consumer: &str) -> String {
    let mut files = base_files();
    files.push(("src/Consumer.php", consumer));
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, &files);
    let uri = Url::from_file_path(dir.path().join("src/Consumer.php")).unwrap();

    backend.initialized(InitializedParams {}).await;
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: consumer.to_string(),
            },
        })
        .await;

    let idx = consumer.rfind("$x;").expect("consumer should use $x");
    let prefix = &consumer[..idx + 1];
    let line = prefix.bytes().filter(|b| *b == b'\n').count() as u32;
    let character = prefix.rsplit('\n').next().unwrap().len() as u32 - 1;
    let hover = backend
        .handle_hover(uri.as_str(), consumer, Position { line, character })
        .expect("hover should resolve");
    match &hover.contents {
        HoverContents::Markup(markup) => markup.value.clone(),
        _ => panic!("expected MarkupContent"),
    }
}

fn consumer(body: &str) -> String {
    format!(
        "<?php\nnamespace App;\nclass Consumer {{\n    public function go(): void {{\n        $x = {body};\n        $x;\n    }}\n}}\n"
    )
}

#[tokio::test]
async fn make_resolves_a_closure_bound_string_key() {
    let text = hover_over_x(&consumer("app()->make('sentry')")).await;
    assert!(
        text.contains("HubAdapter"),
        "expected the 'sentry' binding to resolve to HubAdapter, got: {text}"
    );
}

#[tokio::test]
async fn make_resolves_a_class_string_bound_key() {
    let text = hover_over_x(&consumer("app()->make('clock')")).await;
    assert!(
        text.contains("Clock"),
        "expected the 'clock' binding to resolve to Clock, got: {text}"
    );
}

#[tokio::test]
async fn make_resolves_an_instance_bound_key() {
    let text = hover_over_x(&consumer("app()->make('flags')")).await;
    assert!(
        text.contains("FeatureFlags"),
        "expected the 'flags' binding to resolve to FeatureFlags, got: {text}"
    );
}

/// The `app('sentry')` helper form reaches the same table.
#[tokio::test]
async fn app_helper_resolves_a_provider_bound_string_key() {
    let text = hover_over_x(&consumer("app('sentry')")).await;
    assert!(
        text.contains("HubAdapter"),
        "expected app('sentry') to resolve to HubAdapter, got: {text}"
    );
}

/// A key nothing binds stays unresolved rather than being read as the name of
/// a class that does not exist.
#[tokio::test]
async fn an_unbound_string_key_stays_unresolved() {
    let text = hover_over_x(&consumer("app()->make('nothing.binds.this')")).await;
    assert!(
        !text.contains("nothing.binds.this"),
        "an unbound key must not be reported as a class, got: {text}"
    );
}
