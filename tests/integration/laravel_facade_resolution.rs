//! Facade receivers resolve the way PHP and Laravel's alias loader do:
//! a global alias only where PHP would look in the global namespace, a
//! `config/app.php` overlay on top of the framework defaults, and every
//! spelling of a facade class forwarding to the class its accessor names.
//! Blade component tags reach classes whose names take more than one word.
//!
//! Cases adapted from laravel-lsp's MIT-licensed test suite.

use crate::common::{
    BLADE_COMPONENT_COMPOSER, ILLUMINATE_COMPONENT_STUB, LIVEWIRE_COMPONENT_STUB,
    complete_labels_at_opened, complete_labels_at_opened_with_trigger, create_psr4_workspace,
    definition_locations, goto_definition_at, open_document, open_php_at, position_after,
    workspace_uri,
};

const COMPOSER_JSON: &str = r#"{
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "Illuminate\\Foundation\\": "vendor/illuminate/Foundation/",
            "Illuminate\\Support\\Facades\\": "vendor/illuminate/Support/Facades/",
            "Illuminate\\Auth\\": "vendor/illuminate/Auth/",
            "Illuminate\\Contracts\\": "vendor/illuminate/Contracts/"
        }
    }
}"#;

/// The base facade, with the forwarding `__callStatic()` and the global
/// alias defaults in the shape `Facade::defaultAliases()` declares them.
const FACADE_PHP: &str = r#"<?php
namespace Illuminate\Support\Facades;
abstract class Facade
{
    public static function __callStatic($method, $args)
    {
        return static::resolveFacadeInstance()->$method(...$args);
    }

    public static function defaultAliases()
    {
        return new Collection([
            'Auth' => Auth::class,
            'Cache' => Cache::class,
            'Gate' => Gate::class,
        ]);
    }
}
"#;

/// `registerCoreContainerAliases()`: the `'auth'` key the `Auth` facade's
/// accessor names, bound to the manager class.
const APPLICATION_PHP: &str = r#"<?php
namespace Illuminate\Foundation;
class Application
{
    public function registerCoreContainerAliases()
    {
        foreach ([
            'auth' => [\Illuminate\Auth\AuthManager::class, \Illuminate\Contracts\Auth\Factory::class],
        ] as $key => $aliases) {
            foreach ($aliases as $alias) {
                $this->alias($key, $alias);
            }
        }
    }
}
"#;

const AUTH_FACADE_PHP: &str = r#"<?php
namespace Illuminate\Support\Facades;
class Auth extends Facade
{
    protected static function getFacadeAccessor()
    {
        return 'auth';
    }
}
"#;

/// A facade whose accessor names an interface by `::class`, the way the
/// framework's `Gate` facade names `Illuminate\Contracts\Auth\Access\Gate`.
const GATE_FACADE_PHP: &str = r#"<?php
namespace Illuminate\Support\Facades;
use Illuminate\Contracts\Auth\Access\Gate as GateContract;
class Gate extends Facade
{
    protected static function getFacadeAccessor()
    {
        return GateContract::class;
    }
}
"#;

/// A stand-in with a static member of its own, so a test can tell the
/// alias reached it.
const CACHE_FACADE_PHP: &str = r#"<?php
namespace Illuminate\Support\Facades;
class Cache
{
    public static function forget($key) { return true; }
}
"#;

/// The manager behind `Auth`: it declares `guard()` itself and documents
/// the guard surface it forwards through `__call()` with `@mixin`.
const AUTH_MANAGER_PHP: &str = r#"<?php
namespace Illuminate\Auth;
/**
 * @mixin \Illuminate\Contracts\Auth\Guard
 */
class AuthManager
{
    public function guard($name = null) { return null; }
    public function __call($method, $parameters) { return null; }
}
"#;

const GUARD_CONTRACT_PHP: &str = r#"<?php
namespace Illuminate\Contracts\Auth;
interface Guard
{
    public function check();
    public function id();
}
"#;

const GATE_CONTRACT_PHP: &str = r#"<?php
namespace Illuminate\Contracts\Auth\Access;
interface Gate
{
    public function allows($ability, $arguments = []);
}
"#;

/// An application class that merely shares the `Auth` short name.
const APP_AUTH_PHP: &str = r#"<?php
namespace App\Services;
class Auth
{
    public static function run(): void {}
}
"#;

/// An application facade whose accessor carries a `string` return type.
const TYPED_ACCESSOR_FACADE_PHP: &str = r#"<?php
namespace App\Facades;
use Illuminate\Support\Facades\Facade;
class Guardian extends Facade
{
    protected static function getFacadeAccessor(): string
    {
        return 'auth';
    }
}
"#;

const TOOLKIT_PHP: &str = r#"<?php
namespace App\Support;
class Toolkit
{
    public static function sharpen(): string { return ''; }
}
"#;

const LOCAL_CACHE_PHP: &str = r#"<?php
namespace App\Support;
class LocalCache
{
    public static function flushLocal(): void {}
}
"#;

/// `config/app.php` in the modern shape: the framework defaults merged with
/// a package alias and an override of one default.
const CONFIG_APP_PHP: &str = r#"<?php
use Illuminate\Support\Facades\Facade;
return [
    'name' => 'Acme',
    'aliases' => Facade::defaultAliases()->merge([
        'Toolkit' => App\Support\Toolkit::class,
        'Cache' => App\Support\LocalCache::class,
    ])->toArray(),
];
"#;

fn base_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("vendor/illuminate/Support/Facades/Facade.php", FACADE_PHP),
        (
            "vendor/illuminate/Support/Facades/Auth.php",
            AUTH_FACADE_PHP,
        ),
        (
            "vendor/illuminate/Support/Facades/Gate.php",
            GATE_FACADE_PHP,
        ),
        (
            "vendor/illuminate/Support/Facades/Cache.php",
            CACHE_FACADE_PHP,
        ),
        (
            "vendor/illuminate/Foundation/Application.php",
            APPLICATION_PHP,
        ),
        ("vendor/illuminate/Auth/AuthManager.php", AUTH_MANAGER_PHP),
        (
            "vendor/illuminate/Contracts/Auth/Guard.php",
            GUARD_CONTRACT_PHP,
        ),
        (
            "vendor/illuminate/Contracts/Auth/Access/Gate.php",
            GATE_CONTRACT_PHP,
        ),
        ("src/Services/Auth.php", APP_AUTH_PHP),
        ("src/Facades/Guardian.php", TYPED_ACCESSOR_FACADE_PHP),
        ("src/Support/Toolkit.php", TOOLKIT_PHP),
        ("src/Support/LocalCache.php", LOCAL_CACHE_PHP),
    ]
}

/// Open `content` at `path` in a workspace holding `files` and complete
/// just past the first occurrence of `after`.
async fn complete_after(
    files: &[(&str, &str)],
    path: &str,
    content: &str,
    after: &str,
) -> Vec<String> {
    let mut all = files.to_vec();
    all.push((path, content));
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, &all);
    let uri = open_php_at(&backend, &dir, path, content).await;
    let position = position_after(content, after);
    complete_labels_at_opened(&backend, &uri, position.line, position.character).await
}

fn offers(labels: &[String], name: &str) -> bool {
    labels.iter().any(|label| label.starts_with(name))
}

// ─── Global aliases ─────────────────────────────────────────────────────────

/// With no `namespace` declaration a bare name is looked up in the global
/// namespace, which is where the alias loader puts `Cache`.
#[tokio::test]
async fn bare_alias_in_a_file_without_a_namespace_resolves_to_the_facade() {
    let content = "<?php\nCache::\n";
    let labels = complete_after(&base_files(), "src/legacy.php", content, "Cache::").await;
    assert!(
        offers(&labels, "forget"),
        "a bare alias in the global namespace should reach the facade, got: {labels:?}"
    );
}

/// PHP resolves an unqualified class name against the current namespace
/// and never falls back to the global one, so inside `App\Http` a bare
/// `Cache` is `App\Http\Cache`, not the global alias.
#[tokio::test]
#[ignore = "known gap: an unqualified alias inside a namespace falls back to the global alias"]
async fn bare_alias_in_a_namespaced_file_without_an_import_stays_unresolved() {
    let content = "\
<?php
namespace App\\Http;
class Controller {
    public function go(): void {
        Cache::
    }
}
";
    let labels = complete_after(&base_files(), "src/Http/Controller.php", content, "Cache::").await;
    assert!(
        !offers(&labels, "forget"),
        "an unqualified name in a namespace must not fall back to the global alias, got: {labels:?}"
    );
}

/// An alias the project adds in `config/app.php` resolves like a default.
#[tokio::test]
async fn an_alias_added_in_config_app_resolves() {
    let mut files = base_files();
    files.push(("config/app.php", CONFIG_APP_PHP));
    let content = "\
<?php
namespace App;
class Svc {
    public function go(): void {
        \\Toolkit::
    }
}
";
    let labels = complete_after(&files, "src/Svc.php", content, "\\Toolkit::").await;
    assert!(
        offers(&labels, "sharpen"),
        "the config/app.php alias should reach its class, got: {labels:?}"
    );
}

/// An alias `config/app.php` redefines replaces the framework default.
#[tokio::test]
async fn a_config_app_alias_overrides_the_framework_default() {
    let mut files = base_files();
    files.push(("config/app.php", CONFIG_APP_PHP));
    let content = "\
<?php
namespace App;
class Svc {
    public function go(): void {
        \\Cache::
    }
}
";
    let labels = complete_after(&files, "src/Svc.php", content, "\\Cache::").await;
    assert!(
        offers(&labels, "flushLocal"),
        "the config override should win over the default alias, got: {labels:?}"
    );
    assert!(
        !offers(&labels, "forget"),
        "the overridden default must not still resolve, got: {labels:?}"
    );
}

// ─── Facade spellings ───────────────────────────────────────────────────────

/// A facade imported under another name is still the facade.
#[tokio::test]
async fn a_facade_imported_under_an_alias_forwards_the_concrete_methods() {
    let content = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Auth as Authentication;
class Svc {
    public function go(): void {
        Authentication::
    }
}
";
    let labels = complete_after(&base_files(), "src/Svc.php", content, "Authentication::").await;
    assert!(
        offers(&labels, "guard"),
        "the aliased import should forward AuthManager::guard, got: {labels:?}"
    );
}

/// A facade written out fully qualified needs no import.
#[tokio::test]
async fn an_inline_fully_qualified_facade_forwards_the_concrete_methods() {
    let content = "\
<?php
namespace App;
class Svc {
    public function go(): void {
        \\Illuminate\\Support\\Facades\\Auth::
    }
}
";
    let labels = complete_after(
        &base_files(),
        "src/Svc.php",
        content,
        "\\Illuminate\\Support\\Facades\\Auth::",
    )
    .await;
    assert!(
        offers(&labels, "guard"),
        "the fully-qualified facade should forward AuthManager::guard, got: {labels:?}"
    );
}

/// An application class that happens to be called `Auth` is that class,
/// not the facade.
#[tokio::test]
async fn an_imported_class_sharing_a_facade_name_is_not_the_facade() {
    let content = "\
<?php
namespace App;
use App\\Services\\Auth;
class Svc {
    public function go(): void {
        Auth::
    }
}
";
    let labels = complete_after(&base_files(), "src/Svc.php", content, "Auth::").await;
    assert!(
        offers(&labels, "run"),
        "the imported App\\Services\\Auth should resolve, got: {labels:?}"
    );
    assert!(
        !offers(&labels, "guard"),
        "a class that is not a facade forwards nothing, got: {labels:?}"
    );
}

// ─── Accessor shapes ────────────────────────────────────────────────────────

/// The surface a manager documents with `@mixin` is part of what the
/// facade forwards: `Auth::check()` reaches the guard contract.
#[tokio::test]
async fn a_managers_mixin_surface_is_forwarded_through_the_facade() {
    let content = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Auth;
class Svc {
    public function go(): void {
        Auth::
    }
}
";
    let labels = complete_after(&base_files(), "src/Svc.php", content, "Auth::").await;
    assert!(
        offers(&labels, "guard"),
        "expected the manager's own guard(), got: {labels:?}"
    );
    assert!(
        offers(&labels, "check"),
        "expected Guard::check through the manager's @mixin, got: {labels:?}"
    );
}

/// An accessor returning an interface's `::class` names a container key
/// that the framework binds, so the facade forwards the contract's methods.
#[tokio::test]
async fn a_contract_class_accessor_forwards_the_contract_methods() {
    let content = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Gate;
class Svc {
    public function go(): void {
        Gate::
    }
}
";
    let labels = complete_after(&base_files(), "src/Svc.php", content, "Gate::").await;
    assert!(
        offers(&labels, "allows"),
        "expected the Gate contract's allows(), got: {labels:?}"
    );
}

/// A `string` return type on `getFacadeAccessor()` does not stop the
/// binding key it returns from being read.
#[tokio::test]
async fn a_typed_accessor_returning_a_binding_key_forwards_the_bound_class() {
    let content = "\
<?php
namespace App;
use App\\Facades\\Guardian;
class Svc {
    public function go(): void {
        Guardian::
    }
}
";
    let labels = complete_after(&base_files(), "src/Svc.php", content, "Guardian::").await;
    assert!(
        offers(&labels, "guard"),
        "the typed accessor's 'auth' key should forward AuthManager::guard, got: {labels:?}"
    );
}

// ─── Go-to-definition through a facade ──────────────────────────────────────

async fn definition_targets_after(content: &str, after: &str) -> Vec<String> {
    let mut files = base_files();
    files.push(("src/Svc.php", content));
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, &files);
    let uri = open_php_at(&backend, &dir, "src/Svc.php", content).await;
    let position = position_after(content, after);
    definition_locations(
        goto_definition_at(&backend, &uri, position.line, position.character).await,
    )
    .into_iter()
    .map(|location| location.uri.to_string())
    .collect()
}

/// A facade call jumps to the method on the class the call is forwarded
/// to, not to the facade.
#[tokio::test]
#[ignore = "known gap: go-to-definition on a facade call stops at the facade"]
async fn a_forwarded_facade_call_goes_to_the_concrete_declaration() {
    let content = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Auth;
class Svc {
    public function go(): void {
        Auth::guard();
    }
}
";
    let targets = definition_targets_after(content, "Auth::gu").await;
    assert!(
        targets
            .iter()
            .any(|uri| uri.ends_with("Auth/AuthManager.php")),
        "Auth::guard() should jump to AuthManager::guard, got: {targets:?}"
    );
}

/// A facade call the manager only forwards through `@mixin` jumps to the
/// contract that declares it.
#[tokio::test]
#[ignore = "known gap: go-to-definition on a facade call stops at the facade"]
async fn a_forwarded_mixin_call_goes_to_the_contract_declaration() {
    let content = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Auth;
class Svc {
    public function go(): void {
        Auth::check();
    }
}
";
    let targets = definition_targets_after(content, "Auth::ch").await;
    assert!(
        targets
            .iter()
            .any(|uri| uri.ends_with("Contracts/Auth/Guard.php")),
        "Auth::check() should jump to Guard::check, got: {targets:?}"
    );
}

// ─── Component tag naming ───────────────────────────────────────────────────

/// A workspace holding class-based components whose names take several
/// words, rendered from `resources/views/page.blade.php`.
fn component_workspace(template: &str) -> (phpantom_lsp::Backend, tempfile::TempDir) {
    create_psr4_workspace(
        BLADE_COMPONENT_COMPOSER,
        &[
            (
                "stubs/Illuminate/View/Component.php",
                ILLUMINATE_COMPONENT_STUB,
            ),
            ("stubs/Livewire/Component.php", LIVEWIRE_COMPONENT_STUB),
            (
                "app/View/Components/UserProfile.php",
                "<?php\nnamespace App\\View\\Components;\n\
                 use Illuminate\\View\\Component;\n\
                 class UserProfile extends Component {\n\
                     public function avatarUrl(): string { return ''; }\n\
                     public function render() {}\n\
                 }\n",
            ),
            (
                "app/View/Components/Admin/UserList.php",
                "<?php\nnamespace App\\View\\Components\\Admin;\n\
                 use Illuminate\\View\\Component;\n\
                 class UserList extends Component {\n\
                     public function rows(): array { return []; }\n\
                     public function render() {}\n\
                 }\n",
            ),
            ("resources/views/page.blade.php", template),
        ],
    )
}

/// Method labels carry a trailing `()`; property labels do not.
fn has_member(labels: &[String], name: &str) -> bool {
    labels
        .iter()
        .any(|label| label.trim_end_matches("()") == name)
}

async fn component_members(tag: &str) -> Vec<String> {
    let template = format!("<x-{tag}>\n{{{{ $component-> }}}}\n</x-{tag}>\n");
    let (backend, _dir) = component_workspace(&template);
    let uri = workspace_uri(&backend, "resources/views/page.blade.php");
    open_document(&backend, &uri, "blade", &template).await;
    complete_labels_at_opened_with_trigger(&backend, &uri, 1, 15, ">").await
}

/// `<x-user-profile>` is the kebab spelling of `UserProfile`.
#[tokio::test]
async fn a_multi_word_component_class_answers_to_its_kebab_tag() {
    let labels = component_members("user-profile").await;
    assert!(
        has_member(&labels, "avatarUrl"),
        "<x-user-profile> should resolve to UserProfile, got: {labels:?}"
    );
}

/// Each dot of `<x-admin.user-list>` is a namespace segment, and each
/// segment is kebab-cased on its own: `Admin\UserList`.
#[tokio::test]
async fn a_nested_multi_word_component_answers_to_its_dotted_kebab_tag() {
    let labels = component_members("admin.user-list").await;
    assert!(
        has_member(&labels, "rows"),
        "<x-admin.user-list> should resolve to Admin\\UserList, got: {labels:?}"
    );
}
