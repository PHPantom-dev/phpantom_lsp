use super::*;

// ── find_classes unit tests ──────────────────────────────────────

#[test]
fn simple_class() {
    let content = b"<?php\nclass Foo {}";
    assert_eq!(find_classes(content), vec!["Foo"]);
}

#[test]
fn namespaced_class() {
    let content = b"<?php\nnamespace App\\Models;\nclass User {}";
    assert_eq!(find_classes(content), vec!["App\\Models\\User"]);
}

#[test]
fn multiple_declarations() {
    let content = br"<?php
namespace App;

class Foo {}
interface Bar {}
trait Baz {}
enum Status {}
";
    assert_eq!(
        find_classes(content),
        vec!["App\\Foo", "App\\Bar", "App\\Baz", "App\\Status"]
    );
}

#[test]
fn class_in_comment_ignored() {
    let content = br"<?php
// class Fake {}
/* class AlsoFake {} */
class Real {}
";
    assert_eq!(find_classes(content), vec!["Real"]);
}

#[test]
fn class_in_string_ignored() {
    let content = br#"<?php
$x = "class Fake {}";
$y = 'class AlsoFake {}';
class Real {}
"#;
    assert_eq!(find_classes(content), vec!["Real"]);
}

#[test]
fn no_classes() {
    let content = b"<?php\necho 'hello';";
    assert!(find_classes(content).is_empty());
}

#[test]
fn enum_with_type() {
    let content = b"<?php\nenum Status: int { case Active = 1; }";
    assert_eq!(find_classes(content), vec!["Status"]);
}

#[test]
fn class_constant_not_treated_as_declaration() {
    let content = b"<?php\n$x = SomeClass::class;\nclass Real {}";
    assert_eq!(find_classes(content), vec!["Real"]);
}

#[test]
fn php_attribute() {
    let content = br"<?php
#[Attribute]
class MyAttribute {}
";
    assert_eq!(find_classes(content), vec!["MyAttribute"]);
}

#[test]
fn heredoc() {
    let content = br"<?php
$x = <<<EOT
class Fake {}
EOT;
class Real {}
";
    assert_eq!(find_classes(content), vec!["Real"]);
}

#[test]
fn nowdoc() {
    let content = br"<?php
$x = <<<'EOT'
class Fake {}
EOT;
class Real {}
";
    assert_eq!(find_classes(content), vec!["Real"]);
}

#[test]
fn property_access_class_ignored() {
    let content = br"<?php
namespace Foo;
if ($node->class instanceof Name) {
}
";
    assert!(find_classes(content).is_empty());
}

#[test]
fn nullsafe_property_access_class_ignored() {
    let content = br"<?php
namespace Foo;
if ($node?->class instanceof Name) {
}
";
    assert!(find_classes(content).is_empty());
}

#[test]
fn real_class_not_affected_by_property_access() {
    let content = br"<?php
namespace Foo;
class Real {}
if ($node->class instanceof Name) {
}
";
    assert_eq!(find_classes(content), vec!["Foo\\Real"]);
}

#[test]
fn anonymous_class_ignored() {
    let content = br"<?php
$x = new class extends Foo {};
class Real {}
";
    assert_eq!(find_classes(content), vec!["Real"]);
}

#[test]
fn anonymous_class_implements_ignored() {
    let content = br"<?php
$x = new class implements Bar {};
class Real {}
";
    assert_eq!(find_classes(content), vec!["Real"]);
}

#[test]
fn hash_comment_not_confused_with_attribute() {
    let content = br"<?php
# This is a comment with class keyword
class Real {}
";
    assert_eq!(find_classes(content), vec!["Real"]);
}

#[test]
fn multiple_namespaces() {
    let content = br"<?php
namespace First;
class A {}
namespace Second;
class B {}
";
    assert_eq!(find_classes(content), vec!["First\\A", "Second\\B"]);
}

#[test]
fn global_namespace_after_named() {
    // namespace; with no name resets to global
    let content = br"<?php
namespace Foo;
class A {}
namespace;
class B {}
";
    // When `namespace;` is encountered with no name, the namespace
    // becomes empty (global).
    assert_eq!(find_classes(content), vec!["Foo\\A", "B"]);
}

#[test]
fn escaped_string_does_not_leak() {
    let content = br#"<?php
$x = "escaped \" class Fake {}";
class Real {}
"#;
    assert_eq!(find_classes(content), vec!["Real"]);
}

#[test]
fn escaped_single_quote_string_does_not_leak() {
    let content = br"<?php
$x = 'escaped \' class Fake {}';
class Real {}
";
    assert_eq!(find_classes(content), vec!["Real"]);
}

#[test]
fn block_comment_with_star() {
    let content = br"<?php
/**
 * class Fake {}
 */
class Real {}
";
    assert_eq!(find_classes(content), vec!["Real"]);
}

#[test]
fn empty_content() {
    assert!(find_classes(b"").is_empty());
}

#[test]
fn no_keyword_quick_rejection() {
    let content = b"<?php\necho 'hello world';";
    assert!(find_classes(content).is_empty());
}

#[test]
fn flexible_heredoc_php73() {
    // PHP 7.3+ allows the closing identifier to be indented
    let content = br"<?php
$x = <<<EOT
    class Fake {}
    EOT;
class Real {}
";
    assert_eq!(find_classes(content), vec!["Real"]);
}

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

// ── find_symbols unit tests ─────────────────────────────────────

#[test]
fn symbols_simple_function() {
    let content = b"<?php\nfunction helper(): void {}";
    let result = find_symbols(content);
    assert_eq!(result.functions, vec!["helper"]);
    assert!(result.classes.is_empty());
    assert!(result.constants.is_empty());
}

#[test]
fn symbols_namespaced_function() {
    let content = b"<?php\nnamespace App\\Helpers;\nfunction helper(): void {}";
    let result = find_symbols(content);
    assert_eq!(result.functions, vec!["App\\Helpers\\helper"]);
}

#[test]
fn symbols_closure_not_captured() {
    let content = b"<?php\n$fn = function () { return 1; };";
    let result = find_symbols(content);
    assert!(result.functions.is_empty());
}

#[test]
fn use_function_not_captured() {
    let content =
        b"<?php\nnamespace App\\Cache;\nuse function is_array;\nuse function array_map;\n";
    let result = find_symbols(content);
    assert!(
        result.functions.is_empty(),
        "use function statements should not appear as functions: {:?}",
        result.functions
    );
}

#[test]
fn use_const_not_captured() {
    let content = b"<?php\nnamespace App\\Config;\nuse const PHP_EOL;\n";
    let result = find_symbols(content);
    assert!(
        result.constants.is_empty(),
        "use const statements should not appear as constants: {:?}",
        result.constants
    );
}

#[test]
fn symbols_method_not_captured() {
    let content = br"<?php
class Foo {
    public function bar(): void {}
}
";
    let result = find_symbols(content);
    assert_eq!(result.classes, vec!["Foo"]);
    assert!(
        result.functions.is_empty(),
        "methods should not appear as functions: {:?}",
        result.functions
    );
}

#[test]
fn symbols_define_single_quote() {
    let content = b"<?php\ndefine('MY_CONST', 42);";
    let result = find_symbols(content);
    assert_eq!(result.constants, vec!["MY_CONST"]);
}

#[test]
fn symbols_define_double_quote() {
    let content = b"<?php\ndefine(\"APP_VERSION\", '1.0');";
    let result = find_symbols(content);
    assert_eq!(result.constants, vec!["APP_VERSION"]);
}

#[test]
fn symbols_top_level_const() {
    let content = b"<?php\nconst FOO = 'bar';";
    let result = find_symbols(content);
    assert_eq!(result.constants, vec!["FOO"]);
}

#[test]
fn symbols_namespaced_const() {
    let content = b"<?php\nnamespace App;\nconst VERSION = '1.0';";
    let result = find_symbols(content);
    assert_eq!(result.constants, vec!["App\\VERSION"]);
}

#[test]
fn symbols_class_const_not_captured() {
    let content = br"<?php
class Config {
    const MAX = 100;
    public function foo(): void {}
}
";
    let result = find_symbols(content);
    assert_eq!(result.classes, vec!["Config"]);
    assert!(
        result.constants.is_empty(),
        "class constants should not be captured: {:?}",
        result.constants
    );
    assert!(
        result.functions.is_empty(),
        "methods should not be captured: {:?}",
        result.functions
    );
}

#[test]
fn symbols_mixed_file() {
    let content = br#"<?php
namespace App\Utils;

class Helper {}
interface Renderable {}

function formatDate(): string { return ''; }
function parseJson(): array { return []; }

define('APP_NAME', 'MyApp');
const DEBUG = true;
"#;
    let result = find_symbols(content);
    assert_eq!(
        result.classes,
        vec!["App\\Utils\\Helper", "App\\Utils\\Renderable"]
    );
    assert_eq!(
        result.functions,
        vec!["App\\Utils\\formatDate", "App\\Utils\\parseJson"]
    );
    assert!(
        result.constants.contains(&"APP_NAME".to_string()),
        "should find define(): {:?}",
        result.constants
    );
    assert!(
        result.constants.contains(&"App\\Utils\\DEBUG".to_string()),
        "should find namespaced const: {:?}",
        result.constants
    );
}

#[test]
fn symbols_function_in_comment_ignored() {
    let content = b"<?php\n// function notReal(): void {}\nfunction real(): void {}";
    let result = find_symbols(content);
    assert_eq!(result.functions, vec!["real"]);
}

#[test]
fn symbols_function_named_int() {
    let content = br#"<?php
declare(strict_types=1);
namespace Psl\Type;
function int(): TypeInterface
{
    static $instance = new Internal\IntType();
    return $instance;
}
"#;
    let result = find_symbols(content);
    assert_eq!(result.functions, vec!["Psl\\Type\\int"]);
}

#[test]
fn symbols_define_in_string_ignored() {
    let content = b"<?php\n$s = \"define('NOT_REAL', 1);\";";
    let result = find_symbols(content);
    assert!(result.constants.is_empty());
}

#[test]
fn symbols_braced_namespace() {
    let content = br"<?php
namespace Foo {
    class A {}
    function helper(): void {}
    const BAR = 1;
}
namespace Baz {
    class B {}
    function other(): void {}
}
";
    let result = find_symbols(content);
    assert_eq!(result.classes, vec!["Foo\\A", "Baz\\B"]);
    assert_eq!(result.functions, vec!["Foo\\helper", "Baz\\other"]);
    assert_eq!(result.constants, vec!["Foo\\BAR"]);
}

#[test]
fn symbols_function_with_parenthesized_return() {
    // Ensure `function` keyword followed by `(` is treated as closure.
    let content = b"<?php\n$f = function(int $x): int { return $x; };";
    let result = find_symbols(content);
    assert!(result.functions.is_empty());
}

#[test]
fn symbols_define_in_block_comment_ignored() {
    let content = b"<?php\n/* define('NOPE', 1); */\ndefine('YES', 2);";
    let result = find_symbols(content);
    assert_eq!(result.constants, vec!["YES"]);
}

#[test]
fn symbols_empty_content() {
    let result = find_symbols(b"");
    assert!(result.classes.is_empty());
    assert!(result.functions.is_empty());
    assert!(result.constants.is_empty());
}

#[test]
fn symbols_no_php_symbols() {
    let result = find_symbols(b"<?php\n$x = 1 + 2;\necho $x;");
    assert!(result.classes.is_empty());
    assert!(result.functions.is_empty());
    assert!(result.constants.is_empty());
}

#[test]
fn symbols_heredoc_skipped() {
    let content = br#"<?php
$s = <<<EOT
function fakeFunc(): void {}
define('FAKE', 1);
class FakeClass {}
EOT;
function realFunc(): void {}
"#;
    let result = find_symbols(content);
    assert_eq!(result.functions, vec!["realFunc"]);
    assert!(result.classes.is_empty());
    assert!(result.constants.is_empty());
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
