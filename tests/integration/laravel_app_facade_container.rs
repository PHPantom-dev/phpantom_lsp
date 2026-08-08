//! Tests that the `App` facade's `make()`, `makeWith()`, and `resolve()`
//! resolve a class-string argument to that class, the same way the `app()`
//! helper does.
//!
//! `App::getFacadeAccessor()` returns the container-binding string `'app'`,
//! which core container aliases bind to `self::class` (the `Application`
//! class itself) rather than a literal `Concrete::class`. The facade's own
//! `@method` docblock also flattens the container's argument-dependent
//! return (`($abstract is class-string<TClass> ? TClass : mixed)`) to a bare
//! `object|mixed`, so resolution must fall through to the concrete
//! `Application::make()`/`makeWith()` declarations to recover the narrowed
//! type.

use crate::common::create_psr4_workspace;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "Illuminate\\Foundation\\": "vendor/illuminate/Foundation/",
            "Illuminate\\Support\\Facades\\": "vendor/illuminate/Support/Facades/",
            "Illuminate\\Contracts\\Container\\": "vendor/illuminate/Contracts/Container/"
        }
    }
}"#;

/// Mirrors the real `registerCoreContainerAliases()`: the `'app'` entry's
/// concrete is written as `self::class`.
const APPLICATION_PHP: &str = r#"<?php
namespace Illuminate\Foundation;
use Illuminate\Contracts\Container\Container;
class Application implements Container
{
    public function registerCoreContainerAliases()
    {
        foreach ([
            'app' => [self::class, \Illuminate\Contracts\Container\Container::class],
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

    /**
     * @template TClass
     * @param string|class-string<TClass> $abstract
     * @return ($abstract is class-string<TClass> ? TClass : mixed)
     */
    public function makeWith($abstract, array $parameters = [])
    {
        return new $abstract();
    }

    /**
     * @template TClass
     * @param string|class-string<TClass> $abstract
     * @return ($abstract is class-string<TClass> ? TClass : mixed)
     */
    public function resolve($abstract, array $parameters = [])
    {
        return new $abstract();
    }
}
"#;

const CONTAINER_PHP: &str = r#"<?php
namespace Illuminate\Contracts\Container;
interface Container {}
"#;

const FACADE_PHP: &str = r#"<?php
namespace Illuminate\Support\Facades;
abstract class Facade
{
    public static function __callStatic($method, $args)
    {
        return static::resolveFacadeInstance()->$method(...$args);
    }
}
"#;

/// Mirrors the real `App` facade: `make`/`makeWith` are documented via
/// `@method` with the flattened `object|mixed` return that Laravel's own
/// docblock uses, losing the container's conditional return.
const FACADE_APP_PHP: &str = r#"<?php
namespace Illuminate\Support\Facades;

/**
 * @method static object|mixed make(string $abstract, array $parameters = [])
 * @method static object|mixed makeWith(string|callable $abstract, array $parameters = [])
 */
class App extends Facade
{
    protected static function getFacadeAccessor()
    {
        return 'app';
    }
}
"#;

const CURRENCY_HELPER_PHP: &str = r#"<?php
namespace App;
class CurrencyHelper
{
    public function format(): string { return ''; }
}
"#;

fn base_files() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "vendor/illuminate/Foundation/Application.php",
            APPLICATION_PHP,
        ),
        (
            "vendor/illuminate/Contracts/Container/Container.php",
            CONTAINER_PHP,
        ),
        ("vendor/illuminate/Support/Facades/Facade.php", FACADE_PHP),
        ("vendor/illuminate/Support/Facades/App.php", FACADE_APP_PHP),
        ("src/CurrencyHelper.php", CURRENCY_HELPER_PHP),
    ]
}

async fn open(backend: &phpantom_lsp::Backend, uri: &str, text: &str) {
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: Url::parse(uri).unwrap(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;
}

fn hover_text(hover: &Hover) -> &str {
    match &hover.contents {
        HoverContents::Markup(markup) => &markup.value,
        _ => panic!("Expected MarkupContent"),
    }
}

/// Run the consumer through hover on the `$x` usage line, and assert its
/// resolved type contains `CurrencyHelper`.
fn assert_resolves_to_currency_helper(backend: &phpantom_lsp::Backend, consumer: &str) {
    backend.update_ast("file:///src/Consumer.php", consumer);
    let idx = consumer.rfind("$x;").expect("consumer should use $x");
    let prefix = &consumer[..idx + 1];
    let line = prefix.bytes().filter(|b| *b == b'\n').count() as u32;
    let character = prefix.rsplit('\n').next().unwrap().len() as u32 - 1;
    let hover = backend
        .handle_hover(
            "file:///src/Consumer.php",
            consumer,
            Position { line, character },
        )
        .expect("hover should resolve");
    let text = hover_text(&hover);
    assert!(
        text.contains("CurrencyHelper"),
        "expected the class-string argument to resolve to CurrencyHelper, got: {text}"
    );
}

#[tokio::test]
async fn app_facade_make_resolves_classstring_argument() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\App;
class Consumer {
    public function go(): void {
        $x = App::make(CurrencyHelper::class);
        $x;
    }
}
";
    let mut files = base_files();
    files.push(("src/Consumer.php", consumer));
    let (backend, _dir) = create_psr4_workspace(COMPOSER_JSON, &files);
    open(&backend, "file:///src/Consumer.php", consumer).await;
    assert_resolves_to_currency_helper(&backend, consumer);
}

#[tokio::test]
async fn app_facade_makewith_resolves_classstring_argument() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\App;
class Consumer {
    public function go(): void {
        $x = App::makeWith(CurrencyHelper::class, []);
        $x;
    }
}
";
    let mut files = base_files();
    files.push(("src/Consumer.php", consumer));
    let (backend, _dir) = create_psr4_workspace(COMPOSER_JSON, &files);
    open(&backend, "file:///src/Consumer.php", consumer).await;
    assert_resolves_to_currency_helper(&backend, consumer);
}

/// Hover over the `format` member of a chained call and assert the chain
/// subject resolved to `CurrencyHelper`.
fn assert_chained_format_resolves(backend: &phpantom_lsp::Backend, consumer: &str) {
    backend.update_ast("file:///src/Consumer.php", consumer);
    let idx = consumer
        .find("->format()")
        .expect("consumer should chain format")
        + 2;
    let prefix = &consumer[..idx];
    let line = prefix.bytes().filter(|b| *b == b'\n').count() as u32;
    let character = prefix.rsplit('\n').next().unwrap().len() as u32;
    let hover = backend
        .handle_hover(
            "file:///src/Consumer.php",
            consumer,
            Position { line, character },
        )
        .expect("hover should resolve the chained method");
    let text = hover_text(&hover);
    assert!(
        text.contains("CurrencyHelper"),
        "expected the chained call to resolve to CurrencyHelper, got: {text}"
    );
}

#[tokio::test]
async fn app_facade_make_resolves_when_chained_directly() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\App;
class Consumer {
    public function go(): void {
        App::make(CurrencyHelper::class)->format();
    }
}
";
    let mut files = base_files();
    files.push(("src/Consumer.php", consumer));
    let (backend, _dir) = create_psr4_workspace(COMPOSER_JSON, &files);
    open(&backend, "file:///src/Consumer.php", consumer).await;
    assert_chained_format_resolves(&backend, consumer);
}

#[tokio::test]
async fn app_facade_makewith_resolves_when_chained_directly() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\App;
class Consumer {
    public function go(): void {
        App::makeWith(CurrencyHelper::class, [])->format();
    }
}
";
    let mut files = base_files();
    files.push(("src/Consumer.php", consumer));
    let (backend, _dir) = create_psr4_workspace(COMPOSER_JSON, &files);
    open(&backend, "file:///src/Consumer.php", consumer).await;
    assert_chained_format_resolves(&backend, consumer);
}

/// `resolve()` has no `@method` tag on the facade, so it reaches the
/// concrete container through `__callStatic` forwarding rather than the
/// flattened-tag fall-through the other two take.
#[tokio::test]
async fn app_facade_resolve_resolves_when_chained_directly() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\App;
class Consumer {
    public function go(): void {
        App::resolve(CurrencyHelper::class)->format();
    }
}
";
    let mut files = base_files();
    files.push(("src/Consumer.php", consumer));
    let (backend, _dir) = create_psr4_workspace(COMPOSER_JSON, &files);
    open(&backend, "file:///src/Consumer.php", consumer).await;
    assert_chained_format_resolves(&backend, consumer);
}

#[tokio::test]
async fn app_facade_resolve_resolves_classstring_argument() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\App;
class Consumer {
    public function go(): void {
        $x = App::resolve(CurrencyHelper::class);
        $x;
    }
}
";
    let mut files = base_files();
    files.push(("src/Consumer.php", consumer));
    let (backend, _dir) = create_psr4_workspace(COMPOSER_JSON, &files);
    open(&backend, "file:///src/Consumer.php", consumer).await;
    assert_resolves_to_currency_helper(&backend, consumer);
}
