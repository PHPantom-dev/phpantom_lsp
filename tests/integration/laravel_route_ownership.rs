//! Tests for the route-name diagnostic in a project that registers no
//! routes of its own.
//!
//! A library package's route names are registered by the host application,
//! which we never see, so nothing about them is knowable.  Installed
//! packages still register routes of their own, so "the route table is
//! empty" is the wrong question to ask.

use crate::common::create_psr4_workspace;
use crate::common::open_php;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": { "App\\": "src/", "Acme\\": "vendor/acme/pkg/src/" } }
}"#;

const PROVIDERS_PHP: &str = "<?php\nreturn [\n    Acme\\PkgServiceProvider::class,\n];\n";

const PKG_SERVICE_PROVIDER: &str = "\
<?php
namespace Acme;

class PkgServiceProvider {
    public function boot(): void {
        $this->loadRoutesFrom(__DIR__ . '/../routes/web.php');
    }
}
";

const PKG_ROUTES: &str = "\
<?php
use Illuminate\\Support\\Facades\\Route;

Route::get('pkg/dashboard', 'PkgController@index')->name('pkg.dashboard');
";

const APP_ROUTES: &str = "\
<?php
use Illuminate\\Support\\Facades\\Route;

Route::get('/', 'HomeController@index')->name('home');
";

const CONSUMER: &str = "\
<?php
namespace App;
class Links {
    public function demo(): void {
        route('payment.gateway.callback');
        route('pkg.dashboard');
    }
}
";

async fn route_diagnostics(files: &[(&str, &str)]) -> Vec<String> {
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, files);
    let uri = Url::from_file_path(dir.path().join("src/Links.php")).unwrap();

    backend.initialized(InitializedParams {}).await;
    open_php(&backend, &uri, CONSUMER).await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), CONSUMER, &mut diags);

    diags
        .iter()
        .filter(
            |d| matches!(&d.code, Some(NumberOrString::String(s)) if s == "invalid_laravel_route"),
        )
        .map(|d| d.message.clone())
        .collect()
}

#[tokio::test]
async fn route_names_are_not_judged_when_only_packages_register_routes() {
    let messages = route_diagnostics(&[
        ("bootstrap/providers.php", PROVIDERS_PHP),
        (
            "vendor/acme/pkg/src/PkgServiceProvider.php",
            PKG_SERVICE_PROVIDER,
        ),
        ("vendor/acme/pkg/routes/web.php", PKG_ROUTES),
        ("src/Links.php", CONSUMER),
    ])
    .await;

    assert!(
        messages.is_empty(),
        "a package that registers no routes of its own cannot judge route names, got: {messages:?}"
    );
}

#[tokio::test]
async fn route_names_are_judged_once_the_project_registers_a_route() {
    let messages = route_diagnostics(&[
        ("bootstrap/providers.php", PROVIDERS_PHP),
        (
            "vendor/acme/pkg/src/PkgServiceProvider.php",
            PKG_SERVICE_PROVIDER,
        ),
        ("vendor/acme/pkg/routes/web.php", PKG_ROUTES),
        ("routes/web.php", APP_ROUTES),
        ("src/Links.php", CONSUMER),
    ])
    .await;

    assert_eq!(
        messages.len(),
        1,
        "the missing route should be flagged, got: {messages:?}"
    );
    assert!(
        messages[0].contains("payment.gateway.callback"),
        "unexpected diagnostic: {}",
        messages[0]
    );
}
