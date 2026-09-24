//! End-to-end coverage for direct Laravel config-backed resource names.

use crate::common::create_psr4_workspace;
use phpantom_lsp::Backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^12.0" },
    "autoload": { "psr-4": { "App\\": "app/" } }
}"#;

const PACKAGE_COMPOSER_JSON: &str = r#"{
    "name": "acme/widgets",
    "require": { "illuminate/support": "^12.0" },
    "require-dev": { "laravel/framework": "^12.0" },
    "autoload": { "psr-4": { "App\\": "app/" } }
}"#;

const AUTH_CONFIG: &str = "<?php return ['guards' => ['web' => [], 'admin' => []]];\n";
const CACHE_CONFIG: &str = "<?php return ['stores' => ['array' => [], 'redis' => []]];\n";
const LOGGING_CONFIG: &str = "<?php return ['channels' => ['daily' => [], 'slack' => []]];\n";
const FILESYSTEMS_CONFIG: &str = "<?php return ['disks' => ['local' => [], 'archive' => []]];\n";
const DATABASE_CONFIG: &str = "<?php return ['connections' => ['mysql' => [], 'sqlite' => []]];\n";
const QUEUE_CONFIG: &str = "<?php return ['connections' => ['sync' => [], 'redis' => []]];\n";
const MAIL_CONFIG: &str = "<?php return ['mailers' => ['smtp' => [], 'log' => []]];\n";
const BROADCASTING_CONFIG: &str =
    "<?php return ['connections' => ['reverb' => [], 'log' => []]];\n";

fn workspace_files(source: &str) -> Vec<(&str, &str)> {
    vec![
        ("config/auth.php", AUTH_CONFIG),
        ("config/cache.php", CACHE_CONFIG),
        ("config/logging.php", LOGGING_CONFIG),
        ("config/filesystems.php", FILESYSTEMS_CONFIG),
        ("config/database.php", DATABASE_CONFIG),
        ("config/queue.php", QUEUE_CONFIG),
        ("config/mail.php", MAIL_CONFIG),
        ("config/broadcasting.php", BROADCASTING_CONFIG),
        ("app/NamedResourceConsumer.php", source),
    ]
}

fn position_at_offset(content: &str, offset: usize) -> Position {
    let before = &content[..offset];
    Position::new(
        before.bytes().filter(|byte| *byte == b'\n').count() as u32,
        before
            .rsplit_once('\n')
            .map_or(before.len(), |(_, tail)| tail.len()) as u32,
    )
}

fn position_after(content: &str, unique_prefix: &str) -> Position {
    let offset = content
        .find(unique_prefix)
        .unwrap_or_else(|| panic!("missing `{unique_prefix}`"))
        + unique_prefix.len();
    position_at_offset(content, offset)
}

fn position_of_config_key(content: &str, key: &str) -> Position {
    let marker = format!("'{key}' =>");
    let offset = content
        .find(&marker)
        .unwrap_or_else(|| panic!("missing config declaration `{marker}`"))
        + 1;
    position_at_offset(content, offset)
}

fn definition_location(response: GotoDefinitionResponse) -> Location {
    match response {
        GotoDefinitionResponse::Scalar(location) => location,
        GotoDefinitionResponse::Array(mut locations) => locations.remove(0),
        GotoDefinitionResponse::Link(mut links) => {
            let link = links.remove(0);
            Location::new(link.target_uri, link.target_selection_range)
        }
    }
}

fn text_in_range(content: &str, range: Range) -> &str {
    let start = content
        .split_inclusive('\n')
        .take(range.start.line as usize)
        .map(str::len)
        .sum::<usize>()
        + range.start.character as usize;
    let end = content
        .split_inclusive('\n')
        .take(range.end.line as usize)
        .map(str::len)
        .sum::<usize>()
        + range.end.character as usize;
    &content[start..end]
}

async fn open_workspace(source: &str) -> (Backend, tempfile::TempDir, Url) {
    let files = workspace_files(source);
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, &files);
    backend.initialized(InitializedParams {}).await;
    let uri = Url::from_file_path(dir.path().join("app/NamedResourceConsumer.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: source.to_string(),
            },
        })
        .await;
    (backend, dir, uri)
}

async fn completion_items(backend: &Backend, uri: &Url, position: Position) -> Vec<CompletionItem> {
    let response = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .expect("completion request should succeed");
    match response {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    }
}

fn hover_text(hover: Hover) -> String {
    match hover.contents {
        HoverContents::Markup(markup) => markup.value,
        HoverContents::Scalar(MarkedString::String(text)) => text,
        HoverContents::Scalar(MarkedString::LanguageString(text)) => text.value,
        HoverContents::Array(parts) => parts
            .into_iter()
            .map(|part| match part {
                MarkedString::String(text) => text,
                MarkedString::LanguageString(text) => text.value,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

#[tokio::test]
async fn every_direct_resource_context_completes_only_direct_config_children() {
    let source = r#"<?php
use Illuminate\Support\Facades\Auth;
use Illuminate\Support\Facades\Broadcast;
use Illuminate\Support\Facades\Cache;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Facades\Mail;
use Illuminate\Support\Facades\Queue;
use Illuminate\Support\Facades\Route;
use Illuminate\Support\Facades\Storage;

auth('');
Auth::guard('w');
Route::middleware('auth:web, a');
Route::middleware(['auth:w']);
Route::middleware(array('auth:a'));
Cache::store('');
Log::channel('');
Log::stack(['daily', 's']);
Log::stack(array('d'));
Storage::disk('');
DB::connection('');
Queue::connection('');
Mail::mailer('');
Broadcast::connection('');

class AttributeTargets {
    public function __construct(
        #[\Illuminate\Container\Attributes\Auth('a')] mixed $auth,
        #[\Illuminate\Container\Attributes\Authenticated('w')] mixed $authenticated,
        #[\Illuminate\Container\Attributes\Cache('r')] mixed $cache,
        #[\Illuminate\Container\Attributes\Log('s')] mixed $log,
        #[\Illuminate\Container\Attributes\Storage('l')] mixed $storage,
        #[\Illuminate\Container\Attributes\Database('s')] mixed $database,
        #[\Illuminate\Container\Attributes\DB('m')] mixed $databaseAlias,
    ) {}
}
"#;
    let (backend, _dir, uri) = open_workspace(source).await;

    let cases: &[(&str, &[&str])] = &[
        ("auth('", &["admin", "web"]),
        ("Auth::guard('w", &["web"]),
        ("middleware('auth:web, a", &["admin"]),
        ("middleware(['auth:w", &["web"]),
        ("middleware(array('auth:a", &["admin"]),
        ("Cache::store('", &["array", "null", "redis"]),
        ("Log::channel('", &["daily", "slack"]),
        ("Log::stack(['daily', 's", &["slack"]),
        ("Log::stack(array('d", &["daily"]),
        ("Storage::disk('", &["archive", "local"]),
        ("DB::connection('", &["mysql", "sqlite"]),
        ("Queue::connection('", &["null", "redis", "sync"]),
        ("Mail::mailer('", &["log", "smtp"]),
        ("Broadcast::connection('", &["log", "reverb"]),
        ("Attributes\\Auth('a", &["admin"]),
        ("Attributes\\Authenticated('w", &["web"]),
        ("Attributes\\Cache('r", &["redis"]),
        ("Attributes\\Log('s", &["slack"]),
        ("Attributes\\Storage('l", &["local"]),
        ("Attributes\\Database('s", &["sqlite"]),
        ("Attributes\\DB('m", &["mysql"]),
    ];

    for (prefix, expected) in cases {
        let items = completion_items(&backend, &uri, position_after(source, prefix)).await;
        assert_eq!(
            items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            *expected,
            "completion at `{prefix}`"
        );
        assert!(
            items
                .iter()
                .all(|item| item.kind == Some(CompletionItemKind::PROPERTY))
        );
    }

    let middleware = completion_items(
        &backend,
        &uri,
        position_after(source, "middleware('auth:web, a"),
    )
    .await;
    let Some(CompletionTextEdit::Edit(edit)) = &middleware[0].text_edit else {
        panic!("middleware completion should carry an exact text edit");
    };
    assert_eq!(text_in_range(source, edit.range), " a");
    assert_eq!(edit.new_text, "admin");
}

#[tokio::test]
async fn completion_uses_semantic_aliases_and_rejects_wrong_shapes_and_homonyms() {
    let source = r#"<?php
namespace App;

use Illuminate\Container\Attributes\Cache as CacheAttribute;
use Illuminate\Support\Facades\Cache as LaravelCache;
use Illuminate\Support\Facades\Log as LaravelLog;
use Illuminate\Support\Facades\Route as LaravelRoute;

class Cache { public static function store(string $name): void {} }
class Route { public static function middleware(string $name): void {} }
class MiddlewareHomonym {
    public function boot(): void { $this->middleware('auth:a'); }
}

LaravelCache::store(name: 'r');
LaravelLog::stack(channels: ['s']);
LaravelRoute::middleware(middleware: 'auth:a');
LaravelRoute::get('/', fn () => null)->middleware('auth:w');
LaravelRoute::get('/')->name('home')->middleware('auth:w');
LaravelRoute::get('/')
    ->name('multiline')
    ->middleware('auth:a');
LaravelRoute::get('/', function () { return 1; })->middleware('auth:w');
LaravelRoute::get('/', /* ) ] } */ fn () => null) // keep chaining
    ->middleware('auth:w');
LaravelRoute::get('/')->a()->b()->c()->d()->middleware('auth:w');
factory(LaravelRoute::class)->middleware('auth:w');
\Illuminate\Support\Facades\Cache::store('a');
\Cache::store('r');
\Route::middleware('auth:a');
class AttributeTarget {
    public function __construct(
        #[CacheAttribute(store: 'r')] mixed $named,
        #[\Deprecated('#['), CacheAttribute('r')] mixed $grouped,
        #[ CacheAttribute('a')] mixed $spaced,
    ) {}
}

Cache::store('r');
Route::middleware('auth:a');
LaravelLog::stack('s');
LaravelLog::stack(['key' => 'daily']);
LaravelCache::store(store: 'r');
LaravelCache::store(NAME: 'r');
LaravelCache::store('array', 'r');
class LocalTarget {
    public function __construct(#[Cache('r')] mixed $cache) {}
}
"#;
    let (backend, _dir, uri) = open_workspace(source).await;

    for (prefix, expected) in [
        ("LaravelCache::store(name: 'r", vec!["redis"]),
        ("LaravelLog::stack(channels: ['s", vec!["slack"]),
        ("middleware(middleware: 'auth:a", vec!["admin"]),
        ("get('/', fn () => null)->middleware('auth:w", vec!["web"]),
        ("name('home')->middleware('auth:w", vec!["web"]),
        ("name('multiline')\n    ->middleware('auth:a", vec!["admin"]),
        ("return 1; })->middleware('auth:w", vec!["web"]),
        ("keep chaining\n    ->middleware('auth:w", vec!["web"]),
        ("Facades\\Cache::store('a", vec!["array"]),
        ("\\Cache::store('r", vec!["redis"]),
        ("\\Route::middleware('auth:a", vec!["admin"]),
        ("CacheAttribute(store: 'r", vec!["redis"]),
        ("Deprecated('#['), CacheAttribute('r", vec!["redis"]),
        ("#[ CacheAttribute('a", vec!["array"]),
    ] {
        let labels = completion_items(&backend, &uri, position_after(source, prefix))
            .await
            .into_iter()
            .map(|item| item.label)
            .collect::<Vec<_>>();
        assert_eq!(labels, expected, "completion at `{prefix}`");
    }

    for prefix in [
        "\nCache::store('r",
        "\nRoute::middleware('auth:a",
        "factory(LaravelRoute::class)->middleware('auth:w",
        "a()->b()->c()->d()->middleware('auth:w",
        "LaravelLog::stack('s",
        "LaravelLog::stack(['key",
        "LaravelCache::store(store: 'r",
        "LaravelCache::store(NAME: 'r",
        "LaravelCache::store('array', 'r",
        "#[Cache('r",
        "$this->middleware('auth:a",
    ] {
        let labels = completion_items(&backend, &uri, position_after(source, prefix)).await;
        assert!(
            labels
                .iter()
                .all(|item| !["admin", "array", "daily", "redis", "slack", "web"]
                    .contains(&item.label.as_str())),
            "invalid context `{prefix}` offered resource names: {labels:?}"
        );
    }
}

#[tokio::test]
async fn auth_helper_completion_respects_php_namespace_fallback_and_function_homonyms() {
    let global_fallback = r#"<?php
namespace App;

auth('');
"#;
    let (backend, _dir, uri) = open_workspace(global_fallback).await;
    let labels = completion_items(&backend, &uri, position_after(global_fallback, "auth('"))
        .await
        .into_iter()
        .map(|item| item.label)
        .collect::<Vec<_>>();
    assert_eq!(labels, ["admin", "web"]);

    let fully_qualified = r#"<?php
namespace App;

\auth('');
"#;
    let (backend, _dir, uri) = open_workspace(fully_qualified).await;
    let labels = completion_items(&backend, &uri, position_after(fully_qualified, "\\auth('"))
        .await
        .into_iter()
        .map(|item| item.label)
        .collect::<Vec<_>>();
    assert_eq!(labels, ["admin", "web"]);

    let imported_global_alias = r#"<?php
namespace App;

use function auth as laravel_auth;
laravel_auth('');
"#;
    let (backend, _dir, uri) = open_workspace(imported_global_alias).await;
    let labels = completion_items(
        &backend,
        &uri,
        position_after(imported_global_alias, "laravel_auth('"),
    )
    .await
    .into_iter()
    .map(|item| item.label)
    .collect::<Vec<_>>();
    assert_eq!(labels, ["admin", "web"]);

    for source in [
        r#"<?php
namespace App;

function auth(?string $guard = null): object { return new \stdClass(); }
auth('');
"#,
        r#"<?php
namespace App;

use function Vendor\auth as auth;
auth('');
"#,
        r#"<?php
namespace App;

use function Vendor\auth as laravel_auth;
laravel_auth('');
"#,
        r#"<?php
namespace App;

\Vendor\auth('');
"#,
    ] {
        let (backend, _dir, uri) = open_workspace(source).await;
        let call = if source.contains("laravel_auth('") {
            "laravel_auth('"
        } else if source.contains("\\Vendor\\auth('") {
            "\\Vendor\\auth('"
        } else {
            "auth('"
        };
        let labels = completion_items(&backend, &uri, position_after(source, call)).await;
        assert!(
            labels
                .iter()
                .all(|item| !["admin", "web"].contains(&item.label.as_str())),
            "a local/imported auth() homonym offered guards: {labels:?}"
        );
    }
}

#[tokio::test]
async fn root_facade_aliases_work_inside_a_bracketed_global_namespace() {
    let source = r#"<?php
namespace Vendor { class Marker {} }
namespace {
    Cache::store('r');
    Route::middleware('auth:w');
}
"#;
    let (backend, _dir, uri) = open_workspace(source).await;
    for (prefix, expected) in [
        ("Cache::store('r", vec!["redis"]),
        ("middleware('auth:w", vec!["web"]),
    ] {
        let labels = completion_items(&backend, &uri, position_after(source, prefix))
            .await
            .into_iter()
            .map(|item| item.label)
            .collect::<Vec<_>>();
        assert_eq!(labels, expected, "completion at `{prefix}`");
    }
}

#[tokio::test]
async fn a_cross_file_namespaced_auth_helper_shadows_laravels_global_fallback() {
    let source = r#"<?php
namespace App;

auth('web');
"#;
    let helper = r#"<?php
namespace App;

function auth(?string $guard = null): object { return new \stdClass(); }
"#;
    let files = workspace_files(source)
        .into_iter()
        .chain(std::iter::once(("app/helpers.php", helper)))
        .collect::<Vec<_>>();
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, &files);
    backend.initialized(InitializedParams {}).await;
    let uri = Url::from_file_path(dir.path().join("app/NamedResourceConsumer.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: source.to_string(),
            },
        })
        .await;
    let position = position_after(source, "auth('we");

    let labels = completion_items(&backend, &uri, position)
        .await
        .into_iter()
        .map(|item| item.label)
        .collect::<Vec<_>>();
    assert!(
        labels
            .iter()
            .all(|label| label != "web" && label != "admin"),
        "project auth() helper offered Laravel guards: {labels:?}"
    );
    assert!(
        backend
            .handle_hover(uri.as_str(), source, position)
            .is_none()
    );
    assert!(
        backend
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .expect("definition request should succeed")
            .is_none()
    );
    assert!(
        backend
            .find_references(uri.as_str(), source, position, true)
            .is_none()
    );
}

#[tokio::test]
async fn real_global_classes_shadow_laravels_optional_facade_aliases() {
    let source = r#"<?php
namespace App;

\Cache::store('redis');
\Route::middleware('auth:web');
"#;
    let global_classes = r#"<?php
class Cache { public static function store(string $name): void {} }
class Route { public static function middleware(string $name): void {} }
"#;
    let files = workspace_files(source)
        .into_iter()
        .chain(std::iter::once((
            "app/GlobalFacadeHomonyms.php",
            global_classes,
        )))
        .collect::<Vec<_>>();
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, &files);
    backend.initialized(InitializedParams {}).await;
    let uri = Url::from_file_path(dir.path().join("app/NamedResourceConsumer.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: source.to_string(),
            },
        })
        .await;

    for prefix in ["\\Cache::store('red", "\\Route::middleware('auth:we"] {
        let position = position_after(source, prefix);
        let labels = completion_items(&backend, &uri, position).await;
        assert!(
            labels
                .iter()
                .all(|item| !["admin", "redis", "web"].contains(&item.label.as_str())),
            "global facade homonym offered Laravel resources: {labels:?}"
        );
        assert!(
            backend
                .handle_hover(uri.as_str(), source, position)
                .is_none()
        );
        assert!(
            backend
                .find_references(uri.as_str(), source, position, true)
                .is_none()
        );
    }

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), source, &mut diagnostics);
    assert!(diagnostics.iter().all(|diagnostic| {
        !matches!(
            &diagnostic.code,
            Some(NumberOrString::String(code)) if code.starts_with("invalid_laravel_")
        )
    }));
}

#[tokio::test]
async fn family_hovers_and_diagnostics_use_specific_labels_codes_and_ranges() {
    let source = r#"<?php
use Illuminate\Support\Facades\Auth;
use Illuminate\Support\Facades\Broadcast;
use Illuminate\Support\Facades\Cache;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Facades\Mail;
use Illuminate\Support\Facades\Queue;
use Illuminate\Support\Facades\Storage;
use Illuminate\Support\Facades\Route;

Auth::guard('web');
Route::middleware('auth:web, missing-auth');
Cache::store('redis');
Cache::store('missing-cache');
Log::channel('daily');
Log::stack(['missing-log']);
Storage::disk('local');
Storage::disk('missing-disk');
DB::connection('mysql');
DB::connection('missing-database');
Queue::connection('sync');
Queue::connection('missing-queue');
Mail::mailer('smtp');
Mail::mailer('missing-mailer');
Broadcast::connection('reverb');
Broadcast::connection('missing-broadcast');
"#;
    let (backend, _dir, uri) = open_workspace(source).await;

    for (prefix, label, key, config_file, config_content) in [
        (
            "Auth::guard('we",
            "Auth guard",
            "web",
            "auth.php",
            AUTH_CONFIG,
        ),
        (
            "Cache::store('red",
            "Cache store",
            "redis",
            "cache.php",
            CACHE_CONFIG,
        ),
        (
            "Log::channel('dai",
            "Log channel",
            "daily",
            "logging.php",
            LOGGING_CONFIG,
        ),
        (
            "Storage::disk('loc",
            "Storage disk",
            "local",
            "filesystems.php",
            FILESYSTEMS_CONFIG,
        ),
        (
            "DB::connection('mys",
            "Database connection",
            "mysql",
            "database.php",
            DATABASE_CONFIG,
        ),
        (
            "Queue::connection('sy",
            "Queue connection",
            "sync",
            "queue.php",
            QUEUE_CONFIG,
        ),
        (
            "Mail::mailer('sm",
            "Mailer",
            "smtp",
            "mail.php",
            MAIL_CONFIG,
        ),
        (
            "Broadcast::connection('rev",
            "Broadcast connection",
            "reverb",
            "broadcasting.php",
            BROADCASTING_CONFIG,
        ),
    ] {
        let position = position_after(source, prefix);
        let hover = backend
            .handle_hover(uri.as_str(), source, position)
            .unwrap_or_else(|| panic!("missing hover at `{prefix}`"));
        let text = hover_text(hover);
        assert!(text.contains(&format!("**{label}** `{key}`")), "{text}");
        assert!(text.contains("config/"), "{text}");

        let response = backend
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .expect("definition request should succeed")
            .unwrap_or_else(|| panic!("missing definition at `{prefix}`"));
        let location = definition_location(response);
        assert!(
            location
                .uri
                .path()
                .ends_with(&format!("/config/{config_file}"))
        );
        assert_eq!(
            location.range.start,
            position_of_config_key(config_content, key)
        );
    }

    let unresolved_hover = backend
        .handle_hover(
            uri.as_str(),
            source,
            position_after(source, "Cache::store('missing-ca"),
        )
        .expect("an unresolved resource retains its family hover");
    let unresolved_text = hover_text(unresolved_hover);
    assert!(
        unresolved_text.contains("**Cache store** `missing-cache`")
            && unresolved_text.contains("Laravel cache store"),
        "{unresolved_text}"
    );

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), source, &mut diagnostics);
    let invalid = diagnostics
        .iter()
        .filter_map(|diagnostic| match &diagnostic.code {
            Some(NumberOrString::String(code)) if code.starts_with("invalid_laravel_") => {
                Some((code.as_str(), text_in_range(source, diagnostic.range)))
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(invalid.len(), 8, "diagnostics: {invalid:#?}");

    for (code, payload) in [
        ("invalid_laravel_auth_guard", " missing-auth"),
        ("invalid_laravel_cache_store", "missing-cache"),
        ("invalid_laravel_log_channel", "missing-log"),
        ("invalid_laravel_storage_disk", "missing-disk"),
        ("invalid_laravel_database_connection", "missing-database"),
        ("invalid_laravel_queue_connection", "missing-queue"),
        ("invalid_laravel_mailer", "missing-mailer"),
        ("invalid_laravel_broadcast_connection", "missing-broadcast"),
    ] {
        assert_eq!(
            invalid.iter().find_map(|(actual_code, actual_payload)| {
                (*actual_code == code).then_some(actual_payload)
            }),
            Some(&payload),
            "diagnostics: {invalid:#?}"
        );
    }
}

#[tokio::test]
async fn parameter_attributes_share_completion_navigation_diagnostics_and_references() {
    let source = r#"<?php
namespace App;

use Illuminate\Container\Attributes\Cache as CacheAttribute;

class InjectedResources {
    public function __construct(
        #[CacheAttribute(store: 'redis')] mixed $valid,
        #[CacheAttribute('missing-store')] mixed $invalid,
    ) {}
}

config('cache.stores.redis');
"#;
    let (backend, _dir, uri) = open_workspace(source).await;
    let valid = position_after(source, "store: 'red");

    let labels = completion_items(&backend, &uri, valid)
        .await
        .into_iter()
        .map(|item| item.label)
        .collect::<Vec<_>>();
    assert_eq!(labels, ["redis"]);

    let hover = backend
        .handle_hover(uri.as_str(), source, valid)
        .expect("parameter attribute should hover as a cache store");
    assert!(hover_text(hover).contains("**Cache store** `redis`"));

    let definition = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: valid,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("definition request should succeed")
        .expect("parameter attribute should navigate");
    let definition = definition_location(definition);
    assert!(definition.uri.path().ends_with("/config/cache.php"));
    assert_eq!(
        definition.range.start,
        position_of_config_key(CACHE_CONFIG, "redis")
    );

    let references = backend
        .find_references(uri.as_str(), source, valid, true)
        .expect("parameter attribute should share config references");
    assert_eq!(references.len(), 3, "references: {references:#?}");

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), source, &mut diagnostics);
    let invalid = diagnostics
        .iter()
        .find(|diagnostic| {
            matches!(
                &diagnostic.code,
                Some(NumberOrString::String(code)) if code == "invalid_laravel_cache_store"
            )
        })
        .expect("invalid parameter attribute should be diagnosed");
    assert_eq!(text_in_range(source, invalid.range), "missing-store");
}

#[tokio::test]
async fn database_roles_and_runtime_null_drivers_follow_laravel_semantics() {
    let source = r#"<?php
use Illuminate\Support\Facades\Cache;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Queue;

DB::connection('mysql::read');
DB::connection('mysql::write');
DB::connection('mysql::direct');
DB::connection('mysql::replica');
config('database.connections.mysql');
Cache::store('null');
Queue::connection('null');
"#;
    let (backend, _dir, uri) = open_workspace(source).await;

    let labels = completion_items(
        &backend,
        &uri,
        position_after(source, "DB::connection('mysql::"),
    )
    .await
    .into_iter()
    .map(|item| item.label)
    .collect::<Vec<_>>();
    assert_eq!(labels, ["mysql::read", "mysql::write", "mysql::direct"]);

    let read = position_after(source, "DB::connection('mysql::rea");
    let hover = backend
        .handle_hover(uri.as_str(), source, read)
        .expect("role-qualified database connection should hover");
    assert!(hover_text(hover).contains("**Database connection** `mysql::read`"));
    let definition = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: read,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("definition request should succeed")
        .expect("role-qualified database connection should navigate");
    assert_eq!(
        definition_location(definition).range.start,
        position_of_config_key(DATABASE_CONFIG, "mysql")
    );

    let references = backend
        .find_references(uri.as_str(), source, read, true)
        .expect("all database roles should share one config identity");
    assert_eq!(references.len(), 5, "references: {references:#?}");

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), source, &mut diagnostics);
    let invalid = diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                &diagnostic.code,
                Some(NumberOrString::String(code)) if code.starts_with("invalid_laravel_")
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(invalid.len(), 1, "diagnostics: {invalid:#?}");
    assert_eq!(text_in_range(source, invalid[0].range), "mysql::replica");

    for marker in ["Cache::store('nul", "Queue::connection('nul"] {
        let position = position_after(source, marker);
        let references = backend
            .find_references(uri.as_str(), source, position, true)
            .expect("implicit null driver should retain direct references");
        assert_eq!(references.len(), 1, "references at `{marker}`");
    }
}

#[tokio::test]
async fn generic_config_diagnostics_accept_only_exact_segment_prefixes() {
    let source = "<?php\nconfig('app.mail');\nconfig('app.ma');\n";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            (
                "config/app.php",
                "<?php return ['mail' => ['from' => 'team@example.com']];\n",
            ),
            ("app/NamedResourceConsumer.php", source),
        ],
    );
    backend.initialized(InitializedParams {}).await;
    let uri = Url::from_file_path(dir.path().join("app/NamedResourceConsumer.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: source.to_string(),
            },
        })
        .await;

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), source, &mut diagnostics);
    let invalid = diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                &diagnostic.code,
                Some(NumberOrString::String(code)) if code == "invalid_laravel_config"
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(invalid.len(), 1, "diagnostics: {invalid:#?}");
    assert_eq!(text_in_range(source, invalid[0].range), "app.ma");
}

#[tokio::test]
async fn library_resource_names_belong_to_the_host_application() {
    let source = r#"<?php
use Illuminate\Support\Facades\Auth;
use Illuminate\Support\Facades\Broadcast;
use Illuminate\Support\Facades\Cache;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Facades\Mail;
use Illuminate\Support\Facades\Queue;
use Illuminate\Support\Facades\Storage;

Auth::guard('host-guard');
Broadcast::connection('host-broadcaster');
Cache::store('host-cache');
DB::connection('host-database');
Log::channel('host-log');
Mail::mailer('host-mailer');
Queue::connection('host-queue');
Storage::disk('host-disk');
config('cache.stores.host-cache');
Cache::store('r');
"#;
    let (backend, dir) = create_psr4_workspace(PACKAGE_COMPOSER_JSON, &workspace_files(source));
    backend.initialized(InitializedParams {}).await;
    let uri = Url::from_file_path(dir.path().join("app/NamedResourceConsumer.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: source.to_string(),
            },
        })
        .await;

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), source, &mut diagnostics);
    let invalid = diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                &diagnostic.code,
                Some(NumberOrString::String(code)) if code.starts_with("invalid_laravel_")
            )
        })
        .collect::<Vec<_>>();
    assert!(invalid.is_empty(), "diagnostics: {invalid:#?}");

    let labels = completion_items(&backend, &uri, position_after(source, "Cache::store('r"))
        .await
        .into_iter()
        .map(|item| item.label)
        .collect::<Vec<_>>();
    assert_eq!(labels, ["redis"]);
}

#[tokio::test]
async fn runtime_config_writes_validate_resources_across_files_at_segment_boundaries() {
    let writer = r#"<?php
namespace App;
use Illuminate\Support\Facades\Config;

class ResourceConfiguration {
    public function configure(array $channels, array $filesystems): void {
        Config::set('auth.guards.session', ['driver' => 'session']);
        config(['cache.stores.temp.driver' => 'array']);
        Config::set('logging.channels', $channels);
        Config::set('filesystems', $filesystems);
        Config::set('database.connections.reporting', ['driver' => 'mysql']);
        Config::set('queue.connections.custom', ['driver' => 'sync']);
        Config::set('mail.mailers.outbound.transport', 'smtp');
        Config::set('broadcasting.connections.realtime', ['driver' => 'reverb']);
        Config::set('external.runtime', []);
    }
}
"#;
    let source = r#"<?php
use Illuminate\Support\Facades\Auth;
use Illuminate\Support\Facades\Broadcast;
use Illuminate\Support\Facades\Cache;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Facades\Mail;
use Illuminate\Support\Facades\Queue;
use Illuminate\Support\Facades\Storage;

Auth::guard('session');
Auth::guard('session-typo');
Cache::store('temp');
Cache::store('tem');
Cache::store('temporary');
Log::channel('dynamic');
Storage::disk('dynamic');
DB::connection('reporting::read');
DB::connection('reporting::replica');
Queue::connection('custom');
Queue::connection('custom-typo');
Mail::mailer('outbound');
Mail::mailer('out');
Broadcast::connection('realtime');
Broadcast::connection('realtime-typo');
config('cache.stores.temp');
config('cache.stores.temp.driver');
config('cache.stores.temp.driver.option');
config('cache.stores.te');
config('cache.stores.temporary.driver');
config('logging.channels.dynamic');
config('filesystems.disks.dynamic');
config('external.unknown');
"#;
    let mut files = workspace_files(source);
    files.push(("app/ResourceConfiguration.php", writer));
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, &files);
    backend.initialized(InitializedParams {}).await;
    let uri = Url::from_file_path(dir.path().join("app/NamedResourceConsumer.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: source.to_string(),
            },
        })
        .await;

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), source, &mut diagnostics);
    let invalid = diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                &diagnostic.code,
                Some(NumberOrString::String(code)) if code.starts_with("invalid_laravel_")
            )
        })
        .map(|diagnostic| text_in_range(source, diagnostic.range))
        .collect::<Vec<_>>();
    assert_eq!(
        invalid,
        [
            "session-typo",
            "tem",
            "temporary",
            "reporting::replica",
            "custom-typo",
            "out",
            "realtime-typo",
            "cache.stores.te",
            "cache.stores.temporary.driver",
        ]
    );
}

#[tokio::test]
async fn an_undiscovered_resource_subtree_is_not_treated_as_a_closed_empty_set() {
    let source = r#"<?php
use Illuminate\Support\Facades\Cache;

config(['cache.stores.discovered-at-runtime' => ['driver' => 'array']]);
Cache::store('provided-at-runtime');
"#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("config/cache.php", "<?php return ['default' => 'array'];\n"),
            ("app/NamedResourceConsumer.php", source),
        ],
    );
    backend.initialized(InitializedParams {}).await;
    let uri = Url::from_file_path(dir.path().join("app/NamedResourceConsumer.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: source.to_string(),
            },
        })
        .await;

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), source, &mut diagnostics);
    assert!(diagnostics.iter().all(|diagnostic| {
        !matches!(
            &diagnostic.code,
            Some(NumberOrString::String(code)) if code == "invalid_laravel_cache_store"
        )
    }));
}

#[tokio::test]
async fn direct_and_generic_config_spellings_share_one_reference_set() {
    let source = r#"<?php
use Illuminate\Support\Facades\Cache;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Queue;

Cache::store('redis');
config('cache.stores.redis');
DB::connection('mysql');
config('database.connections.mysql');
Queue::connection('redis');
config('queue.connections.redis');
"#;
    let (backend, _dir, uri) = open_workspace(source).await;

    for (direct, generic) in [
        ("Cache::store('red", "config('cache.stores.red"),
        ("DB::connection('mys", "config('database.connections.mys"),
        ("Queue::connection('red", "config('queue.connections.red"),
    ] {
        let from_direct = backend
            .find_references(uri.as_str(), source, position_after(source, direct), true)
            .expect("direct resource should have references");
        let from_generic = backend
            .find_references(uri.as_str(), source, position_after(source, generic), true)
            .expect("generic config key should have references");
        assert_eq!(from_direct, from_generic, "references for `{direct}`");
        assert_eq!(from_direct.len(), 3, "references for `{direct}`");
        assert_eq!(
            backend
                .find_references(uri.as_str(), source, position_after(source, direct), false)
                .expect("usages should remain without the declaration")
                .len(),
            2,
            "usage-only references for `{direct}`"
        );
    }
}

#[tokio::test]
async fn config_resource_rename_stays_disabled_for_both_source_spellings() {
    let source = r#"<?php
use Illuminate\Support\Facades\Cache;

Cache::store('redis');
config('cache.stores.redis');
"#;
    let (backend, _dir, uri) = open_workspace(source).await;

    for marker in ["Cache::store('red", "config('cache.stores.red"] {
        let edit = backend
            .rename(RenameParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: position_after(source, marker),
                },
                new_name: "memcached".to_string(),
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .expect("rename request should succeed");
        assert!(edit.is_none(), "rename unexpectedly enabled at `{marker}`");
    }
}

#[tokio::test]
async fn auth_and_log_array_spellings_share_references_with_generic_config_keys() {
    let source = r#"<?php
use Illuminate\Support\Facades\Auth;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Facades\Route;

auth('web');
Auth::guard('web');
Route::middleware('auth:web');
class Guarded {
    public function __construct(
        #[\Illuminate\Container\Attributes\Auth('web')] mixed $guard,
    ) {}
}
config('auth.guards.web');

Log::stack(['daily']);
config('logging.channels.daily');
"#;
    let (backend, _dir, uri) = open_workspace(source).await;

    let auth_positions = [
        "auth('we",
        "Auth::guard('we",
        "middleware('auth:we",
        "Attributes\\Auth('we",
        "config('auth.guards.we",
    ];
    let expected_auth = backend
        .find_references(
            uri.as_str(),
            source,
            position_after(source, auth_positions[0]),
            true,
        )
        .expect("auth guard should have references");
    assert_eq!(
        expected_auth.len(),
        6,
        "auth references: {expected_auth:#?}"
    );
    for prefix in &auth_positions[1..] {
        assert_eq!(
            backend
                .find_references(uri.as_str(), source, position_after(source, prefix), true,)
                .expect("auth guard spelling should have references"),
            expected_auth,
            "auth references from `{prefix}`"
        );
    }

    let log_from_array = backend
        .find_references(
            uri.as_str(),
            source,
            position_after(source, "Log::stack(['dai"),
            true,
        )
        .expect("Log::stack value should have references");
    let log_from_config = backend
        .find_references(
            uri.as_str(),
            source,
            position_after(source, "config('logging.channels.dai"),
            true,
        )
        .expect("generic logging config key should have references");
    assert_eq!(log_from_array, log_from_config);
    assert_eq!(
        log_from_array.len(),
        3,
        "log references: {log_from_array:#?}"
    );
}
