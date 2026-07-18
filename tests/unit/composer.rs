use phpantom_lsp::composer::{
    extract_path_repo_psr4_mappings, extract_require_once_paths, normalise_path,
    parse_autoload_classmap, parse_autoload_files, parse_composer_json, resolve_class_path,
};
use std::fs;
use std::path::Path;

/// Helper: create a temporary workspace with a composer.json and
/// optional PHP class files.
struct TestWorkspace {
    dir: tempfile::TempDir,
}

impl TestWorkspace {
    fn new(composer_json: &str) -> Self {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        fs::write(dir.path().join("composer.json"), composer_json)
            .expect("failed to write composer.json");
        TestWorkspace { dir }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    /// Create a PHP file at the given relative path with minimal content.
    fn create_php_file(&self, relative_path: &str, content: &str) {
        let full_path = self.dir.path().join(relative_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).expect("failed to create dirs");
        }
        fs::write(&full_path, content).expect("failed to write PHP file");
    }
}

#[test]
fn test_parse_basic_psr4() {
    let ws = TestWorkspace::new(
        r#"{
            "autoload": {
                "psr-4": {
                    "Klarna\\": "src/Klarna/"
                }
            }
        }"#,
    );

    let (mappings, _vendor_dir) = parse_composer_json(ws.root());
    assert_eq!(mappings.len(), 1);
    assert_eq!(mappings[0].prefix, "Klarna\\");
    assert_eq!(mappings[0].base_path, "src/Klarna/");
}

#[test]
fn test_parse_autoload_dev() {
    let ws = TestWorkspace::new(
        r#"{
            "autoload": {
                "psr-4": {
                    "Klarna\\": "src/Klarna/"
                }
            },
            "autoload-dev": {
                "psr-4": {
                    "Klarna\\Rest\\Tests\\": "tests/"
                }
            }
        }"#,
    );

    let (mappings, _vendor_dir) = parse_composer_json(ws.root());
    assert_eq!(mappings.len(), 2);

    // Longest prefix first
    assert_eq!(mappings[0].prefix, "Klarna\\Rest\\Tests\\");
    assert_eq!(mappings[0].base_path, "tests/");
    assert_eq!(mappings[1].prefix, "Klarna\\");
    assert_eq!(mappings[1].base_path, "src/Klarna/");
}

#[test]
fn test_parse_array_paths() {
    let ws = TestWorkspace::new(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": ["src/", "lib/"]
                }
            }
        }"#,
    );

    let (mappings, _vendor_dir) = parse_composer_json(ws.root());
    assert_eq!(mappings.len(), 2);
    assert_eq!(mappings[0].prefix, "App\\");
    assert_eq!(mappings[0].base_path, "src/");
    assert_eq!(mappings[1].prefix, "App\\");
    assert_eq!(mappings[1].base_path, "lib/");
}

#[test]
fn test_parse_no_composer_json() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let (mappings, _vendor_dir) = parse_composer_json(dir.path());
    assert!(mappings.is_empty());
}

#[test]
fn test_parse_invalid_json() {
    let ws = TestWorkspace::new("not valid json {{{");
    let (mappings, _vendor_dir) = parse_composer_json(ws.root());
    assert!(mappings.is_empty());
}

#[test]
fn test_parse_no_psr4_section() {
    let ws = TestWorkspace::new(
        r#"{
            "name": "vendor/project",
            "autoload": {
                "classmap": ["src/"]
            }
        }"#,
    );

    let (mappings, _vendor_dir) = parse_composer_json(ws.root());
    assert!(mappings.is_empty());
}

#[test]
fn test_resolve_simple_class() {
    let ws = TestWorkspace::new(
        r#"{
            "autoload": {
                "psr-4": {
                    "Klarna\\": "src/Klarna/"
                }
            }
        }"#,
    );
    ws.create_php_file(
        "src/Klarna/Customer.php",
        "<?php\nnamespace Klarna;\nclass Customer {}\n",
    );

    let (mappings, _vendor_dir) = parse_composer_json(ws.root());
    let result = resolve_class_path(&mappings, ws.root(), "Klarna\\Customer");

    assert!(result.is_some());
    let path = result.unwrap();
    assert!(path.ends_with("src/Klarna/Customer.php"));
}

#[test]
fn test_resolve_nested_namespace() {
    let ws = TestWorkspace::new(
        r#"{
            "autoload": {
                "psr-4": {
                    "Klarna\\": "src/Klarna/"
                }
            }
        }"#,
    );
    ws.create_php_file(
        "src/Klarna/Rest/Order.php",
        "<?php\nnamespace Klarna\\Rest;\nclass Order {}\n",
    );

    let (mappings, _vendor_dir) = parse_composer_json(ws.root());
    let result = resolve_class_path(&mappings, ws.root(), "Klarna\\Rest\\Order");

    assert!(result.is_some());
    let path = result.unwrap();
    assert!(path.ends_with("src/Klarna/Rest/Order.php"));
}

#[test]
fn test_resolve_canonical_fqn() {
    let ws = TestWorkspace::new(
        r#"{
            "autoload": {
                "psr-4": {
                    "Klarna\\": "src/Klarna/"
                }
            }
        }"#,
    );
    ws.create_php_file(
        "src/Klarna/Customer.php",
        "<?php\nnamespace Klarna;\nclass Customer {}\n",
    );

    let (mappings, _vendor_dir) = parse_composer_json(ws.root());
    let result = resolve_class_path(&mappings, ws.root(), "Klarna\\Customer");

    assert!(result.is_some());
}

#[test]
fn test_resolve_nonexistent_file_returns_none() {
    let ws = TestWorkspace::new(
        r#"{
            "autoload": {
                "psr-4": {
                    "Klarna\\": "src/Klarna/"
                }
            }
        }"#,
    );

    let (mappings, _vendor_dir) = parse_composer_json(ws.root());
    let result = resolve_class_path(&mappings, ws.root(), "Klarna\\DoesNotExist");

    assert!(result.is_none());
}

#[test]
fn test_resolve_no_matching_prefix() {
    let ws = TestWorkspace::new(
        r#"{
            "autoload": {
                "psr-4": {
                    "Klarna\\": "src/Klarna/"
                }
            }
        }"#,
    );

    let (mappings, _vendor_dir) = parse_composer_json(ws.root());
    let result = resolve_class_path(&mappings, ws.root(), "Acme\\Foo");

    assert!(result.is_none());
}

#[test]
fn test_resolve_longest_prefix_wins() {
    let ws = TestWorkspace::new(
        r#"{
            "autoload": {
                "psr-4": {
                    "Klarna\\": "src/Klarna/",
                    "Klarna\\Rest\\Tests\\": "tests/"
                }
            }
        }"#,
    );
    ws.create_php_file(
        "tests/OrderTest.php",
        "<?php\nnamespace Klarna\\Rest\\Tests;\nclass OrderTest {}\n",
    );

    let (mappings, _vendor_dir) = parse_composer_json(ws.root());
    let result = resolve_class_path(&mappings, ws.root(), "Klarna\\Rest\\Tests\\OrderTest");

    assert!(result.is_some());
    let path = result.unwrap();
    assert!(path.ends_with("tests/OrderTest.php"));
}

#[test]
fn test_resolve_builtin_types_return_none() {
    let ws = TestWorkspace::new(
        r#"{
            "autoload": {
                "psr-4": {
                    "": "src/"
                }
            }
        }"#,
    );

    let (mappings, _vendor_dir) = parse_composer_json(ws.root());

    for builtin in &[
        "self", "static", "parent", "string", "int", "float", "bool", "array", "object", "mixed",
        "void", "never", "null", "true", "false", "callable", "iterable",
    ] {
        assert!(
            resolve_class_path(&mappings, ws.root(), builtin).is_none(),
            "builtin type '{}' should not resolve",
            builtin
        );
    }
}

#[test]
fn test_resolve_array_paths_first_match() {
    let ws = TestWorkspace::new(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": ["src/", "lib/"]
                }
            }
        }"#,
    );
    // File exists only in lib/
    ws.create_php_file(
        "lib/Service.php",
        "<?php\nnamespace App;\nclass Service {}\n",
    );

    let (mappings, _vendor_dir) = parse_composer_json(ws.root());
    let result = resolve_class_path(&mappings, ws.root(), "App\\Service");

    assert!(result.is_some());
    let path = result.unwrap();
    assert!(path.ends_with("lib/Service.php"));
}

#[test]
fn test_normalise_path_adds_trailing_slash() {
    assert_eq!(normalise_path("src"), "src/");
    assert_eq!(normalise_path("src/"), "src/");
    assert_eq!(normalise_path(""), "");
}

#[test]
fn test_normalise_path_converts_backslashes() {
    assert_eq!(normalise_path("src\\Klarna\\"), "src/Klarna/");
}

#[test]
fn test_parse_composer_json_returns_vendor_dir() {
    // parse_composer_json returns the vendor dir from config.vendor-dir
    let ws = TestWorkspace::new(
        r#"{
            "config": {
                "vendor-dir": "php-packages"
            }
        }"#,
    );

    let (mappings, vendor_dir) = parse_composer_json(ws.root());
    assert_eq!(vendor_dir, "php-packages");
    // No PSR-4 sections → no mappings
    assert!(mappings.is_empty());
}

#[test]
fn test_parse_composer_json_excludes_vendor_psr4() {
    // Even when vendor/composer/autoload_psr4.php exists, its mappings
    // should NOT appear in the result — only composer.json's own
    // autoload.psr-4 entries are returned.
    let ws = TestWorkspace::new(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
    );

    ws.create_php_file(
        "vendor/composer/autoload_psr4.php",
        r#"<?php

$vendorDir = dirname(__DIR__);
$baseDir = dirname($vendorDir);

return array(
    'App\\' => array($baseDir . '/src'),
    'Monolog\\' => array($vendorDir . '/monolog/monolog/src/Monolog'),
);
"#,
    );

    let (mappings, _vendor_dir) = parse_composer_json(ws.root());

    // Only App\ from composer.json — Monolog\ from vendor autoload is excluded
    assert_eq!(mappings.len(), 1);
    assert_eq!(mappings[0].prefix, "App\\");
    assert_eq!(mappings[0].base_path, "src/");
}

#[test]
fn test_prefix_without_trailing_backslash() {
    let ws = TestWorkspace::new(
        r#"{
            "autoload": {
                "psr-4": {
                    "App": "src/"
                }
            }
        }"#,
    );
    ws.create_php_file(
        "src/Service.php",
        "<?php\nnamespace App;\nclass Service {}\n",
    );

    let (mappings, _vendor_dir) = parse_composer_json(ws.root());
    // The prefix gets normalised to "App\"
    assert_eq!(mappings[0].prefix, "App\\");

    let result = resolve_class_path(&mappings, ws.root(), "App\\Service");
    assert!(result.is_some());
}

// ─── parse_autoload_files Tests ─────────────────────────────────────────────

#[test]
fn test_autoload_files_vendor_dir_entries() {
    let ws = TestWorkspace::new(r#"{"autoload":{"psr-4":{}}}"#);

    // Create the PHP files that the autoload list refers to
    ws.create_php_file(
        "vendor/amphp/amp/src/functions.php",
        "<?php\nfunction delay(float $s): void {}\n",
    );
    ws.create_php_file(
        "vendor/symfony/deprecation-contracts/function.php",
        "<?php\nfunction trigger_deprecation(): void {}\n",
    );

    // Create the autoload_files.php manifest
    let autoload_content = concat!(
        "<?php\n",
        "\n",
        "// autoload_files.php @generated by Composer\n",
        "\n",
        "$vendorDir = dirname(__DIR__);\n",
        "$baseDir = dirname($vendorDir);\n",
        "\n",
        "return array(\n",
        "    '88254829cb0eed057c30eaabb6d8edc4' => $vendorDir . '/amphp/amp/src/functions.php',\n",
        "    '6e3fae29631ef280660b3cdad06f25a8' => $vendorDir . '/symfony/deprecation-contracts/function.php',\n",
        ");\n",
    );
    ws.create_php_file("vendor/composer/autoload_files.php", autoload_content);

    let files = parse_autoload_files(ws.root(), "vendor");
    assert_eq!(files.len(), 2, "Should find 2 vendor autoload files");

    let names: Vec<String> = files
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert!(names.contains(&"functions.php".to_string()));
    assert!(names.contains(&"function.php".to_string()));
}

#[test]
fn test_autoload_files_basedir_entries() {
    let ws = TestWorkspace::new(r#"{"autoload":{"psr-4":{}}}"#);

    ws.create_php_file(
        "app/Http/helpers.php",
        "<?php\nfunction view(string $name): string { return ''; }\n",
    );

    let autoload_content = concat!(
        "<?php\n",
        "$vendorDir = dirname(__DIR__);\n",
        "$baseDir = dirname($vendorDir);\n",
        "\n",
        "return array(\n",
        "    '224ac75459a4044275cfdffe33336135' => $baseDir . '/app/Http/helpers.php',\n",
        ");\n",
    );
    ws.create_php_file("vendor/composer/autoload_files.php", autoload_content);

    let files = parse_autoload_files(ws.root(), "vendor");
    assert_eq!(files.len(), 1, "Should find 1 baseDir autoload file");
    assert!(
        files[0].ends_with("app/Http/helpers.php"),
        "Path should end with app/Http/helpers.php, got: {:?}",
        files[0]
    );
}

#[test]
fn test_autoload_files_missing_file_skipped() {
    let ws = TestWorkspace::new(r#"{"autoload":{"psr-4":{}}}"#);

    // Reference a file that does NOT exist on disk
    let autoload_content = concat!(
        "<?php\n",
        "$vendorDir = dirname(__DIR__);\n",
        "$baseDir = dirname($vendorDir);\n",
        "\n",
        "return array(\n",
        "    'abc123' => $vendorDir . '/nonexistent/functions.php',\n",
        ");\n",
    );
    ws.create_php_file("vendor/composer/autoload_files.php", autoload_content);

    let files = parse_autoload_files(ws.root(), "vendor");
    assert!(
        files.is_empty(),
        "Non-existent files should be skipped, got: {:?}",
        files
    );
}

#[test]
fn test_autoload_files_no_autoload_file() {
    let ws = TestWorkspace::new(r#"{"autoload":{"psr-4":{}}}"#);

    // No autoload_files.php at all
    let files = parse_autoload_files(ws.root(), "vendor");
    assert!(
        files.is_empty(),
        "Missing autoload_files.php should return empty vec"
    );
}

#[test]
fn test_autoload_files_mixed_vendor_and_basedir() {
    let ws = TestWorkspace::new(r#"{"autoload":{"psr-4":{}}}"#);

    ws.create_php_file(
        "vendor/some/pkg/src/functions.php",
        "<?php\nfunction pkg_func(): void {}\n",
    );
    ws.create_php_file(
        "src/helpers.php",
        "<?php\nfunction my_helper(): string { return ''; }\n",
    );

    let autoload_content = concat!(
        "<?php\n",
        "$vendorDir = dirname(__DIR__);\n",
        "$baseDir = dirname($vendorDir);\n",
        "\n",
        "return array(\n",
        "    'aaa' => $vendorDir . '/some/pkg/src/functions.php',\n",
        "    'bbb' => $baseDir . '/src/helpers.php',\n",
        ");\n",
    );
    ws.create_php_file("vendor/composer/autoload_files.php", autoload_content);

    let files = parse_autoload_files(ws.root(), "vendor");
    assert_eq!(files.len(), 2, "Should find both vendor and baseDir files");

    // Verify both paths are absolute and exist
    for f in &files {
        assert!(f.is_absolute(), "Path should be absolute: {:?}", f);
        assert!(f.is_file(), "Path should exist on disk: {:?}", f);
    }
}

// ─── extract_require_once_paths Tests ───────────────────────────────────────

#[test]
fn test_require_once_statement_form() {
    let content = concat!(
        "<?php\n",
        "require_once 'Trustly/exceptions.php';\n",
        "require_once 'Trustly/Data/data.php';\n",
    );
    let paths = extract_require_once_paths(content);
    assert_eq!(paths.len(), 2);
    assert_eq!(paths[0], "Trustly/exceptions.php");
    assert_eq!(paths[1], "Trustly/Data/data.php");
}

#[test]
fn test_require_once_function_form() {
    let content = concat!(
        "<?php\n",
        "require_once('Trustly/exceptions.php');\n",
        "require_once('Trustly/Data/data.php');\n",
    );
    let paths = extract_require_once_paths(content);
    assert_eq!(paths.len(), 2);
    assert_eq!(paths[0], "Trustly/exceptions.php");
    assert_eq!(paths[1], "Trustly/Data/data.php");
}

#[test]
fn test_require_once_double_quotes() {
    let content = concat!(
        "<?php\n",
        "require_once \"Trustly/exceptions.php\";\n",
        "require_once(\"Trustly/Data/data.php\");\n",
    );
    let paths = extract_require_once_paths(content);
    assert_eq!(paths.len(), 2);
    assert_eq!(paths[0], "Trustly/exceptions.php");
    assert_eq!(paths[1], "Trustly/Data/data.php");
}

#[test]
fn test_require_once_mixed_forms() {
    let content = concat!(
        "<?php\n",
        "/**\n",
        " * Main include file for working with the trustly-client-php code.\n",
        " */\n",
        "\n",
        "require_once('Trustly/exceptions.php');\n",
        "require_once('Trustly/Data/data.php');\n",
        "require_once 'Trustly/Api/api.php';\n",
    );
    let paths = extract_require_once_paths(content);
    assert_eq!(paths.len(), 3);
    assert_eq!(paths[0], "Trustly/exceptions.php");
    assert_eq!(paths[1], "Trustly/Data/data.php");
    assert_eq!(paths[2], "Trustly/Api/api.php");
}

#[test]
fn test_require_once_skips_dynamic_expressions() {
    let content = concat!(
        "<?php\n",
        "require_once __DIR__ . '/Trustly/exceptions.php';\n",
        "require_once $path;\n",
        "require_once 'Trustly/Data/data.php';\n",
    );
    let paths = extract_require_once_paths(content);
    assert_eq!(
        paths.len(),
        1,
        "Should skip dynamic expressions and only find the string literal"
    );
    assert_eq!(paths[0], "Trustly/Data/data.php");
}

#[test]
fn test_require_once_ignores_other_includes() {
    let content = concat!(
        "<?php\n",
        "include 'config.php';\n",
        "include_once 'helpers.php';\n",
        "require 'bootstrap.php';\n",
        "require_once 'Trustly/exceptions.php';\n",
    );
    let paths = extract_require_once_paths(content);
    assert_eq!(
        paths.len(),
        1,
        "Should only extract require_once, not include/include_once/require"
    );
    assert_eq!(paths[0], "Trustly/exceptions.php");
}

#[test]
fn test_require_once_empty_file() {
    let content = "<?php\n";
    let paths = extract_require_once_paths(content);
    assert!(paths.is_empty());
}

#[test]
fn test_require_once_with_extra_whitespace() {
    let content = concat!(
        "<?php\n",
        "  require_once  (  'Trustly/exceptions.php'  )  ;\n",
        "    require_once   'Trustly/Data/data.php'  ;\n",
    );
    let paths = extract_require_once_paths(content);
    assert_eq!(paths.len(), 2);
    assert_eq!(paths[0], "Trustly/exceptions.php");
    assert_eq!(paths[1], "Trustly/Data/data.php");
}

// ─── autoload_classmap.php tests ────────────────────────────────────────────

#[test]
fn test_classmap_basic_vendor_entries() {
    let ws = TestWorkspace::new(r#"{"name": "test/project"}"#);

    let classmap_content = concat!(
        "<?php\n",
        "\n",
        "// autoload_classmap.php @generated by Composer\n",
        "\n",
        "$vendorDir = dirname(__DIR__);\n",
        "$baseDir = dirname($vendorDir);\n",
        "\n",
        "return array(\n",
        "    'AWS\\\\CRT\\\\Auth\\\\AwsCredentials' => $vendorDir . '/aws/aws-crt-php/src/AWS/CRT/Auth/AwsCredentials.php',\n",
        "    'AWS\\\\CRT\\\\Auth\\\\CredentialsProvider' => $vendorDir . '/aws/aws-crt-php/src/AWS/CRT/Auth/CredentialsProvider.php',\n",
        ");\n",
    );
    ws.create_php_file("vendor/composer/autoload_classmap.php", classmap_content);

    let classmap = parse_autoload_classmap(ws.root(), "vendor");
    assert_eq!(classmap.len(), 2, "Should find 2 classmap entries");

    let creds_path = classmap.get("AWS\\CRT\\Auth\\AwsCredentials");
    assert!(creds_path.is_some(), "Should have AwsCredentials entry");
    assert!(
        creds_path
            .unwrap()
            .ends_with("vendor/aws/aws-crt-php/src/AWS/CRT/Auth/AwsCredentials.php"),
        "AwsCredentials path should resolve to vendor dir, got: {:?}",
        creds_path
    );

    let provider_path = classmap.get("AWS\\CRT\\Auth\\CredentialsProvider");
    assert!(
        provider_path.is_some(),
        "Should have CredentialsProvider entry"
    );
    assert!(
        provider_path
            .unwrap()
            .ends_with("vendor/aws/aws-crt-php/src/AWS/CRT/Auth/CredentialsProvider.php"),
        "CredentialsProvider path should resolve to vendor dir, got: {:?}",
        provider_path
    );
}

#[test]
fn test_classmap_basedir_entries() {
    let ws = TestWorkspace::new(r#"{"name": "test/project"}"#);

    let classmap_content = concat!(
        "<?php\n",
        "\n",
        "$vendorDir = dirname(__DIR__);\n",
        "$baseDir = dirname($vendorDir);\n",
        "\n",
        "return array(\n",
        "    'App\\\\Models\\\\User' => $baseDir . '/app/Models/User.php',\n",
        "    'App\\\\Http\\\\Controllers\\\\HomeController' => $baseDir . '/app/Http/Controllers/HomeController.php',\n",
        ");\n",
    );
    ws.create_php_file("vendor/composer/autoload_classmap.php", classmap_content);

    let classmap = parse_autoload_classmap(ws.root(), "vendor");
    assert_eq!(classmap.len(), 2, "Should find 2 baseDir classmap entries");

    let user_path = classmap.get("App\\Models\\User");
    assert!(user_path.is_some(), "Should have User entry");
    assert!(
        user_path.unwrap().ends_with("app/Models/User.php"),
        "User path should resolve to baseDir, got: {:?}",
        user_path
    );

    let controller_path = classmap.get("App\\Http\\Controllers\\HomeController");
    assert!(
        controller_path.is_some(),
        "Should have HomeController entry"
    );
    assert!(
        controller_path
            .unwrap()
            .ends_with("app/Http/Controllers/HomeController.php"),
        "HomeController path should resolve to baseDir, got: {:?}",
        controller_path
    );
}

#[test]
fn test_classmap_mixed_vendor_and_basedir() {
    let ws = TestWorkspace::new(r#"{"name": "test/project"}"#);

    let classmap_content = concat!(
        "<?php\n",
        "\n",
        "$vendorDir = dirname(__DIR__);\n",
        "$baseDir = dirname($vendorDir);\n",
        "\n",
        "return array(\n",
        "    'Monolog\\\\Logger' => $vendorDir . '/monolog/monolog/src/Monolog/Logger.php',\n",
        "    'App\\\\Services\\\\PaymentService' => $baseDir . '/app/Services/PaymentService.php',\n",
        ");\n",
    );
    ws.create_php_file("vendor/composer/autoload_classmap.php", classmap_content);

    let classmap = parse_autoload_classmap(ws.root(), "vendor");
    assert_eq!(
        classmap.len(),
        2,
        "Should find both vendor and baseDir entries"
    );

    assert!(
        classmap.contains_key("Monolog\\Logger"),
        "Should have vendor class"
    );
    assert!(
        classmap.contains_key("App\\Services\\PaymentService"),
        "Should have baseDir class"
    );
}

#[test]
fn test_classmap_missing_file_returns_empty() {
    let ws = TestWorkspace::new(r#"{"name": "test/project"}"#);
    // No vendor directory at all — should not panic
    let classmap = parse_autoload_classmap(ws.root(), "vendor");
    assert!(
        classmap.is_empty(),
        "Missing autoload_classmap.php should return empty map"
    );
}

#[test]
fn test_classmap_empty_array() {
    let ws = TestWorkspace::new(r#"{"name": "test/project"}"#);

    let classmap_content = concat!(
        "<?php\n",
        "\n",
        "$vendorDir = dirname(__DIR__);\n",
        "$baseDir = dirname($vendorDir);\n",
        "\n",
        "return array(\n",
        ");\n",
    );
    ws.create_php_file("vendor/composer/autoload_classmap.php", classmap_content);

    let classmap = parse_autoload_classmap(ws.root(), "vendor");
    assert!(
        classmap.is_empty(),
        "Empty classmap array should return empty map"
    );
}

#[test]
fn test_classmap_custom_vendor_dir() {
    let ws = TestWorkspace::new(r#"{"name": "test/project", "config": {"vendor-dir": "libs"}}"#);

    let classmap_content = concat!(
        "<?php\n",
        "\n",
        "$vendorDir = dirname(__DIR__);\n",
        "$baseDir = dirname($vendorDir);\n",
        "\n",
        "return array(\n",
        "    'Acme\\\\Widget' => $vendorDir . '/acme/widget/src/Widget.php',\n",
        ");\n",
    );
    ws.create_php_file("libs/composer/autoload_classmap.php", classmap_content);

    let classmap = parse_autoload_classmap(ws.root(), "libs");
    assert_eq!(classmap.len(), 1);

    let widget_path = classmap.get("Acme\\Widget");
    assert!(widget_path.is_some(), "Should have Widget entry");
    assert!(
        widget_path
            .unwrap()
            .ends_with("libs/acme/widget/src/Widget.php"),
        "Path should use custom vendor dir 'libs', got: {:?}",
        widget_path
    );
}

#[test]
fn test_classmap_top_level_class() {
    let ws = TestWorkspace::new(r#"{"name": "test/project"}"#);

    // Classes without a namespace (top-level) are also valid classmap entries
    let classmap_content = concat!(
        "<?php\n",
        "\n",
        "$vendorDir = dirname(__DIR__);\n",
        "$baseDir = dirname($vendorDir);\n",
        "\n",
        "return array(\n",
        "    'SomeGlobalClass' => $vendorDir . '/legacy/SomeGlobalClass.php',\n",
        "    'Namespaced\\\\Example' => $vendorDir . '/example/src/Example.php',\n",
        ");\n",
    );
    ws.create_php_file("vendor/composer/autoload_classmap.php", classmap_content);

    let classmap = parse_autoload_classmap(ws.root(), "vendor");
    assert_eq!(classmap.len(), 2);

    assert!(
        classmap.contains_key("SomeGlobalClass"),
        "Should handle top-level (no-namespace) classes"
    );
    assert!(
        classmap.contains_key("Namespaced\\Example"),
        "Should handle namespaced classes alongside top-level ones"
    );
}

/// Helper: build a `vendor/composer/installed.json` (Composer 2 format)
/// with the given package objects and write it into the workspace.
fn write_installed_json(ws: &TestWorkspace, packages: &str) {
    let full = ws.dir.path().join("vendor/composer/installed.json");
    fs::create_dir_all(full.parent().unwrap()).expect("failed to create vendor/composer");
    fs::write(full, format!(r#"{{ "packages": [{packages}] }}"#))
        .expect("failed to write installed.json");
}

#[test]
fn test_path_repo_psr4_symlinked_package_inside_workspace() {
    let ws = TestWorkspace::new(r#"{ "autoload": { "psr-4": { "App\\": "app/" } } }"#);
    // The module lives inside the workspace at app-modules/common/.
    ws.create_php_file("app-modules/common/src/.gitkeep", "");
    write_installed_json(
        &ws,
        r#"{
            "name": "demo/common",
            "dist": { "type": "path" },
            "transport-options": { "symlink": true },
            "install-path": "../../app-modules/common",
            "autoload": { "psr-4": { "Demo\\Common\\": "src/" } }
        }"#,
    );

    let mappings = extract_path_repo_psr4_mappings(ws.root(), "vendor");
    assert_eq!(mappings.len(), 1, "expected one path-repo mapping");
    assert_eq!(mappings[0].prefix, "Demo\\Common\\");
    assert!(
        mappings[0]
            .base_path
            .trim_end_matches('/')
            .ends_with("app-modules/common/src"),
        "base_path should point at the module's src dir, got {}",
        mappings[0].base_path
    );
}

#[test]
fn test_path_repo_psr4_skips_copied_package() {
    let ws = TestWorkspace::new(r#"{ "autoload": { "psr-4": { "App\\": "app/" } } }"#);
    ws.create_php_file("app-modules/common/src/.gitkeep", "");
    // symlink == false → the package was copied, treat as a normal vendor
    // snapshot and do NOT add it to the project's PSR-4 mappings.
    write_installed_json(
        &ws,
        r#"{
            "name": "demo/common",
            "dist": { "type": "path" },
            "transport-options": { "symlink": false },
            "install-path": "../../app-modules/common",
            "autoload": { "psr-4": { "Demo\\Common\\": "src/" } }
        }"#,
    );

    assert!(extract_path_repo_psr4_mappings(ws.root(), "vendor").is_empty());
}

#[test]
fn test_path_repo_psr4_skips_non_path_dist() {
    let ws = TestWorkspace::new(r#"{ "autoload": { "psr-4": { "App\\": "app/" } } }"#);
    ws.create_php_file("vendor/vendorpkg/lib/src/.gitkeep", "");
    // dist.type == "zip" → a normal downloaded dependency, not a path repo.
    write_installed_json(
        &ws,
        r#"{
            "name": "vendorpkg/lib",
            "dist": { "type": "zip" },
            "transport-options": { "symlink": true },
            "install-path": "../vendorpkg/lib",
            "autoload": { "psr-4": { "Vendor\\Lib\\": "src/" } }
        }"#,
    );

    assert!(extract_path_repo_psr4_mappings(ws.root(), "vendor").is_empty());
}

#[test]
fn test_path_repo_psr4_skips_package_outside_workspace() {
    let ws = TestWorkspace::new(r#"{ "autoload": { "psr-4": { "App\\": "app/" } } }"#);
    // A path repo whose resolved location is outside the workspace (e.g.
    // a shared library elsewhere on disk) is not project code.
    let external = tempfile::tempdir().expect("failed to create external dir");
    fs::create_dir_all(external.path().join("src")).expect("failed to create external src");
    let install_path = external.path().to_string_lossy().replace('\\', "\\\\");
    write_installed_json(
        &ws,
        &format!(
            r#"{{
                "name": "shared/lib",
                "dist": {{ "type": "path" }},
                "transport-options": {{ "symlink": true }},
                "install-path": "{install_path}",
                "autoload": {{ "psr-4": {{ "Shared\\Lib\\": "src/" }} }}
            }}"#
        ),
    );

    assert!(extract_path_repo_psr4_mappings(ws.root(), "vendor").is_empty());
}

#[test]
fn test_path_repo_psr4_skips_package_inside_vendor() {
    let ws = TestWorkspace::new(r#"{ "autoload": { "psr-4": { "App\\": "app/" } } }"#);
    // A path repo whose canonical location resolves back inside vendor/ —
    // because the symlink was materialised into a real copy, or its target
    // is itself in vendor — is an ordinary vendored dependency, not project
    // code.  It must not be added to the PSR-4 mappings, or the diagnostics
    // pass would walk and analyse the entire dependency as user source.
    ws.create_php_file("vendor/acme/shared/src/.gitkeep", "");
    write_installed_json(
        &ws,
        r#"{
            "name": "acme/shared",
            "dist": { "type": "path" },
            "transport-options": { "symlink": true },
            "install-path": "../acme/shared",
            "autoload": { "psr-4": { "Acme\\Shared\\": "src/" } }
        }"#,
    );

    assert!(extract_path_repo_psr4_mappings(ws.root(), "vendor").is_empty());
}

#[test]
fn test_path_repo_psr4_no_installed_json() {
    let ws = TestWorkspace::new(r#"{ "autoload": { "psr-4": { "App\\": "app/" } } }"#);
    assert!(extract_path_repo_psr4_mappings(ws.root(), "vendor").is_empty());
}

#[test]
fn test_path_repo_psr4_missing_dir_skipped() {
    let ws = TestWorkspace::new(r#"{ "autoload": { "psr-4": { "App\\": "app/" } } }"#);
    // The module dir exists but the declared src/ subdirectory does not,
    // so no mapping is produced.
    ws.create_php_file("app-modules/common/composer.json", "{}");
    write_installed_json(
        &ws,
        r#"{
            "name": "demo/common",
            "dist": { "type": "path" },
            "transport-options": { "symlink": true },
            "install-path": "../../app-modules/common",
            "autoload": { "psr-4": { "Demo\\Common\\": "src/" } }
        }"#,
    );

    assert!(extract_path_repo_psr4_mappings(ws.root(), "vendor").is_empty());
}
