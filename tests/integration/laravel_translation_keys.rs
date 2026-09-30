//! Translation keys: group files, JSON catalogues, and package namespaces.
//!
//! Cases adapted from laravel-lsp's MIT-licensed test suite.
//!
//! A key is resolved the way Laravel's `Translator` and `FileLoader` resolve
//! it: the locale's JSON catalogue first, then `lang/<locale>/<group>.php`
//! walked along the dotted path, and for a `package::group.key` the
//! directory the package's provider registered with `loadTranslationsFrom()`,
//! overlaid with the application's published copy under
//! `lang/vendor/<package>/<locale>/`.

use crate::common::{
    LARAVEL_APP_COMPOSER, complete_labels_at_opened, create_initialized_psr4_workspace,
    create_psr4_workspace, definition_locations, goto_definition_at, markup_hover_at,
    messages_with_code, open_blade_template, position_after, position_of,
};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// An application that also carries a package under `packages/billing`,
/// autoloaded from the root `composer.json` the way a path package is.
const PACKAGE_COMPOSER: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": { "App\\": "app/", "Acme\\Billing\\": "packages/billing/src/" } }
}"#;

const PACKAGE_PROVIDER_LIST: &str =
    "<?php\nreturn [\n    Acme\\Billing\\BillingServiceProvider::class,\n];\n";

const APP_PROVIDER_LIST: &str =
    "<?php\nreturn [\n    App\\Providers\\AppServiceProvider::class,\n];\n";

/// A package provider registering its lines under the `billing` namespace.
const BILLING_PROVIDER: &str = "\
<?php
namespace Acme\\Billing;

class BillingServiceProvider
{
    public function boot(): void
    {
        $this->loadTranslationsFrom(__DIR__.'/../lang', 'billing');
    }
}
";

const BILLING_INVOICE: &str = "\
<?php
return [
    'total' => 'Total',
    'lines' => [
        'tax' => 'Tax',
    ],
];
";

const AUTH_EN: &str = "\
<?php
return [
    'failed' => 'These credentials do not match our records.',
    'throttle' => [
        'message' => 'Too many attempts.',
    ],
];
";

const AUTH_ES: &str = "\
<?php
return [
    'failed' => 'Estas credenciales no coinciden.',
];
";

const MESSAGES_EN: &str = "\
<?php
return [
    'welcome' => 'Welcome',
    'apples' => '{0} No apples|[1,*] Some apples',
];
";

const JSON_EN: &str = "\
{
    \"Welcome to our app\": \"Welcome to our app\",
    \"Sign in\": \"Log in\"
}
";

const JSON_FR: &str = "\
{
    \"Welcome to our app\": \"Bienvenue dans notre application\"
}
";

/// A class in `app/` whose one method runs `body`.
fn caller(body: &str) -> String {
    format!(
        "<?php\nnamespace App;\nclass Demo {{\n    public function go(): void {{\n        {body}\n    }}\n}}\n"
    )
}

/// A workspace holding `files` plus `app/Demo.php` running `body`, scanned
/// and with the demo open.
async fn workspace(
    composer: &str,
    files: &[(&str, &str)],
    body: &str,
) -> (phpantom_lsp::Backend, tempfile::TempDir, Url, String) {
    let content = caller(body);
    let mut all: Vec<(&str, &str)> = files.to_vec();
    all.push(("app/Demo.php", content.as_str()));
    let (backend, dir, uri) =
        create_initialized_psr4_workspace(composer, &all, "app/Demo.php").await;
    drop(all);
    (backend, dir, uri, content)
}

/// The go-to-definition targets a couple of characters into the first
/// occurrence of `needle` in `content`.
async fn definitions_at(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    content: &str,
    needle: &str,
) -> Vec<Location> {
    let position = position_of(content, needle);
    definition_locations(
        goto_definition_at(backend, uri, position.line, position.character + 2).await,
    )
}

/// The hover a couple of characters into the first occurrence of `needle`.
async fn hover_on(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    content: &str,
    needle: &str,
) -> String {
    let position = position_of(content, needle);
    markup_hover_at(backend, uri, position.line, position.character + 2).await
}

/// The `invalid_laravel_trans` messages the slow pass reports on `uri`.
fn trans_diagnostics(backend: &phpantom_lsp::Backend, uri: &Url, content: &str) -> Vec<String> {
    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), content, &mut diags);
    messages_with_code(&diags, "invalid_laravel_trans")
}

fn location_in<'a>(found: &'a [Location], suffix: &str) -> Option<&'a Location> {
    found
        .iter()
        .find(|location| location.uri.as_str().ends_with(suffix))
}

// ─── Group files across locales ─────────────────────────────────────────────

/// Every locale that declares a key is a place the key is defined, each at
/// the line that declares it.
#[tokio::test]
async fn a_key_resolves_in_every_locale_that_declares_it() {
    let (backend, _dir, uri, content) = workspace(
        LARAVEL_APP_COMPOSER,
        &[("lang/en/auth.php", AUTH_EN), ("lang/es/auth.php", AUTH_ES)],
        "__('auth.failed');",
    )
    .await;

    let found = definitions_at(&backend, &uri, &content, "auth.failed").await;
    assert_eq!(found.len(), 2, "one location per locale, got {found:?}");
    for suffix in ["/lang/en/auth.php", "/lang/es/auth.php"] {
        let location = location_in(&found, suffix)
            .unwrap_or_else(|| panic!("expected a location in {suffix}, got {found:?}"));
        assert_eq!(location.range.start.line, 2, "'failed' is on line 2");
    }
}

/// Hover quotes the line in the application's configured locale, whatever
/// order the locale directories are read in.
#[tokio::test]
async fn hover_quotes_the_configured_locale() {
    for (config, expected) in [
        (None, "`These credentials do not match our records.`"),
        (
            Some("<?php\nreturn ['locale' => 'es', 'fallback_locale' => 'en'];\n"),
            "`Estas credenciales no coinciden.`",
        ),
        (
            Some("<?php\nreturn ['locale' => 'fr', 'fallback_locale' => 'en'];\n"),
            "`These credentials do not match our records.`",
        ),
    ] {
        let mut files = vec![("lang/es/auth.php", AUTH_ES), ("lang/en/auth.php", AUTH_EN)];
        if let Some(app) = config {
            files.push(("config/app.php", app));
        }
        let (backend, _dir, uri, content) =
            workspace(LARAVEL_APP_COMPOSER, &files, "__('auth.failed');").await;
        let text = hover_on(&backend, &uri, &content, "auth.failed").await;
        assert!(text.contains(expected), "config {config:?}: got {text}");
    }
}

/// A locale that has no file for the group simply has nothing to offer.
#[tokio::test]
async fn a_locale_without_the_group_file_contributes_no_location() {
    let (backend, _dir, uri, content) = workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("lang/en/auth.php", AUTH_EN),
            ("lang/es/messages.php", MESSAGES_EN),
        ],
        "__('auth.failed');",
    )
    .await;

    let found = definitions_at(&backend, &uri, &content, "auth.failed").await;
    assert_eq!(found.len(), 1, "only en ships auth.php, got {found:?}");
    assert!(
        location_in(&found, "/lang/en/auth.php").is_some(),
        "got {found:?}"
    );
}

/// A line nested inside a group is quoted like a top-level one.
#[tokio::test]
async fn a_nested_line_is_what_hover_quotes() {
    let (backend, _dir, uri, content) = workspace(
        LARAVEL_APP_COMPOSER,
        &[("lang/en/auth.php", AUTH_EN)],
        "__('auth.throttle.message');",
    )
    .await;

    let text = hover_on(&backend, &uri, &content, "auth.throttle.message").await;
    assert!(
        text.contains("`Too many attempts.`") && text.contains("lang/en/auth.php"),
        "got {text}"
    );
}

/// The pre-`lang/` layout, `resources/lang`, is still a translation root.
#[tokio::test]
async fn the_legacy_resources_lang_directory_resolves() {
    let (backend, _dir, uri, content) = workspace(
        LARAVEL_APP_COMPOSER,
        &[("resources/lang/en/messages.php", MESSAGES_EN)],
        "__('messages.welcome');",
    )
    .await;

    let found = definitions_at(&backend, &uri, &content, "messages.welcome").await;
    let location = location_in(&found, "/resources/lang/en/messages.php")
        .unwrap_or_else(|| panic!("expected resources/lang/en/messages.php, got {found:?}"));
    assert_eq!(location.range.start.line, 2);
}

/// `trans_choice()` names a key the same way `__()` does.
#[tokio::test]
async fn trans_choice_reaches_the_line() {
    let (backend, _dir, uri, content) = workspace(
        LARAVEL_APP_COMPOSER,
        &[("lang/en/messages.php", MESSAGES_EN)],
        "trans_choice('messages.apples', 3);",
    )
    .await;

    let found = definitions_at(&backend, &uri, &content, "messages.apples").await;
    let location = location_in(&found, "/lang/en/messages.php")
        .unwrap_or_else(|| panic!("expected lang/en/messages.php, got {found:?}"));
    assert_eq!(location.range.start.line, 3);
}

/// `FileLoader` loads `{path}/{locale}/{group}.php`, and a group may name a
/// subdirectory: `admin/users.title` is the `title` line of
/// `lang/en/admin/users.php`.
#[tokio::test]
async fn a_subdirectory_group_reaches_its_file() {
    let (backend, _dir, uri, content) = workspace(
        LARAVEL_APP_COMPOSER,
        &[(
            "lang/en/admin/users.php",
            "<?php\nreturn [\n    'title' => 'Users',\n];\n",
        )],
        "__('admin/users.title');",
    )
    .await;

    let found = definitions_at(&backend, &uri, &content, "admin/users.title").await;
    let location = location_in(&found, "/lang/en/admin/users.php")
        .unwrap_or_else(|| panic!("expected lang/en/admin/users.php, got {found:?}"));
    assert_eq!(location.range.start.line, 2);
}

/// The same subdirectory group is a key the diagnostic knows, and its
/// leaf's bare name (`users.title`) is not.
#[tokio::test]
async fn a_subdirectory_group_is_known() {
    let (backend, _dir, uri, content) = workspace(
        LARAVEL_APP_COMPOSER,
        &[(
            "lang/en/admin/users.php",
            "<?php\nreturn [\n    'title' => 'Users',\n];\n",
        )],
        "__('admin/users.title');\n        __('users.title');",
    )
    .await;

    let messages = trans_diagnostics(&backend, &uri, &content);
    assert_eq!(
        messages.len(),
        1,
        "only the key missing its directory is unknown, got {messages:?}"
    );
    assert!(messages[0].contains("'users.title'"), "got {messages:?}");
}

// ─── JSON catalogues ────────────────────────────────────────────────────────

/// A phrase key is looked up in each locale's JSON catalogue.
#[tokio::test]
async fn a_json_phrase_resolves_in_every_locale_catalogue() {
    let (backend, _dir, uri, content) = workspace(
        LARAVEL_APP_COMPOSER,
        &[("lang/en.json", JSON_EN), ("lang/fr.json", JSON_FR)],
        "__('Welcome to our app');",
    )
    .await;

    let found = definitions_at(&backend, &uri, &content, "Welcome to our app").await;
    for suffix in ["/lang/en.json", "/lang/fr.json"] {
        assert!(
            location_in(&found, suffix).is_some(),
            "expected a location in {suffix}, got {found:?}"
        );
    }
}

/// Navigating to a phrase lands on the entry that declares it, not on the
/// top of the catalogue.
#[tokio::test]
async fn a_json_phrase_lands_on_its_own_line() {
    let (backend, _dir, uri, content) = workspace(
        LARAVEL_APP_COMPOSER,
        &[("lang/en.json", JSON_EN)],
        "__('Sign in');",
    )
    .await;

    let found = definitions_at(&backend, &uri, &content, "Sign in").await;
    let location = location_in(&found, "/lang/en.json")
        .unwrap_or_else(|| panic!("expected lang/en.json, got {found:?}"));
    assert_eq!(location.range.start.line, 2, "\"Sign in\" is on line 2");
}

/// Hover quotes the translated phrase and names the catalogue.
#[tokio::test]
async fn a_json_phrase_is_what_hover_quotes() {
    let (backend, _dir, uri, content) = workspace(
        LARAVEL_APP_COMPOSER,
        &[("lang/en.json", JSON_EN)],
        "__('Sign in');",
    )
    .await;

    let text = hover_on(&backend, &uri, &content, "Sign in").await;
    assert!(
        text.contains("`Log in`") && text.contains("lang/en.json"),
        "got {text}"
    );
}

/// `Translator::get()` consults the JSON catalogue before it parses the key
/// into a group, so a dotted key the catalogue declares is the catalogue's
/// line even when a group file declares it too.
#[tokio::test]
async fn a_json_line_wins_over_a_group_file_line_for_the_same_key() {
    let (backend, _dir, uri, content) = workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("lang/en/auth.php", AUTH_EN),
            (
                "lang/en.json",
                "{\n    \"auth.failed\": \"Wrong email or password.\"\n}\n",
            ),
        ],
        "__('auth.failed');",
    )
    .await;

    let text = hover_on(&backend, &uri, &content, "auth.failed").await;
    assert!(
        text.contains("`Wrong email or password.`"),
        "the JSON catalogue is read first, got {text}"
    );
}

/// A phrase the catalogue declares is a known key.
#[tokio::test]
async fn a_json_phrase_is_a_known_key() {
    let (backend, _dir, uri, content) = workspace(
        LARAVEL_APP_COMPOSER,
        &[("lang/en.json", JSON_EN), ("lang/en/auth.php", AUTH_EN)],
        "__('Sign in');\n        __('auth.nope');",
    )
    .await;

    let messages = trans_diagnostics(&backend, &uri, &content);
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(messages[0].contains("'auth.nope'"), "got {messages:?}");
}

/// A package's `loadJsonTranslationsFrom()` directory adds its phrases to
/// the same catalogue the application's JSON files fill.
#[tokio::test]
async fn a_package_json_phrase_is_a_known_key() {
    let provider = "\
<?php
namespace Acme\\Billing;

class BillingServiceProvider
{
    public function boot(): void
    {
        $this->loadJsonTranslationsFrom(__DIR__.'/../lang');
    }
}
";
    let (backend, _dir, uri, content) = workspace(
        PACKAGE_COMPOSER,
        &[
            ("bootstrap/providers.php", PACKAGE_PROVIDER_LIST),
            ("packages/billing/src/BillingServiceProvider.php", provider),
            (
                "packages/billing/lang/en.json",
                "{\n    \"Pay now\": \"Pay now\"\n}\n",
            ),
            ("lang/en/auth.php", AUTH_EN),
        ],
        "__('Pay now');\n        __('auth.nope');",
    )
    .await;

    let messages = trans_diagnostics(&backend, &uri, &content);
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(messages[0].contains("'auth.nope'"), "got {messages:?}");
}

/// The same package phrase is somewhere go-to-definition can land.
#[tokio::test]
async fn a_package_json_phrase_reaches_its_catalogue() {
    let provider = "\
<?php
namespace Acme\\Billing;

class BillingServiceProvider
{
    public function boot(): void
    {
        $this->loadJsonTranslationsFrom(__DIR__.'/../lang');
    }
}
";
    let (backend, _dir, uri, content) = workspace(
        PACKAGE_COMPOSER,
        &[
            ("bootstrap/providers.php", PACKAGE_PROVIDER_LIST),
            ("packages/billing/src/BillingServiceProvider.php", provider),
            (
                "packages/billing/lang/en.json",
                "{\n    \"Pay now\": \"Pay now\"\n}\n",
            ),
        ],
        "__('Pay now');",
    )
    .await;

    let found = definitions_at(&backend, &uri, &content, "Pay now").await;
    assert!(
        location_in(&found, "/packages/billing/lang/en.json").is_some(),
        "got {found:?}"
    );
}

// ─── Package namespaces ─────────────────────────────────────────────────────

fn billing_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("bootstrap/providers.php", PACKAGE_PROVIDER_LIST),
        (
            "packages/billing/src/BillingServiceProvider.php",
            BILLING_PROVIDER,
        ),
        ("packages/billing/lang/en/invoice.php", BILLING_INVOICE),
    ]
}

/// A `package::group.key` resolves inside the directory the package's
/// provider registered for that namespace.
#[tokio::test]
async fn a_namespaced_key_reaches_the_registered_package_directory() {
    let (backend, _dir, uri, content) = workspace(
        PACKAGE_COMPOSER,
        &billing_files(),
        "__('billing::invoice.total');",
    )
    .await;

    let found = definitions_at(&backend, &uri, &content, "billing::invoice.total").await;
    let location = location_in(&found, "/packages/billing/lang/en/invoice.php")
        .unwrap_or_else(|| panic!("expected the package's invoice.php, got {found:?}"));
    assert_eq!(location.range.start.line, 2, "'total' is on line 2");
}

/// A nested package key lands on its leaf, not on the top of the file.
#[tokio::test]
async fn a_nested_namespaced_key_lands_on_the_leaf_line() {
    let (backend, _dir, uri, content) = workspace(
        PACKAGE_COMPOSER,
        &billing_files(),
        "__('billing::invoice.lines.tax');",
    )
    .await;

    let found = definitions_at(&backend, &uri, &content, "billing::invoice.lines.tax").await;
    let location = location_in(&found, "/packages/billing/lang/en/invoice.php")
        .unwrap_or_else(|| panic!("expected the package's invoice.php, got {found:?}"));
    assert_eq!(location.range.start.line, 4, "'tax' is on line 4");
}

/// Hover on a package key quotes the package's line and names its file.
#[tokio::test]
async fn hover_quotes_a_package_line_and_names_its_file() {
    let (backend, _dir, uri, content) = workspace(
        PACKAGE_COMPOSER,
        &billing_files(),
        "__('billing::invoice.total');",
    )
    .await;

    let text = hover_on(&backend, &uri, &content, "billing::invoice.total").await;
    assert!(
        text.contains("`Total`") && text.contains("en/invoice.php"),
        "got {text}"
    );
}

/// A registered namespace whose group file does not exist has nowhere to
/// navigate to.
#[tokio::test]
async fn a_namespaced_key_without_a_group_file_has_no_definition() {
    let (backend, _dir, uri, content) = workspace(
        PACKAGE_COMPOSER,
        &billing_files(),
        "__('billing::missing.title');",
    )
    .await;

    let found = definitions_at(&backend, &uri, &content, "billing::missing.title").await;
    assert!(found.is_empty(), "got {found:?}");
}

/// The package's own keys are judged against what it ships.
#[tokio::test]
async fn a_package_key_is_judged_against_what_the_package_ships() {
    let (backend, _dir, uri, content) = workspace(
        PACKAGE_COMPOSER,
        &billing_files(),
        "__('billing::invoice.total');\n        __('billing::invoice.lines.tax');\n        __('billing::invoice.nope');",
    )
    .await;

    let messages = trans_diagnostics(&backend, &uri, &content);
    assert_eq!(
        messages.len(),
        1,
        "only the key the package lacks is unknown, got {messages:?}"
    );
    assert!(
        messages[0].contains("'billing::invoice.nope'"),
        "got {messages:?}"
    );
}

/// `FileLoader::loadNamespaceOverrides()` lays
/// `lang/vendor/<namespace>/<locale>/<group>.php` over the package's own
/// file, so the published copy is a definition of the key too.
#[tokio::test]
async fn a_published_override_is_a_definition_of_the_package_key() {
    let mut files = billing_files();
    files.push((
        "lang/vendor/billing/en/invoice.php",
        "<?php\nreturn [\n    'total' => 'Grand total',\n];\n",
    ));
    let (backend, _dir, uri, content) =
        workspace(PACKAGE_COMPOSER, &files, "__('billing::invoice.total');").await;

    let found = definitions_at(&backend, &uri, &content, "billing::invoice.total").await;
    let location = location_in(&found, "/lang/vendor/billing/en/invoice.php")
        .unwrap_or_else(|| panic!("expected the published override, got {found:?}"));
    assert_eq!(location.range.start.line, 2);
}

/// The override replaces the package's line, so it is the one hover quotes.
#[tokio::test]
async fn a_published_override_is_the_line_hover_quotes() {
    let mut files = billing_files();
    files.push((
        "lang/vendor/billing/en/invoice.php",
        "<?php\nreturn [\n    'total' => 'Grand total',\n];\n",
    ));
    let (backend, _dir, uri, content) =
        workspace(PACKAGE_COMPOSER, &files, "__('billing::invoice.total');").await;

    let text = hover_on(&backend, &uri, &content, "billing::invoice.total").await;
    assert!(text.contains("`Grand total`"), "got {text}");
}

/// The override is merged with `array_replace_recursive()`, so a key only
/// the published copy adds is as real as one the package ships.
#[tokio::test]
async fn a_key_only_the_published_override_adds_is_known() {
    let mut files = billing_files();
    files.push((
        "lang/vendor/billing/en/invoice.php",
        "<?php\nreturn [\n    'discount' => 'Discount',\n];\n",
    ));
    let (backend, _dir, uri, content) = workspace(
        PACKAGE_COMPOSER,
        &files,
        "__('billing::invoice.discount');\n        __('billing::invoice.nope');",
    )
    .await;

    let messages = trans_diagnostics(&backend, &uri, &content);
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("'billing::invoice.nope'"),
        "got {messages:?}"
    );
}

/// `loadNamespaced()` returns nothing for a namespace no provider
/// registered, published files or not.
#[tokio::test]
async fn an_unregistered_namespace_resolves_nowhere() {
    let (backend, _dir, uri, content) = workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("lang/en/auth.php", AUTH_EN),
            (
                "lang/vendor/ghost/en/messages.php",
                "<?php\nreturn [\n    'hi' => 'Hi',\n];\n",
            ),
        ],
        "__('ghost::messages.hi');",
    )
    .await;

    let found = definitions_at(&backend, &uri, &content, "ghost::messages.hi").await;
    assert!(found.is_empty(), "got {found:?}");

    let messages = trans_diagnostics(&backend, &uri, &content);
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("'ghost::messages.hi'"),
        "got {messages:?}"
    );
}

/// A published package file under `lang/vendor/` is only ever read for its
/// namespace, never as an application group of the same name.
#[tokio::test]
async fn a_published_package_file_is_not_an_application_group() {
    let mut files = billing_files();
    files.push((
        "lang/vendor/billing/en/invoice.php",
        "<?php\nreturn [\n    'total' => 'Grand total',\n];\n",
    ));
    let (backend, _dir, uri, content) =
        workspace(PACKAGE_COMPOSER, &files, "__('invoice.total');").await;

    let found = definitions_at(&backend, &uri, &content, "invoice.total").await;
    assert!(
        location_in(&found, "/lang/vendor/billing/en/invoice.php").is_none(),
        "`invoice.total` is not `billing::invoice.total`, got {found:?}"
    );
}

/// Nor does it make the bare key valid.
#[tokio::test]
async fn a_key_only_a_published_package_file_declares_is_unknown() {
    let (backend, _dir, uri, content) = workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("lang/en/auth.php", AUTH_EN),
            (
                "lang/vendor/billing/en/invoice.php",
                "<?php\nreturn [\n    'total' => 'Grand total',\n];\n",
            ),
        ],
        "__('invoice.total');",
    )
    .await;

    let messages = trans_diagnostics(&backend, &uri, &content);
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(messages[0].contains("'invoice.total'"), "got {messages:?}");
}

/// A package's own `lang/` directory holds lines for its namespace, not
/// application groups.
#[tokio::test]
async fn a_package_translation_is_not_an_application_group() {
    let mut files = billing_files();
    files.push(("lang/en/auth.php", AUTH_EN));
    let (backend, _dir, uri, content) =
        workspace(PACKAGE_COMPOSER, &files, "__('invoice.total');").await;

    let messages = trans_diagnostics(&backend, &uri, &content);
    assert_eq!(
        messages.len(),
        1,
        "`invoice.total` is only reachable as `billing::invoice.total`, got {messages:?}"
    );
}

// ─── How a provider names its directory ─────────────────────────────────────

/// An application provider registering `lang/app` through `lang_path()`.
const LANG_PATH_PROVIDER: &str = "\
<?php
namespace App\\Providers;

class AppServiceProvider
{
    public function boot(): void
    {
        $this->loadTranslationsFrom(lang_path('app'), 'app');
    }
}
";

const NOTIFICATION_LINES: &str = "\
<?php
return [
    'status_change' => [
        'title' => 'Status changed',
    ],
];
";

/// `base_path('lang/custom')` names a directory relative to the project root.
#[tokio::test]
async fn a_namespace_registered_through_base_path_resolves() {
    let provider = "\
<?php
namespace App\\Providers;

class AppServiceProvider
{
    public function boot(): void
    {
        $this->loadTranslationsFrom(base_path('lang/custom'), 'custom');
    }
}
";
    let (backend, _dir, uri, content) = workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("bootstrap/providers.php", APP_PROVIDER_LIST),
            ("app/Providers/AppServiceProvider.php", provider),
            (
                "lang/custom/en/labels.php",
                "<?php\nreturn [\n    'save' => 'Save',\n];\n",
            ),
        ],
        "__('custom::labels.save');",
    )
    .await;

    let found = definitions_at(&backend, &uri, &content, "custom::labels.save").await;
    let location = location_in(&found, "/lang/custom/en/labels.php")
        .unwrap_or_else(|| panic!("expected lang/custom/en/labels.php, got {found:?}"));
    assert_eq!(location.range.start.line, 2);
}

/// `lang_path('app')` names `lang/app`, which is how an application groups
/// lines of its own under a namespace.
#[tokio::test]
async fn a_namespace_registered_through_lang_path_resolves() {
    let (backend, _dir, uri, content) = workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("bootstrap/providers.php", APP_PROVIDER_LIST),
            ("app/Providers/AppServiceProvider.php", LANG_PATH_PROVIDER),
            ("lang/app/en/notification.php", NOTIFICATION_LINES),
        ],
        "__('app::notification.status_change.title');",
    )
    .await;

    let found = definitions_at(
        &backend,
        &uri,
        &content,
        "app::notification.status_change.title",
    )
    .await;
    let location = location_in(&found, "/lang/app/en/notification.php")
        .unwrap_or_else(|| panic!("expected lang/app/en/notification.php, got {found:?}"));
    assert_eq!(location.range.start.line, 3, "'title' is on line 3");
}

/// The same registration makes the key one the diagnostic knows.
#[tokio::test]
async fn a_key_under_a_lang_path_namespace_is_known() {
    let (backend, _dir, uri, content) = workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("bootstrap/providers.php", APP_PROVIDER_LIST),
            ("app/Providers/AppServiceProvider.php", LANG_PATH_PROVIDER),
            ("lang/app/en/notification.php", NOTIFICATION_LINES),
        ],
        "__('app::notification.status_change.title');\n        __('app::notification.nope');",
    )
    .await;

    let messages = trans_diagnostics(&backend, &uri, &content);
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("'app::notification.nope'"),
        "got {messages:?}"
    );
}

/// `dirname(__DIR__).'/lang'` is the package root's `lang/` directory.
#[tokio::test]
async fn a_namespace_registered_through_dirname_dir_resolves() {
    let provider = "\
<?php
namespace Acme\\Billing;

class BillingServiceProvider
{
    public function boot(): void
    {
        $this->loadTranslationsFrom(dirname(__DIR__).'/lang', 'billing');
    }
}
";
    let (backend, _dir, uri, content) = workspace(
        PACKAGE_COMPOSER,
        &[
            ("bootstrap/providers.php", PACKAGE_PROVIDER_LIST),
            ("packages/billing/src/BillingServiceProvider.php", provider),
            ("packages/billing/lang/en/invoice.php", BILLING_INVOICE),
        ],
        "__('billing::invoice.total');",
    )
    .await;

    let found = definitions_at(&backend, &uri, &content, "billing::invoice.total").await;
    let location = location_in(&found, "/packages/billing/lang/en/invoice.php")
        .unwrap_or_else(|| panic!("expected the package's invoice.php, got {found:?}"));
    assert_eq!(location.range.start.line, 2);
}

/// A `spatie/laravel-package-tools` provider never calls
/// `loadTranslationsFrom()` itself: `->hasTranslations()` has the base class
/// register `<package>/resources/lang` under the package's short name, which
/// drops a leading `laravel-`.
#[tokio::test]
async fn a_package_tools_provider_registers_its_short_name() {
    let provider = "\
<?php
namespace Acme\\Billing;

use Spatie\\LaravelPackageTools\\Package;
use Spatie\\LaravelPackageTools\\PackageServiceProvider;

class BillingServiceProvider extends PackageServiceProvider
{
    public function configurePackage(Package $package): void
    {
        $package
            ->name('laravel-billing')
            ->hasTranslations();
    }
}
";
    let (backend, _dir, uri, content) = workspace(
        PACKAGE_COMPOSER,
        &[
            ("bootstrap/providers.php", PACKAGE_PROVIDER_LIST),
            ("packages/billing/src/BillingServiceProvider.php", provider),
            (
                "packages/billing/resources/lang/en/invoice.php",
                BILLING_INVOICE,
            ),
        ],
        "__('billing::invoice.total');",
    )
    .await;

    let found = definitions_at(&backend, &uri, &content, "billing::invoice.total").await;
    assert!(
        location_in(&found, "/packages/billing/resources/lang/en/invoice.php").is_some(),
        "got {found:?}"
    );
}

// ─── The diagnostic ─────────────────────────────────────────────────────────

/// A nested leaf, the group above it, and the whole file are all keys;
/// a leaf the group lacks is not.
#[tokio::test]
async fn nested_keys_and_groups_are_known_and_a_missing_leaf_is_not() {
    let (backend, _dir, uri, content) = workspace(
        LARAVEL_APP_COMPOSER,
        &[("lang/en/auth.php", AUTH_EN)],
        "__('auth.throttle.message');\n        __('auth.throttle');\n        __('auth');\n        __('auth.throttle.nope');",
    )
    .await;

    let messages = trans_diagnostics(&backend, &uri, &content);
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("'auth.throttle.nope'"),
        "got {messages:?}"
    );
}

/// A key whose group file no locale ships is unknown.
#[tokio::test]
async fn a_key_whose_group_file_does_not_exist_is_unknown() {
    let (backend, _dir, uri, content) = workspace(
        LARAVEL_APP_COMPOSER,
        &[("lang/en/auth.php", AUTH_EN)],
        "__('validation.required');",
    )
    .await;

    let messages = trans_diagnostics(&backend, &uri, &content);
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("'validation.required'"),
        "got {messages:?}"
    );
}

// ─── Completion ─────────────────────────────────────────────────────────────

/// Completion offers group lines at every depth, the groups themselves, and
/// the JSON catalogue's phrases.
#[tokio::test]
async fn completion_offers_phrases_nested_keys_and_groups() {
    let (backend, _dir, uri, content) = workspace(
        LARAVEL_APP_COMPOSER,
        &[("lang/en/auth.php", AUTH_EN), ("lang/en.json", JSON_EN)],
        "__('');",
    )
    .await;

    let position = position_after(&content, "__('");
    let labels = complete_labels_at_opened(&backend, &uri, position.line, position.character).await;
    for expected in [
        "auth.failed",
        "auth.throttle",
        "auth.throttle.message",
        "Sign in",
        "Welcome to our app",
    ] {
        assert!(
            labels.iter().any(|label| label == expected),
            "expected `{expected}`, got {labels:?}"
        );
    }
}

/// A package's keys are offered under its namespace.
#[tokio::test]
async fn completion_offers_package_keys_under_their_namespace() {
    let (backend, _dir, uri, content) =
        workspace(PACKAGE_COMPOSER, &billing_files(), "__('billing::');").await;

    let position = position_after(&content, "__('billing::");
    let labels = complete_labels_at_opened(&backend, &uri, position.line, position.character).await;
    for expected in ["billing::invoice.total", "billing::invoice.lines.tax"] {
        assert!(
            labels.iter().any(|label| label == expected),
            "expected `{expected}`, got {labels:?}"
        );
    }
}

// ─── Blade ──────────────────────────────────────────────────────────────────

/// Open `template` as `resources/views/welcome.blade.php` in a scanned
/// workspace that ships `lang/en/messages.php`.
async fn blade_workspace(template: &str) -> (phpantom_lsp::Backend, tempfile::TempDir, Url) {
    let (backend, dir) = create_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("lang/en/messages.php", MESSAGES_EN),
            ("resources/views/welcome.blade.php", template),
        ],
    );
    backend.initialized(InitializedParams {}).await;
    let uri = open_blade_template(&backend, "resources/views/welcome.blade.php").await;
    (backend, dir, uri)
}

/// A key echoed from a template resolves like one in PHP.
#[tokio::test]
async fn a_translation_in_a_blade_echo_reaches_the_line() {
    let template = "<h1>{{ __('messages.welcome') }}</h1>\n";
    let (backend, _dir, uri) = blade_workspace(template).await;

    let found = definitions_at(&backend, &uri, template, "messages.welcome").await;
    let location = location_in(&found, "/lang/en/messages.php")
        .unwrap_or_else(|| panic!("expected lang/en/messages.php, got {found:?}"));
    assert_eq!(location.range.start.line, 2);
}

/// `@lang('key')` compiles to `app('translator')->get('key')`.
#[tokio::test]
async fn the_lang_directive_reaches_the_line() {
    let template = "<h1>@lang('messages.welcome')</h1>\n";
    let (backend, _dir, uri) = blade_workspace(template).await;

    let found = definitions_at(&backend, &uri, template, "messages.welcome").await;
    let location = location_in(&found, "/lang/en/messages.php")
        .unwrap_or_else(|| panic!("expected lang/en/messages.php, got {found:?}"));
    assert_eq!(location.range.start.line, 2);
}

/// `@choice('key', $n)` compiles to `app('translator')->choice('key', $n)`.
#[tokio::test]
async fn the_choice_directive_reaches_the_line() {
    let template = "<p>@choice('messages.apples', 3)</p>\n";
    let (backend, _dir, uri) = blade_workspace(template).await;

    let found = definitions_at(&backend, &uri, template, "messages.apples").await;
    let location = location_in(&found, "/lang/en/messages.php")
        .unwrap_or_else(|| panic!("expected lang/en/messages.php, got {found:?}"));
    assert_eq!(location.range.start.line, 3);
}
