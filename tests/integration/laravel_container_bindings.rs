//! Tests that a container binding registered under a *string* key by a
//! service provider resolves to the class it binds.
//!
//! `$this->app->singleton('sentry', fn () => new HubAdapter())` is the only
//! record that `'sentry'` names a class at all: nothing in the string itself
//! points at `HubAdapter`. The provider scan indexes those keys so both the
//! `app('sentry')` helper form and the container's own
//! `app()->make('sentry')` resolve to the bound class.

use crate::common::{APP_HELPERS_PHP, consumer_class, create_psr4_workspace, position_of};
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

const PROVIDERS_PHP: &str = "<?php\nreturn [\n    App\\AppServiceProvider::class,\n    Sentry\\Laravel\\ServiceProvider::class,\n];\n";

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

const HUB_INTERFACE_PHP: &str = r#"<?php
namespace Sentry\State;
interface HubInterface
{
    public function getLastEventId(): string;
}
"#;

const SENTRY_CLIENT_PHP: &str = r#"<?php
namespace Sentry;
class Client
{
    public function getOptions(): array { return []; }
}
"#;

/// A package that declares its container key on a base provider and binds
/// under `static::$abstract` from the subclass the application registers.
const SENTRY_BASE_PROVIDER_PHP: &str = r#"<?php
namespace Sentry\Laravel;

abstract class BaseServiceProvider
{
    public static $abstract = 'sentry.hub';
}
"#;

const SENTRY_PROVIDER_PHP: &str = r#"<?php
namespace Sentry\Laravel;

use Sentry\Client;
use Sentry\State\HubInterface;

class ServiceProvider extends BaseServiceProvider
{
    public function register(): void
    {
        $this->app->alias(HubInterface::class, static::$abstract);
        $this->app->singleton(static::$abstract . '.client', fn () => new Client());
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

fn base_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("bootstrap/providers.php", PROVIDERS_PHP),
        ("src/AppServiceProvider.php", APP_SERVICE_PROVIDER),
        ("src/Support/Clock.php", CLOCK_PHP),
        ("src/Support/FeatureFlags.php", FEATURE_FLAGS_PHP),
        ("src/helpers.php", APP_HELPERS_PHP),
        ("vendor/sentry/HubAdapter.php", HUB_ADAPTER_PHP),
        ("vendor/sentry/Client.php", SENTRY_CLIENT_PHP),
        ("vendor/sentry/State/HubInterface.php", HUB_INTERFACE_PHP),
        (
            "vendor/sentry/Laravel/BaseServiceProvider.php",
            SENTRY_BASE_PROVIDER_PHP,
        ),
        (
            "vendor/sentry/Laravel/ServiceProvider.php",
            SENTRY_PROVIDER_PHP,
        ),
        (
            "vendor/illuminate/Foundation/Application.php",
            APPLICATION_PHP,
        ),
    ]
}

/// Open a consumer that assigns `$x` and hover the following `$x;` statement,
/// returning the hover text.
async fn hover_over_x(consumer: &str) -> String {
    hover_over_x_with_extra_files(&[], consumer).await
}

/// Like [`hover_over_x`], but with additional project files alongside the
/// standard fixture set.
async fn hover_over_x_with_extra_files(extra: &[(&str, &str)], consumer: &str) -> String {
    let mut files = base_files();
    files.extend_from_slice(extra);
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

/// Open a consumer and hover the container key inside `needle`, returning the
/// hover text for the string literal itself rather than for the value it
/// produces.
async fn hover_over_key(extra: &[(&str, &str)], consumer: &str, key: &str) -> String {
    let mut files = base_files();
    files.extend_from_slice(extra);
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

    let position = position_of(consumer, key);
    let hover = backend
        .handle_hover(uri.as_str(), consumer, position)
        .expect("hover should resolve");
    match &hover.contents {
        HoverContents::Markup(markup) => markup.value.clone(),
        _ => panic!("expected MarkupContent"),
    }
}

#[tokio::test]
async fn make_resolves_a_closure_bound_string_key() {
    let text = hover_over_x(&consumer_class("app()->make('sentry')")).await;
    assert!(
        text.contains("HubAdapter"),
        "expected the 'sentry' binding to resolve to HubAdapter, got: {text}"
    );
}

#[tokio::test]
async fn make_resolves_a_class_string_bound_key() {
    let text = hover_over_x(&consumer_class("app()->make('clock')")).await;
    assert!(
        text.contains("Clock"),
        "expected the 'clock' binding to resolve to Clock, got: {text}"
    );
}

#[tokio::test]
async fn make_resolves_an_instance_bound_key() {
    let text = hover_over_x(&consumer_class("app()->make('flags')")).await;
    assert!(
        text.contains("FeatureFlags"),
        "expected the 'flags' binding to resolve to FeatureFlags, got: {text}"
    );
}

/// The `app('sentry')` helper form reaches the same table.
#[tokio::test]
async fn app_helper_resolves_a_provider_bound_string_key() {
    let text = hover_over_x(&consumer_class("app('sentry')")).await;
    assert!(
        text.contains("HubAdapter"),
        "expected app('sentry') to resolve to HubAdapter, got: {text}"
    );
}

/// `alias(Contract::class, 'key')` is the other way a provider gives a class a
/// string name, and the key it uses is not always a literal: Sentry keeps it in
/// a static property on the base provider its subclass extends.
#[tokio::test]
async fn alias_resolves_a_key_named_by_an_inherited_static_property() {
    let text = hover_over_x(&consumer_class("app('sentry.hub')")).await;
    assert!(
        text.contains("HubInterface"),
        "expected the aliased 'sentry.hub' key to resolve to HubInterface, got: {text}"
    );
}

/// The same folded key, this time built into a longer one by concatenation.
#[tokio::test]
async fn make_resolves_a_key_built_from_an_inherited_static_property() {
    let text = hover_over_x(&consumer_class("app()->make('sentry.hub.client')")).await;
    assert!(
        text.contains("Client"),
        "expected the 'sentry.hub.client' binding to resolve to Client, got: {text}"
    );
}

/// A key nothing binds stays unresolved rather than being read as the name of
/// a class that does not exist.
#[tokio::test]
async fn an_unbound_string_key_stays_unresolved() {
    let text = hover_over_x(&consumer_class("app()->make('nothing.binds.this')")).await;
    assert!(
        !text.contains("nothing.binds.this"),
        "an unbound key must not be reported as a class, got: {text}"
    );
}

/// A provider that lists its registrations in the `$bindings` / `$singletons`
/// arrays Laravel reads instead of writing them out in `register()`.
const ARRAY_BINDING_PROVIDER_PHP: &str = r#"<?php
namespace App;

use App\Support\Clock;
use App\Support\FeatureFlags;
use Sentry\State\HubInterface;
use Sentry\Client;

class ArrayBindingProvider
{
    public array $bindings = [
        'array.clock' => Clock::class,
        HubInterface::class => Client::class,
    ];

    public $singletons = [
        'array.flags' => FeatureFlags::class,
    ];
}
"#;

const PROVIDERS_WITH_ARRAYS_PHP: &str = "<?php\nreturn [\n    App\\AppServiceProvider::class,\n    Sentry\\Laravel\\ServiceProvider::class,\n    App\\ArrayBindingProvider::class,\n];\n";

fn array_binding_files() -> [(&'static str, &'static str); 2] {
    [
        ("src/ArrayBindingProvider.php", ARRAY_BINDING_PROVIDER_PHP),
        ("bootstrap/providers.php", PROVIDERS_WITH_ARRAYS_PHP),
    ]
}

/// Laravel applies a provider's `$bindings` array itself, so the keys in it
/// bind exactly as a `bind()` call in `register()` would.
#[tokio::test]
async fn a_bindings_array_entry_resolves_to_its_class() {
    let text = hover_over_x_with_extra_files(
        &array_binding_files(),
        &consumer_class("app('array.clock')"),
    )
    .await;
    assert!(
        text.contains("Clock"),
        "expected the $bindings entry 'array.clock' to resolve to Clock, got: {text}"
    );
}

/// `$singletons` is read the same way.
#[tokio::test]
async fn a_singletons_array_entry_resolves_to_its_class() {
    let text = hover_over_x_with_extra_files(
        &array_binding_files(),
        &consumer_class("app('array.flags')"),
    )
    .await;
    assert!(
        text.contains("FeatureFlags"),
        "expected the $singletons entry 'array.flags' to resolve to FeatureFlags, got: {text}"
    );
}

/// A `$bindings` entry keyed by a contract binds a name that already resolves
/// on its own, and the contract is what the application declared it wants:
/// asking for it must not hand back the concrete behind it.
#[tokio::test]
async fn a_bindings_array_entry_keyed_by_a_contract_keeps_the_contract() {
    let text = hover_over_x_with_extra_files(
        &array_binding_files(),
        &consumer_class("app(\\Sentry\\State\\HubInterface::class)"),
    )
    .await;
    assert!(
        text.contains("HubInterface"),
        "expected the contract to stay the type, got: {text}"
    );
    assert!(
        !text.contains("Client"),
        "the concrete behind the contract must not replace it, got: {text}"
    );
}

/// A factory whose body builds nothing recognisable still says what it hands
/// back when it declares a return type.
#[tokio::test]
async fn a_factory_resolves_through_its_declared_return_type() {
    const TYPED_FACTORY_PROVIDER_PHP: &str = r#"<?php
namespace App;

use App\Support\Clock;

class TypedFactoryProvider
{
    public function register(): void
    {
        $this->app->singleton('typed.clock', function ($app): Clock {
            return $app->make(self::class)->build();
        });
    }
}
"#;
    const PROVIDERS_WITH_TYPED_PHP: &str = "<?php\nreturn [\n    App\\AppServiceProvider::class,\n    Sentry\\Laravel\\ServiceProvider::class,\n    App\\TypedFactoryProvider::class,\n];\n";

    let extra = [
        ("src/TypedFactoryProvider.php", TYPED_FACTORY_PROVIDER_PHP),
        ("bootstrap/providers.php", PROVIDERS_WITH_TYPED_PHP),
    ];
    let text = hover_over_x_with_extra_files(&extra, &consumer_class("app('typed.clock')")).await;
    assert!(
        text.contains("Clock"),
        "expected the declared return type to settle the binding, got: {text}"
    );
}

/// Hovering the key itself reports what it resolves to and which provider
/// registered it, rather than describing the value the call produces.
#[tokio::test]
async fn hovering_a_container_key_reports_its_binding() {
    let source = consumer_class("app('sentry')");
    let text = hover_over_key(&[], &source, "sentry');").await;
    assert!(
        text.contains("Resolves to `Sentry\\HubAdapter`"),
        "expected the hover to name the bound class, got: {text}"
    );
    assert!(
        text.contains("AppServiceProvider.php"),
        "expected the hover to name the registering provider, got: {text}"
    );
}

/// A key the framework declares in its own core alias table has no
/// registration of its own, so its class is all it has to show.
#[tokio::test]
async fn hovering_a_core_alias_reports_only_its_class() {
    let source = consumer_class("app('app')");
    let text = hover_over_key(&[], &source, "app');").await;
    assert!(
        text.contains("Illuminate\\Foundation\\Application"),
        "expected the core alias to resolve to the application, got: {text}"
    );
    assert!(
        !text.contains("Registered in"),
        "a core alias has no provider registration to name, got: {text}"
    );
}

/// A key nothing binds is described as the container key it is, without
/// claiming a class it does not resolve to.
#[tokio::test]
async fn hovering_an_unbound_container_key_claims_nothing() {
    let source = consumer_class("app('nothing.binds.this')");
    let text = hover_over_key(&[], &source, "nothing.binds.this');").await;
    assert!(
        text.contains("**Container** `nothing.binds.this`"),
        "expected the key to be named as a container key, got: {text}"
    );
    assert!(
        !text.contains("Resolves to"),
        "an unbound key must not claim a class, got: {text}"
    );
    assert!(
        !text.contains("Registered in"),
        "an unbound key has no registration, got: {text}"
    );
}

/// Go-to-definition on a key nothing binds offers nothing rather than
/// guessing at a registration.
#[tokio::test]
async fn goto_definition_on_an_unbound_container_key_offers_nothing() {
    let source = consumer_class("app('nothing.binds.this')");
    let response = goto_definition_on_key(&source, "nothing.binds.this');").await;
    assert!(
        response.is_none(),
        "expected no definition for an unbound key, got: {response:?}"
    );
}

/// Open a consumer and run go-to-definition on the container key inside
/// `needle`.
async fn goto_definition_on_key(consumer: &str, needle: &str) -> Option<GotoDefinitionResponse> {
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

    backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: position_of(consumer, needle),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
}

/// Go-to-definition on the key lands on the registration that bound it.
#[tokio::test]
async fn goto_definition_on_a_container_key_lands_on_its_registration() {
    let source = consumer_class("app('sentry')");
    let response = goto_definition_on_key(&source, "sentry');")
        .await
        .expect("the key should navigate");

    let locations = match response {
        GotoDefinitionResponse::Array(locations) => locations,
        GotoDefinitionResponse::Scalar(location) => vec![location],
        GotoDefinitionResponse::Link(_) => panic!("expected plain locations"),
    };
    let registration = &locations[0];
    assert!(
        registration.uri.path().ends_with("AppServiceProvider.php"),
        "expected the registration site first, got: {:?}",
        locations
    );
    let provider_line = APP_SERVICE_PROVIDER
        .lines()
        .position(|line| line.contains("singleton('sentry'"))
        .expect("the fixture should register 'sentry'") as u32;
    assert_eq!(
        registration.range.start.line, provider_line,
        "expected the line the key is bound on, got: {:?}",
        locations
    );
    assert!(
        locations
            .iter()
            .any(|location| location.uri.path().ends_with("HubAdapter.php")),
        "expected the bound class to be offered too, got: {:?}",
        locations
    );
}

/// A core alias has no registration of its own, so go-to-definition offers
/// the class alone rather than nothing.
#[tokio::test]
async fn goto_definition_on_a_core_alias_offers_the_class_alone() {
    let source = consumer_class("app('app')");
    let response = goto_definition_on_key(&source, "app');")
        .await
        .expect("the core alias should navigate");
    let locations = match response {
        GotoDefinitionResponse::Array(locations) => locations,
        GotoDefinitionResponse::Scalar(location) => vec![location],
        GotoDefinitionResponse::Link(_) => panic!("expected plain locations"),
    };
    assert_eq!(locations.len(), 1, "got: {locations:?}");
    assert!(
        locations[0].uri.path().ends_with("Application.php"),
        "expected the bound class, got: {locations:?}"
    );
}

/// Two requests that arrive together must both resolve the key.
///
/// The alias tables are built on first use, and in a real session the
/// requests that need them overlap constantly: the diagnostic pass walks the
/// open file on the blocking pool while the editor's own hover and
/// go-to-definition requests are in flight.  Whichever of them gets there
/// first has to finish building before any of the others reads the tables,
/// or the losers see a table that is still empty and report a bound key as
/// bound to nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_requests_all_resolve_the_same_key() {
    use std::sync::Arc;

    let source = consumer_class("app('app')");
    let mut files = base_files();
    files.push(("src/Consumer.php", &source));

    // A fresh workspace per round: the race is over the *first* build, so
    // each round needs alias tables nothing has built yet.
    for round in 0..8 {
        let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, &files);
        let uri = Url::from_file_path(dir.path().join("src/Consumer.php")).unwrap();

        backend.initialized(InitializedParams {}).await;
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "php".to_string(),
                    version: 1,
                    text: source.clone(),
                },
            })
            .await;

        let backend = Arc::new(backend);
        let position = position_of(&source, "app');");
        let request = || {
            let backend = Arc::clone(&backend);
            let uri = uri.clone();
            async move {
                backend
                    .goto_definition(GotoDefinitionParams {
                        text_document_position_params: TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier { uri },
                            position,
                        },
                        work_done_progress_params: WorkDoneProgressParams::default(),
                        partial_result_params: PartialResultParams::default(),
                    })
                    .await
                    .unwrap()
            }
        };

        let (first, second, third) = tokio::join!(request(), request(), request());
        for (index, response) in [first, second, third].into_iter().enumerate() {
            assert!(
                response.is_some(),
                "round {round}, request {index}: the core alias should navigate \
                 however many requests are in flight"
            );
        }
    }
}

/// A key bound to a class the project does not actually have still navigates
/// to the registration: that is where the mistake is.
#[tokio::test]
async fn goto_definition_falls_back_to_the_registration_alone() {
    const GHOST_PROVIDER_PHP: &str = r#"<?php
namespace App;

class GhostServiceProvider
{
    public function register(): void
    {
        $this->app->singleton('ghost', fn () => new \App\Missing\Ghost());
    }
}
"#;
    const PROVIDERS_WITH_GHOST_PHP: &str = "<?php\nreturn [\n    App\\AppServiceProvider::class,\n    Sentry\\Laravel\\ServiceProvider::class,\n    App\\GhostServiceProvider::class,\n];\n";

    let source = consumer_class("app('ghost')");
    let mut files = base_files();
    files.push(("src/GhostServiceProvider.php", GHOST_PROVIDER_PHP));
    files.push(("bootstrap/providers.php", PROVIDERS_WITH_GHOST_PHP));
    files.push(("src/Consumer.php", &source));
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, &files);
    let uri = Url::from_file_path(dir.path().join("src/Consumer.php")).unwrap();

    backend.initialized(InitializedParams {}).await;
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: source.clone(),
            },
        })
        .await;

    let response = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: position_of(&source, "ghost');"),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .expect("the registration should still navigate");

    let locations = match response {
        GotoDefinitionResponse::Array(locations) => locations,
        GotoDefinitionResponse::Scalar(location) => vec![location],
        GotoDefinitionResponse::Link(_) => panic!("expected plain locations"),
    };
    assert_eq!(locations.len(), 1, "got: {locations:?}");
    assert!(
        locations[0]
            .uri
            .path()
            .ends_with("GhostServiceProvider.php"),
        "expected the registration, got: {locations:?}"
    );
}

/// Find-references on a container key collects every call that asks for it,
/// however the call is spelled, plus the registration that bound it.
#[tokio::test]
async fn find_references_on_a_container_key_collects_every_call() {
    const USAGES_PHP: &str = r#"<?php
namespace App;
class Usages
{
    public function go(): void
    {
        app('sentry');
        resolve('sentry');
        app()->make('sentry');
        \Illuminate\Support\Facades\App::make('sentry');
        app('clock');
    }
}
"#;
    let mut files = base_files();
    files.push(("src/Usages.php", USAGES_PHP));
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, &files);
    let uri = Url::from_file_path(dir.path().join("src/Usages.php")).unwrap();

    backend.initialized(InitializedParams {}).await;
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: USAGES_PHP.to_string(),
            },
        })
        .await;

    let locations = backend
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri.clone()),
                position: position_of(USAGES_PHP, "sentry');"),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await
        .unwrap()
        .unwrap_or_default();

    let usages = locations.iter().filter(|l| l.uri == uri).count();
    assert_eq!(
        usages, 4,
        "expected all four spellings of the 'sentry' lookup and not the 'clock' one, got: {locations:?}"
    );
    assert!(
        locations
            .iter()
            .any(|l| l.uri.path().ends_with("AppServiceProvider.php")),
        "expected the registration among the references, got: {locations:?}"
    );
}

/// A dotted container key's first segment can collide with an unrelated
/// project class (`demo.bakery` vs `App\Demo`). The whole key must still
/// resolve to the class bound in the provider, not the short-name match on
/// its truncated first segment.
#[tokio::test]
async fn a_dotted_key_does_not_resolve_to_a_class_named_after_its_first_segment() {
    const BAKERY_SERVICE_PHP: &str = r#"<?php
namespace App;
class BakeryService
{
    public function bake(string $item): string { return $item; }
}
"#;
    const BAKERY_SERVICE_PROVIDER_PHP: &str = r#"<?php
namespace App;
class BakeryServiceProvider
{
    public function register(): void
    {
        $this->app->singleton('demo.bakery', fn () => new BakeryService());
    }
}
"#;
    const PROVIDERS_WITH_BAKERY_PHP: &str = "<?php\nreturn [\n    App\\AppServiceProvider::class,\n    Sentry\\Laravel\\ServiceProvider::class,\n    App\\BakeryServiceProvider::class,\n];\n";

    let extra_files = [
        ("src/Demo.php", "<?php\nnamespace App;\nclass Demo {}\n"),
        ("src/BakeryService.php", BAKERY_SERVICE_PHP),
        ("src/BakeryServiceProvider.php", BAKERY_SERVICE_PROVIDER_PHP),
        ("bootstrap/providers.php", PROVIDERS_WITH_BAKERY_PHP),
    ];
    let text =
        hover_over_x_with_extra_files(&extra_files, &consumer_class("app('demo.bakery')")).await;

    assert!(
        text.contains("BakeryService"),
        "expected 'demo.bakery' to resolve to BakeryService, got: {text}"
    );
    assert!(
        !text.contains("class Demo"),
        "'demo.bakery' must not resolve to the unrelated App\\Demo class, got: {text}"
    );
}
