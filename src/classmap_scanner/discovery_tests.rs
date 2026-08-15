use super::*;

// ── scan_directories integration tests ──────────────────────────

#[test]
fn scan_directories_finds_classes() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("User.php"),
        "<?php\nnamespace App\\Models;\nclass User {}",
    )
    .unwrap();
    std::fs::write(
        src.join("Order.php"),
        "<?php\nnamespace App\\Models;\nclass Order {}",
    )
    .unwrap();

    let vendor_dir_paths = vec![dir.path().join("vendor")];
    let classmap = scan_directories(&[src], &vendor_dir_paths);
    assert_eq!(classmap.len(), 2);
    assert!(classmap.contains_key("App\\Models\\User"));
    assert!(classmap.contains_key("App\\Models\\Order"));
}

#[test]
fn scan_directories_skips_hidden() {
    let dir = tempfile::tempdir().unwrap();
    let hidden = dir.path().join(".hidden");
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::write(hidden.join("Secret.php"), "<?php\nclass Secret {}").unwrap();

    let classmap = scan_directories(&[dir.path().to_path_buf()], &[]);
    assert!(!classmap.contains_key("Secret"));
}

#[test]
fn scan_directories_skips_vendor() {
    let dir = tempfile::tempdir().unwrap();
    let vendor = dir.path().join("vendor");
    std::fs::create_dir_all(&vendor).unwrap();
    std::fs::write(vendor.join("Lib.php"), "<?php\nclass Lib {}").unwrap();

    let vendor_dir_paths = vec![vendor];
    let classmap = scan_directories(&[dir.path().to_path_buf()], &vendor_dir_paths);
    assert!(!classmap.contains_key("Lib"));
}

#[test]
fn psr4_filtering() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    let models = src.join("Models");
    std::fs::create_dir_all(&models).unwrap();

    // Compliant: App\Models\User in src/Models/User.php
    std::fs::write(
        models.join("User.php"),
        "<?php\nnamespace App\\Models;\nclass User {}",
    )
    .unwrap();

    // Non-compliant: class name doesn't match file path
    std::fs::write(
        models.join("Misplaced.php"),
        "<?php\nnamespace App\\Wrong;\nclass Misplaced {}",
    )
    .unwrap();

    let classmap = scan_psr4_directories(&[("App\\".to_string(), src)], &[], &[]);
    assert!(classmap.contains_key("App\\Models\\User"));
    assert!(!classmap.contains_key("App\\Wrong\\Misplaced"));
}

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

#[test]
fn scan_workspace_fallback_finds_all() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("lib");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("Foo.php"), "<?php\nclass Foo {}").unwrap();
    std::fs::write(dir.path().join("Bar.php"), "<?php\nclass Bar {}").unwrap();

    let vendor_dir_paths = vec![dir.path().join("vendor")];
    let classmap = scan_workspace_fallback(dir.path(), &vendor_dir_paths);
    assert!(classmap.contains_key("Foo"));
    assert!(classmap.contains_key("Bar"));
}

// ── scan_workspace_fallback_full tests ───────────────────────────

#[test]
fn scan_workspace_fallback_full_finds_all_symbol_types() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("helpers.php"),
        "<?php\nfunction myHelper(): void {}\ndefine('MY_CONST', 1);\nconst DEBUG = true;",
    )
    .unwrap();
    std::fs::write(dir.path().join("Model.php"), "<?php\nclass User {}").unwrap();

    let skip = std::collections::HashSet::new();
    let result = scan_workspace_fallback_full(dir.path(), &skip, None);
    assert!(result.classmap.contains_key("User"));
    assert!(
        result.function_index.contains_key("myHelper"),
        "should find function: {:?}",
        result.function_index
    );
    assert!(
        result.constant_index.contains_key("MY_CONST"),
        "should find define constant: {:?}",
        result.constant_index
    );
    assert!(
        result.constant_index.contains_key("DEBUG"),
        "should find top-level const: {:?}",
        result.constant_index
    );
}

#[test]
fn scan_workspace_fallback_full_skips_vendor() {
    let dir = tempfile::tempdir().unwrap();
    let vendor = dir.path().join("vendor");
    std::fs::create_dir_all(&vendor).unwrap();
    std::fs::write(
        vendor.join("lib.php"),
        "<?php\nfunction vendorFunc(): void {}",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("app.php"),
        "<?php\nfunction appFunc(): void {}",
    )
    .unwrap();

    let mut skip = std::collections::HashSet::new();
    skip.insert(vendor.clone());
    let result = scan_workspace_fallback_full(dir.path(), &skip, None);
    assert!(result.function_index.contains_key("appFunc"));
    assert!(
        !result.function_index.contains_key("vendorFunc"),
        "vendor functions should be excluded"
    );
}

#[test]
fn scan_workspace_fallback_full_skips_hidden_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let hidden = dir.path().join(".hidden");
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::write(
        hidden.join("secret.php"),
        "<?php\nfunction secretFunc(): void {}",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("public.php"),
        "<?php\nfunction publicFunc(): void {}",
    )
    .unwrap();

    let skip = std::collections::HashSet::new();
    let result = scan_workspace_fallback_full(dir.path(), &skip, None);
    assert!(result.function_index.contains_key("publicFunc"));
    assert!(
        !result.function_index.contains_key("secretFunc"),
        "hidden dir functions should be excluded"
    );
}

// ── scan_include_paths ──────────────────────────────────────────

/// A hidden directory holding first-party PHP (Castor's `.castor/`) is
/// invisible to the workspace walk, which is the case `include` exists
/// for.
#[test]
fn scan_include_paths_walks_hidden_directory() {
    let dir = tempfile::tempdir().unwrap();
    let hidden = dir.path().join(".castor");
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::write(
        hidden.join("docker.php"),
        "<?php\nnamespace docker;\nfunction up(): void {}\n",
    )
    .unwrap();

    let skip = std::collections::HashSet::new();
    assert!(
        !scan_workspace_fallback_full(dir.path(), &skip, None)
            .function_index
            .contains_key("docker\\up"),
        "precondition: the workspace walk must not see it"
    );

    let result = scan_include_paths(&[hidden], None);
    assert!(
        result.function_index.contains_key("docker\\up"),
        "got: {:?}",
        result.function_index.keys().collect::<Vec<_>>()
    );
}

/// A generated stub is hidden *and* gitignored; naming it directly has
/// to defeat both filters.
#[test]
fn scan_include_paths_reads_hidden_gitignored_file() {
    let dir = tempfile::tempdir().unwrap();
    // `ignore` only honours `.gitignore` inside a git repository.
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();
    std::fs::write(dir.path().join(".gitignore"), "/.castor.stub.php\n").unwrap();
    let stub = dir.path().join(".castor.stub.php");
    std::fs::write(
        &stub,
        "<?php\nnamespace Castor {\n    function run(string $command): void {}\n    function io(): void {}\n}\n",
    )
    .unwrap();

    let skip = std::collections::HashSet::new();
    assert!(
        !scan_workspace_fallback_full(dir.path(), &skip, None)
            .function_index
            .contains_key("Castor\\run"),
        "precondition: the workspace walk must not see it"
    );

    let result = scan_include_paths(&[stub], None);
    assert!(result.function_index.contains_key("Castor\\run"));
    assert!(result.function_index.contains_key("Castor\\io"));
}

#[test]
fn scan_include_paths_reports_classes_and_constants() {
    let dir = tempfile::tempdir().unwrap();
    let hidden = dir.path().join(".tools");
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::write(
        hidden.join("helpers.php"),
        "<?php\nclass Helper {}\nconst TOOL_VERSION = 1;\n",
    )
    .unwrap();

    let result = scan_include_paths(&[hidden], None);
    assert!(result.classmap.contains_key("Helper"));
    assert!(result.constant_index.contains_key("TOOL_VERSION"));
}

/// Non-PHP files in an included directory must be left alone.
#[test]
fn scan_include_paths_ignores_non_php_files() {
    let dir = tempfile::tempdir().unwrap();
    let included = dir.path().join(".castor");
    std::fs::create_dir_all(&included).unwrap();
    std::fs::write(included.join("notes.md"), "function notAFunction() {}").unwrap();
    std::fs::write(
        included.join("tasks.php"),
        "<?php\nfunction realFunction(): void {}\n",
    )
    .unwrap();

    let result = scan_include_paths(&[included], None);
    assert!(result.function_index.contains_key("realFunction"));
    assert!(!result.function_index.contains_key("notAFunction"));
}

#[test]
fn scan_include_paths_with_no_paths_is_empty() {
    let result = scan_include_paths(&[], None);
    assert!(result.classmap.is_empty());
    assert!(result.function_index.is_empty());
    assert!(result.constant_index.is_empty());
}

// ── is_drupal_php_file ──────────────────────────────────────────

#[test]
fn drupal_php_file_accepts_php() {
    assert!(is_drupal_php_file(Path::new("module.php")));
}

#[test]
fn drupal_php_file_accepts_module() {
    assert!(is_drupal_php_file(Path::new("mymodule.module")));
}

#[test]
fn drupal_php_file_accepts_install() {
    assert!(is_drupal_php_file(Path::new("mymodule.install")));
}

#[test]
fn drupal_php_file_accepts_theme() {
    assert!(is_drupal_php_file(Path::new("mytheme.theme")));
}

#[test]
fn drupal_php_file_accepts_profile() {
    assert!(is_drupal_php_file(Path::new("myprofile.profile")));
}

#[test]
fn drupal_php_file_accepts_inc() {
    assert!(is_drupal_php_file(Path::new("helpers.inc")));
}

#[test]
fn drupal_php_file_accepts_engine() {
    assert!(is_drupal_php_file(Path::new("phptemplate.engine")));
}

#[test]
fn drupal_php_file_rejects_txt() {
    assert!(!is_drupal_php_file(Path::new("README.txt")));
}

#[test]
fn drupal_php_file_rejects_yml() {
    assert!(!is_drupal_php_file(Path::new("mymodule.info.yml")));
}

#[test]
fn drupal_php_file_rejects_no_extension() {
    assert!(!is_drupal_php_file(Path::new("Makefile")));
}

// ── scan_drupal_directories ─────────────────────────────────────

#[test]
fn scan_drupal_directories_finds_php_and_module_files() {
    let dir = tempfile::tempdir().unwrap();
    let web_root = dir.path();

    // core/lib/Drupal/Core/Entity
    let entity_dir = web_root.join("core/lib/Drupal/Core/Entity");
    std::fs::create_dir_all(&entity_dir).unwrap();
    std::fs::write(
        entity_dir.join("EntityInterface.php"),
        "<?php\nnamespace Drupal\\Core\\Entity;\ninterface EntityInterface {}",
    )
    .unwrap();

    // modules/contrib/token
    let token_dir = web_root.join("modules/contrib/token/src");
    std::fs::create_dir_all(&token_dir).unwrap();
    std::fs::write(
        token_dir.join("TokenService.php"),
        "<?php\nnamespace Drupal\\token;\nclass TokenService {}",
    )
    .unwrap();

    // A .module file in modules/custom
    let custom_dir = web_root.join("modules/custom/mymod");
    std::fs::create_dir_all(&custom_dir).unwrap();
    std::fs::write(
        custom_dir.join("mymod.module"),
        "<?php\nfunction mymod_help() {}",
    )
    .unwrap();

    let result = scan_drupal_directories(web_root, None);
    assert!(
        result
            .classmap
            .contains_key("Drupal\\Core\\Entity\\EntityInterface"),
        "should index core PHP files; keys: {:?}",
        result.classmap.keys().collect::<Vec<_>>()
    );
    assert!(
        result.classmap.contains_key("Drupal\\token\\TokenService"),
        "should index contrib module PHP files; keys: {:?}",
        result.classmap.keys().collect::<Vec<_>>()
    );
    assert!(
        result.function_index.contains_key("mymod_help"),
        "should index .module files; functions: {:?}",
        result.function_index.keys().collect::<Vec<_>>()
    );
}

#[test]
fn scan_drupal_directories_skips_test_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let web_root = dir.path();

    let test_dir = web_root.join("modules/contrib/token/tests/src");
    std::fs::create_dir_all(&test_dir).unwrap();
    std::fs::write(
        test_dir.join("TokenTest.php"),
        "<?php\nnamespace Drupal\\Tests\\token;\nclass TokenTest {}",
    )
    .unwrap();

    // Also test the "Tests" casing
    let test_dir2 = web_root.join("core/Tests");
    std::fs::create_dir_all(&test_dir2).unwrap();
    std::fs::write(
        test_dir2.join("CoreTest.php"),
        "<?php\nnamespace Drupal\\Tests;\nclass CoreTest {}",
    )
    .unwrap();

    let result = scan_drupal_directories(web_root, None);
    assert!(
        !result
            .classmap
            .contains_key("Drupal\\Tests\\token\\TokenTest"),
        "should skip tests/ directories"
    );
    assert!(
        !result.classmap.contains_key("Drupal\\Tests\\CoreTest"),
        "should skip Tests/ directories"
    );
}

#[test]
fn scan_drupal_directories_skips_nonexistent_dirs() {
    let dir = tempfile::tempdir().unwrap();
    // Empty web root — none of the expected subdirectories exist
    let result = scan_drupal_directories(dir.path(), None);
    assert!(result.classmap.is_empty());
    assert!(result.function_index.is_empty());
    assert!(result.constant_index.is_empty());
}

#[test]
fn scan_drupal_directories_ignores_non_php_files() {
    let dir = tempfile::tempdir().unwrap();
    let web_root = dir.path();

    let core_dir = web_root.join("core");
    std::fs::create_dir_all(&core_dir).unwrap();
    std::fs::write(core_dir.join("core.services.yml"), "services: {}").unwrap();
    std::fs::write(core_dir.join("README.txt"), "Drupal core").unwrap();
    std::fs::write(
        core_dir.join("install.php"),
        "<?php\nfunction install_begin() {}",
    )
    .unwrap();

    let result = scan_drupal_directories(web_root, None);
    // Only the .php file should be indexed
    assert!(
        result.function_index.contains_key("install_begin"),
        "should index .php files"
    );
    assert_eq!(
        result.classmap.len() + result.function_index.len() + result.constant_index.len(),
        1,
        "should not index .yml or .txt files"
    );
}

#[test]
fn psr4_prefixes_sharing_a_directory_both_resolve() {
    // A package can point two namespace prefixes at the same directory
    // (Laravel does something close to this with `Illuminate\Support`).
    // The parallel walk visits such a directory once, so the file has to
    // be handed to both mappings or one prefix loses its classes.
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("Thing.php"),
        "<?php\nnamespace One;\nclass Thing {}",
    )
    .unwrap();
    std::fs::write(
        src.join("Other.php"),
        "<?php\nnamespace Two;\nclass Other {}",
    )
    .unwrap();

    let classmap = scan_psr4_directories(
        &[
            ("One\\".to_string(), src.clone()),
            ("Two\\".to_string(), src),
        ],
        &[],
        &[],
    );
    assert!(classmap.contains_key("One\\Thing"));
    assert!(classmap.contains_key("Two\\Other"));
}

#[test]
fn psr4_nested_mapping_does_not_shadow_its_parent() {
    // Laravel maps both `src/Illuminate` and `src/Illuminate/Collections`,
    // so the nested directory is reached by two walks.  Each mapping must
    // still see the files below it under its own namespace prefix.
    let dir = tempfile::tempdir().unwrap();
    let outer = dir.path().join("src");
    let inner = outer.join("Nested");
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::write(
        inner.join("Item.php"),
        "<?php\nnamespace Outer\\Nested;\nclass Item {}",
    )
    .unwrap();
    std::fs::write(
        inner.join("Other.php"),
        "<?php\nnamespace Inner;\nclass Other {}",
    )
    .unwrap();

    let classmap = scan_psr4_directories(
        &[
            ("Outer\\".to_string(), outer),
            ("Inner\\".to_string(), inner),
        ],
        &[],
        &[],
    );
    assert!(classmap.contains_key("Outer\\Nested\\Item"));
    assert!(classmap.contains_key("Inner\\Other"));
}

#[test]
fn scan_directories_follows_a_symlinked_root() {
    // A monorepo or path repository can expose a source directory through
    // a symlink; the walk has to descend into the root it was given even
    // though it does not follow symlinks found inside the tree.
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    std::fs::create_dir_all(&real).unwrap();
    std::fs::write(real.join("Linked.php"), "<?php\nclass Linked {}").unwrap();

    let link = dir.path().join("link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&real, &link).unwrap();

    let classmap = scan_directories(&[link], &[]);
    assert!(classmap.contains_key("Linked"));
}
