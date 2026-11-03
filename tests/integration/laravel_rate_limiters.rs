//! Provider-defined rate limiter names across Laravel's string features.

use super::common::*;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": {"laravel/framework": "^12.0"},
    "autoload": {"psr-4": {"App\\": "src/"}}
}"#;

const PROVIDER: &str = r#"<?php
namespace App\Providers;
use Illuminate\Support\Facades\RateLimiter as Limits;
class AppServiceProvider {
    public function boot(): void {
        Limits::for('uploads', fn () => null);
        Limits::FOR(callback: fn () => null, name: 'backups');
    }
}
"#;

const PROVIDER_LIST: &str = "<?php return [App\\Providers\\AppServiceProvider::class];";

async fn workspace(
    source: &str,
    provider: &str,
    extra: &[(&str, &str)],
) -> (phpantom_lsp::Backend, tempfile::TempDir, Uri) {
    let mut files = vec![
        ("bootstrap/providers.php", PROVIDER_LIST),
        ("src/Providers/AppServiceProvider.php", provider),
        ("src/test.php", source),
    ];
    files.extend_from_slice(extra);
    create_initialized_psr4_workspace(COMPOSER, &files, "src/test.php").await
}

#[tokio::test]
async fn rate_limiters_complete_only_the_throttle_parameter() {
    let source =
        "<?php use Illuminate\\Support\\Facades\\Route; Route::middleware(['web', 'throttle:up']);";
    let (backend, _dir, uri) = workspace(source, PROVIDER, &[]).await;
    let pos = position_after(source, "throttle:up");
    let items = complete_at_opened(&backend, &uri, pos.line, pos.character).await;
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0].label, "uploads");
    let CompletionItemTextEdit::TextEdit(edit) = items[0].text_edit.as_ref().unwrap() else {
        panic!("text edit")
    };
    assert_eq!(edit.range.start, position_after(source, "throttle:"));
    assert_eq!(edit.new_text, "uploads");
}

#[tokio::test]
async fn rate_limiters_complete_queue_constructors_and_named_arguments() {
    for expression in [
        "new RateLimited('')",
        "new RateLimited(limiterName: '')",
        "new RedisLimit('')",
        "new \\Illuminate\\Queue\\Middleware\\RateLimited('')",
    ] {
        let source = format!(
            "<?php namespace Jobs; use Illuminate\\Queue\\Middleware\\RateLimited; use Illuminate\\Queue\\Middleware\\RateLimitedWithRedis as RedisLimit; {expression};"
        );
        let (backend, _dir, uri) = workspace(&source, PROVIDER, &[]).await;
        let pos = position_after(&source, "'");
        let labels = complete_labels_at_opened(&backend, &uri, pos.line, pos.character).await;
        assert_eq!(labels, ["backups", "uploads"], "{expression}");
    }
}

#[tokio::test]
async fn rate_limiters_navigate_and_hover_on_uses() {
    let source = r#"<?php
use Illuminate\Support\Facades\Route;
use Illuminate\Queue\Middleware\RateLimited;
Route::get('/', fn () => 'ok')->middleware('throttle:uploads');
new RateLimited(limiterName: 'backups');
"#;
    let (backend, _dir, uri) = workspace(source, PROVIDER, &[]).await;
    for (needle, name) in [("throttle:upl", "uploads"), ("'back", "backups")] {
        let pos = position_after(source, needle);
        let locations =
            definition_locations(goto_definition_at(&backend, &uri, pos.line, pos.character).await);
        assert_eq!(locations.len(), 1, "{needle}: {locations:?}");
        assert!(
            locations[0]
                .uri
                .path()
                .to_string()
                .ends_with("src/Providers/AppServiceProvider.php")
        );
        let mut expected = position_after(PROVIDER, &format!("'{name}"));
        expected.character -= name.len() as u32;
        assert_eq!(locations[0].range.start, expected);
        let hover = hover_text_at(&backend, &uri, pos.line, pos.character)
            .await
            .unwrap();
        assert!(hover.contains("Rate limiter"), "{hover}");
        assert!(hover.contains("AppServiceProvider.php"), "{hover}");
    }
}

#[tokio::test]
async fn rate_limiters_unknown_names_hover_without_a_declaration() {
    let source = "<?php new \\Illuminate\\Queue\\Middleware\\RateLimited('unknown');";
    let (backend, _dir, uri) = workspace(source, PROVIDER, &[]).await;
    let pos = position_after(source, "'unk");
    let hover = hover_text_at(&backend, &uri, pos.line, pos.character)
        .await
        .unwrap();
    assert!(hover.contains("Rate limiter"), "{hover}");
    assert!(!hover.contains("Registered in"), "{hover}");
    assert!(
        definition_locations(goto_definition_at(&backend, &uri, pos.line, pos.character).await)
            .is_empty()
    );
}

#[tokio::test]
async fn rate_limiters_diagnose_unknown_names_and_preserve_numeric_limits() {
    let source = r#"<?php
use Illuminate\Support\Facades\Route;
use Illuminate\Queue\Middleware\RateLimited;
Route::middleware(['throttle:uploads', 'throttle:missing', 'throttle:60,1', 'throttle:60', 'throttle:1e2', 'throttle:10|60', 'throttle:60,1,prefix']);
new RateLimited('backups');
new RateLimited('typo');
"#;
    let (backend, _dir, uri) = workspace(source, PROVIDER, &[]).await;
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), source, &mut diagnostics);
    let diagnostics = with_code(diagnostics, "invalid_laravel_rate_limiter");
    assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
    assert_eq!(
        message_text(&diagnostics[0].message),
        "Unknown rate limiter: 'missing'"
    );
    assert_eq!(
        diagnostics[0].range.start,
        position_after(source, "'throttle:uploads', 'throttle:")
    );
    assert_eq!(
        message_text(&diagnostics[1].message),
        "Unknown rate limiter: 'typo'"
    );
}

#[tokio::test]
async fn rate_limiters_leave_unread_and_dynamic_registration_sets_open() {
    let source = "<?php new \\Illuminate\\Queue\\Middleware\\RateLimited('unknown');";
    for (provider, list) in [
        (PROVIDER.to_string(), "<?php return [];"),
        (
            PROVIDER.to_string(),
            "<?php return [Missing\\Provider::class];",
        ),
        (
            PROVIDER.replace("'uploads'", "$configuredName"),
            PROVIDER_LIST,
        ),
        (
            PROVIDER.replace("'uploads'", "'up' . 'loads'"),
            PROVIDER_LIST,
        ),
        (
            PROVIDER.to_string(),
            "<?php return [App\\Providers\\AppServiceProvider::class, Missing\\Provider::class];",
        ),
    ] {
        let (backend, _dir, uri) =
            workspace(source, &provider, &[("bootstrap/providers.php", list)]).await;
        let messages = slow_diagnostic_messages(
            &backend,
            uri.as_str(),
            source,
            "invalid_laravel_rate_limiter",
        );
        assert!(messages.is_empty(), "{provider} / {list}: {messages:?}");
    }
}

#[tokio::test]
async fn rate_limiters_follow_provider_helpers_and_namespace_aliases() {
    let source = "<?php new \\Illuminate\\Queue\\Middleware\\RateLimited('');";
    let provider = r#"<?php namespace App\Providers;
use App\Limits;
class AppServiceProvider { public function boot() { Limits::register(); } }
"#;
    let helper = r#"<?php namespace App;
use Illuminate\Support\Facades as Facades;
class Limits { public static function register() { Facades\RateLimiter:: /* call */ for ('reports', fn () => null); } }
"#;
    let (backend, _dir, uri) = workspace(source, provider, &[("src/Limits.php", helper)]).await;
    let pos = position_after(source, "'");
    let labels = complete_labels_at_opened(&backend, &uri, pos.line, pos.character).await;
    assert_eq!(labels, ["reports"]);
}

#[tokio::test]
async fn rate_limiters_ignore_unregistered_providers_and_homonyms() {
    let source = r#"<?php namespace App;
use Illuminate\Support\Facades\Route;
new RateLimited('missing');
new \RateLimited('missing');
new \Other\RateLimited('missing');
new \Illuminate\Queue\Middleware\RateLimited(other: 'missing');
Route::middleware('throttle:fake');
"#;
    let homonym = r#"<?php namespace App\Providers;
class AppServiceProvider { public function boot() { RateLimiter::for('fake', fn () => null); \RateLimiter::for('also-fake', fn () => null); } }
"#;
    let (backend, _dir, uri) = workspace(
        source,
        homonym,
        &[("src/Providers/UnusedProvider.php", PROVIDER)],
    )
    .await;
    assert_eq!(
        slow_diagnostic_messages(
            &backend,
            uri.as_str(),
            source,
            "invalid_laravel_rate_limiter"
        ),
        ["Unknown rate limiter: 'fake'"]
    );
}

#[tokio::test]
async fn rate_limiters_refresh_after_provider_buffer_edits() {
    let source = "<?php new \\Illuminate\\Queue\\Middleware\\RateLimited('uploads');";
    let (backend, dir, uri) = workspace(source, PROVIDER, &[]).await;
    assert!(
        slow_diagnostic_messages(
            &backend,
            uri.as_str(),
            source,
            "invalid_laravel_rate_limiter"
        )
        .is_empty()
    );
    let changed = PROVIDER.replace("'uploads'", "'pictures'");
    open_php_at(
        &backend,
        &dir,
        "src/Providers/AppServiceProvider.php",
        &changed,
    )
    .await;
    assert_eq!(
        slow_diagnostic_messages(
            &backend,
            uri.as_str(),
            source,
            "invalid_laravel_rate_limiter"
        ),
        ["Unknown rate limiter: 'uploads'"]
    );
    open_php_at(
        &backend,
        &dir,
        "src/Providers/AppServiceProvider.php",
        PROVIDER,
    )
    .await;
    assert!(
        slow_diagnostic_messages(
            &backend,
            uri.as_str(),
            source,
            "invalid_laravel_rate_limiter"
        )
        .is_empty()
    );
}

#[tokio::test]
async fn rate_limiters_references_include_declarations_and_both_consumers() {
    let source = "<?php use Illuminate\\Support\\Facades\\Route; Route::middleware('throttle:uploads'); new \\Illuminate\\Queue\\Middleware\\RateLimited('uploads');";
    let (backend, _dir, uri) = workspace(source, PROVIDER, &[]).await;
    let pos = position_after(source, "throttle:up");
    let locations = references_response(&backend, &uri, pos, true)
        .await
        .unwrap_or_default();
    assert_eq!(locations.len(), 3, "{locations:?}");
    assert_eq!(
        locations
            .iter()
            .filter(|location| location.uri == uri)
            .count(),
        2
    );
}

#[tokio::test]
async fn rate_limiters_are_gated_on_a_laravel_project() {
    let source = "<?php new \\Illuminate\\Queue\\Middleware\\RateLimited('missing');";
    let (backend, _dir, uri) = create_initialized_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        &[
            ("src/test.php", source),
            ("bootstrap/providers.php", PROVIDER_LIST),
            ("src/Providers/AppServiceProvider.php", PROVIDER),
        ],
        "src/test.php",
    )
    .await;
    assert!(
        slow_diagnostic_messages(
            &backend,
            uri.as_str(),
            source,
            "invalid_laravel_rate_limiter"
        )
        .is_empty()
    );
    let pos = position_after(source, "'mi");
    assert!(
        definition_locations(goto_definition_at(&backend, &uri, pos.line, pos.character).await)
            .is_empty()
    );
}

#[tokio::test]
async fn rate_limiters_decode_literals_and_escape_completion_edits() {
    let provider = PROVIDER.replace("'uploads'", r"'baker\'s\\uploads'");
    for (expression, needle, replacement) in [
        (
            r"Route::middleware('throttle:baker\'s');",
            r"throttle:baker\'s",
            r"baker\'s\\uploads",
        ),
        (
            r#"new \Illuminate\Queue\Middleware\RateLimited("baker");"#,
            "\"baker",
            r"baker's\\uploads",
        ),
    ] {
        let source = format!("<?php use Illuminate\\Support\\Facades\\Route; {expression}");
        let (backend, _dir, uri) = workspace(&source, &provider, &[]).await;
        let pos = position_after(&source, needle);
        let items = complete_at_opened(&backend, &uri, pos.line, pos.character).await;
        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0].label, r"baker's\uploads");
        let CompletionItemTextEdit::TextEdit(edit) = items[0].text_edit.as_ref().unwrap() else {
            panic!("text edit")
        };
        assert_eq!(edit.new_text, replacement);
    }
    let source = r"<?php use Illuminate\Support\Facades\Route; Route::middleware('throttle:baker\'s\\uploads');";
    let (backend, _dir, uri) = workspace(source, &provider, &[]).await;
    assert!(
        slow_diagnostic_messages(
            &backend,
            uri.as_str(),
            source,
            "invalid_laravel_rate_limiter"
        )
        .is_empty()
    );
    let pos = position_after(source, "throttle:baker");
    assert_eq!(
        definition_locations(goto_definition_at(&backend, &uri, pos.line, pos.character).await)
            .len(),
        1
    );
}

#[tokio::test]
async fn rate_limiters_support_controller_middleware_attributes_and_objects() {
    let source = r#"<?php
use Illuminate\Routing\Attributes\Controllers\Middleware as UsesMiddleware;
use Illuminate\Routing\Controllers\Middleware;
#[UsesMiddleware(['throttle:uploads'])]
class Controller {
    public static function middleware(): array { return [new Middleware(['throttle:backups'])]; }
}
"#;
    let (backend, _dir, uri) = workspace(source, PROVIDER, &[]).await;
    for needle in ["throttle:up", "throttle:back"] {
        let pos = position_after(source, needle);
        assert_eq!(
            definition_locations(goto_definition_at(&backend, &uri, pos.line, pos.character).await)
                .len(),
            1
        );
        let labels = complete_labels_at_opened(&backend, &uri, pos.line, pos.character).await;
        assert_eq!(labels.len(), 1, "{needle}: {labels:?}");
    }
}

#[tokio::test]
async fn rate_limiters_refresh_when_the_provider_list_or_helper_changes() {
    let source = "<?php new \\Illuminate\\Queue\\Middleware\\RateLimited('');";
    let provider = r"<?php namespace App\Providers; use App\Limits; class AppServiceProvider { public function boot() { Limits::register(); } }";
    let helper = r"<?php namespace App; class Limits { public static function register() { \Illuminate\Support\Facades\RateLimiter::for('helper', fn () => null); } }";
    let (backend, dir, uri) = workspace(source, provider, &[("src/Limits.php", helper)]).await;
    let pos = position_after(source, "'");
    assert_eq!(
        complete_labels_at_opened(&backend, &uri, pos.line, pos.character).await,
        ["helper"]
    );
    open_php_at(
        &backend,
        &dir,
        "src/Limits.php",
        &helper.replace("'helper'", "'edited'"),
    )
    .await;
    assert_eq!(
        complete_labels_at_opened(&backend, &uri, pos.line, pos.character).await,
        ["edited"]
    );
    open_php_at(
        &backend,
        &dir,
        "bootstrap/providers.php",
        "<?php return [];",
    )
    .await;
    assert!(
        !complete_labels_at_opened(&backend, &uri, pos.line, pos.character)
            .await
            .iter()
            .any(|name| name == "edited")
    );
}

#[tokio::test]
async fn rate_limiters_discover_package_providers_and_only_their_own_helpers() {
    let installed = r#"{"packages":[
        {"name":"acme/limits","version":"1.0.0","install-path":"../acme/limits",
         "autoload":{"psr-4":{"Acme\\Limits\\":""}},
         "extra":{"laravel":{"providers":["Acme\\Limits\\Provider"]}}},
        {"name":"other/library","version":"1.0.0","install-path":"../other/library",
         "autoload":{"psr-4":{"Other\\":""}}}
    ]}"#;
    let provider = r"<?php namespace Acme\Limits; use Acme\Limits\Helper; use Other\Unregistered; class Provider { public function boot() { Helper::register(); Unregistered::unrelated(); } }";
    let helper = r"<?php namespace Acme\Limits; class Helper { public static function register() { \Illuminate\Support\Facades\RateLimiter::for('package', fn () => null); } }";
    let unrelated = r"<?php namespace Other; class Unregistered { public static function unrelated() {} public static function register() { \Illuminate\Support\Facades\RateLimiter::for('unrelated', fn () => null); } }";
    let source = "<?php new \\Illuminate\\Queue\\Middleware\\RateLimited('');";
    let (backend, _dir, uri) = workspace(
        source,
        PROVIDER,
        &[
            ("vendor/composer/installed.json", installed),
            ("vendor/acme/limits/Provider.php", provider),
            ("vendor/acme/limits/Helper.php", helper),
            ("vendor/other/library/Unregistered.php", unrelated),
        ],
    )
    .await;
    let pos = position_after(source, "'");
    assert_eq!(
        complete_labels_at_opened(&backend, &uri, pos.line, pos.character).await,
        ["backups", "package", "uploads"]
    );
}

#[tokio::test]
async fn rate_limiters_delete_registration_files_without_stale_names_or_errors() {
    let source = "<?php new \\Illuminate\\Queue\\Middleware\\RateLimited('uploads');";
    let (backend, dir, uri) = workspace(source, PROVIDER, &[]).await;
    let pos = position_after(source, "'up");
    assert_eq!(
        definition_locations(goto_definition_at(&backend, &uri, pos.line, pos.character).await)
            .len(),
        1
    );
    let provider_path = dir.path().join("src/Providers/AppServiceProvider.php");
    std::fs::remove_file(&provider_path).unwrap();
    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: Uri::from_file_path(provider_path).unwrap(),
                kind: FileChangeType::Deleted,
            }],
        })
        .await;
    assert!(
        definition_locations(goto_definition_at(&backend, &uri, pos.line, pos.character).await)
            .is_empty()
    );
    assert!(
        slow_diagnostic_messages(
            &backend,
            uri.as_str(),
            source,
            "invalid_laravel_rate_limiter"
        )
        .is_empty()
    );
}

#[tokio::test]
async fn rate_limiters_do_not_complete_numeric_parameters_or_unrelated_calls() {
    for (expression, needle) in [
        ("Route::middleware('throttle:60,');", "throttle:60,"),
        ("Route::middleware('throttle:60');", "throttle:60"),
        ("Route::middleware('other:');", "other:"),
        ("$custom->middleware('throttle:');", "throttle:"),
        ("new RateLimited('');", "'"),
        ("new \\RateLimited('');", "'"),
        (
            "new \\Illuminate\\Queue\\Middleware\\RateLimited(other: '');",
            "'",
        ),
        (
            "new \\Illuminate\\Queue\\Middleware\\RateLimited(['']);",
            "'",
        ),
        (
            "new \\Illuminate\\Queue\\Middleware\\RateLimited('ok', '');",
            "'ok', '",
        ),
    ] {
        let source =
            format!("<?php namespace App; use Illuminate\\Support\\Facades\\Route; {expression}");
        let (backend, _dir, uri) = workspace(&source, PROVIDER, &[]).await;
        let pos = position_after(&source, needle);
        let labels = complete_labels_at_opened(&backend, &uri, pos.line, pos.character).await;
        assert!(
            !labels
                .iter()
                .any(|name| matches!(name.as_str(), "uploads" | "backups")),
            "{source}: {labels:?}"
        );
    }
}

#[tokio::test]
async fn rate_limiters_complete_in_unfinished_constructor_strings() {
    for source in [
        "<?php use Illuminate\\Queue\\Middleware\\RateLimited; new RateLimited('up",
        "<?php new \\Illuminate\\Queue\\Middleware\\RateLimited(limiterName: 'up",
        "<?php use Illuminate\\Support\\Facades\\Route; Route::middleware('throttle:up",
    ] {
        let (backend, _dir, uri) = workspace(source, PROVIDER, &[]).await;
        let pos = offset_position(source, source.len());
        assert_eq!(
            complete_labels_at_opened(&backend, &uri, pos.line, pos.character).await,
            ["uploads"],
            "{source}"
        );
    }
}

#[tokio::test]
async fn rate_limiters_handle_sources_disappearing_before_discovery() {
    let provider = r"<?php namespace App\Providers; use App\Limits; class AppServiceProvider { public function boot() { Limits::register(); } }";
    let helper = r"<?php namespace App; class Limits { public static function register() { \Illuminate\Support\Facades\RateLimiter::for('helper', fn () => null); } }";
    let source = "<?php new \\Illuminate\\Queue\\Middleware\\RateLimited('unknown');";
    for path in ["src/Providers/AppServiceProvider.php", "src/Limits.php"] {
        let (backend, dir, uri) = workspace("<?php", provider, &[("src/Limits.php", helper)]).await;
        // The workspace index still knows the class, but the file vanished
        // before a watcher notification or the first limiter lookup.
        std::fs::remove_file(dir.path().join(path)).unwrap();
        open_php(&backend, &uri, source).await;
        assert!(
            slow_diagnostic_messages(
                &backend,
                uri.as_str(),
                source,
                "invalid_laravel_rate_limiter"
            )
            .is_empty(),
            "{path}"
        );
    }
}

#[tokio::test]
async fn rate_limiters_deduplicate_provider_files_and_shared_helpers() {
    let provider = r"<?php namespace App\Providers; use App\Limits;
class AppServiceProvider { public function boot() { Limits::register(); } }
class OtherProvider { public function boot() { Limits::register(); } }
";
    let extra_provider = r"<?php namespace App\Providers; use App\Limits; use App\Providers\AppServiceProvider;
class ExtraProvider { public function boot() { Limits::register(); AppServiceProvider::class; } }
";
    let helper = r"<?php namespace App; class Limits { public static function register() {
\Illuminate\Support\Facades\RateLimiter::for('shared', fn () => null);
\Illuminate\Support\Facades\RateLimiter::for('shared', fn () => null);
} }";
    let source = "<?php new \\Illuminate\\Queue\\Middleware\\RateLimited('shared');";
    let list = r"<?php return [App\Providers\AppServiceProvider::class, App\Providers\OtherProvider::class, App\Providers\ExtraProvider::class];";
    let (backend, _dir, uri) = workspace(
        source,
        provider,
        &[
            ("bootstrap/providers.php", list),
            ("src/Providers/ExtraProvider.php", extra_provider),
            ("src/Limits.php", helper),
        ],
    )
    .await;
    let pos = position_after(source, "'sha");
    assert_eq!(
        complete_labels_at_opened(&backend, &uri, pos.line, pos.character).await,
        ["shared"]
    );
    let locations =
        definition_locations(goto_definition_at(&backend, &uri, pos.line, pos.character).await);
    assert_eq!(locations.len(), 1);
    assert_eq!(
        locations[0].range.start.line, 2,
        "the later declaration replaces the first"
    );
}
