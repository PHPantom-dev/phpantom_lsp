//! A Laravel application in a subdirectory of the workspace, the layout of
//! a monorepo opened at its top (`backend/` beside `frontend/`).  The
//! application's conventional files are found under its own base path,
//! not the workspace root's.

use crate::common::{messages_with_code, open_php_file};
use phpantom_lsp::Backend;
use std::fs;
use std::path::Path;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const BACKEND_COMPOSER: &str = r#"{
    "require": { "laravel/framework": "^13.0" },
    "autoload": {
        "psr-4": {
            "App\\": "app/",
            "Illuminate\\Contracts\\Auth\\": "stubs/Contracts/Auth/",
            "Illuminate\\Support\\Facades\\": "stubs/Support/Facades/"
        }
    }
}"#;

const AUTHENTICATABLE_PHP: &str = "\
<?php
namespace Illuminate\\Contracts\\Auth;
interface Authenticatable {
    public function getAuthIdentifier();
}
";

const AUTH_FACADE_PHP: &str = "\
<?php
namespace Illuminate\\Support\\Facades;
/**
 * @method static \\Illuminate\\Contracts\\Auth\\Authenticatable|null user()
 */
class Auth {}
";

const USER_PHP: &str = "\
<?php
namespace App\\Models;
use Illuminate\\Contracts\\Auth\\Authenticatable;
class User implements Authenticatable {
    public function getAuthIdentifier() { return 1; }
    public function isAdmin(): bool { return true; }
}
";

const AUTH_CONFIG: &str = "\
<?php
return [
    'defaults' => ['guard' => 'web'],
    'guards' => ['web' => ['driver' => 'session', 'provider' => 'users']],
    'providers' => ['users' => ['driver' => 'eloquent', 'model' => App\\Models\\User::class]],
];
";

const CONTROLLER_PHP: &str = "\
<?php
namespace App\\Http\\Controllers;
use Illuminate\\Support\\Facades\\Auth;
class HomeController {
    public function show(): mixed {
        Auth::user()?->isAdmin();
        Auth::user()?->isMissing();
        view('missing');
        return view('welcome');
    }
}
";

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// The workspace root holds no `composer.json`; the application is in
/// `backend/`, beside a `frontend/` with no PHP in it.
async fn monorepo_backend() -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "frontend/package.json", "{}\n");
    write(root, "backend/composer.json", BACKEND_COMPOSER);
    write(root, "backend/artisan", "#!/usr/bin/env php\n<?php\n");
    write(
        root,
        "backend/stubs/Contracts/Auth/Authenticatable.php",
        AUTHENTICATABLE_PHP,
    );
    write(
        root,
        "backend/stubs/Support/Facades/Auth.php",
        AUTH_FACADE_PHP,
    );
    write(root, "backend/app/Models/User.php", USER_PHP);
    write(root, "backend/config/auth.php", AUTH_CONFIG);
    write(
        root,
        "backend/resources/views/welcome.blade.php",
        "<p>Hello</p>\n",
    );
    write(
        root,
        "backend/app/Http/Controllers/HomeController.php",
        CONTROLLER_PHP,
    );
    let backend = Backend::new_test_with_workspace(root.to_path_buf(), Vec::new());
    backend.initialized(InitializedParams {}).await;
    (backend, dir)
}

fn slow_diagnostics(backend: &Backend, uri: &Uri) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), CONTROLLER_PHP, &mut diags);
    diags
}

#[tokio::test]
async fn views_resolve_under_the_subprojects_resources() {
    let (backend, _dir) = monorepo_backend().await;
    let uri = open_php_file(&backend, "backend/app/Http/Controllers/HomeController.php").await;

    let messages = messages_with_code(&slow_diagnostics(&backend, &uri), "invalid_laravel_view");
    assert_eq!(
        messages.len(),
        1,
        "only the view the app lacks is unknown, got {messages:?}"
    );
    assert!(messages[0].contains("'missing'"), "got {messages:?}");
}

#[tokio::test]
async fn the_auth_model_comes_from_the_subprojects_config() {
    let (backend, _dir) = monorepo_backend().await;
    let uri = open_php_file(&backend, "backend/app/Http/Controllers/HomeController.php").await;

    let messages = messages_with_code(&slow_diagnostics(&backend, &uri), "unknown_member");
    assert_eq!(
        messages.len(),
        1,
        "only the method the model lacks is unknown, got {messages:?}"
    );
    assert!(
        messages[0].contains("App\\Models\\User::isMissing"),
        "got {messages:?}"
    );
}
