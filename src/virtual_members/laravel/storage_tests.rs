use super::super::config_values::parse_config_tree;
use super::*;
use crate::types::MethodInfo;

fn tree(config: &str) -> ConfigNode {
    parse_config_tree(config).unwrap()
}

/// A driver index built from one provider file, as the startup scan builds it
/// (minus the closure-body inference, which needs a `Backend`).
fn drivers(provider: &str) -> LaravelStorageDriverIndex {
    let mut index = LaravelStorageDriverIndex::default();
    index.set_file(
        "file:///app/Providers/AppServiceProvider.php".to_string(),
        extract_storage_driver_registrations(provider),
    );
    index.rebuild();
    index
}

fn adapter() -> PhpType {
    PhpType::named(atom(FILESYSTEM_ADAPTER_FQN))
}

/// A fresh Laravel install's `local`/`public` disks both resolve to the
/// concrete adapter, so the union collapses to it.
#[test]
fn only_local_disks_resolve_to_the_adapter() {
    let tree = tree(
        r#"<?php return [
            'disks' => [
                'local' => ['driver' => 'local', 'root' => 'storage/app'],
                'public' => ['driver' => 'local', 'root' => 'storage/app/public'],
            ],
        ];"#,
    );
    assert_eq!(
        disk_union(&tree, &LaravelStorageDriverIndex::default()),
        Some(adapter())
    );
}

/// Every framework-shipped driver, including `scoped` (which delegates to
/// another disk's driver via `build()` rather than building anything itself),
/// resolves to the adapter.
#[test]
fn every_builtin_driver_resolves_to_the_adapter() {
    let tree = tree(
        r#"<?php return [
            'disks' => [
                'local' => ['driver' => 'local'],
                'ftp_disk' => ['driver' => 'ftp'],
                'sftp_disk' => ['driver' => 'sftp'],
                's3' => ['driver' => 's3'],
                'cdn' => ['driver' => 'scoped', 'disk' => 's3', 'prefix' => 'public'],
            ],
        ];"#,
    );
    assert_eq!(
        disk_union(&tree, &LaravelStorageDriverIndex::default()),
        Some(adapter())
    );
}

/// A disk built by a driver with no discoverable `Storage::extend()`
/// registration cannot be classified, so the whole correction is dropped.
#[test]
fn unregistered_custom_driver_is_unresolvable() {
    let tree = tree(
        r#"<?php return [
            'disks' => [
                'local' => ['driver' => 'local'],
                'dropbox' => ['driver' => 'dropbox'],
            ],
        ];"#,
    );
    assert_eq!(
        disk_union(&tree, &LaravelStorageDriverIndex::default()),
        None
    );
}

/// The documented `Storage::extend()` shape returns a `FilesystemAdapter`, so
/// a project with a custom driver alongside built-in ones still gets the
/// concrete adapter on every disk.
#[test]
fn registered_custom_driver_folds_into_the_adapter() {
    let tree = tree(
        r#"<?php return [
            'disks' => [
                'local' => ['driver' => 'local'],
                'dropbox' => ['driver' => 'dropbox'],
            ],
        ];"#,
    );
    let drivers = drivers(
        r#"<?php
        namespace App\Providers;

        use Illuminate\Filesystem\FilesystemAdapter;
        use Illuminate\Support\Facades\Storage;

        class AppServiceProvider {
            public function boot(): void {
                Storage::extend('dropbox', function ($app, $config): FilesystemAdapter {
                    return new FilesystemAdapter($app, $config);
                });
            }
        }"#,
    );
    assert_eq!(disk_union(&tree, &drivers), Some(adapter()));
}

/// A custom driver that builds something else widens the disk type to a union
/// rather than dropping the correction for the built-in disks alongside it.
#[test]
fn custom_driver_of_another_type_widens_to_a_union() {
    let tree = tree(
        r#"<?php return [
            'disks' => [
                'local' => ['driver' => 'local'],
                'memory' => ['driver' => 'memory'],
            ],
        ];"#,
    );
    let drivers = drivers(
        r#"<?php
        namespace App\Providers;

        use App\Filesystem\MemoryDisk;
        use Illuminate\Support\Facades\Storage;

        class AppServiceProvider {
            public function boot(): void {
                Storage::extend('memory', function ($app, $config): MemoryDisk {
                    return new MemoryDisk();
                });
            }
        }"#,
    );
    assert_eq!(
        disk_union(&tree, &drivers).map(|ty| ty.to_string()),
        Some("Illuminate\\Filesystem\\FilesystemAdapter|App\\Filesystem\\MemoryDisk".to_string())
    );
}

/// `FilesystemManager::resolve()` consults `$this->customCreators` before its
/// own `create*Driver()` methods, so a registration named after a built-in
/// driver replaces it.
#[test]
fn custom_driver_overrides_a_builtin_of_the_same_name() {
    let tree = tree(r#"<?php return ['disks' => ['local' => ['driver' => 'local']]];"#);
    let drivers = drivers(
        r#"<?php
        namespace App\Providers;

        use App\Filesystem\AuditedDisk;
        use Illuminate\Support\Facades\Storage;

        class AppServiceProvider {
            public function boot(): void {
                Storage::extend('local', function ($app, $config): AuditedDisk {
                    return new AuditedDisk();
                });
            }
        }"#,
    );
    assert_eq!(
        disk_union(&tree, &drivers).map(|ty| ty.to_string()),
        Some("App\\Filesystem\\AuditedDisk".to_string())
    );
}

/// An env-overridable driver could resolve to anything at runtime, so it is
/// treated the same as an unregistered custom driver.
#[test]
fn dynamic_driver_is_unresolvable() {
    let tree = tree(
        r#"<?php return [
            'disks' => [
                'local' => ['driver' => env('DISK_DRIVER', 'local')],
            ],
        ];"#,
    );
    assert_eq!(
        disk_union(&tree, &LaravelStorageDriverIndex::default()),
        None
    );
}

/// A disk with no readable `driver` key at all cannot be classified, so it
/// drops the correction rather than being silently ignored.
#[test]
fn missing_driver_key_is_unresolvable() {
    let tree = tree(
        r#"<?php return [
            'disks' => [
                'local' => ['root' => 'storage/app'],
            ],
        ];"#,
    );
    assert_eq!(
        disk_union(&tree, &LaravelStorageDriverIndex::default()),
        None
    );
}

/// A config with no `disks` array at all (the tree could not be read as a
/// real `filesystems.php`) is unresolvable, not vacuously the adapter.
#[test]
fn missing_disks_key_is_unresolvable() {
    let tree = tree(r#"<?php return ['default' => 'local'];"#);
    assert_eq!(
        disk_union(&tree, &LaravelStorageDriverIndex::default()),
        None
    );
}

/// An empty `disks` array is unresolvable for the same reason: a project that
/// genuinely configures zero disks is not something the correction should
/// have to reason about, and it is indistinguishable from an unreadable one.
#[test]
fn empty_disks_is_unresolvable() {
    let tree = tree(r#"<?php return ['disks' => []];"#);
    assert_eq!(
        disk_union(&tree, &LaravelStorageDriverIndex::default()),
        None
    );
}

/// The registration is recognized through the manager as well as the facade,
/// and the driver name is kept exactly as written.
#[test]
fn extend_is_recognized_through_the_manager() {
    let regs = extract_storage_driver_registrations(
        r#"<?php
        use Illuminate\Filesystem\FilesystemManager;

        FilesystemManager::extend('My-Driver', fn ($app, $config) => null);"#,
    );
    assert_eq!(regs.len(), 1);
    assert_eq!(regs[0].driver, "My-Driver");
}

/// `extend()` on an unrelated class is not a filesystem driver.
#[test]
fn extend_on_another_facade_is_ignored() {
    let regs = extract_storage_driver_registrations(
        r#"<?php
        use Illuminate\Support\Facades\Validator;

        Validator::extend('phone', function ($attribute, $value) { return true; });"#,
    );
    assert!(regs.is_empty());
}

/// A computed driver name cannot be matched against a disk's `driver` key, so
/// it contributes nothing rather than a guess.
#[test]
fn dynamic_driver_name_is_skipped() {
    let regs = extract_storage_driver_registrations(
        r#"<?php
        use Illuminate\Support\Facades\Storage;

        Storage::extend($name, fn ($app, $config) => null);"#,
    );
    assert!(regs.is_empty());
}

/// A registration whose closure builds a scalar says nothing useful about the
/// disk, so it is left out of the merged lookup and the disk stays
/// unclassified.
#[test]
fn non_class_return_type_is_not_indexed() {
    let mut index = LaravelStorageDriverIndex::default();
    index.set_file(
        "file:///app/Providers/AppServiceProvider.php".to_string(),
        vec![StorageDriverRegistration {
            driver: "memory".to_string(),
            return_type: Some(PhpType::string()),
            closure_text: None,
            closure_offset: 0,
        }],
    );
    index.rebuild();
    assert!(index.is_empty());
}

fn make_method_typed(name: &str, return_type: Option<PhpType>) -> MethodInfo {
    MethodInfo {
        return_type,
        ..MethodInfo::virtual_method(name, None)
    }
}

#[test]
fn filesystem_contract_return_becomes_adapter() {
    let adapter = PhpType::named(atom(FILESYSTEM_ADAPTER_FQN));
    let mut method = make_method_typed("disk", Some(PhpType::named(atom(FILESYSTEM_CONTRACT_FQN))));
    assert!(refine_disk_return_type(&mut method, &adapter));
    assert_eq!(method.return_type.unwrap().to_string(), adapter.to_string());
}

#[test]
fn cloud_contract_return_becomes_adapter() {
    let adapter = PhpType::named(atom(FILESYSTEM_ADAPTER_FQN));
    let mut method = make_method_typed("cloud", Some(PhpType::named(atom(CLOUD_CONTRACT_FQN))));
    assert!(refine_disk_return_type(&mut method, &adapter));
    assert_eq!(method.return_type.unwrap().to_string(), adapter.to_string());
}

#[test]
fn non_contract_return_is_left_untouched() {
    let adapter = PhpType::named(atom(FILESYSTEM_ADAPTER_FQN));
    let original = PhpType::named(atom("App\\Storage\\CustomAdapter"));
    let mut method = make_method_typed("disk", Some(original.clone()));
    assert!(!refine_disk_return_type(&mut method, &adapter));
    assert_eq!(
        method.return_type.unwrap().to_string(),
        original.to_string()
    );
}

// ─── Storage facade name resolution ─────────────────────────────────────────

fn resolves(content: &str, class_name: &str) -> bool {
    is_storage_facade_name(class_name, &storage_facade_local_names(content))
}

#[test]
fn the_facade_answers_to_its_fqn_its_imports_and_the_global_alias() {
    let namespaced = "<?php\nnamespace App;\n";
    assert!(resolves(
        namespaced,
        "Illuminate\\Support\\Facades\\Storage"
    ));
    assert!(resolves(
        namespaced,
        "\\Illuminate\\Support\\Facades\\Storage"
    ));
    // Laravel's global class alias is reachable from any namespace.
    assert!(resolves(namespaced, "\\Storage"));
    // …but the bare short name in a namespace is whatever that namespace holds.
    assert!(!resolves(namespaced, "Storage"));
    // A file with no namespace of its own sits in the global one.
    assert!(resolves("<?php\n", "Storage"));

    for import in [
        "use Illuminate\\Support\\Facades\\Storage;",
        "use Illuminate\\Support\\Facades\\{Storage};",
        "use Illuminate\\Support\\Facades\\{Cache, Storage};",
        "use Illuminate\\Support\\Facades\\{\n    Cache,\n    Storage,\n};",
    ] {
        let content = format!("<?php\nnamespace App;\n{import}\n");
        assert!(resolves(&content, "Storage"), "import: {import}");
    }

    for import in [
        "use Illuminate\\Support\\Facades\\Storage as Disks;",
        "use Illuminate\\Support\\Facades\\{Storage as Disks, Cache};",
    ] {
        let content = format!("<?php\nnamespace App;\n{import}\n");
        assert!(resolves(&content, "Disks"), "import: {import}");
        assert!(!resolves(&content, "Storage"), "import: {import}");
    }
}

#[test]
fn an_unrelated_or_malformed_storage_import_does_not_name_the_facade() {
    for import in [
        // Another vendor's class took the short name.
        "use Vendor\\Storage;",
        "use Vendor\\Package\\{Storage};",
        // None of these is an import PHP would accept.
        "use Illuminate\\Support\\Facades\\Storage nope;",
        "use Illuminate\\Support\\Facades\\Storage as;",
        "use Illuminate\\Support\\Facades\\Storage as Disks trailing;",
        // An unclosed group import names nothing.
        "use Illuminate\\Support\\Facades\\{Storage;",
        // A `use function` item lives in another symbol table.
        "use function Illuminate\\Support\\Facades\\Storage;",
    ] {
        let content = format!("<?php\nnamespace App;\n{import}\n");
        let content = content.as_str();
        assert!(!resolves(content, "Storage"), "import: {import}");
        assert!(!resolves(content, "Disks"), "import: {import}");
    }

    // A class of the same name elsewhere is not the facade.
    assert!(!resolves("<?php\n", "Vendor\\Storage"));
    assert!(!resolves("<?php\n", "\\Acme\\Storage"));
}
