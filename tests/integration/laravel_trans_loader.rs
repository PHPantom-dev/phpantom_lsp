//! Tests for the translation-key diagnostic in a project that serves its
//! lines from somewhere other than the `lang/` directories.
//!
//! Rebinding `translator` or `translation.loader` moves the strings out of
//! reach: `vendor/`'s own `lang/` files are still on disk, so the enumerated
//! set is non-empty while covering none of the application's keys.
use crate::common::message_text;

use crate::common::{LARAVEL_SRC_COMPOSER, create_psr4_workspace, open_php};
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const PROVIDERS_PHP: &str = "<?php\nreturn [\n    App\\TranslationServiceProvider::class,\n];\n";

const LANG_MESSAGES: &str = "<?php\nreturn [\n    'welcome' => 'Welcome',\n];\n";

/// The framework's own provider, which every Laravel project registers.  Its
/// bindings must not read as a replacement.
const FILE_LOADER_PROVIDER: &str = "\
<?php
namespace App;

class TranslationServiceProvider {
    public function register(): void {
        $this->app->singleton('translation.loader', function ($app) {
            return new FileLoader($app['files'], [__DIR__.'/lang', $app['path.lang']]);
        });
    }
}
";

const DATABASE_LOADER_PROVIDER: &str = "\
<?php
namespace App;

class TranslationServiceProvider {
    public function register(): void {
        $this->app->singleton('translation.loader', function ($app) {
            $fileLoader = new FileLoader($app->make('files'), $app->make('path.lang'));

            return new DatabaseTranslationLoader($fileLoader);
        });
    }
}
";

const CONSUMER: &str = "\
<?php
namespace App;
class Greeting {
    public function demo(): void {
        __('messages.welcome');
        __('checkout.headline');
    }
}
";

async fn trans_diagnostics(provider: &str) -> Vec<String> {
    let (backend, dir) = create_psr4_workspace(
        LARAVEL_SRC_COMPOSER,
        &[
            ("bootstrap/providers.php", PROVIDERS_PHP),
            ("src/TranslationServiceProvider.php", provider),
            ("lang/en/messages.php", LANG_MESSAGES),
            ("src/Greeting.php", CONSUMER),
        ],
    );
    let uri = Uri::from_file_path(dir.path().join("src/Greeting.php")).unwrap();

    backend.initialized(InitializedParams {}).await;
    open_php(&backend, &uri, CONSUMER).await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), CONSUMER, &mut diags);

    diags
        .iter()
        .filter(|d| matches!(&d.code, Some(Code::String(s)) if s == "invalid_laravel_trans"))
        .map(|d| message_text(&d.message).to_string())
        .collect()
}

#[tokio::test]
async fn translation_keys_are_judged_against_the_file_loader() {
    let messages = trans_diagnostics(FILE_LOADER_PROVIDER).await;

    assert_eq!(
        messages.len(),
        1,
        "the key missing from lang/ should be flagged, got: {messages:?}"
    );
    assert!(
        messages[0].contains("checkout.headline"),
        "unexpected diagnostic: {}",
        messages[0]
    );
}

#[tokio::test]
async fn translation_keys_are_not_judged_when_the_loader_is_replaced() {
    let messages = trans_diagnostics(DATABASE_LOADER_PROVIDER).await;

    assert!(
        messages.is_empty(),
        "a database-backed loader puts the valid keys out of reach, got: {messages:?}"
    );
}
