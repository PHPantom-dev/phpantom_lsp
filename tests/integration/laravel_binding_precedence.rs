//! Tests that a container key two service providers bind resolves to the class
//! the container would actually hold.
//!
//! Swapping an implementation out is written by re-binding the key from an
//! application provider, usually one that subclasses the provider it replaces.
//! Both providers are scanned, so the key needs the same precedence the
//! container applies: the framework's default loses to the application's
//! registration, and a provider loses to one that extends it, regardless of
//! which of them is scanned first.

use crate::common::{APP_HELPERS_PHP, consumer_class, create_psr4_workspace};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "Illuminate\\": "vendor/illuminate/"
        }
    }
}"#;

/// The application's replacement is listed *before* the provider it replaces,
/// so scan order alone would hand the key back to the framework.
const APPLICATION_REPLACES_FRAMEWORK: &str = "<?php\nreturn [\n    App\\Providers\\TranslationServiceProvider::class,\n    Illuminate\\Translation\\TranslationServiceProvider::class,\n];\n";

/// Only the framework's own provider, which must keep the key.
const FRAMEWORK_ONLY: &str =
    "<?php\nreturn [\n    Illuminate\\Translation\\TranslationServiceProvider::class,\n];\n";

/// Two providers the application lists itself, the subclass first.
const SUBCLASS_BEFORE_PARENT: &str = "<?php\nreturn [\n    App\\Providers\\TracingClientServiceProvider::class,\n    App\\Providers\\ClientServiceProvider::class,\n];\n";

const FRAMEWORK_PROVIDER: &str = r#"<?php
namespace Illuminate\Translation;

class TranslationServiceProvider
{
    public function register(): void
    {
        $this->app->singleton('translator', function ($app) {
            return new Translator($app);
        });
    }
}
"#;

const FRAMEWORK_TRANSLATOR: &str = r#"<?php
namespace Illuminate\Translation;
class Translator
{
    public function get(string $key): string { return ''; }
}
"#;

const APPLICATION_PROVIDER: &str = r#"<?php
namespace App\Providers;

use App\Translation\DatabaseTranslator;
use Illuminate\Translation\TranslationServiceProvider as BaseProvider;

class TranslationServiceProvider extends BaseProvider
{
    public function register(): void
    {
        $this->app->singleton('translator', function ($app) {
            return new DatabaseTranslator($app);
        });
    }
}
"#;

const DATABASE_TRANSLATOR: &str = r#"<?php
namespace App\Translation;
class DatabaseTranslator
{
    public function get(string $key): string { return ''; }
}
"#;

const CLIENT_PROVIDER: &str = r#"<?php
namespace App\Providers;

use App\Support\Client;

class ClientServiceProvider
{
    public function register(): void
    {
        $this->app->singleton('acme.client', fn () => new Client());
    }
}
"#;

const TRACING_CLIENT_PROVIDER: &str = r#"<?php
namespace App\Providers;

use App\Support\TracingClient;

class TracingClientServiceProvider extends ClientServiceProvider
{
    public function register(): void
    {
        $this->app->singleton('acme.client', fn () => new TracingClient());
    }
}
"#;

const CLIENT: &str = r#"<?php
namespace App\Support;
class Client
{
    public function send(string $body): bool { return true; }
}
"#;

const TRACING_CLIENT: &str = r#"<?php
namespace App\Support;
class TracingClient
{
    public function trace(): string { return ''; }
}
"#;

/// The container, whose core alias table already names the framework's
/// translator: that entry is what an application binding has to beat.
const APPLICATION_PHP: &str = r#"<?php
namespace Illuminate\Foundation;
class Application
{
    public function registerCoreContainerAliases()
    {
        foreach ([
            'app' => [self::class],
            'translator' => [\Illuminate\Translation\Translator::class],
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

fn base_files(providers: &'static str) -> Vec<(&'static str, &'static str)> {
    vec![
        ("bootstrap/providers.php", providers),
        ("src/helpers.php", APP_HELPERS_PHP),
        (
            "src/Providers/TranslationServiceProvider.php",
            APPLICATION_PROVIDER,
        ),
        ("src/Providers/ClientServiceProvider.php", CLIENT_PROVIDER),
        (
            "src/Providers/TracingClientServiceProvider.php",
            TRACING_CLIENT_PROVIDER,
        ),
        ("src/Support/Client.php", CLIENT),
        ("src/Support/TracingClient.php", TRACING_CLIENT),
        (
            "src/Translation/DatabaseTranslator.php",
            DATABASE_TRANSLATOR,
        ),
        (
            "vendor/illuminate/Translation/TranslationServiceProvider.php",
            FRAMEWORK_PROVIDER,
        ),
        (
            "vendor/illuminate/Translation/Translator.php",
            FRAMEWORK_TRANSLATOR,
        ),
        (
            "vendor/illuminate/Foundation/Application.php",
            APPLICATION_PHP,
        ),
    ]
}

/// Open a consumer that assigns `$x` and hover the following `$x;` statement,
/// returning the hover text.
async fn hover_over_x(providers: &'static str, consumer: &str) -> String {
    let mut files = base_files(providers);
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

#[tokio::test]
async fn an_application_provider_replaces_a_framework_binding() {
    let text = hover_over_x(
        APPLICATION_REPLACES_FRAMEWORK,
        &consumer_class("app()->make('translator')"),
    )
    .await;
    assert!(
        text.contains("DatabaseTranslator"),
        "expected the application's 'translator' binding to win, got: {text}"
    );
}

#[tokio::test]
async fn the_app_helper_reaches_the_replaced_binding_too() {
    let text = hover_over_x(
        APPLICATION_REPLACES_FRAMEWORK,
        &consumer_class("app('translator')"),
    )
    .await;
    assert!(
        text.contains("DatabaseTranslator"),
        "expected app('translator') to resolve to the replacement, got: {text}"
    );
}

/// Nothing replaces the framework's binding, so it keeps the key.
#[tokio::test]
async fn a_framework_binding_stands_when_nothing_replaces_it() {
    let text = hover_over_x(FRAMEWORK_ONLY, &consumer_class("app()->make('translator')")).await;
    assert!(
        text.contains("Translator") && !text.contains("DatabaseTranslator"),
        "expected the framework's Translator, got: {text}"
    );
}

/// Two providers the application registers itself, so only the `extends`
/// relationship between them says which one the container ends up with.
#[tokio::test]
async fn a_subclass_provider_replaces_its_parents_binding() {
    let text = hover_over_x(
        SUBCLASS_BEFORE_PARENT,
        &consumer_class("app('acme.client')"),
    )
    .await;
    assert!(
        text.contains("TracingClient"),
        "expected the subclass provider's binding to win, got: {text}"
    );
}
