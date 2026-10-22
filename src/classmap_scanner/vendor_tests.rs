use super::*;

#[test]
fn scan_vendor_packages_installed_json_v2() {
    let dir = tempfile::tempdir().unwrap();
    let vendor = dir.path().join("vendor");
    let composer_dir = vendor.join("composer");
    std::fs::create_dir_all(&composer_dir).unwrap();

    // Create a fake package
    let pkg_src = vendor.join("acme").join("logger").join("src");
    std::fs::create_dir_all(&pkg_src).unwrap();
    std::fs::write(
        pkg_src.join("Logger.php"),
        "<?php\nnamespace Acme\\Logger;\nclass Logger {}",
    )
    .unwrap();

    // Composer 2 format installed.json with install-path
    let installed = serde_json::json!({
        "packages": [
            {
                "name": "acme/logger",
                "install-path": "../acme/logger",
                "autoload": {
                    "psr-4": {
                        "Acme\\Logger\\": "src/"
                    }
                }
            }
        ]
    });
    std::fs::write(
        composer_dir.join("installed.json"),
        serde_json::to_string(&installed).unwrap(),
    )
    .unwrap();

    let result = scan_vendor_packages(dir.path(), "vendor");
    let classmap = result.classmap;
    assert!(
        classmap.contains_key("Acme\\Logger\\Logger"),
        "classmap keys: {:?}",
        classmap.keys().collect::<Vec<_>>()
    );
}

#[test]
fn scan_vendor_packages_install_path_non_standard_location() {
    // Packages installed via path repositories or custom installers
    // may not live under vendor/<name>/.  The install-path field
    // (relative to vendor/composer/) is the authoritative location.
    let dir = tempfile::tempdir().unwrap();
    let vendor = dir.path().join("vendor");
    let composer_dir = vendor.join("composer");
    std::fs::create_dir_all(&composer_dir).unwrap();

    // Package lives in a non-standard location outside the vendor dir
    let custom_location = dir.path().join("packages").join("my-lib").join("src");
    std::fs::create_dir_all(&custom_location).unwrap();
    std::fs::write(
        custom_location.join("Widget.php"),
        "<?php\nnamespace My\\Lib;\nclass Widget {}",
    )
    .unwrap();

    // install-path is relative to vendor/composer/
    let installed = serde_json::json!({
        "packages": [
            {
                "name": "my/lib",
                "install-path": "../../packages/my-lib",
                "autoload": {
                    "psr-4": {
                        "My\\Lib\\": "src/"
                    }
                }
            }
        ]
    });
    std::fs::write(
        composer_dir.join("installed.json"),
        serde_json::to_string(&installed).unwrap(),
    )
    .unwrap();

    let result = scan_vendor_packages(dir.path(), "vendor");
    let classmap = result.classmap;
    assert!(
        classmap.contains_key("My\\Lib\\Widget"),
        "install-path should resolve non-standard locations; keys: {:?}",
        classmap.keys().collect::<Vec<_>>()
    );
}

#[test]
fn scan_vendor_packages_falls_back_to_name_without_install_path() {
    // Composer 1 format: no install-path field, falls back to
    // vendor/<name>/.
    let dir = tempfile::tempdir().unwrap();
    let vendor = dir.path().join("vendor");
    let composer_dir = vendor.join("composer");
    std::fs::create_dir_all(&composer_dir).unwrap();

    let pkg_src = vendor.join("old").join("pkg").join("src");
    std::fs::create_dir_all(&pkg_src).unwrap();
    std::fs::write(
        pkg_src.join("Legacy.php"),
        "<?php\nnamespace Old\\Pkg;\nclass Legacy {}",
    )
    .unwrap();

    // No install-path — Composer 1 style
    let installed = serde_json::json!([
        {
            "name": "old/pkg",
            "autoload": {
                "psr-4": {
                    "Old\\Pkg\\": "src/"
                }
            }
        }
    ]);
    std::fs::write(
        composer_dir.join("installed.json"),
        serde_json::to_string(&installed).unwrap(),
    )
    .unwrap();

    let result = scan_vendor_packages(dir.path(), "vendor");
    let classmap = result.classmap;
    assert!(
        classmap.contains_key("Old\\Pkg\\Legacy"),
        "should fall back to vendor/<name> when install-path is absent; keys: {:?}",
        classmap.keys().collect::<Vec<_>>()
    );
}

#[test]
fn scan_vendor_packages_classmap_entry() {
    let dir = tempfile::tempdir().unwrap();
    let vendor = dir.path().join("vendor");
    let composer_dir = vendor.join("composer");
    std::fs::create_dir_all(&composer_dir).unwrap();

    // Create a fake package with classmap autoloading
    let pkg_lib = vendor.join("acme").join("utils").join("lib");
    std::fs::create_dir_all(&pkg_lib).unwrap();
    std::fs::write(pkg_lib.join("Helper.php"), "<?php\nclass Helper {}").unwrap();

    let installed = serde_json::json!({
        "packages": [
            {
                "name": "acme/utils",
                "install-path": "../acme/utils",
                "autoload": {
                    "classmap": ["lib/"]
                }
            }
        ]
    });
    std::fs::write(
        composer_dir.join("installed.json"),
        serde_json::to_string(&installed).unwrap(),
    )
    .unwrap();

    let result = scan_vendor_packages(dir.path(), "vendor");
    assert!(result.classmap.contains_key("Helper"));
}

#[test]
fn scan_vendor_packages_custom_autoloader_full_scans_package() {
    // Mirrors Rector: the package's only autoload entry is a `files`
    // bootstrap that registers its own `spl_autoload_register`
    // callback. No PSR-4 or classmap entry covers the real classes,
    // which live in `src/` and `rules/` under the `Rector\`
    // namespace. Because we cannot execute the runtime autoloader,
    // the scanner must full-scan the package directory to discover
    // them.
    let dir = tempfile::tempdir().unwrap();
    let vendor = dir.path().join("vendor");
    let composer_dir = vendor.join("composer");
    std::fs::create_dir_all(&composer_dir).unwrap();

    let pkg = vendor.join("rector").join("rector");
    std::fs::create_dir_all(pkg.join("src").join("Config")).unwrap();
    std::fs::create_dir_all(pkg.join("rules").join("CodingStyle")).unwrap();
    std::fs::write(
        pkg.join("bootstrap.php"),
        "<?php\nspl_autoload_register(function (string $class): void {});",
    )
    .unwrap();
    std::fs::write(
        pkg.join("src").join("Config").join("RectorConfig.php"),
        "<?php\nnamespace Rector\\Config;\nclass RectorConfig {}",
    )
    .unwrap();
    std::fs::write(
        pkg.join("rules").join("CodingStyle").join("SomeRector.php"),
        "<?php\nnamespace Rector\\CodingStyle;\nclass SomeRector {}",
    )
    .unwrap();

    let installed = serde_json::json!({
        "packages": [
            {
                "name": "rector/rector",
                "install-path": "../rector/rector",
                "autoload": {
                    "files": ["bootstrap.php"]
                }
            }
        ]
    });
    std::fs::write(
        composer_dir.join("installed.json"),
        serde_json::to_string(&installed).unwrap(),
    )
    .unwrap();

    let result = scan_vendor_packages(dir.path(), "vendor");
    assert!(
        result.classmap.contains_key("Rector\\Config\\RectorConfig"),
        "classes under src/ must be discovered via the full-scan fallback"
    );
    assert!(
        result
            .classmap
            .contains_key("Rector\\CodingStyle\\SomeRector"),
        "classes under rules/ must be discovered via the full-scan fallback"
    );
}

#[test]
fn scan_vendor_packages_files_autoload_without_autoloader_is_not_full_scanned() {
    // A plain `files` autoload (no spl_autoload_register) must NOT
    // trigger a full package scan — only the listed file is indexed.
    // This guards against regressing the custom-autoloader heuristic
    // into an unconditional full scan of every `files` package.
    let dir = tempfile::tempdir().unwrap();
    let vendor = dir.path().join("vendor");
    let composer_dir = vendor.join("composer");
    std::fs::create_dir_all(&composer_dir).unwrap();

    let pkg = vendor.join("acme").join("helpers");
    std::fs::create_dir_all(pkg.join("src")).unwrap();
    std::fs::write(
        pkg.join("functions.php"),
        "<?php\nfunction acme_helper(): void {}",
    )
    .unwrap();
    // A class that is only reachable via a real PSR-4 autoloader —
    // there is none declared, so it must stay undiscovered.
    std::fs::write(
        pkg.join("src").join("Internal.php"),
        "<?php\nnamespace Acme\\Helpers;\nclass Internal {}",
    )
    .unwrap();

    let installed = serde_json::json!({
        "packages": [
            {
                "name": "acme/helpers",
                "install-path": "../acme/helpers",
                "autoload": {
                    "files": ["functions.php"]
                }
            }
        ]
    });
    std::fs::write(
        composer_dir.join("installed.json"),
        serde_json::to_string(&installed).unwrap(),
    )
    .unwrap();

    let result = scan_vendor_packages(dir.path(), "vendor");
    assert!(
        !result.classmap.contains_key("Acme\\Helpers\\Internal"),
        "a plain files autoload must not trigger a full package scan"
    );
}
