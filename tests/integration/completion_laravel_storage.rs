//! Integration tests for resolving `Storage::disk()` / `FilesystemManager`'s
//! disk-returning methods from the declared `Filesystem`/`Cloud` contract to
//! what the disks in `config/filesystems.php` are actually built from: the
//! concrete `FilesystemAdapter` for a driver the framework ships, and the
//! registered closure's own return type for a `Storage::extend()` driver.

use crate::common::{complete_labels_at_opened, create_psr4_workspace, open_php_at};

const COMPOSER_JSON: &str = r#"{
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "Illuminate\\Contracts\\Filesystem\\": "vendor/illuminate/Contracts/Filesystem/",
            "Illuminate\\Filesystem\\": "vendor/illuminate/Filesystem/",
            "Illuminate\\Support\\Facades\\": "vendor/illuminate/Support/Facades/"
        }
    }
}"#;

const FILESYSTEM_CONTRACT_PHP: &str = "\
<?php
namespace Illuminate\\Contracts\\Filesystem;
interface Filesystem {
    public function read(string $path);
}
";

const CLOUD_CONTRACT_PHP: &str = "\
<?php
namespace Illuminate\\Contracts\\Filesystem;
interface Cloud extends Filesystem {
    public function url(string $path);
}
";

const FILESYSTEM_ADAPTER_PHP: &str = "\
<?php
namespace Illuminate\\Filesystem;
use Illuminate\\Contracts\\Filesystem\\Cloud;
class FilesystemAdapter implements Cloud {
    public function read(string $path) { return null; }
    public function url(string $path) { return ''; }
    public function assertExists($path, $content = null) { return $this; }
    public function download($path, $name = null) { return null; }
}
";

const FILESYSTEM_MANAGER_PHP: &str = "\
<?php
namespace Illuminate\\Filesystem;
class FilesystemManager {
    /** @return \\Illuminate\\Contracts\\Filesystem\\Filesystem */
    public function drive($name = null) { return $this->disk($name); }
    /** @return \\Illuminate\\Contracts\\Filesystem\\Filesystem */
    public function disk($name = null) { return null; }
    /** @return \\Illuminate\\Contracts\\Filesystem\\Cloud */
    public function cloud() { return null; }
    /** @return \\Illuminate\\Contracts\\Filesystem\\Filesystem */
    public function build($config) { return null; }
}
";

const STORAGE_FACADE_PHP: &str = "\
<?php
namespace Illuminate\\Support\\Facades;
/**
 * @method static \\Illuminate\\Contracts\\Filesystem\\Filesystem drive(string|null $name = null)
 * @method static \\Illuminate\\Contracts\\Filesystem\\Filesystem disk(string|null $name = null)
 * @method static \\Illuminate\\Contracts\\Filesystem\\Cloud cloud()
 * @method static \\Illuminate\\Contracts\\Filesystem\\Filesystem build(string|array $config)
 */
class Storage {}
";

fn base_files() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "vendor/illuminate/Contracts/Filesystem/Filesystem.php",
            FILESYSTEM_CONTRACT_PHP,
        ),
        (
            "vendor/illuminate/Contracts/Filesystem/Cloud.php",
            CLOUD_CONTRACT_PHP,
        ),
        (
            "vendor/illuminate/Filesystem/FilesystemAdapter.php",
            FILESYSTEM_ADAPTER_PHP,
        ),
        (
            "vendor/illuminate/Filesystem/FilesystemManager.php",
            FILESYSTEM_MANAGER_PHP,
        ),
        (
            "vendor/illuminate/Support/Facades/Storage.php",
            STORAGE_FACADE_PHP,
        ),
    ]
}

async fn complete_labels(
    files: &[(&str, &str)],
    open_path: &str,
    content: &str,
    line: u32,
    character: u32,
) -> Vec<String> {
    complete_labels_with_provider(files, None, open_path, content, line, character).await
}

/// Like [`complete_labels`] but opens a service provider first, which is what
/// registers its `Storage::extend()` drivers in the index.
async fn complete_labels_with_provider(
    files: &[(&str, &str)],
    provider: Option<(&str, &str)>,
    open_path: &str,
    content: &str,
    line: u32,
    character: u32,
) -> Vec<String> {
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, files);
    if let Some((path, text)) = provider {
        open_php_at(&backend, &dir, path, text).await;
    }
    let uri = open_php_at(&backend, &dir, open_path, content).await;
    complete_labels_at_opened(&backend, &uri, line, character).await
}

const CONTROLLER_PHP: &str = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Storage;
class C {
    public function show() {
        Storage::disk('s3')->
    }
}
";

/// When every configured disk uses a driver the framework ships,
/// `Storage::disk()` resolves to the concrete `FilesystemAdapter`, so
/// adapter-only members like `assertExists()` complete.
#[tokio::test]
async fn disk_with_only_builtin_drivers_resolves_to_adapter() {
    let mut files = base_files();
    files.push((
        "config/filesystems.php",
        "<?php return [
            'disks' => [
                'local' => ['driver' => 'local'],
                's3' => ['driver' => 's3'],
            ],
        ];",
    ));

    let labels = complete_labels(&files, "src/C.php", CONTROLLER_PHP, 5, 29).await;
    assert!(
        labels.iter().any(|l| l.starts_with("assertExists")),
        "expected FilesystemAdapter::assertExists in completions, got: {labels:?}"
    );
}

/// When a disk is built by a driver the framework does not ship and no
/// `Storage::extend()` registration for it can be found, the correction does
/// not fire and `disk()` keeps returning the bare contract.
#[tokio::test]
async fn disk_with_unregistered_custom_driver_keeps_contract() {
    let mut files = base_files();
    files.push((
        "config/filesystems.php",
        "<?php return [
            'disks' => [
                'local' => ['driver' => 'local'],
                'dropbox' => ['driver' => 'dropbox'],
            ],
        ];",
    ));

    let labels = complete_labels(&files, "src/C.php", CONTROLLER_PHP, 5, 29).await;
    assert!(
        !labels.iter().any(|l| l.starts_with("assertExists")),
        "a custom driver disk must not be widened to FilesystemAdapter, got: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("read")),
        "the declared Filesystem contract's own members should still complete, got: {labels:?}"
    );
}

const EXTEND_ADAPTER_PROVIDER_PHP: &str = "\
<?php
namespace App\\Providers;
use Illuminate\\Filesystem\\FilesystemAdapter;
use Illuminate\\Support\\Facades\\Storage;
class AppServiceProvider {
    public function boot(): void {
        Storage::extend('dropbox', function ($app, $config) {
            return new FilesystemAdapter();
        });
    }
}
";

const MEMORY_DISK_PHP: &str = "\
<?php
namespace App\\Filesystem;
class MemoryDisk {
    public function read(string $path) { return null; }
    public function flushMemory(): void {}
}
";

const EXTEND_MEMORY_PROVIDER_PHP: &str = "\
<?php
namespace App\\Providers;
use App\\Filesystem\\MemoryDisk;
use Illuminate\\Support\\Facades\\Storage;
class AppServiceProvider {
    public function boot(): void {
        Storage::extend('memory', function ($app, $config) {
            return new MemoryDisk();
        });
    }
}
";

/// The documented `Storage::extend()` shape returns an unannotated
/// `FilesystemAdapter`, so reading the closure body folds the custom driver
/// into the same concrete type the built-in disks resolve to.
#[tokio::test]
async fn disk_with_extend_returning_adapter_resolves_to_adapter() {
    let mut files = base_files();
    files.push((
        "config/filesystems.php",
        "<?php return [
            'disks' => [
                'local' => ['driver' => 'local'],
                'dropbox' => ['driver' => 'dropbox'],
            ],
        ];",
    ));
    files.push((
        "src/Providers/AppServiceProvider.php",
        EXTEND_ADAPTER_PROVIDER_PHP,
    ));

    let labels = complete_labels_with_provider(
        &files,
        Some((
            "src/Providers/AppServiceProvider.php",
            EXTEND_ADAPTER_PROVIDER_PHP,
        )),
        "src/C.php",
        CONTROLLER_PHP,
        5,
        29,
    )
    .await;
    assert!(
        labels.iter().any(|l| l.starts_with("assertExists")),
        "a custom driver that builds a FilesystemAdapter should not hold the \
         built-in disks back, got: {labels:?}"
    );
}

/// A custom driver that builds something else widens the disk type to a union
/// of both, rather than dropping the correction for every disk in the project.
#[tokio::test]
async fn disk_with_extend_returning_other_type_offers_both() {
    let mut files = base_files();
    files.push((
        "config/filesystems.php",
        "<?php return [
            'disks' => [
                'local' => ['driver' => 'local'],
                'memory' => ['driver' => 'memory'],
            ],
        ];",
    ));
    files.push(("src/Filesystem/MemoryDisk.php", MEMORY_DISK_PHP));
    files.push((
        "src/Providers/AppServiceProvider.php",
        EXTEND_MEMORY_PROVIDER_PHP,
    ));

    let labels = complete_labels_with_provider(
        &files,
        Some((
            "src/Providers/AppServiceProvider.php",
            EXTEND_MEMORY_PROVIDER_PHP,
        )),
        "src/C.php",
        CONTROLLER_PHP,
        5,
        29,
    )
    .await;
    assert!(
        labels.iter().any(|l| l.starts_with("assertExists")),
        "the built-in disks' adapter members should complete, got: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("flushMemory")),
        "the custom driver's own members should complete, got: {labels:?}"
    );
}

/// `cloud()` declares the separate `Cloud` contract, which is also
/// corrected to the concrete adapter (which itself implements `Cloud`).
#[tokio::test]
async fn cloud_resolves_to_adapter() {
    let mut files = base_files();
    files.push((
        "config/filesystems.php",
        "<?php return [
            'disks' => [
                'local' => ['driver' => 'local'],
                's3' => ['driver' => 's3'],
            ],
        ];",
    ));

    let controller = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Storage;
class C {
    public function show() {
        Storage::cloud()->
    }
}
";
    let labels = complete_labels(&files, "src/C.php", controller, 5, 26).await;
    assert!(
        labels.iter().any(|l| l.starts_with("assertExists")),
        "expected FilesystemAdapter::assertExists in completions, got: {labels:?}"
    );
}
