//! Integration tests for the self-generated classmap feature.
//!
//! These tests verify that PHPantom can build a classmap by scanning PHP
//! files when Composer's `autoload_classmap.php` is missing or incomplete,
//! and that cross-file resolution works using the self-scanned classmap.

use crate::common::create_psr4_workspace;
use phpantom_lsp::classmap_scanner;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── classmap_scanner::find_classes unit-level integration ──────────────────

#[test]
fn find_classes_extracts_namespaced_class() {
    let content = b"<?php\nnamespace App\\Models;\nclass User {}";
    let classes = classmap_scanner::find_classes(content);
    assert_eq!(classes, vec!["App\\Models\\User"]);
}

#[test]
fn find_classes_extracts_interface_trait_enum() {
    let content = br"<?php
namespace App\Contracts;
interface Cacheable {}
trait Loggable {}
enum Status: string { case Active = 'active'; }
";
    let classes = classmap_scanner::find_classes(content);
    assert_eq!(
        classes,
        vec![
            "App\\Contracts\\Cacheable",
            "App\\Contracts\\Loggable",
            "App\\Contracts\\Status"
        ]
    );
}

#[test]
fn find_classes_skips_anonymous_classes() {
    let content = br"<?php
namespace App;
class Real {}
$anon = new class extends Real {};
$anon2 = new class implements \Countable { public function count(): int { return 0; } };
";
    let classes = classmap_scanner::find_classes(content);
    assert_eq!(classes, vec!["App\\Real"]);
}

// ─── scan_psr4_directories ─────────────────────────────────────────────────

#[test]
fn scan_psr4_directories_respects_namespace_filtering() {
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

    // Non-compliant: namespace doesn't match PSR-4 expectation
    std::fs::write(
        models.join("WrongNs.php"),
        "<?php\nnamespace Wrong\\Namespace;\nclass WrongNs {}",
    )
    .unwrap();

    let classmap = classmap_scanner::scan_psr4_directories(&[("App\\".to_string(), src)], &[], &[]);
    assert!(classmap.contains_key("App\\Models\\User"));
    assert!(
        !classmap.contains_key("Wrong\\Namespace\\WrongNs"),
        "Non-PSR-4-compliant class should be excluded"
    );
}

#[test]
fn scan_psr4_directories_handles_classmap_entries() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("lib");
    std::fs::create_dir_all(&lib).unwrap();

    // classmap entries don't filter by namespace
    std::fs::write(lib.join("Legacy.php"), "<?php\nclass LegacyHelper {}").unwrap();

    let classmap = classmap_scanner::scan_psr4_directories(&[], &[lib], &[]);
    assert!(classmap.contains_key("LegacyHelper"));
}

// ─── scan_vendor_packages ──────────────────────────────────────────────────

#[test]
fn scan_vendor_packages_composer_v1_format() {
    let dir = tempfile::tempdir().unwrap();
    let vendor = dir.path().join("vendor");
    let composer_dir = vendor.join("composer");
    std::fs::create_dir_all(&composer_dir).unwrap();

    let pkg_src = vendor.join("acme").join("tools").join("src");
    std::fs::create_dir_all(&pkg_src).unwrap();
    std::fs::write(
        pkg_src.join("Hammer.php"),
        "<?php\nnamespace Acme\\Tools;\nclass Hammer {}",
    )
    .unwrap();

    // Composer 1 format: top-level array
    let installed = serde_json::json!([
        {
            "name": "acme/tools",
            "autoload": {
                "psr-4": {
                    "Acme\\Tools\\": "src/"
                }
            }
        }
    ]);
    std::fs::write(
        composer_dir.join("installed.json"),
        serde_json::to_string(&installed).unwrap(),
    )
    .unwrap();

    let result = classmap_scanner::scan_vendor_packages(dir.path(), "vendor");
    let classmap = result.classmap;
    assert!(
        classmap.contains_key("Acme\\Tools\\Hammer"),
        "keys: {:?}",
        classmap.keys().collect::<Vec<_>>()
    );
}

#[test]
fn scan_vendor_packages_composer_v2_format() {
    let dir = tempfile::tempdir().unwrap();
    let vendor = dir.path().join("vendor");
    let composer_dir = vendor.join("composer");
    std::fs::create_dir_all(&composer_dir).unwrap();

    let pkg_src = vendor.join("foo").join("bar").join("src");
    std::fs::create_dir_all(&pkg_src).unwrap();
    std::fs::write(
        pkg_src.join("Baz.php"),
        "<?php\nnamespace Foo\\Bar;\nclass Baz {}",
    )
    .unwrap();

    let installed = serde_json::json!({
        "packages": [
            {
                "name": "foo/bar",
                "install-path": "../foo/bar",
                "autoload": {
                    "psr-4": {
                        "Foo\\Bar\\": "src/"
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

    let result = classmap_scanner::scan_vendor_packages(dir.path(), "vendor");
    assert!(result.classmap.contains_key("Foo\\Bar\\Baz"));
}

#[test]
fn scan_vendor_packages_missing_installed_json() {
    let dir = tempfile::tempdir().unwrap();
    // No installed.json at all
    let result = classmap_scanner::scan_vendor_packages(dir.path(), "vendor");
    assert!(result.classmap.is_empty());
}

#[test]
fn scan_vendor_packages_multiple_psr4_paths() {
    let dir = tempfile::tempdir().unwrap();
    let vendor = dir.path().join("vendor");
    let composer_dir = vendor.join("composer");
    std::fs::create_dir_all(&composer_dir).unwrap();

    // Package with multiple PSR-4 source dirs
    let src1 = vendor.join("multi").join("pkg").join("src");
    let src2 = vendor.join("multi").join("pkg").join("lib");
    std::fs::create_dir_all(&src1).unwrap();
    std::fs::create_dir_all(&src2).unwrap();
    std::fs::write(
        src1.join("Alpha.php"),
        "<?php\nnamespace Multi\\Pkg;\nclass Alpha {}",
    )
    .unwrap();
    std::fs::write(
        src2.join("Beta.php"),
        "<?php\nnamespace Multi\\Pkg;\nclass Beta {}",
    )
    .unwrap();

    let installed = serde_json::json!({
        "packages": [
            {
                "name": "multi/pkg",
                "install-path": "../multi/pkg",
                "autoload": {
                    "psr-4": {
                        "Multi\\Pkg\\": ["src/", "lib/"]
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

    let result = classmap_scanner::scan_vendor_packages(dir.path(), "vendor");
    let classmap = result.classmap;
    assert!(classmap.contains_key("Multi\\Pkg\\Alpha"));
    assert!(classmap.contains_key("Multi\\Pkg\\Beta"));
}

// ─── scan_workspace_fallback ───────────────────────────────────────────────

#[test]
fn scan_workspace_fallback_skips_hidden_and_vendor() {
    let dir = tempfile::tempdir().unwrap();

    // Visible file
    std::fs::write(dir.path().join("Visible.php"), "<?php\nclass Visible {}").unwrap();

    // Hidden directory
    let hidden = dir.path().join(".hidden");
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::write(hidden.join("Secret.php"), "<?php\nclass Secret {}").unwrap();

    // Vendor directory
    let vendor = dir.path().join("vendor");
    std::fs::create_dir_all(&vendor).unwrap();
    std::fs::write(vendor.join("Vendored.php"), "<?php\nclass Vendored {}").unwrap();

    // node_modules (listed in .ignore so the ignore-crate walker skips it)
    let nm = dir.path().join("node_modules");
    std::fs::create_dir_all(&nm).unwrap();
    std::fs::write(nm.join("Fake.php"), "<?php\nclass Fake {}").unwrap();

    // .ignore file — the `ignore` crate always respects these without
    // requiring a git repository to be initialised.
    std::fs::write(dir.path().join(".ignore"), "node_modules/\n").unwrap();

    let vendor_dir_paths = vec![dir.path().join("vendor")];
    let classmap = classmap_scanner::scan_workspace_fallback(dir.path(), &vendor_dir_paths);
    assert!(classmap.contains_key("Visible"));
    assert!(!classmap.contains_key("Secret"));
    assert!(!classmap.contains_key("Vendored"));
    assert!(!classmap.contains_key("Fake"));
}

#[test]
fn scan_workspace_fallback_recurses_into_subdirectories() {
    let dir = tempfile::tempdir().unwrap();
    let deep = dir.path().join("a").join("b").join("c");
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(
        deep.join("Deep.php"),
        "<?php\nnamespace A\\B\\C;\nclass Deep {}",
    )
    .unwrap();

    let vendor_dir_paths = vec![dir.path().join("vendor")];
    let classmap = classmap_scanner::scan_workspace_fallback(dir.path(), &vendor_dir_paths);
    assert!(classmap.contains_key("A\\B\\C\\Deep"));
}

// ─── Cross-file resolution via self-scanned classmap ───────────────────────

/// When we manually populate the classmap (simulating what the self-scan
/// produces), cross-file resolution should work: completing on a variable
/// whose type is defined in another file should yield that class's members.
#[tokio::test]
async fn self_scan_classmap_enables_cross_file_completion() {
    // Set up a workspace with two files: one defines a class, the other uses it.
    let (backend, dir) = create_psr4_workspace(
        r#"{"autoload": {"psr-4": {"App\\": "src/"}}}"#,
        &[
            (
                "src/Models/Product.php",
                r#"<?php
namespace App\Models;

class Product {
    public function getName(): string { return ''; }
    public function getPrice(): float { return 0.0; }
}
"#,
            ),
            (
                "src/Service.php",
                r#"<?php
namespace App;

use App\Models\Product;

class Service {
    public function handle(): void {
        $product = new Product();
        $product->
    }
}
"#,
            ),
        ],
    );

    let service_uri = Url::from_file_path(dir.path().join("src/Service.php")).unwrap();
    let service_content = std::fs::read_to_string(dir.path().join("src/Service.php")).unwrap();

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: service_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: service_content,
            },
        })
        .await;

    // Trigger completion at `$product->`
    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: service_uri.clone(),
                },
                position: Position {
                    line: 8,
                    character: 19,
                },
            },
            work_done_progress_params: WorkDoneProgressParams {
                work_done_token: None,
            },
            partial_result_params: PartialResultParams {
                partial_result_token: None,
            },
            context: None,
        })
        .await
        .unwrap();

    let items = match result {
        Some(CompletionResponse::List(list)) => list.items,
        Some(CompletionResponse::Array(items)) => items,
        None => vec![],
    };

    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("getName")),
        "Expected 'getName' in completions, got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| l.starts_with("getPrice")),
        "Expected 'getPrice' in completions, got: {:?}",
        labels
    );
}

/// When the classmap is populated from self-scanning, classes should be
/// resolvable even without running `composer dump-autoload -o`.
#[tokio::test]
async fn self_scan_classmap_populates_backend_classmap() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src").join("Models");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("User.php"),
        "<?php\nnamespace App\\Models;\nclass User {\n    public string $email;\n}\n",
    )
    .unwrap();

    // Build classmap via self-scan
    let classmap = classmap_scanner::scan_psr4_directories(
        &[("App\\".to_string(), dir.path().join("src"))],
        &[],
        &[],
    );

    assert!(classmap.contains_key("App\\Models\\User"));
    let user_path = classmap.get("App\\Models\\User").unwrap();
    assert!(user_path.ends_with("User.php"));

    // Now set up a backend with this classmap and verify resolution
    let mappings = vec![phpantom_lsp::composer::Psr4Mapping {
        prefix: "App\\".to_string(),
        base_path: "src/".to_string(),
    }];
    let backend =
        phpantom_lsp::Backend::new_test_with_workspace(dir.path().to_path_buf(), mappings);

    // Inject the self-scanned classmap into the backend
    {
        let mut idx = backend.fqn_uri_index().write();
        for (fqn, path) in &classmap {
            idx.insert(fqn.clone(), Url::from_file_path(path).unwrap().to_string());
        }
    }

    let test_file = r#"<?php
namespace App;
use App\Models\User;
class Test {
    public function run(): void {
        $u = new User();
        $u->
    }
}
"#;

    let uri = Url::parse("file:///test.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: test_file.to_string(),
            },
        })
        .await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 6,
                    character: 13,
                },
            },
            work_done_progress_params: WorkDoneProgressParams {
                work_done_token: None,
            },
            partial_result_params: PartialResultParams {
                partial_result_token: None,
            },
            context: None,
        })
        .await
        .unwrap();

    let items = match result {
        Some(CompletionResponse::List(list)) => list.items,
        Some(CompletionResponse::Array(items)) => items,
        None => vec![],
    };

    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"email"),
        "Expected 'email' property in completions, got: {:?}",
        labels
    );
}

// ─── Edge cases in the scanner ─────────────────────────────────────────────

#[test]
fn scanner_handles_php_with_html_template() {
    // PHP files that mix HTML and PHP (common in legacy projects)
    let content = br#"<html>
<body>
<?php
class LegacyPage {
    public function render(): string { return ''; }
}
?>
</body>
</html>"#;
    let classes = classmap_scanner::find_classes(content);
    assert_eq!(classes, vec!["LegacyPage"]);
}

#[test]
fn scanner_handles_multiple_php_blocks() {
    let content = br"<?php
namespace App;
class First {}
?>
Some HTML
<?php
class Second {}
";
    // Both classes should be in the App namespace since the second <?php
    // block inherits the namespace from the first (this is how PHP works
    // in practice, though the scanner just tracks the last namespace seen).
    let classes = classmap_scanner::find_classes(content);
    assert_eq!(classes, vec!["App\\First", "App\\Second"]);
}

#[test]
fn scanner_handles_class_with_complex_body() {
    let content = br#"<?php
namespace App\Services;

class UserService {
    private const TABLE = 'users';

    public function __construct(
        private readonly \PDO $db,
        private string $prefix = 'app_',
    ) {}

    public function find(int $id): ?array {
        $sql = "SELECT * FROM {$this->prefix}" . self::TABLE . " WHERE id = :id";
        // class keyword in a comment
        $stmt = $this->db->prepare($sql);
        $stmt->execute(['id' => $id]);
        return $stmt->fetch(\PDO::FETCH_ASSOC) ?: null;
    }
}
"#;
    let classes = classmap_scanner::find_classes(content);
    assert_eq!(classes, vec!["App\\Services\\UserService"]);
}

#[test]
fn scanner_handles_enum_with_methods() {
    let content = br"<?php
namespace App\Enums;

enum Color: string {
    case Red = 'red';
    case Green = 'green';
    case Blue = 'blue';

    public function label(): string {
        return match($this) {
            self::Red => 'Red Color',
            self::Green => 'Green Color',
            self::Blue => 'Blue Color',
        };
    }
}
";
    let classes = classmap_scanner::find_classes(content);
    assert_eq!(classes, vec!["App\\Enums\\Color"]);
}

#[test]
fn scanner_handles_abstract_and_final_classes() {
    let content = br"<?php
namespace App;

abstract class Base {}
final class Concrete extends Base {}
";
    let classes = classmap_scanner::find_classes(content);
    assert_eq!(classes, vec!["App\\Base", "App\\Concrete"]);
}

#[test]
fn scanner_handles_readonly_class() {
    // PHP 8.2 readonly classes
    let content = br"<?php
namespace App\DTO;

readonly class Point {
    public function __construct(
        public float $x,
        public float $y,
    ) {}
}
";
    let classes = classmap_scanner::find_classes(content);
    assert_eq!(classes, vec!["App\\DTO\\Point"]);
}

#[test]
fn scanner_handles_braced_namespace() {
    let content = br"<?php
namespace App\Models {
    class User {}
}
namespace App\Services {
    class UserService {}
}
";
    let classes = classmap_scanner::find_classes(content);
    assert_eq!(
        classes,
        vec!["App\\Models\\User", "App\\Services\\UserService"]
    );
}

#[test]
fn first_class_wins_in_classmap() {
    // When two files define the same FQN, the first one scanned should win.
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("A.php"), "<?php\nclass Dup {}").unwrap();
    std::fs::write(src.join("B.php"), "<?php\nclass Dup {}").unwrap();

    let classmap = classmap_scanner::scan_directories(&[src], &[]);
    // Should have exactly one entry
    assert_eq!(classmap.len(), 1);
    assert!(classmap.contains_key("Dup"));
}

#[test]
fn scan_directories_ignores_non_php_files() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("Real.php"), "<?php\nclass Real {}").unwrap();
    std::fs::write(src.join("README.md"), "# class Fake").unwrap();
    std::fs::write(src.join("style.css"), ".class { }").unwrap();
    std::fs::write(src.join("data.json"), r#"{"class": "Fake"}"#).unwrap();

    let classmap = classmap_scanner::scan_directories(&[src], &[]);
    assert_eq!(classmap.len(), 1);
    assert!(classmap.contains_key("Real"));
}

// ─── Config strategy tests ─────────────────────────────────────────────────

#[test]
fn config_strategy_defaults_to_full() {
    use phpantom_lsp::config::{Config, IndexingStrategy};
    let config: Config = toml::from_str("").unwrap();
    assert_eq!(config.indexing.strategy(), IndexingStrategy::Full);
}

#[test]
fn config_strategy_self_scan() {
    use phpantom_lsp::config::{Config, IndexingStrategy};
    let config: Config = toml::from_str("[indexing]\nstrategy = \"self\"\n").unwrap();
    assert_eq!(config.indexing.strategy, Some(IndexingStrategy::SelfScan));
}

#[test]
fn config_strategy_none() {
    use phpantom_lsp::config::{Config, IndexingStrategy};
    let config: Config = toml::from_str("[indexing]\nstrategy = \"none\"\n").unwrap();
    assert_eq!(config.indexing.strategy, Some(IndexingStrategy::None));
}

#[test]
fn config_strategy_full() {
    use phpantom_lsp::config::{Config, IndexingStrategy};
    let config: Config = toml::from_str("[indexing]\nstrategy = \"full\"\n").unwrap();
    assert_eq!(config.indexing.strategy, Some(IndexingStrategy::Full));
}

#[test]
fn config_invalid_strategy_errors() {
    use phpantom_lsp::config::Config;
    let result = toml::from_str::<Config>("[indexing]\nstrategy = \"invalid\"\n");
    assert!(result.is_err());
}

// ─── Vendor package edge cases ─────────────────────────────────────────────

#[test]
fn scan_vendor_packages_with_classmap_file_entry() {
    let dir = tempfile::tempdir().unwrap();
    let vendor = dir.path().join("vendor");
    let composer_dir = vendor.join("composer");
    std::fs::create_dir_all(&composer_dir).unwrap();

    // Package that references a single file, not a directory
    let pkg = vendor.join("legacy").join("lib");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("functions.php"),
        "<?php\nclass LegacyGlobal {}\nclass AnotherGlobal {}",
    )
    .unwrap();

    let installed = serde_json::json!({
        "packages": [
            {
                "name": "legacy/lib",
                "install-path": "../legacy/lib",
                "autoload": {
                    "classmap": ["functions.php"]
                }
            }
        ]
    });
    std::fs::write(
        composer_dir.join("installed.json"),
        serde_json::to_string(&installed).unwrap(),
    )
    .unwrap();

    let result = classmap_scanner::scan_vendor_packages(dir.path(), "vendor");
    let classmap = result.classmap;
    assert!(
        classmap.contains_key("LegacyGlobal"),
        "keys: {:?}",
        classmap.keys().collect::<Vec<_>>()
    );
    assert!(classmap.contains_key("AnotherGlobal"));
}

#[test]
fn scan_vendor_packages_skips_missing_package_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let vendor = dir.path().join("vendor");
    let composer_dir = vendor.join("composer");
    std::fs::create_dir_all(&composer_dir).unwrap();

    // Package listed in installed.json but directory doesn't exist
    let installed = serde_json::json!({
        "packages": [
            {
                "name": "ghost/package",
                "install-path": "../ghost/package",
                "autoload": {
                    "psr-4": {
                        "Ghost\\": "src/"
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

    // Should not panic, just return empty
    let result = classmap_scanner::scan_vendor_packages(dir.path(), "vendor");
    assert!(result.classmap.is_empty());
}

#[test]
fn scan_vendor_packages_skips_packages_without_autoload() {
    let dir = tempfile::tempdir().unwrap();
    let vendor = dir.path().join("vendor");
    let composer_dir = vendor.join("composer");
    std::fs::create_dir_all(&composer_dir).unwrap();

    let installed = serde_json::json!({
        "packages": [
            {
                "name": "some/meta-package"
            }
        ]
    });
    std::fs::write(
        composer_dir.join("installed.json"),
        serde_json::to_string(&installed).unwrap(),
    )
    .unwrap();

    let result = classmap_scanner::scan_vendor_packages(dir.path(), "vendor");
    assert!(result.classmap.is_empty());
}

// ─── Custom vendor dir name ────────────────────────────────────────────────

#[test]
fn scan_workspace_fallback_respects_custom_vendor_dir() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(dir.path().join("App.php"), "<?php\nclass App {}").unwrap();

    // Custom vendor dir named "libs"
    let custom_vendor = dir.path().join("libs");
    std::fs::create_dir_all(&custom_vendor).unwrap();
    std::fs::write(
        custom_vendor.join("Vendored.php"),
        "<?php\nclass Vendored {}",
    )
    .unwrap();

    let vendor_dir_paths = vec![dir.path().join("libs")];
    let classmap = classmap_scanner::scan_workspace_fallback(dir.path(), &vendor_dir_paths);
    assert!(classmap.contains_key("App"));
    assert!(
        !classmap.contains_key("Vendored"),
        "Custom vendor dir should be excluded"
    );
}

// ─── Large-ish realistic scenario ──────────────────────────────────────────

#[test]
fn scan_realistic_laravel_like_structure() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // app/ directory (PSR-4: App\ => app/)
    let dirs = &["app/Models", "app/Http/Controllers", "app/Services"];
    for d in dirs {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }

    std::fs::write(
        root.join("app/Models/User.php"),
        "<?php\nnamespace App\\Models;\nclass User {}",
    )
    .unwrap();
    std::fs::write(
        root.join("app/Models/Post.php"),
        "<?php\nnamespace App\\Models;\nclass Post {}",
    )
    .unwrap();
    std::fs::write(
        root.join("app/Http/Controllers/UserController.php"),
        "<?php\nnamespace App\\Http\\Controllers;\nclass UserController {}",
    )
    .unwrap();
    std::fs::write(
        root.join("app/Services/AuthService.php"),
        "<?php\nnamespace App\\Services;\nclass AuthService {}",
    )
    .unwrap();

    // database/ directory (classmap autoload)
    std::fs::create_dir_all(root.join("database/seeders")).unwrap();
    std::fs::write(
        root.join("database/seeders/DatabaseSeeder.php"),
        "<?php\nnamespace Database\\Seeders;\nclass DatabaseSeeder {}",
    )
    .unwrap();

    let classmap = classmap_scanner::scan_psr4_directories(
        &[("App\\".to_string(), root.join("app"))],
        &[root.join("database")],
        &[],
    );

    assert_eq!(classmap.len(), 5);
    assert!(classmap.contains_key("App\\Models\\User"));
    assert!(classmap.contains_key("App\\Models\\Post"));
    assert!(classmap.contains_key("App\\Http\\Controllers\\UserController"));
    assert!(classmap.contains_key("App\\Services\\AuthService"));
    assert!(classmap.contains_key("Database\\Seeders\\DatabaseSeeder"));
}
