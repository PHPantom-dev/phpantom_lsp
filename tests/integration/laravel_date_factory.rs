//! Tests for the configured Laravel date class (`Date::use()` /
//! `DateFactory::use()`).
//!
//! A service provider that calls `Date::use(CarbonImmutable::class)` makes the
//! `now()` / `today()` helpers and the `Date` facade resolve to that concrete
//! class.  The single-file refresh that runs on every edit must keep this
//! selection current: adding, changing, or *removing* the `Date::use()` call in
//! a provider must be reflected without restarting the language server, and an
//! edit to an unrelated file must never override the project's real
//! configuration.

use crate::common::create_psr4_workspace;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

const PROVIDERS_PHP: &str = "<?php\nreturn [\n    App\\Providers\\AppServiceProvider::class,\n];\n";

/// A provider that selects `CarbonImmutable` as the date factory class.
const PROVIDER_WITH_IMMUTABLE: &str = "\
<?php
namespace App\\Providers;
use Illuminate\\Support\\Facades\\Date;
use Carbon\\CarbonImmutable;
class AppServiceProvider {
    public function boot(): void {
        Date::use(CarbonImmutable::class);
    }
}
";

/// The same provider after the `Date::use()` call was deleted.
const PROVIDER_WITHOUT_USE: &str = "\
<?php
namespace App\\Providers;
class AppServiceProvider {
    public function boot(): void {
    }
}
";

async fn open(backend: &phpantom_lsp::Backend, uri: &str, text: &str) {
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: Url::parse(uri).unwrap(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;
}

/// Read the configured date class the backend currently resolves to.
///
/// `None` means discovery has not run; `Some(None)` means no project override
/// was found; `Some(Some(fqn))` is the selected class.
fn configured_date_class(backend: &phpantom_lsp::Backend) -> Option<Option<String>> {
    backend.laravel_date_class().read().clone()
}

#[tokio::test]
async fn removing_date_use_from_provider_clears_configured_class() {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("bootstrap/providers.php", PROVIDERS_PHP),
            (
                "src/Providers/AppServiceProvider.php",
                PROVIDER_WITH_IMMUTABLE,
            ),
        ],
    );

    // Startup discovery selects the provider's configured date class.
    backend.initialized(InitializedParams {}).await;
    assert_eq!(
        configured_date_class(&backend),
        Some(Some("Carbon\\CarbonImmutable".to_string())),
        "startup scan should pick up Date::use(CarbonImmutable::class)"
    );

    // Deleting the `Date::use()` call from the provider must clear the
    // selection so the helpers fall back to the framework default, rather
    // than keeping the stale class until the server restarts.
    let provider_uri = Url::from_file_path(dir.path().join("src/Providers/AppServiceProvider.php"))
        .unwrap()
        .to_string();
    open(&backend, &provider_uri, PROVIDER_WITHOUT_USE).await;

    assert_eq!(
        configured_date_class(&backend),
        Some(None),
        "removing Date::use() from a provider should clear the configured class"
    );
}

#[tokio::test]
async fn changing_date_use_in_provider_updates_configured_class() {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("bootstrap/providers.php", PROVIDERS_PHP),
            (
                "src/Providers/AppServiceProvider.php",
                PROVIDER_WITH_IMMUTABLE,
            ),
        ],
    );

    backend.initialized(InitializedParams {}).await;
    assert_eq!(
        configured_date_class(&backend),
        Some(Some("Carbon\\CarbonImmutable".to_string())),
    );

    // Switching the argument to a different class re-runs the scan and
    // records the new selection.
    let changed = "\
<?php
namespace App\\Providers;
use Illuminate\\Support\\Facades\\Date;
use Carbon\\Carbon;
class AppServiceProvider {
    public function boot(): void {
        Date::use(Carbon::class);
    }
}
";
    let provider_uri = Url::from_file_path(dir.path().join("src/Providers/AppServiceProvider.php"))
        .unwrap()
        .to_string();
    open(&backend, &provider_uri, changed).await;

    assert_eq!(
        configured_date_class(&backend),
        Some(Some("Carbon\\Carbon".to_string())),
        "changing the Date::use() argument should update the configured class"
    );
}

#[tokio::test]
async fn date_use_in_unrelated_file_does_not_override() {
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("bootstrap/providers.php", PROVIDERS_PHP),
            (
                "src/Providers/AppServiceProvider.php",
                PROVIDER_WITH_IMMUTABLE,
            ),
        ],
    );

    backend.initialized(InitializedParams {}).await;
    assert_eq!(
        configured_date_class(&backend),
        Some(Some("Carbon\\CarbonImmutable".to_string())),
    );

    // An ordinary (non-provider) file that happens to contain a
    // `Date::use()` call must not override the project's real configuration.
    let unrelated = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Date;
use Carbon\\Carbon;
class Helper {
    public function reset(): void {
        Date::use(Carbon::class);
    }
}
";
    open(&backend, "file:///src/Helper.php", unrelated).await;

    assert_eq!(
        configured_date_class(&backend),
        Some(Some("Carbon\\CarbonImmutable".to_string())),
        "a Date::use() call in a non-provider file must not override the configured class"
    );
}
