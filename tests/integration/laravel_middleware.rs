//! Tests for middleware alias and group names.
//!
//! The aliases and groups the framework, `bootstrap/app.php`, and the
//! service providers register are offered in `->middleware('…')`, hover
//! names the class an alias runs (or the members of a group), go-to-definition
//! jumps to the registration and the class, find-references links the uses,
//! and a name nothing registers is flagged.

use crate::common::{
    complete_labels_at_opened, create_initialized_psr4_workspace, definition_locations,
    goto_definition_at, hover_text_at, message_text, open_php_str, position_after,
    references_response,
};
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Illuminate\\Routing\\": "framework/Routing/",
        "Illuminate\\Foundation\\Configuration\\": "framework/Configuration/",
        "Illuminate\\Auth\\Middleware\\": "framework/Auth/"
    } }
}"#;

const FRAMEWORK_MIDDLEWARE: &str = r#"<?php
namespace Illuminate\Foundation\Configuration;

class Middleware
{
    public function getMiddlewareGroups()
    {
        $middleware = [
            'web' => array_values(array_filter([
                \Illuminate\Cookie\Middleware\EncryptCookies::class,
            ])),
            'api' => array_values(array_filter([
                \Illuminate\Routing\Middleware\SubstituteBindings::class,
            ])),
        ];
        return $middleware;
    }

    protected function defaultAliases()
    {
        $aliases = [
            'auth' => \Illuminate\Auth\Middleware\Authenticate::class,
            'can' => \Illuminate\Auth\Middleware\Authorize::class,
            'throttle' => \Illuminate\Routing\Middleware\ThrottleRequests::class,
        ];
        return $aliases;
    }
}
"#;

const AUTHENTICATE: &str = r#"<?php
namespace Illuminate\Auth\Middleware;

use Closure;

class Authenticate
{
    public function handle($request, Closure $next, string ...$guards) {}
}
"#;

const BOOTSTRAP_APP: &str = r#"<?php
use App\Http\Middleware\EnsureAdmin;
use App\Http\Middleware\TrackVisit;
use Illuminate\Foundation\Application;
use Illuminate\Foundation\Configuration\Middleware;

return Application::configure(basePath: dirname(__DIR__))
    ->withMiddleware(function (Middleware $middleware) {
        $middleware->alias([
            'admin' => EnsureAdmin::class,
        ]);
        $middleware->appendToGroup('web', TrackVisit::class);
    })->create();
"#;

const ENSURE_ADMIN: &str = r#"<?php
namespace App\Http\Middleware;

class EnsureAdmin
{
    public function handle($request, \Closure $next) {}
}
"#;

const PROVIDERS_PHP: &str = "<?php\nreturn [];\n";

async fn workspace(
    routes: &str,
    extra: &[(&str, &str)],
) -> (phpantom_lsp::Backend, tempfile::TempDir, Uri) {
    let mut files = vec![
        ("bootstrap/providers.php", PROVIDERS_PHP),
        ("bootstrap/app.php", BOOTSTRAP_APP),
        (
            "framework/Configuration/Middleware.php",
            FRAMEWORK_MIDDLEWARE,
        ),
        ("framework/Auth/Authenticate.php", AUTHENTICATE),
        ("src/Http/Middleware/EnsureAdmin.php", ENSURE_ADMIN),
        ("src/routes.php", routes),
    ];
    files.extend_from_slice(extra);
    create_initialized_psr4_workspace(COMPOSER_JSON, &files, "src/routes.php").await
}

fn middleware_diagnostics(diags: &[Diagnostic]) -> Vec<String> {
    diags
        .iter()
        .filter(|d| matches!(&d.code, Some(Code::String(c)) if c == "invalid_laravel_middleware"))
        .map(|d| message_text(&d.message).to_string())
        .collect()
}

#[tokio::test]
async fn aliases_and_groups_complete_in_route_middleware() {
    let routes = r#"<?php
use Illuminate\Support\Facades\Route;

Route::middleware('')->get('/', fn () => 'home');
Route::get('/admin', fn () => 'admin')->middleware(['auth:web', 'a']);
"#;
    let (backend, _dir, uri) = workspace(routes, &[]).await;

    let position = position_after(routes, "middleware('");
    let labels = complete_labels_at_opened(&backend, &uri, position.line, position.character).await;
    for expected in ["admin", "api", "auth", "can", "web"] {
        assert!(
            labels.iter().any(|label| label == expected),
            "`{expected}` should complete, got {labels:?}"
        );
    }

    let position = position_after(routes, "'auth:web', 'a");
    let labels = complete_labels_at_opened(&backend, &uri, position.line, position.character).await;
    assert_eq!(labels, vec!["admin", "api", "auth"]);
}

#[tokio::test]
async fn hover_names_the_class_and_its_parameters() {
    let routes = r#"<?php
use Illuminate\Support\Facades\Route;

Route::middleware(['auth:web', 'web'])->get('/', fn () => 'home');
"#;
    let (backend, _dir, uri) = workspace(routes, &[]).await;

    let position = position_after(routes, "'au");
    let hover = hover_text_at(&backend, &uri, position.line, position.character)
        .await
        .expect("hover on an alias");
    assert!(
        hover.contains("Illuminate\\Auth\\Middleware\\Authenticate"),
        "{hover}"
    );
    assert!(hover.contains("string ...$guards"), "{hover}");

    let position = position_after(routes, "'we");
    let hover = hover_text_at(&backend, &uri, position.line, position.character)
        .await
        .expect("hover on a group");
    assert!(
        hover.contains("Illuminate\\Cookie\\Middleware\\EncryptCookies")
            && hover.contains("App\\Http\\Middleware\\TrackVisit"),
        "the group's members, the framework's and the application's: {hover}"
    );
}

#[tokio::test]
async fn definition_jumps_to_the_registration_and_the_class() {
    let routes = r#"<?php
use Illuminate\Support\Facades\Route;

Route::get('/', fn () => 'home')->middleware('admin');
"#;
    let (backend, _dir, uri) = workspace(routes, &[]).await;

    let position = position_after(routes, "'adm");
    let response = goto_definition_at(&backend, &uri, position.line, position.character).await;
    let paths: Vec<String> = definition_locations(response)
        .into_iter()
        .map(|location| location.uri.path().to_string())
        .collect();
    assert_eq!(paths.len(), 2, "{paths:?}");
    assert!(paths[0].ends_with("bootstrap/app.php"), "{paths:?}");
    assert!(paths[1].ends_with("EnsureAdmin.php"), "{paths:?}");
}

#[tokio::test]
async fn references_link_the_uses_to_the_registration() {
    let routes = r#"<?php
use Illuminate\Support\Facades\Route;

Route::middleware('admin')->get('/', fn () => 'home');
Route::get('/x', fn () => 'x')->withoutMiddleware(['admin']);
"#;
    let (backend, _dir, uri) = workspace(routes, &[]).await;

    let position = position_after(routes, "'adm");
    let locations = references_response(&backend, &uri, position, true)
        .await
        .unwrap_or_default();
    let in_routes = locations.iter().filter(|l| l.uri == uri).count();
    assert_eq!(in_routes, 2, "{locations:?}");
    assert!(
        locations
            .iter()
            .any(|l| l.uri.path().to_string().ends_with("bootstrap/app.php")),
        "{locations:?}"
    );
}

#[tokio::test]
async fn unknown_names_are_flagged() {
    let routes = r#"<?php
use Illuminate\Support\Facades\Route;

Route::middleware(['auth:web', 'can:update,post', 'web', 'admin', 'admn'])->get('/', fn () => 'home');
Route::middleware('App\Http\Middleware\EnsureAdmin')->get('/a', fn () => 'a');
Route::middleware('EnsureAdmin')->get('/b', fn () => 'b');
"#;
    let (backend, _dir, uri) = workspace(routes, &[]).await;
    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), routes, &mut diags);
    assert_eq!(
        middleware_diagnostics(&diags),
        vec![
            "Unknown middleware 'admn'".to_string(),
            "Unknown middleware 'EnsureAdmin'".to_string(),
        ]
    );
}

#[tokio::test]
async fn a_name_registered_at_runtime_silences_the_check() {
    let provider = r#"<?php
namespace App\Providers;

class AppServiceProvider
{
    public function boot(\Illuminate\Routing\Router $router): void
    {
        foreach (config('acme.middleware') as $name => $class) {
            $router->aliasMiddleware($name, $class);
        }
    }
}
"#;
    let routes = r#"<?php
use Illuminate\Support\Facades\Route;

Route::middleware('acme')->get('/', fn () => 'home');
"#;
    let (backend, _dir, uri) = workspace(
        routes,
        &[
            (
                "bootstrap/providers.php",
                "<?php\nreturn [App\\Providers\\AppServiceProvider::class];\n",
            ),
            ("src/Providers/AppServiceProvider.php", provider),
        ],
    )
    .await;
    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), routes, &mut diags);
    assert!(
        middleware_diagnostics(&diags).is_empty(),
        "{:?}",
        middleware_diagnostics(&diags)
    );
}

#[tokio::test]
async fn controller_attributes_and_middleware_objects_name_middleware() {
    let controller = r#"<?php
namespace App\Http\Controllers;

use Illuminate\Routing\Attributes\Controllers\Middleware;
use Illuminate\Routing\Controllers\HasMiddleware;
use Illuminate\Routing\Controllers\Middleware as RouteMiddleware;

#[Middleware('admn')]
class PostController implements HasMiddleware
{
    public static function middleware(): array
    {
        return [new RouteMiddleware('gest', only: ['index'])];
    }

    #[Middleware(['auth', 'web'])]
    public function index() {}
}
"#;
    let routes = "<?php\n";
    let (backend, dir, _uri) = workspace(
        routes,
        &[("src/Http/Controllers/PostController.php", controller)],
    )
    .await;
    let uri =
        Uri::from_file_path(dir.path().join("src/Http/Controllers/PostController.php")).unwrap();
    open_php_str(&backend, uri.as_str(), controller).await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), controller, &mut diags);
    assert_eq!(
        middleware_diagnostics(&diags),
        vec![
            "Unknown middleware 'admn'".to_string(),
            "Unknown middleware 'gest'".to_string(),
        ]
    );

    let position = position_after(controller, "#[Middleware('");
    let labels = complete_labels_at_opened(&backend, &uri, position.line, position.character).await;
    assert!(labels.iter().any(|label| label == "admin"), "{labels:?}");

    let position = position_after(controller, "RouteMiddleware('");
    let labels = complete_labels_at_opened(&backend, &uri, position.line, position.character).await;
    assert!(labels.iter().any(|label| label == "auth"), "{labels:?}");
}

#[tokio::test]
async fn controller_middleware_follows_the_resolved_parent_and_checks_each_guard() {
    let body = r#"{
    public function __construct()
    {
        $this->middleware(middleware: ['admn', 'key' => 'auth:web,admin,typo']);
        $this->middleware(array('can:publish,Post', 'throttle:uploads'));
        $this->withoutMiddleware('route-only');
    }
}
"#;
    let controller = format!(
        r#"<?php
namespace App;
use App\Http\Base as ParentController;
class Bakery extends ParentController
{body}"#
    );
    let other = format!("<?php\nnamespace App;\nclass Pipeline extends Controller\n{body}");
    let config = "<?php return ['guards' => ['web' => [], 'admin' => []]];\n";
    let (backend, dir, _) = workspace(
        "<?php\n",
        &[
            ("framework/Routing/Controller.php", r"<?php namespace Illuminate\Routing; class Controller { public function middleware($middleware) {} }"),
            ("src/Http/Base.php", r"<?php namespace App\Http; class Base extends \Illuminate\Routing\Controller {}"),
            ("src/Controller.php", r"<?php namespace App; class Controller { public function middleware($middleware) {} }"),
            ("src/Bakery.php", &controller),
            ("src/Pipeline.php", &other),
            ("config/auth.php", config),
            ("bootstrap/providers.php", r"<?php return [App\Providers\AppServiceProvider::class];"),
            ("src/Providers/AppServiceProvider.php", r"<?php namespace App\Providers;
                use Illuminate\Support\Facades\{Gate, RateLimiter};
                class AppServiceProvider { public function boot() {
                    Gate::define('publish', fn () => true);
                    RateLimiter::for('uploads', fn () => null);
                } }"),
        ],
    ).await;
    for (path, source, is_controller) in [
        ("src/Bakery.php", &controller, true),
        ("src/Pipeline.php", &other, false),
    ] {
        let uri = Uri::from_file_path(dir.path().join(path)).unwrap();
        open_php_str(&backend, uri.as_str(), source).await;
        let mut diags = Vec::new();
        backend.collect_slow_diagnostics(uri.as_str(), source, &mut diags);
        let aliases = middleware_diagnostics(&diags);
        let guards: Vec<_> = diags
            .iter()
            .filter(
                |d| matches!(&d.code, Some(Code::String(c)) if c == "invalid_laravel_auth_guard"),
            )
            .map(|d| d.range)
            .collect();
        if is_controller {
            assert_eq!(aliases, ["Unknown middleware 'admn'"]);
            assert_eq!(
                guards,
                [Range::new(
                    position_after(source, "admin,"),
                    position_after(source, "admin,typo")
                )]
            );
        } else {
            assert!(aliases.is_empty() && guards.is_empty(), "{diags:?}");
        }
        for (prefix, expected) in [
            ("['ad", "admin"),
            ("auth:w", "web"),
            ("auth:web,ad", "admin"),
        ] {
            let pos = position_after(source, prefix);
            let labels = complete_labels_at_opened(&backend, &uri, pos.line, pos.character).await;
            assert_eq!(
                labels.iter().any(|l| l == expected),
                is_controller,
                "{path} {prefix} {labels:?}"
            );
        }
        for prefix in ["can:pub", "throttle:up"] {
            let pos = position_after(source, prefix);
            let defs = definition_locations(
                goto_definition_at(&backend, &uri, pos.line, pos.character).await,
            );
            assert_eq!(
                defs.iter().any(|d| d
                    .uri
                    .path()
                    .ends_with("/src/Providers/AppServiceProvider.php")),
                is_controller,
                "{path} {prefix} {defs:?}"
            );
        }
        let pos = position_after(source, "auth:web,ad");
        let defs =
            definition_locations(goto_definition_at(&backend, &uri, pos.line, pos.character).await);
        assert_eq!(
            defs.iter()
                .any(|d| d.uri.path().ends_with("/config/auth.php")
                    && d.range.start == position_after(config, "'web' => [], '")),
            is_controller,
            "{path} {defs:?}"
        );
    }

    let config_uri = Uri::from_file_path(dir.path().join("config/auth.php")).unwrap();
    open_php_str(&backend, config_uri.as_str(), config).await;
    let refs = references_response(&backend, &config_uri, position_after(config, "'ad"), false)
        .await
        .unwrap_or_default();
    assert!(
        refs.iter()
            .any(|location| location.uri.path().ends_with("/src/Bakery.php")),
        "{refs:?}"
    );
    assert!(
        !refs
            .iter()
            .any(|location| location.uri.path().ends_with("/src/Pipeline.php")),
        "{refs:?}"
    );

    let uri = Uri::from_file_path(dir.path().join("src/Bakery.php")).unwrap();
    let (before_call, _) = controller.split_once("$this->middleware(").unwrap();
    let unfinished = format!("{before_call}$this->middleware('auth:ad");
    let pos = position_after(&unfinished, "auth:ad");
    open_php_str(&backend, uri.as_str(), &unfinished).await;
    let labels = complete_labels_at_opened(&backend, &uri, pos.line, pos.character).await;
    assert!(labels.iter().any(|l| l == "admin"), "{labels:?}");
}
