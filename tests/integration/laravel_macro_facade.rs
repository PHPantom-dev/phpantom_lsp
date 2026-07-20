//! Tests that a macro registered through a facade (`View::macro(...)`) also
//! attaches to the facade's concrete container-bound class, so an instance
//! call on the concrete type (`$factory->extends()`) resolves.
//!
//! The facade's `getFacadeAccessor()` is parsed statically for its
//! container-binding string (`'view'`), which is looked up in the core
//! container alias table to find the concrete class. No application booting.

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
            "Illuminate\\View\\": "vendor/illuminate/View/"
        }
    }
}"#;

/// Core container aliases binding the `'view'` string to the concrete factory.
const APPLICATION_PHP: &str = r#"<?php
namespace Illuminate\Foundation;
class Application
{
    public function registerCoreContainerAliases()
    {
        foreach ([
            'view' => [\Illuminate\View\Factory::class, \Illuminate\Contracts\View\Factory::class],
        ] as $key => $aliases) {
            foreach ($aliases as $alias) {
                $this->alias($key, $alias);
            }
        }
    }
}
"#;

/// `Facade::defaultAliases()` so the `View` facade FQN is a known facade.
const FACADE_PHP: &str = r#"<?php
namespace Illuminate\Support\Facades;
abstract class Facade
{
    public static function defaultAliases()
    {
        return new Collection([
            'View' => View::class,
        ]);
    }
}
"#;

/// The `View` facade: a string-alias accessor resolved via the container table.
const FACADE_VIEW_PHP: &str = r#"<?php
namespace Illuminate\Support\Facades;
class View extends Facade
{
    protected static function getFacadeAccessor()
    {
        return 'view';
    }
}
"#;

/// The concrete class bound to `'view'`.
const VIEW_FACTORY_PHP: &str = r#"<?php
namespace Illuminate\View;
class Factory
{
    public function make(): string { return ''; }
}
"#;

/// A provider registering a macro through the facade.
const PROVIDER_PHP: &str = r#"<?php
namespace App\Providers;
use Illuminate\Support\Facades\View;
class ViewServiceProvider
{
    public function boot(): void
    {
        View::macro('extends', function (string $layout): string {
            return $layout;
        });
    }
}
"#;

fn base_files() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "vendor/illuminate/Foundation/Application.php",
            APPLICATION_PHP,
        ),
        ("vendor/illuminate/Support/Facades/Facade.php", FACADE_PHP),
        (
            "vendor/illuminate/Support/Facades/View.php",
            FACADE_VIEW_PHP,
        ),
        ("vendor/illuminate/View/Factory.php", VIEW_FACTORY_PHP),
        ("src/Providers/ViewServiceProvider.php", PROVIDER_PHP),
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

#[tokio::test]
async fn facade_macro_resolves_on_concrete_instance() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\View\\Factory;
class Consumer {
    public function go(Factory $f): void {
        $f->
    }
}
";
    let mut files = base_files();
    files.push(("src/Consumer.php", consumer));
    let (backend, _dir) = create_psr4_workspace(COMPOSER_JSON, &files);

    // Opening the provider registers the macro (and expands it onto the
    // concrete view factory via the facade accessor).
    open(
        &backend,
        "file:///src/Providers/ViewServiceProvider.php",
        PROVIDER_PHP,
    )
    .await;
    open(&backend, "file:///src/Consumer.php", consumer).await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse("file:///src/Consumer.php").unwrap(),
                },
                position: Position {
                    line: 5,
                    character: 12,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    let items = match result.expect("completion should return results") {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    let names: Vec<&str> = items
        .iter()
        .filter_map(|i| i.filter_text.as_deref())
        .collect();

    assert!(
        names.contains(&"extends"),
        "facade-registered macro should attach to the concrete factory, got: {names:?}"
    );
    assert!(
        names.contains(&"make"),
        "real methods should still appear, got: {names:?}"
    );
}

#[tokio::test]
async fn facade_macro_instance_call_is_not_flagged() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\View\\Factory;
class Consumer {
    public function go(Factory $f): string {
        return $f->extends('layout');
    }
}
";
    let mut files = base_files();
    files.push(("src/Consumer.php", consumer));
    let (backend, _dir) = create_psr4_workspace(COMPOSER_JSON, &files);
    open(
        &backend,
        "file:///src/Providers/ViewServiceProvider.php",
        PROVIDER_PHP,
    )
    .await;

    let uri = "file:///src/Consumer.php";
    backend.update_ast(uri, consumer);
    let mut diagnostics = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, consumer, &mut diagnostics);

    let members: Vec<&str> = diagnostics
        .iter()
        .filter(|d| {
            d.code
                .as_ref()
                .is_some_and(|c| matches!(c, NumberOrString::String(s) if s == "unknown_member"))
        })
        .map(|d| d.message.as_str())
        .collect();

    assert!(
        members.is_empty(),
        "facade macro instance call should resolve, got: {members:?}"
    );
}

#[tokio::test]
async fn static_facade_macro_call_still_resolves() {
    // The facade itself keeps the macro too, so the static form works.
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\View;
class Consumer {
    public function go(): string {
        return View::extends('layout');
    }
}
";
    let mut files = base_files();
    files.push(("src/Consumer.php", consumer));
    let (backend, _dir) = create_psr4_workspace(COMPOSER_JSON, &files);
    open(
        &backend,
        "file:///src/Providers/ViewServiceProvider.php",
        PROVIDER_PHP,
    )
    .await;

    let uri = "file:///src/Consumer.php";
    backend.update_ast(uri, consumer);
    let mut diagnostics = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, consumer, &mut diagnostics);

    let members: Vec<&str> = diagnostics
        .iter()
        .filter(|d| {
            d.code
                .as_ref()
                .is_some_and(|c| matches!(c, NumberOrString::String(s) if s == "unknown_member"))
        })
        .map(|d| d.message.as_str())
        .collect();

    assert!(
        members.is_empty(),
        "static facade macro call should resolve, got: {members:?}"
    );
}
