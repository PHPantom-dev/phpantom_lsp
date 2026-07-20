//! Integration tests for resolving the authenticated-user model from
//! `config/auth.php` (`Request::user()` / `Guard::user()`).

use crate::common::create_psr4_workspace;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "App\\Models\\": "src/Models/",
            "Illuminate\\Contracts\\Auth\\": "vendor/illuminate/Contracts/Auth/",
            "Illuminate\\Http\\": "vendor/illuminate/Http/",
            "Illuminate\\Foundation\\Http\\": "vendor/illuminate/Foundation/Http/",
            "Illuminate\\Support\\Facades\\": "vendor/illuminate/Support/Facades/"
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

const GUARD_PHP: &str = "\
<?php
namespace Illuminate\\Contracts\\Auth;
interface Guard {
    /** @return \\Illuminate\\Contracts\\Auth\\Authenticatable|null */
    public function user();
}
";

const REQUEST_PHP: &str = "\
<?php
namespace Illuminate\\Http;
class Request {
    /** @return \\Illuminate\\Contracts\\Auth\\Authenticatable|null */
    public function user($guard = null) { return null; }
}
";

const USER_PHP: &str = "\
<?php
namespace App\\Models;
use Illuminate\\Contracts\\Auth\\Authenticatable;
class User implements Authenticatable {
    public function getAuthIdentifier() { return 1; }
    public function isActive(): bool { return true; }
}
";

const ADMIN_PHP: &str = "\
<?php
namespace App\\Models;
use Illuminate\\Contracts\\Auth\\Authenticatable;
class Admin implements Authenticatable {
    public function getAuthIdentifier() { return 1; }
    public function isSuperUser(): bool { return true; }
}
";

/// The `Auth` facade, whose `guard()` returns a `Guard` for the named
/// guard (`Auth::guard('admin')`).
const AUTH_FACADE_PHP: &str = "\
<?php
namespace Illuminate\\Support\\Facades;
use Illuminate\\Contracts\\Auth\\Guard;
class Auth {
    public static function guard($name = null): Guard { return null; }
}
";

/// A `FormRequest` base extending `Request`, mirroring Laravel's own.
const FORM_REQUEST_PHP: &str = "\
<?php
namespace Illuminate\\Foundation\\Http;
use Illuminate\\Http\\Request;
class FormRequest extends Request {}
";

fn base_files() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "vendor/illuminate/Contracts/Auth/Authenticatable.php",
            AUTHENTICATABLE_PHP,
        ),
        ("vendor/illuminate/Contracts/Auth/Guard.php", GUARD_PHP),
        ("vendor/illuminate/Http/Request.php", REQUEST_PHP),
        (
            "vendor/illuminate/Foundation/Http/FormRequest.php",
            FORM_REQUEST_PHP,
        ),
        (
            "vendor/illuminate/Support/Facades/Auth.php",
            AUTH_FACADE_PHP,
        ),
        ("src/Models/User.php", USER_PHP),
        ("src/Models/Admin.php", ADMIN_PHP),
    ]
}

/// A two-guard config: the default `web` guard maps to `User`, and the
/// named `admin` guard maps to `Admin`.  Both are hard literals so the
/// default resolves precisely to `User` with no fan-out.
const MULTI_GUARD_CONFIG: (&str, &str) = (
    "config/auth.php",
    "<?php return [
        'defaults' => ['guard' => 'web'],
        'guards' => [
            'web' => ['provider' => 'users'],
            'admin' => ['provider' => 'admins'],
        ],
        'providers' => [
            'users' => ['model' => App\\Models\\User::class],
            'admins' => ['model' => App\\Models\\Admin::class],
        ],
    ];",
);

/// A global `auth()` helper returning a `Guard`, mirroring Laravel's.
const AUTH_HELPER_PHP: &str = "\
<?php
function auth($guard = null): \\Illuminate\\Contracts\\Auth\\Guard { return null; }
";

async fn complete_labels(
    files: &[(&str, &str)],
    open_path: &str,
    content: &str,
    line: u32,
    character: u32,
) -> Vec<String> {
    complete_labels_with_opens(files, &[], open_path, content, line, character).await
}

/// Like [`complete_labels`], but first opens each `(path, content)` in
/// `pre_open` so their symbols (e.g. a global `auth()` helper) are
/// indexed before the completion request runs.
async fn complete_labels_with_opens(
    files: &[(&str, &str)],
    pre_open: &[(&str, &str)],
    open_path: &str,
    content: &str,
    line: u32,
    character: u32,
) -> Vec<String> {
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, files);
    for (path, text) in pre_open {
        let uri = Url::from_file_path(dir.path().join(path)).unwrap();
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri,
                    language_id: "php".to_string(),
                    version: 1,
                    text: text.to_string(),
                },
            })
            .await;
    }
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

/// A hard-literal single guard resolves `$request->user()` precisely to the
/// configured `User` model, so its own members complete.
#[tokio::test]
async fn request_user_resolves_configured_model() {
    let mut files = base_files();
    files.push((
        "config/auth.php",
        "<?php return [
            'defaults' => ['guard' => 'web'],
            'guards' => ['web' => ['provider' => 'users']],
            'providers' => ['users' => ['model' => App\\Models\\User::class]],
        ];",
    ));

    let controller = "\
<?php
namespace App;
use Illuminate\\Http\\Request;
class C {
    public function show(Request $request) {
        $request->user()->
    }
}
";
    // Cursor right after `->` on the `$request->user()->` line (0-indexed
    // line 5, after the arrow).
    let labels = complete_labels(&files, "src/C.php", controller, 5, 26).await;
    assert!(
        labels.iter().any(|l| l.starts_with("isActive")),
        "expected User::isActive in completions, got: {labels:?}"
    );
}

/// An env-overridable guard fans out to every configured guard's model, so
/// members of both `User` and `Admin` are offered.
#[tokio::test]
async fn request_user_fans_out_over_guards() {
    let mut files = base_files();
    files.push((
        "config/auth.php",
        "<?php return [
            'defaults' => ['guard' => env('AUTH_GUARD', 'web')],
            'guards' => [
                'web' => ['provider' => 'users'],
                'api' => ['provider' => 'admins'],
            ],
            'providers' => [
                'users' => ['model' => App\\Models\\User::class],
                'admins' => ['model' => App\\Models\\Admin::class],
            ],
        ];",
    ));

    let controller = "\
<?php
namespace App;
use Illuminate\\Http\\Request;
class C {
    public function show(Request $request) {
        $request->user()->
    }
}
";
    let labels = complete_labels(&files, "src/C.php", controller, 5, 26).await;
    assert!(
        labels.iter().any(|l| l.starts_with("isActive")),
        "expected User::isActive, got: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("isSuperUser")),
        "expected Admin::isSuperUser from fan-out, got: {labels:?}"
    );
}

/// The `Guard` contract's `user()` is patched the same way, so a
/// `Guard`-typed value resolves the configured model.
#[tokio::test]
async fn guard_user_resolves_configured_model() {
    let mut files = base_files();
    files.push((
        "config/auth.php",
        "<?php return [
            'defaults' => ['guard' => 'web'],
            'guards' => ['web' => ['provider' => 'users']],
            'providers' => ['users' => ['model' => App\\Models\\User::class]],
        ];",
    ));

    let controller = "\
<?php
namespace App;
use Illuminate\\Contracts\\Auth\\Guard;
class C {
    public function show(Guard $guard) {
        $guard->user()->
    }
}
";
    let labels = complete_labels(&files, "src/C.php", controller, 5, 24).await;
    assert!(
        labels.iter().any(|l| l.starts_with("isActive")),
        "expected User::isActive via Guard::user(), got: {labels:?}"
    );
}

/// With no resolvable model (bare `env()`), the floor is raised to every
/// concrete class that implements `Authenticatable` in the project, so members
/// of all of them are offered rather than only the bare contract.
#[tokio::test]
async fn unresolvable_model_raises_floor_to_implementors() {
    let mut files = base_files();
    files.push((
        "config/auth.php",
        "<?php return [
            'defaults' => ['guard' => 'web'],
            'guards' => ['web' => ['provider' => 'users']],
            'providers' => ['users' => ['model' => env('AUTH_MODEL')]],
        ];",
    ));

    let controller = "\
<?php
namespace App;
use Illuminate\\Http\\Request;
class C {
    public function show(Request $request) {
        $request->user()->
    }
}
";
    let labels = complete_labels(&files, "src/C.php", controller, 5, 26).await;
    assert!(
        labels.iter().any(|l| l.starts_with("isActive")),
        "expected User::isActive from the raised floor, got: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("isSuperUser")),
        "expected Admin::isSuperUser from the raised floor, got: {labels:?}"
    );
}

/// `Auth::guard('admin')->user()` resolves to the model configured for
/// the **named** guard (`Admin`), not the default guard's `User`.
#[tokio::test]
async fn named_guard_via_facade_resolves_that_guards_model() {
    let mut files = base_files();
    files.push(MULTI_GUARD_CONFIG);

    let controller = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Auth;
class C {
    public function show() {
        Auth::guard('admin')->user()->
    }
}
";
    let labels = complete_labels(&files, "src/C.php", controller, 5, 38).await;
    assert!(
        labels.iter().any(|l| l.starts_with("isSuperUser")),
        "expected Admin::isSuperUser via Auth::guard('admin'), got: {labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l.starts_with("isActive")),
        "did not expect the default guard's User::isActive, got: {labels:?}"
    );
}

/// The guard name passed directly to `Request::user('admin')` selects
/// the named guard's model.
#[tokio::test]
async fn named_guard_via_user_argument_resolves_that_guards_model() {
    let mut files = base_files();
    files.push(MULTI_GUARD_CONFIG);

    let controller = "\
<?php
namespace App;
use Illuminate\\Http\\Request;
class C {
    public function show(Request $request) {
        $request->user('admin')->
    }
}
";
    let labels = complete_labels(&files, "src/C.php", controller, 5, 33).await;
    assert!(
        labels.iter().any(|l| l.starts_with("isSuperUser")),
        "expected Admin::isSuperUser via user('admin'), got: {labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l.starts_with("isActive")),
        "did not expect the default guard's User::isActive, got: {labels:?}"
    );
}

/// With no guard argument, `$request->user()` still resolves to the
/// default guard's model even when other guards are configured.
#[tokio::test]
async fn default_guard_unaffected_by_named_guards() {
    let mut files = base_files();
    files.push(MULTI_GUARD_CONFIG);

    let controller = "\
<?php
namespace App;
use Illuminate\\Http\\Request;
class C {
    public function show(Request $request) {
        $request->user()->
    }
}
";
    let labels = complete_labels(&files, "src/C.php", controller, 5, 26).await;
    assert!(
        labels.iter().any(|l| l.starts_with("isActive")),
        "expected default guard's User::isActive, got: {labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l.starts_with("isSuperUser")),
        "did not expect the admin guard's Admin::isSuperUser, got: {labels:?}"
    );
}

/// `$this->user()` inside a `FormRequest` subclass resolves the default
/// guard's model, exercising the receiver-subtype gate on an inherited
/// `user()`.
#[tokio::test]
async fn form_request_this_user_resolves_default_model() {
    let mut files = base_files();
    files.push(MULTI_GUARD_CONFIG);

    let form_request = "\
<?php
namespace App;
use Illuminate\\Foundation\\Http\\FormRequest;
class StoreRequest extends FormRequest {
    public function authorize(): bool {
        $this->user()->
    }
}
";
    let labels = complete_labels(&files, "src/StoreRequest.php", form_request, 5, 23).await;
    assert!(
        labels.iter().any(|l| l.starts_with("isActive")),
        "expected User::isActive via FormRequest $this->user(), got: {labels:?}"
    );
}

/// The configured model is referenced by an imported short name
/// (`use App\Models\AdminUser; ... 'model' => AdminUser::class`) and only
/// reaches `Authenticatable` through a base class it extends, mirroring a
/// real Laravel app's `AdminUser extends Illuminate\Foundation\Auth\User`.
/// The `use` statement in `config/auth.php` must be resolved so the model
/// FQN is recognized, otherwise `$this->user()` widens to the bare
/// contract and its members do not complete.
#[tokio::test]
async fn config_model_via_use_short_name_resolves() {
    const COMPOSER: &str = r#"{
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "App\\Models\\": "src/Models/",
            "Illuminate\\Contracts\\Auth\\": "vendor/illuminate/Contracts/Auth/",
            "Illuminate\\Http\\": "vendor/illuminate/Http/",
            "Illuminate\\Foundation\\Auth\\": "vendor/illuminate/Foundation/Auth/",
            "Illuminate\\Foundation\\Http\\": "vendor/illuminate/Foundation/Http/"
        }
    }
}"#;
    let files: Vec<(&str, &str)> = vec![
        (
            "vendor/illuminate/Contracts/Auth/Authenticatable.php",
            AUTHENTICATABLE_PHP,
        ),
        ("vendor/illuminate/Http/Request.php", REQUEST_PHP),
        (
            "vendor/illuminate/Foundation/Http/FormRequest.php",
            FORM_REQUEST_PHP,
        ),
        (
            "vendor/illuminate/Foundation/Auth/User.php",
            "<?php
namespace Illuminate\\Foundation\\Auth;
use Illuminate\\Contracts\\Auth\\Authenticatable as AuthenticatableContract;
class User implements AuthenticatableContract {
    public function getAuthIdentifier() { return 1; }
}
",
        ),
        (
            "src/Models/AdminUser.php",
            "<?php
namespace App\\Models;
use Illuminate\\Foundation\\Auth\\User;
final class AdminUser extends User {
    public function isAdmin(): bool { return true; }
}
",
        ),
        (
            "config/auth.php",
            "<?php
use App\\Models\\AdminUser;
return [
    'defaults' => ['guard' => 'web'],
    'guards' => ['web' => ['provider' => 'users']],
    'providers' => ['users' => ['model' => AdminUser::class]],
];",
        ),
    ];

    let form_request = "\
<?php
namespace App;
use Illuminate\\Foundation\\Http\\FormRequest;
class StoreRequest extends FormRequest {
    public function authorize(): bool {
        $this->user()->
    }
}
";
    let (backend, dir) = create_psr4_workspace(COMPOSER, &files);
    let uri = Url::from_file_path(dir.path().join("src/StoreRequest.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: form_request.to_string(),
            },
        })
        .await;
    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 5,
                    character: 23,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();
    let labels: Vec<String> = match result {
        Some(CompletionResponse::Array(items)) => items.into_iter().map(|i| i.label).collect(),
        Some(CompletionResponse::List(list)) => list.items.into_iter().map(|i| i.label).collect(),
        _ => Vec::new(),
    };
    assert!(
        labels.iter().any(|l| l.starts_with("isAdmin")),
        "expected AdminUser::isAdmin via config model imported by use, got: {labels:?}"
    );
}

/// `$request->user()` where `$request` is a `Request`-typed **property**
/// resolves the default guard's model.
#[tokio::test]
async fn request_property_user_resolves_default_model() {
    let mut files = base_files();
    files.push(MULTI_GUARD_CONFIG);

    let handler = "\
<?php
namespace App;
use Illuminate\\Http\\Request;
class Handler {
    public function __construct(private Request $request) {}
    public function run() {
        $this->request->user()->
    }
}
";
    let labels = complete_labels(&files, "src/Handler.php", handler, 6, 32).await;
    assert!(
        labels.iter().any(|l| l.starts_with("isActive")),
        "expected User::isActive via $this->request->user(), got: {labels:?}"
    );
}

/// `auth('admin')->user()` through the global `auth()` helper resolves
/// the named guard's model.
#[tokio::test]
async fn named_guard_via_helper_resolves_that_guards_model() {
    let mut files = base_files();
    files.push(MULTI_GUARD_CONFIG);

    let controller = "\
<?php
namespace App;
class C {
    public function show() {
        auth('admin')->user()->
    }
}
";
    let labels = complete_labels_with_opens(
        &files,
        &[("src/helpers.php", AUTH_HELPER_PHP)],
        "src/C.php",
        controller,
        4,
        31,
    )
    .await;
    assert!(
        labels.iter().any(|l| l.starts_with("isSuperUser")),
        "expected Admin::isSuperUser via auth('admin'), got: {labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l.starts_with("isActive")),
        "did not expect the default guard's User::isActive, got: {labels:?}"
    );
}
