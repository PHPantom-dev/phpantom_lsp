//! Integration tests for Laravel container string aliases
//! (`resolve('blade.compiler')`) and global facade class aliases (`\App`),
//! both sourced by parsing the installed framework's own declarations.

use crate::common::create_psr4_workspace;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "Illuminate\\Foundation\\": "vendor/illuminate/Foundation/",
            "Illuminate\\Support\\Facades\\": "vendor/illuminate/Support/Facades/",
            "Illuminate\\View\\Compilers\\": "vendor/illuminate/View/Compilers/",
            "Illuminate\\Cache\\": "vendor/illuminate/Cache/"
        }
    }
}"#;

/// `registerCoreContainerAliases()` in the shape Laravel actually declares it.
const APPLICATION_PHP: &str = r#"<?php
namespace Illuminate\Foundation;
class Application
{
    public function registerCoreContainerAliases()
    {
        foreach ([
            'app' => [self::class],
            'blade.compiler' => [\Illuminate\View\Compilers\BladeCompiler::class],
            'cache' => [\Illuminate\Cache\CacheManager::class],
        ] as $key => $aliases) {
            foreach ($aliases as $alias) {
                $this->alias($key, $alias);
            }
        }
    }
}
"#;

/// `Facade::defaultAliases()` returning the global alias collection.
const FACADE_PHP: &str = r#"<?php
namespace Illuminate\Support\Facades;
abstract class Facade
{
    public static function defaultAliases()
    {
        return new Collection([
            'App' => App::class,
            'Cache' => Cache::class,
        ]);
    }
}
"#;

const FACADE_APP_PHP: &str = r#"<?php
namespace Illuminate\Support\Facades;
class App
{
    public static function environment(...$environments) { return 'testing'; }
}
"#;

const FACADE_CACHE_PHP: &str = r#"<?php
namespace Illuminate\Support\Facades;
class Cache
{
    public static function forget($key) { return true; }
}
"#;

const BLADE_COMPILER_PHP: &str = r#"<?php
namespace Illuminate\View\Compilers;
class BladeCompiler
{
    public function component($class, $alias) {}
    public function compileString($value) { return ''; }
}
"#;

const CACHE_MANAGER_PHP: &str = r#"<?php
namespace Illuminate\Cache;
class CacheManager
{
    public function store($name = null) { return null; }
}
"#;

fn base_files() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "vendor/illuminate/Foundation/Application.php",
            APPLICATION_PHP,
        ),
        ("vendor/illuminate/Support/Facades/Facade.php", FACADE_PHP),
        ("vendor/illuminate/Support/Facades/App.php", FACADE_APP_PHP),
        (
            "vendor/illuminate/Support/Facades/Cache.php",
            FACADE_CACHE_PHP,
        ),
        (
            "vendor/illuminate/View/Compilers/BladeCompiler.php",
            BLADE_COMPILER_PHP,
        ),
        (
            "vendor/illuminate/Cache/CacheManager.php",
            CACHE_MANAGER_PHP,
        ),
    ]
}

async fn complete_labels(
    files: &[(&str, &str)],
    open_path: &str,
    content: &str,
    line: u32,
    character: u32,
) -> Vec<String> {
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, files);
    let uri = Url::from_file_path(dir.path().join(open_path)).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: content.to_string(),
            },
        })
        .await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position { line, character },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    let items = match result {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        _ => Vec::new(),
    };
    items.into_iter().map(|i| i.label).collect()
}

/// `resolve('blade.compiler')` binds the string to its concrete class, so the
/// instance members of `BladeCompiler` complete.
#[tokio::test]
async fn resolve_container_alias_resolves_concrete_class() {
    let content = "\
<?php
namespace App;
class Svc {
    public function make() {
        resolve('blade.compiler')->
    }
}
";
    let labels = complete_labels(&base_files(), "src/Svc.php", content, 4, 35).await;
    assert!(
        labels.iter().any(|l| l.starts_with("component")),
        "expected BladeCompiler::component in completions, got: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("compileString")),
        "expected BladeCompiler::compileString in completions, got: {labels:?}"
    );
}

/// `app('cache')` resolves the same way as `resolve('cache')`.
#[tokio::test]
async fn app_helper_container_alias_resolves_concrete_class() {
    let content = "\
<?php
namespace App;
class Svc {
    public function make() {
        app('cache')->
    }
}
";
    let labels = complete_labels(&base_files(), "src/Svc.php", content, 4, 22).await;
    assert!(
        labels.iter().any(|l| l.starts_with("store")),
        "expected CacheManager::store in completions, got: {labels:?}"
    );
}

/// A bare `\App` refers to the global facade alias, so its static members
/// complete even without an explicit `use` import.
#[tokio::test]
async fn global_facade_alias_resolves() {
    let content = "\
<?php
namespace App;
class Svc {
    public function run() {
        \\App::
    }
}
";
    let labels = complete_labels(&base_files(), "src/Svc.php", content, 4, 14).await;
    assert!(
        labels.iter().any(|l| l.starts_with("environment")),
        "expected facade App::environment in completions, got: {labels:?}"
    );
}

/// The binding survives being assigned to a variable before use, not just
/// when chained directly off the call.
#[tokio::test]
async fn resolve_container_alias_resolves_through_variable_assignment() {
    let content = "\
<?php
namespace App;
class Svc {
    public function make() {
        $compiler = resolve('blade.compiler');
        $compiler->
    }
}
";
    let labels = complete_labels(&base_files(), "src/Svc.php", content, 5, 19).await;
    assert!(
        labels.iter().any(|l| l.starts_with("component")),
        "expected BladeCompiler::component in completions, got: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("compileString")),
        "expected BladeCompiler::compileString in completions, got: {labels:?}"
    );
}

/// An unknown string binding stays unresolved (no guessing): `resolve()` of a
/// service-provider-registered name offers nothing.
#[tokio::test]
async fn unknown_container_alias_stays_unresolved() {
    let content = "\
<?php
namespace App;
class Svc {
    public function make() {
        resolve('sentry')->
    }
}
";
    let labels = complete_labels(&base_files(), "src/Svc.php", content, 4, 27).await;
    assert!(
        !labels.iter().any(|l| l.starts_with("component")),
        "unknown binding must not resolve to a concrete class, got: {labels:?}"
    );
}

/// A project class whose short name collides with a global facade alias
/// (`Cache`) resolves to the project class when referenced unqualified in
/// the same namespace, not the facade. The alias table is only a fallback
/// reached after namespace-aware resolution misses, so a same-namespace
/// class always wins.
#[tokio::test]
async fn same_namespace_class_wins_over_facade_alias() {
    let local_cache = "\
<?php
namespace App;
class Cache {
    public function projectOnly() {}
}
";
    let mut files = base_files();
    files.push(("src/Cache.php", local_cache));
    let content = "\
<?php
namespace App;
class Svc {
    public function make(Cache $c) {
        $c->
    }
}
";
    let labels = complete_labels(&files, "src/Svc.php", content, 4, 12).await;
    assert!(
        labels.iter().any(|l| l.starts_with("projectOnly")),
        "expected project App\\Cache::projectOnly, got: {labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l.starts_with("forget")),
        "must not resolve to facade Cache::forget, got: {labels:?}"
    );
}
