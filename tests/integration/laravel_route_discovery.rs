//! Tests for discovering route files that do not live under `routes/`.
//!
//! A project is free to keep its route files anywhere and wire them up from
//! a `RouteServiceProvider` — with `Route::…->group(base_path('…'))`, or with
//! a plain `require` inside a `Route::group([…], function () { … })` body.
//! Both forms must be followed, along with the group prefixes they apply,
//! or every `route('…')` call naming one of those routes is reported as
//! unknown.

use crate::common::{create_psr4_workspace, open_php};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

const PROVIDERS_PHP: &str =
    "<?php\nreturn [\n    App\\Providers\\RouteServiceProvider::class,\n];\n";

/// A provider that keeps its route files under `app/Contexts/…/Routes/`
/// rather than `routes/`, and reaches them both ways.
const ROUTE_SERVICE_PROVIDER: &str = "\
<?php
namespace App\\Providers;

use Illuminate\\Support\\Facades\\Route;

class RouteServiceProvider {
    public function map(): void {
        Route::middleware('kiosk')
            ->group(base_path('app/Contexts/Kiosk/Routes/kiosk.php'));

        Route::group(['as' => 'api.', 'prefix' => 'api'], function (): void {
            require base_path('app/Contexts/Api/Routes/api.php');
        });
    }
}
";

/// Route registrations guarded by configuration, as a route file that only
/// applies on a configured domain writes them.
const KIOSK_ROUTES: &str = "\
<?php
use Illuminate\\Support\\Facades\\Route;

$fqdn = config('kiosk.url');

if ($fqdn) {
    Route::domain($fqdn)->name('kiosk.')->prefix('kiosk')->group(function (): void {
        Route::get('/register', 'register')->name('register');
    });
}
";

const API_ROUTES: &str = "\
<?php
use Illuminate\\Support\\Facades\\Route;

Route::get('/users', 'index')->name('users.index');
";

const CONSUMER: &str = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        route('kiosk.register');
        route('api.users.index');
        route('kiosk.nope');
    }
}
";

fn workspace() -> (phpantom_lsp::Backend, tempfile::TempDir) {
    create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("bootstrap/providers.php", PROVIDERS_PHP),
            (
                "src/Providers/RouteServiceProvider.php",
                ROUTE_SERVICE_PROVIDER,
            ),
            ("app/Contexts/Kiosk/Routes/kiosk.php", KIOSK_ROUTES),
            ("app/Contexts/Api/Routes/api.php", API_ROUTES),
            ("src/Services/Service.php", CONSUMER),
        ],
    )
}

#[tokio::test]
async fn routes_outside_the_routes_directory_are_not_reported_as_unknown() {
    let (backend, dir) = workspace();
    backend.initialized(InitializedParams {}).await;

    let uri = Url::from_file_path(dir.path().join("src/Services/Service.php")).unwrap();
    open_php(&backend, &uri, CONSUMER).await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), CONSUMER, &mut diags);

    let messages: Vec<&String> = diags
        .iter()
        .filter(
            |d| matches!(&d.code, Some(NumberOrString::String(s)) if s == "invalid_laravel_route"),
        )
        .map(|d| &d.message)
        .collect();

    assert_eq!(
        messages.len(),
        1,
        "only the genuinely missing route should be flagged, got: {messages:?}"
    );
    assert!(
        messages[0].contains("kiosk.nope"),
        "the flagged route should be the missing one, got: {}",
        messages[0]
    );
}

const PACKAGE_COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": { "Vendor\\Package\\": "src/" } }
}"#;

const PACKAGE_CONSUMER: &str = "\
<?php
namespace Vendor\\Package;
class Widget {
    public function link(): string {
        return route('package.widgets.show');
    }
}
";

#[tokio::test]
async fn a_package_with_no_route_files_does_not_flag_route_calls() {
    let (backend, dir) = create_psr4_workspace(
        PACKAGE_COMPOSER_JSON,
        &[("src/Widget.php", PACKAGE_CONSUMER)],
    );
    backend.initialized(InitializedParams {}).await;

    let uri = Url::from_file_path(dir.path().join("src/Widget.php")).unwrap();
    open_php(&backend, &uri, PACKAGE_CONSUMER).await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), PACKAGE_CONSUMER, &mut diags);

    let messages: Vec<&String> = diags
        .iter()
        .filter(
            |d| matches!(&d.code, Some(NumberOrString::String(s)) if s == "invalid_laravel_route"),
        )
        .map(|d| &d.message)
        .collect();

    assert!(
        messages.is_empty(),
        "a package with no route files of its own must not flag route() calls, got: {messages:?}"
    );
}

#[tokio::test]
async fn goto_definition_reaches_a_route_outside_the_routes_directory() {
    let (backend, dir) = workspace();
    backend.initialized(InitializedParams {}).await;

    let uri = Url::from_file_path(dir.path().join("src/Services/Service.php")).unwrap();
    open_php(&backend, &uri, CONSUMER).await;

    // Cursor inside 'kiosk.register' on line 4.
    let result = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 4,
                    character: 20,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .expect("goto_definition should not error")
        .expect("route('kiosk.register') should resolve to its ->name('register')");

    let target = match result {
        GotoDefinitionResponse::Scalar(location) => location.uri,
        GotoDefinitionResponse::Array(locations) => locations[0].uri.clone(),
        GotoDefinitionResponse::Link(links) => links[0].target_uri.clone(),
    };
    assert!(
        target
            .as_str()
            .ends_with("/app/Contexts/Kiosk/Routes/kiosk.php"),
        "should jump to the kiosk route file, got: {target}"
    );
}

// ─── Dynamic group name prefixes ────────────────────────────────────────────

/// A project's own route file that includes a group whose `->name()` argument
/// is a variable, as Filament does with `Route::name($panelId . '.')`.  Routes
/// registered inside such a group cannot be enumerated statically, so any
/// route call whose name falls under the known static prefix must not be
/// flagged.
const DYNAMIC_ROUTES: &str = "\
<?php
use Illuminate\\Support\\Facades\\Route;

Route::get('/', fn() => 'welcome')->name('home');

Route::name('filament.')
    ->group(function () {
        Route::name($panelId . '.')->group(function () {
            Route::get('/dashboard', fn() => 'hi')->name('pages.dashboard');
        });
    });
";

const CONSUMER_DYNAMIC: &str = "\
<?php
namespace App;
class Nav {
    public function links(): void {
        route('filament.admin.resources.vps.index');
        route('home');
        route('totally.bogus');
    }
}
";

#[tokio::test]
async fn routes_under_a_dynamic_group_prefix_are_not_flagged() {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("routes/web.php", DYNAMIC_ROUTES),
            ("src/Nav.php", CONSUMER_DYNAMIC),
        ],
    );
    backend.initialized(InitializedParams {}).await;

    let uri = Url::from_file_path(dir.path().join("src/Nav.php")).unwrap();
    open_php(&backend, &uri, CONSUMER_DYNAMIC).await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), CONSUMER_DYNAMIC, &mut diags);

    let messages: Vec<&String> = diags
        .iter()
        .filter(
            |d| matches!(&d.code, Some(NumberOrString::String(s)) if s == "invalid_laravel_route"),
        )
        .map(|d| &d.message)
        .collect();

    assert_eq!(
        messages.len(),
        1,
        "only the genuinely missing route should be flagged, got: {messages:?}"
    );
    assert!(
        messages[0].contains("totally.bogus"),
        "the flagged route should be the missing one, got: {}",
        messages[0]
    );
}

/// A group whose name is entirely a variable and sits under no enclosing
/// literal group (`Route::name($panelId)->group(…)` at the top of a routes
/// file) spells out no known prefix at all, unlike `DYNAMIC_ROUTES` above.
/// The routes it registers must still be recognised by the names written out
/// inside it, or every `route()` call naming one is wrongly flagged unknown.
const BARE_DYNAMIC_ROUTES: &str = "\
<?php
use Illuminate\\Support\\Facades\\Route;

Route::get('/', fn() => 'welcome')->name('home');

Route::name($panelId)->group(function () {
    Route::get('/dashboard', fn() => 'hi')->name('pages.dashboard');
});
";

const CONSUMER_BARE_DYNAMIC: &str = "\
<?php
namespace App;
class Nav {
    public function links(): void {
        route('filament.admin.pages.dashboard');
        route('home');
        route('totally.bogus');
    }
}
";

#[tokio::test]
async fn routes_under_a_wholly_unknown_group_name_are_not_flagged() {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("routes/web.php", BARE_DYNAMIC_ROUTES),
            ("src/Nav.php", CONSUMER_BARE_DYNAMIC),
        ],
    );
    backend.initialized(InitializedParams {}).await;

    let uri = Url::from_file_path(dir.path().join("src/Nav.php")).unwrap();
    open_php(&backend, &uri, CONSUMER_BARE_DYNAMIC).await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), CONSUMER_BARE_DYNAMIC, &mut diags);

    let messages: Vec<&String> = diags
        .iter()
        .filter(
            |d| matches!(&d.code, Some(NumberOrString::String(s)) if s == "invalid_laravel_route"),
        )
        .map(|d| &d.message)
        .collect();

    assert_eq!(
        messages.len(),
        1,
        "only the genuinely missing route should be flagged, got: {messages:?}"
    );
    assert!(
        messages[0].contains("totally.bogus"),
        "the flagged route should be the missing one, got: {}",
        messages[0]
    );
}
