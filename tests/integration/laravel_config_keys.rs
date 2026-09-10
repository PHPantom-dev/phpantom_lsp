//! Tests for the config-key diagnostic.
//!
//! A library package ships no `config/` directory of its own: its keys come
//! from the host application's config files, which we never see.  Keys under
//! such a root are unjudgeable, while a typo inside a config file we did read
//! must still be reported.  `Config::set()` is a write: it declares the key it
//! names, so a later read of that key is as valid as a read of one declared
//! on disk.

use crate::common::create_psr4_workspace;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

/// A package: the framework is a dev dependency of its test suite, and the
/// configuration it reads belongs to whatever application installs it.
const PACKAGE_COMPOSER_JSON: &str = r#"{
    "name": "acme/widgets",
    "require": { "illuminate/support": "^11.0" },
    "require-dev": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

const APP_CONFIG: &str = "\
<?php
return [
    'name' => 'Acme',
    'timezone' => 'UTC',
];
";

const FILESYSTEMS_CONFIG: &str = "\
<?php
return [
    'disks' => [
        'local' => ['driver' => 'local'],
    ],
];
";

const CONSUMER: &str = "\
<?php
namespace App;

use Illuminate\\Support\\Facades\\Config;

class Settings {
    public function demo(): void {
        config('app.name');
        config('acme.gateway.key');
        Config::set('filesystems.disks.ondemand', ['driver' => 'local']);
        config('app.tzimezone');
    }
}
";

/// The shapes that declare a key at runtime, each followed by a read of what
/// it declared.
const RUNTIME_WRITER: &str = "\
<?php
namespace App;

use Illuminate\\Support\\Facades\\Config;
use Illuminate\\Support\\Facades\\Storage;

class Fixtures {
    public function setUp(): void {
        Config::set('filesystems.disks.ondemand', ['driver' => 'local']);
        config(['filesystems.disks.inline' => ['driver' => 'local']]);
        Storage::fake('scratch');
    }

    public function demo(): void {
        Storage::disk('ondemand');
        config('filesystems.disks.inline.driver');
        Storage::disk('scratch');
        Storage::disk('nowhere');
    }
}
";

async fn config_diagnostics_for(
    composer_json: &str,
    consumer_path: &str,
    consumer: &str,
) -> Vec<String> {
    let (backend, dir) = create_psr4_workspace(
        composer_json,
        &[
            ("config/app.php", APP_CONFIG),
            ("config/filesystems.php", FILESYSTEMS_CONFIG),
            (consumer_path, consumer),
        ],
    );
    backend.initialized(InitializedParams {}).await;

    let uri = Url::from_file_path(dir.path().join(consumer_path)).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: consumer.to_string(),
            },
        })
        .await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), consumer, &mut diags);

    diags
        .iter()
        .filter(
            |d| matches!(&d.code, Some(NumberOrString::String(s)) if s == "invalid_laravel_config"),
        )
        .map(|d| d.message.clone())
        .collect()
}

#[tokio::test]
async fn only_a_typo_in_a_config_file_we_read_is_reported() {
    let messages = config_diagnostics_for(COMPOSER_JSON, "src/Settings.php", CONSUMER).await;

    assert_eq!(
        messages.len(),
        1,
        "an unknown root file and a written key are unjudgeable, got: {messages:?}"
    );
    assert!(
        messages[0].contains("app.tzimezone"),
        "the flagged key should be the typo, got: {}",
        messages[0]
    );
}

#[tokio::test]
async fn a_key_written_at_runtime_is_a_declaration() {
    let messages = config_diagnostics_for(COMPOSER_JSON, "src/Fixtures.php", RUNTIME_WRITER).await;

    assert_eq!(
        messages.len(),
        1,
        "only the disk nothing configures is unknown, got: {messages:?}"
    );
    assert!(
        messages[0].contains("filesystems.disks.nowhere"),
        "the flagged key should be the unconfigured disk, got: {}",
        messages[0]
    );
}

#[tokio::test]
async fn a_library_reads_config_its_host_application_declares() {
    let messages =
        config_diagnostics_for(PACKAGE_COMPOSER_JSON, "src/Settings.php", CONSUMER).await;

    assert!(
        messages.is_empty(),
        "a package's config comes from the application that installs it, got: {messages:?}"
    );
}
