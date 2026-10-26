//! Tests for the config-key diagnostic.
//!
//! An application owns its whole configuration, so a key under a root no
//! config file declares is reported as readily as a typo inside one.  A
//! library package ships no `config/` directory of its own: its keys come
//! from the host application's config files, which we never see, so none of
//! them are judged.  Neither are keys under a namespace a service provider
//! registers from a file we cannot read.  `Config::set()` is a write: it
//! declares the key it names, so a later read of that key is as valid as a
//! read of one declared on disk.
use crate::common::message_text;

use crate::common::{LARAVEL_SRC_COMPOSER, create_psr4_workspace, open_php};
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

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
    config_diagnostics_with(composer_json, consumer_path, consumer, &[]).await
}

async fn config_diagnostics_with(
    composer_json: &str,
    consumer_path: &str,
    consumer: &str,
    extra: &[(&str, &str)],
) -> Vec<String> {
    let mut files = vec![
        ("config/app.php", APP_CONFIG),
        ("config/filesystems.php", FILESYSTEMS_CONFIG),
        (consumer_path, consumer),
    ];
    files.extend_from_slice(extra);
    let (backend, dir) = create_psr4_workspace(composer_json, &files);
    backend.initialized(InitializedParams {}).await;

    let uri = Uri::from_file_path(dir.path().join(consumer_path)).unwrap();
    open_php(&backend, &uri, consumer).await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), consumer, &mut diags);

    diags
        .iter()
        .filter(
            |d| matches!(&d.code, Some(Code::String(s)) if s == "invalid_laravel_config" || s == "invalid_laravel_storage_disk"),
        )
        .map(|d| message_text(&d.message).to_string())
        .collect()
}

#[tokio::test]
async fn an_unknown_key_in_an_application_is_reported() {
    let messages = config_diagnostics_for(LARAVEL_SRC_COMPOSER, "src/Settings.php", CONSUMER).await;

    assert_eq!(
        messages,
        vec![
            "Unknown config key: 'acme.gateway.key'".to_string(),
            "Unknown config key: 'app.tzimezone'".to_string(),
        ],
        "a key under an undeclared root and a typo are both unknown, a written key is not"
    );
}

const PROVIDER_LIST: &str =
    "<?php\nreturn [\n    App\\Providers\\AcmeServiceProvider::class,\n];\n";

fn acme_provider(register: &str) -> String {
    format!(
        "<?php\nnamespace App\\Providers;\n\nclass AcmeServiceProvider\n{{\n    public function register(): void\n    {{\n        {register}\n    }}\n}}\n"
    )
}

/// A namespace a provider registers is real even when the file behind it is
/// missing, so its keys cannot be judged; the rest of the configuration
/// still can.
#[tokio::test]
async fn a_provider_namespace_whose_file_is_missing_is_unjudgeable() {
    let provider =
        acme_provider("$this->mergeConfigFrom(__DIR__.'/../../config-missing/acme.php', 'acme');");
    let messages = config_diagnostics_with(
        LARAVEL_SRC_COMPOSER,
        "src/Settings.php",
        CONSUMER,
        &[
            ("bootstrap/providers.php", PROVIDER_LIST),
            ("src/Providers/AcmeServiceProvider.php", &provider),
        ],
    )
    .await;

    assert_eq!(
        messages,
        vec!["Unknown config key: 'app.tzimezone'".to_string()],
    );
}

/// A provider that names its config namespace at runtime may have registered
/// any root, so only keys under a root we read are judged.
#[tokio::test]
async fn a_provider_namespace_named_at_runtime_leaves_unknown_roots_alone() {
    let provider = acme_provider(
        "$this->mergeConfigFrom(__DIR__.'/../../config-missing/acme.php', $this->name);",
    );
    let messages = config_diagnostics_with(
        LARAVEL_SRC_COMPOSER,
        "src/Settings.php",
        CONSUMER,
        &[
            ("bootstrap/providers.php", PROVIDER_LIST),
            ("src/Providers/AcmeServiceProvider.php", &provider),
        ],
    )
    .await;

    assert_eq!(
        messages,
        vec!["Unknown config key: 'app.tzimezone'".to_string()],
    );
}

#[tokio::test]
async fn a_key_written_at_runtime_is_a_declaration() {
    let messages =
        config_diagnostics_for(LARAVEL_SRC_COMPOSER, "src/Fixtures.php", RUNTIME_WRITER).await;

    assert_eq!(
        messages.len(),
        1,
        "only the disk nothing configures is unknown, got: {messages:?}"
    );
    assert!(
        messages[0].contains("storage disk: 'nowhere'"),
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
