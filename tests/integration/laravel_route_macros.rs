//! Tests for route names a router macro registers.
//!
//! `laravel/ui` ships `Route::auth()` as a mixin on the router: the macro body
//! registers `login`, `register`, and the `password.*` routes on `$this`, and
//! calls a second macro for the password group.  A route file that calls the
//! macro has all of those names, so none of them may be reported as unknown.

use crate::common::{create_psr4_workspace, open_php};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0", "laravel/ui": "^4.0" },
    "autoload": { "psr-4": { "App\\": "src/", "Ui\\": "ui/" } }
}"#;

const PROVIDERS_PHP: &str = "<?php\nreturn [\n    Ui\\UiServiceProvider::class,\n];\n";

const UI_SERVICE_PROVIDER: &str = "\
<?php
namespace Ui;

use Illuminate\\Support\\Facades\\Route;

class UiServiceProvider {
    public function boot(): void {
        Route::mixin(new AuthRouteMethods());
    }
}
";

/// The shape `laravel/ui` uses: each method returns the closure that becomes
/// the macro, and `auth()` delegates part of its registrations to a second
/// macro through `$this`.
const AUTH_ROUTE_METHODS: &str = "\
<?php
namespace Ui;

class AuthRouteMethods {
    public function auth() {
        return function ($options = []) {
            $this->get('login', 'Auth\\\\LoginController@showLoginForm')->name('login');
            $this->post('login', 'Auth\\\\LoginController@login');
            $this->post('logout', 'Auth\\\\LoginController@logout')->name('logout');

            if ($options['register'] ?? true) {
                $this->get('register', 'Auth\\\\RegisterController@show')->name('register');
            }

            $this->resetPassword();
        };
    }

    public function resetPassword() {
        return function () {
            $this->get('password/reset', 'Auth\\\\ForgotPasswordController@show')->name('password.request');
            $this->post('password/email', 'Auth\\\\ForgotPasswordController@send')->name('password.email');
            $this->get('password/reset/{token}', 'Auth\\\\ResetPasswordController@show')->name('password.reset');
            $this->post('password/reset', 'Auth\\\\ResetPasswordController@reset')->name('password.update');
        };
    }
}
";

const WEB_ROUTES: &str = "\
<?php
use Illuminate\\Support\\Facades\\Auth;
use Illuminate\\Support\\Facades\\Route;

Auth::routes();

Route::name('admin.')->prefix('admin')->group(function (): void {
    Route::auth();
});
";

const CONSUMER: &str = "\
<?php
namespace App;
class Links {
    public function demo(): void {
        route('login');
        route('register');
        route('password.update');
        route('admin.password.reset');
        route('password.nope');
    }
}
";

fn workspace() -> (phpantom_lsp::Backend, tempfile::TempDir) {
    create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("bootstrap/providers.php", PROVIDERS_PHP),
            ("ui/UiServiceProvider.php", UI_SERVICE_PROVIDER),
            ("ui/AuthRouteMethods.php", AUTH_ROUTE_METHODS),
            ("routes/web.php", WEB_ROUTES),
            ("src/Links.php", CONSUMER),
        ],
    )
}

#[tokio::test]
async fn routes_registered_by_a_router_macro_are_not_reported_as_unknown() {
    let (backend, dir) = workspace();
    backend.initialized(InitializedParams {}).await;

    let uri = Url::from_file_path(dir.path().join("src/Links.php")).unwrap();
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
        messages[0].contains("password.nope"),
        "the flagged route should be the missing one, got: {}",
        messages[0]
    );
}

#[tokio::test]
async fn goto_definition_reaches_a_route_declared_in_a_macro_body() {
    let (backend, dir) = workspace();
    backend.initialized(InitializedParams {}).await;

    let uri = Url::from_file_path(dir.path().join("src/Links.php")).unwrap();
    open_php(&backend, &uri, CONSUMER).await;

    // Cursor inside 'password.update' on line 6.
    let result = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 6,
                    character: 22,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap();

    let target = match result {
        Some(GotoDefinitionResponse::Scalar(location)) => location.uri,
        Some(GotoDefinitionResponse::Array(locations)) => {
            locations.first().expect("no location returned").uri.clone()
        }
        other => panic!("expected a definition location, got: {other:?}"),
    };
    assert!(
        target.path().ends_with("ui/AuthRouteMethods.php"),
        "expected the macro body, got: {target}"
    );
}
