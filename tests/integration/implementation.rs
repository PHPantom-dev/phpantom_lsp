use crate::common::{create_psr4_workspace, create_test_backend};
use phpantom_lsp::Backend;
use phpantom_lsp::composer::parse_autoload_classmap;
use std::fs;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::request::{GotoImplementationParams, GotoImplementationResponse};
use tower_lsp::lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

async fn implementation_at(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    line: u32,
    character: u32,
) -> Vec<Location> {
    let params = GotoImplementationParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    match backend.goto_implementation(params).await.unwrap() {
        Some(GotoImplementationResponse::Scalar(loc)) => vec![loc],
        Some(GotoImplementationResponse::Array(locs)) => locs,
        Some(GotoImplementationResponse::Link(links)) => links
            .into_iter()
            .map(|l| Location {
                uri: l.target_uri,
                range: l.target_selection_range,
            })
            .collect(),
        None => vec![],
    }
}

async fn open(backend: &phpantom_lsp::Backend, uri: &Url, text: &str) {
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;
}

// ─── Interface name → implementing classes ──────────────────────────────────

/// Cursor on an interface name → jumps to all classes that implement it.
#[tokio::test]
async fn test_implementation_interface_name() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///impl_iface.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "interface Renderable {\n",                              // 1
        "    public function render(): string;\n",               // 2
        "}\n",                                                   // 3
        "class HtmlView implements Renderable {\n",              // 4
        "    public function render(): string { return ''; }\n", // 5
        "}\n",                                                   // 6
        "class JsonView implements Renderable {\n",              // 7
        "    public function render(): string { return ''; }\n", // 8
        "}\n",                                                   // 9
        "class PlainClass {\n",                                  // 10
        "    public function render(): string { return ''; }\n", // 11
        "}\n",                                                   // 12
    );

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;

    // Cursor on "Renderable" on line 1 (the interface declaration)
    let locations = implementation_at(&backend, &uri, 1, 12).await;

    assert!(
        locations.len() >= 2,
        "Should find at least 2 implementors of Renderable, got {}",
        locations.len()
    );

    let lines: Vec<u32> = locations.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&4),
        "Should include HtmlView (line 4), got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&7),
        "Should include JsonView (line 7), got lines: {:?}",
        lines
    );
    // PlainClass does NOT implement Renderable, so it should NOT be included.
    assert!(
        !lines.contains(&10),
        "Should NOT include PlainClass (line 10), got lines: {:?}",
        lines
    );
}

// ─── Abstract class name → extending classes ────────────────────────────────

/// Cursor on an abstract class name → jumps to concrete subclasses.
#[tokio::test]
async fn test_implementation_abstract_class_name() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///impl_abstract.php").unwrap();
    let text = concat!(
        "<?php\n",                                              // 0
        "abstract class Shape {\n",                             // 1
        "    abstract public function area(): float;\n",        // 2
        "}\n",                                                  // 3
        "class Circle extends Shape {\n",                       // 4
        "    public function area(): float { return 3.14; }\n", // 5
        "}\n",                                                  // 6
        "class Square extends Shape {\n",                       // 7
        "    public function area(): float { return 1.0; }\n",  // 8
        "}\n",                                                  // 9
    );

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;

    // Cursor on "Shape" on line 1
    let locations = implementation_at(&backend, &uri, 1, 18).await;

    assert!(
        locations.len() >= 2,
        "Should find at least 2 subclasses of Shape, got {}",
        locations.len()
    );

    let lines: Vec<u32> = locations.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&4),
        "Should include Circle (line 4), got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&7),
        "Should include Square (line 7), got lines: {:?}",
        lines
    );
}

// ─── Method call on interface → concrete method implementations ─────────────

/// Cursor on a method call where the variable is typed as an interface →
/// jumps to the concrete method implementations.
#[tokio::test]
async fn test_implementation_method_on_interface() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///impl_method.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "interface Renderable {\n",                              // 1
        "    public function render(): string;\n",               // 2
        "}\n",                                                   // 3
        "class HtmlView implements Renderable {\n",              // 4
        "    public function render(): string { return ''; }\n", // 5
        "}\n",                                                   // 6
        "class JsonView implements Renderable {\n",              // 7
        "    public function render(): string { return ''; }\n", // 8
        "}\n",                                                   // 9
        "class Service {\n",                                     // 10
        "    public function handle(Renderable $view) {\n",      // 11
        "        $view->render();\n",                            // 12
        "    }\n",                                               // 13
        "}\n",                                                   // 14
    );

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;

    // Cursor on "render" on line 12 (`$view->render()`)
    let locations = implementation_at(&backend, &uri, 12, 16).await;

    assert!(
        locations.len() >= 2,
        "Should find at least 2 implementations of render(), got {}",
        locations.len()
    );

    let lines: Vec<u32> = locations.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&5),
        "Should include HtmlView::render() (line 5), got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&8),
        "Should include JsonView::render() (line 8), got lines: {:?}",
        lines
    );
}

// ─── Method call on abstract class → concrete method implementations ────────

/// Cursor on a method call where the variable is typed as an abstract class →
/// jumps to the concrete overrides.
#[tokio::test]
async fn test_implementation_method_on_abstract_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///impl_abstract_method.php").unwrap();
    let text = concat!(
        "<?php\n",                                              // 0
        "abstract class Shape {\n",                             // 1
        "    abstract public function area(): float;\n",        // 2
        "}\n",                                                  // 3
        "class Circle extends Shape {\n",                       // 4
        "    public function area(): float { return 3.14; }\n", // 5
        "}\n",                                                  // 6
        "class Square extends Shape {\n",                       // 7
        "    public function area(): float { return 1.0; }\n",  // 8
        "}\n",                                                  // 9
        "function calc(Shape $s) {\n",                          // 10
        "    $s->area();\n",                                    // 11
        "}\n",                                                  // 12
    );

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;

    // Cursor on "area" on line 11 (`$s->area()`)
    let locations = implementation_at(&backend, &uri, 11, 10).await;

    assert!(
        locations.len() >= 2,
        "Should find at least 2 implementations of area(), got {}",
        locations.len()
    );

    let lines: Vec<u32> = locations.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&5),
        "Should include Circle::area() (line 5), got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&8),
        "Should include Square::area() (line 8), got lines: {:?}",
        lines
    );
}

// ─── Concrete class → subclasses ────────────────────────────────────────────

/// Cursor on a non-final concrete class name → should return subclasses.
#[tokio::test]
async fn test_implementation_concrete_class_returns_subclasses() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///impl_concrete.php").unwrap();
    let text = concat!(
        "<?php\n",                      // 0
        "class User {\n",               // 1
        "    public string $name;\n",   // 2
        "}\n",                          // 3
        "class Admin extends User {\n", // 4
        "    public string $role;\n",   // 5
        "}\n",                          // 6
    );

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;

    // Cursor on "User" on line 1 — User is concrete but not final,
    // so Admin (which extends it) should be returned.
    let locations = implementation_at(&backend, &uri, 1, 7).await;
    let lines: Vec<u32> = locations.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&4),
        "Should include Admin (line 4), got lines: {:?}",
        lines
    );
}

/// Cursor on a final class name → should NOT return implementations.
#[tokio::test]
async fn test_implementation_final_class_returns_none() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///impl_final.php").unwrap();
    let text = concat!(
        "<?php\n",                   // 0
        "final class Singleton {\n", // 1
        "    public string $id;\n",  // 2
        "}\n",                       // 3
    );

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;

    // Cursor on "Singleton" on line 1 — Singleton is final, so no implementations.
    let locations = implementation_at(&backend, &uri, 1, 14).await;
    assert!(
        locations.is_empty(),
        "Final class should not return implementations, got {:?}",
        locations
    );
}

/// Concrete class with abstract subclass → the abstract subclass is included
/// because we are exploring the class hierarchy (not looking for instantiable
/// implementations of an interface).
#[tokio::test]
async fn test_implementation_concrete_class_includes_abstract_subclass() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///impl_concrete_abstract.php").unwrap();
    let text = concat!(
        "<?php\n",                                // 0
        "class Base {\n",                         // 1
        "    public function run(): void {}\n",   // 2
        "}\n",                                    // 3
        "abstract class Middle extends Base {\n", // 4
        "}\n",                                    // 5
        "class Leaf extends Middle {\n",          // 6
        "}\n",                                    // 7
    );

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;

    // Cursor on "Base" on line 1 — Base is concrete, so both Middle
    // (abstract) and Leaf (concrete) should be included.
    let locations = implementation_at(&backend, &uri, 1, 7).await;
    let lines: Vec<u32> = locations.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&4),
        "Should include abstract Middle (line 4), got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&6),
        "Should include concrete Leaf (line 6), got lines: {:?}",
        lines
    );
}

// ─── Transitive implements (class extends class that implements iface) ──────

/// A class that extends another class which implements the interface should
/// be included as an implementor.
#[tokio::test]
async fn test_implementation_transitive_via_parent() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///impl_transitive.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "interface Renderable {\n",                              // 1
        "    public function render(): string;\n",               // 2
        "}\n",                                                   // 3
        "class BaseView implements Renderable {\n",              // 4
        "    public function render(): string { return ''; }\n", // 5
        "}\n",                                                   // 6
        "class AdminView extends BaseView {\n",                  // 7
        "    public function render(): string { return ''; }\n", // 8
        "}\n",                                                   // 9
    );

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;

    // Cursor on "Renderable" on line 1
    let locations = implementation_at(&backend, &uri, 1, 12).await;

    assert!(
        locations.len() >= 2,
        "Should find at least 2 implementors (direct + transitive), got {}",
        locations.len()
    );

    let lines: Vec<u32> = locations.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&4),
        "Should include BaseView (line 4), got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&7),
        "Should include AdminView (line 7, transitive), got lines: {:?}",
        lines
    );
}

// ─── Multiple interfaces ────────────────────────────────────────────────────

/// A class implementing multiple interfaces should appear when querying
/// any of the interfaces.
#[tokio::test]
async fn test_implementation_multiple_interfaces() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///impl_multi.php").unwrap();
    let text = concat!(
        "<?php\n",                                                  // 0
        "interface Serializable {\n",                               // 1
        "    public function serialize(): string;\n",               // 2
        "}\n",                                                      // 3
        "interface Printable {\n",                                  // 4
        "    public function print(): void;\n",                     // 5
        "}\n",                                                      // 6
        "class Report implements Serializable, Printable {\n",      // 7
        "    public function serialize(): string { return ''; }\n", // 8
        "    public function print(): void {}\n",                   // 9
        "}\n",                                                      // 10
    );

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;

    // Cursor on "Serializable" on line 1
    let locs_serial = implementation_at(&backend, &uri, 1, 12).await;
    assert!(
        !locs_serial.is_empty(),
        "Serializable should have at least one implementor"
    );
    let serial_lines: Vec<u32> = locs_serial.iter().map(|l| l.range.start.line).collect();
    assert!(
        serial_lines.contains(&7),
        "Report should implement Serializable, got lines: {:?}",
        serial_lines
    );

    // Cursor on "Printable" on line 4
    let locs_print = implementation_at(&backend, &uri, 4, 12).await;
    assert!(
        !locs_print.is_empty(),
        "Printable should have at least one implementor"
    );
    let print_lines: Vec<u32> = locs_print.iter().map(|l| l.range.start.line).collect();
    assert!(
        print_lines.contains(&7),
        "Report should implement Printable, got lines: {:?}",
        print_lines
    );
}

// ─── Enum implements interface ──────────────────────────────────────────────

/// Enums can implement interfaces — they should show up as implementors.
#[tokio::test]
async fn test_implementation_enum_implements_interface() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///impl_enum.php").unwrap();
    let text = concat!(
        "<?php\n",                                    // 0
        "interface HasLabel {\n",                     // 1
        "    public function label(): string;\n",     // 2
        "}\n",                                        // 3
        "enum Color: string implements HasLabel {\n", // 4
        "    case Red = 'red';\n",                    // 5
        "    public function label(): string {\n",    // 6
        "        return $this->value;\n",             // 7
        "    }\n",                                    // 8
        "}\n",                                        // 9
    );

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;

    // Cursor on "HasLabel" on line 1
    let locations = implementation_at(&backend, &uri, 1, 12).await;

    assert!(
        !locations.is_empty(),
        "HasLabel should have at least one implementor (Color enum)"
    );

    let lines: Vec<u32> = locations.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&4),
        "Should include Color enum (line 4), got lines: {:?}",
        lines
    );
}

// ─── Cross-file PSR-4 implementation ────────────────────────────────────────

/// Go-to-implementation works across files via PSR-4 autoloading.
#[tokio::test]
async fn test_implementation_cross_file_psr4() {
    let composer = r#"{
        "autoload": {
            "psr-4": {
                "App\\": "src/"
            }
        }
    }"#;

    let interface_php = concat!(
        "<?php\n",
        "namespace App\\Contracts;\n",
        "interface Logger {\n",
        "    public function log(string $msg): void;\n",
        "}\n",
    );

    let file_logger_php = concat!(
        "<?php\n",
        "namespace App\\Logging;\n",
        "use App\\Contracts\\Logger;\n",
        "class FileLogger implements Logger {\n",
        "    public function log(string $msg): void {}\n",
        "}\n",
    );

    let db_logger_php = concat!(
        "<?php\n",
        "namespace App\\Logging;\n",
        "use App\\Contracts\\Logger;\n",
        "class DbLogger implements Logger {\n",
        "    public function log(string $msg): void {}\n",
        "}\n",
    );

    let service_php = concat!(
        "<?php\n",
        "namespace App\\Services;\n",
        "use App\\Contracts\\Logger;\n",
        "class AppService {\n",
        "    public function run(Logger $logger) {\n",
        "        $logger->log('hello');\n",
        "    }\n",
        "}\n",
    );

    let (backend, _dir) = create_psr4_workspace(
        composer,
        &[
            ("src/Contracts/Logger.php", interface_php),
            ("src/Logging/FileLogger.php", file_logger_php),
            ("src/Logging/DbLogger.php", db_logger_php),
            ("src/Services/AppService.php", service_php),
        ],
    );

    // Open all files so they are in the ast_map.
    let iface_uri = Url::parse("file:///logger_iface.php").unwrap();
    open(&backend, &iface_uri, interface_php).await;

    let file_logger_uri = Url::parse("file:///file_logger.php").unwrap();
    open(&backend, &file_logger_uri, file_logger_php).await;

    let db_logger_uri = Url::parse("file:///db_logger.php").unwrap();
    open(&backend, &db_logger_uri, db_logger_php).await;

    let service_uri = Url::parse("file:///service.php").unwrap();
    open(&backend, &service_uri, service_php).await;

    // Cursor on "Logger" on line 2 of interface file (interface Logger)
    let locations = implementation_at(&backend, &iface_uri, 2, 12).await;

    assert!(
        locations.len() >= 2,
        "Should find at least 2 implementors of Logger across files, got {}",
        locations.len()
    );
}

// ─── Method on interface across files ───────────────────────────────────────

/// Method implementations across files — cursor on `$logger->log()`.
#[tokio::test]
async fn test_implementation_method_cross_file() {
    let composer = r#"{
        "autoload": {
            "psr-4": {
                "App\\": "src/"
            }
        }
    }"#;

    let interface_php = concat!(
        "<?php\n",
        "namespace App\\Contracts;\n",
        "interface Formatter {\n",
        "    public function format(string $data): string;\n",
        "}\n",
    );

    let html_formatter_php = concat!(
        "<?php\n",
        "namespace App\\Formatters;\n",
        "use App\\Contracts\\Formatter;\n",
        "class HtmlFormatter implements Formatter {\n",
        "    public function format(string $data): string { return $data; }\n",
        "}\n",
    );

    let json_formatter_php = concat!(
        "<?php\n",
        "namespace App\\Formatters;\n",
        "use App\\Contracts\\Formatter;\n",
        "class JsonFormatter implements Formatter {\n",
        "    public function format(string $data): string { return $data; }\n",
        "}\n",
    );

    let service_php = concat!(
        "<?php\n",                                      // 0
        "namespace App\\Services;\n",                   // 1
        "use App\\Contracts\\Formatter;\n",             // 2
        "class RenderService {\n",                      // 3
        "    public function render(Formatter $f) {\n", // 4
        "        $f->format('hello');\n",               // 5
        "    }\n",                                      // 6
        "}\n",                                          // 7
    );

    let (backend, _dir) = create_psr4_workspace(
        composer,
        &[
            ("src/Contracts/Formatter.php", interface_php),
            ("src/Formatters/HtmlFormatter.php", html_formatter_php),
            ("src/Formatters/JsonFormatter.php", json_formatter_php),
            ("src/Services/RenderService.php", service_php),
        ],
    );

    // Open all files
    let iface_uri = Url::parse("file:///formatter_iface.php").unwrap();
    open(&backend, &iface_uri, interface_php).await;

    let html_uri = Url::parse("file:///html_formatter.php").unwrap();
    open(&backend, &html_uri, html_formatter_php).await;

    let json_uri = Url::parse("file:///json_formatter.php").unwrap();
    open(&backend, &json_uri, json_formatter_php).await;

    let service_uri = Url::parse("file:///render_service.php").unwrap();
    open(&backend, &service_uri, service_php).await;

    // Cursor on "format" on line 5 of service file (`$f->format('hello')`)
    let locations = implementation_at(&backend, &service_uri, 5, 14).await;

    assert!(
        locations.len() >= 2,
        "Should find at least 2 implementations of format() across files, got {}",
        locations.len()
    );
}

// ─── Interface with no implementors ─────────────────────────────────────────

/// An interface with no implementing classes should return no locations.
#[tokio::test]
async fn test_implementation_no_implementors() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///impl_none.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface Cacheable {\n",
        "    public function cache(): void;\n",
        "}\n",
    );

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;

    let locations = implementation_at(&backend, &uri, 1, 12).await;
    assert!(
        locations.is_empty(),
        "Interface with no implementors should return empty, got {:?}",
        locations
    );
}

// ─── Does not crash on variable ─────────────────────────────────────────────

/// Invoking go-to-implementation on a variable should not crash.
#[tokio::test]
async fn test_implementation_on_variable_no_crash() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///impl_var.php").unwrap();
    let text = concat!("<?php\n", "$x = 42;\n", "$x;\n",);

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;

    // Should not panic, just return empty.
    let locations = implementation_at(&backend, &uri, 2, 1).await;
    let _ = locations;
}

// ─── Abstract class: only concrete subclasses included ──────────────────────

/// Abstract subclasses should NOT be included — only concrete ones.
#[tokio::test]
async fn test_implementation_skips_abstract_subclasses() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///impl_skip_abstract.php").unwrap();
    let text = concat!(
        "<?php\n",                                                  // 0
        "abstract class Animal {\n",                                // 1
        "    abstract public function speak(): string;\n",          // 2
        "}\n",                                                      // 3
        "abstract class Pet extends Animal {\n",                    // 4
        "}\n",                                                      // 5
        "class Dog extends Pet {\n",                                // 6
        "    public function speak(): string { return 'woof'; }\n", // 7
        "}\n",                                                      // 8
        "class Cat extends Pet {\n",                                // 9
        "    public function speak(): string { return 'meow'; }\n", // 10
        "}\n",                                                      // 11
    );

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;

    // Cursor on "Animal" on line 1
    let locations = implementation_at(&backend, &uri, 1, 18).await;

    let lines: Vec<u32> = locations.iter().map(|l| l.range.start.line).collect();

    // Pet is abstract, so it should NOT be included.
    assert!(
        !lines.contains(&4),
        "Should NOT include abstract Pet (line 4), got lines: {:?}",
        lines
    );

    // Dog and Cat are concrete — they should be included.
    // Dog extends Pet extends Animal (transitive).
    assert!(
        lines.contains(&6),
        "Should include Dog (line 6), got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&9),
        "Should include Cat (line 9), got lines: {:?}",
        lines
    );
}

// ─── Method on interface: only classes that override the method ──────────────

/// When a class implements an interface but inherits the method from its
/// parent (doesn't override it), it should NOT appear in method-level
/// implementation results.
#[tokio::test]
async fn test_implementation_method_only_overriders() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///impl_override.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "interface Renderable {\n",                              // 1
        "    public function render(): string;\n",               // 2
        "}\n",                                                   // 3
        "class BaseView implements Renderable {\n",              // 4
        "    public function render(): string { return ''; }\n", // 5
        "}\n",                                                   // 6
        "class ChildView extends BaseView {\n",                  // 7
        "    // Does NOT override render()\n",                   // 8
        "}\n",                                                   // 9
        "function show(Renderable $v) {\n",                      // 10
        "    $v->render();\n",                                   // 11
        "}\n",                                                   // 12
    );

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;

    // Cursor on "render" on line 11 (`$v->render()`)
    let locations = implementation_at(&backend, &uri, 11, 10).await;

    let lines: Vec<u32> = locations.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&5),
        "Should include BaseView::render() (line 5), got lines: {:?}",
        lines
    );
    // ChildView does NOT override render(), so it should NOT appear.
    assert!(
        !lines.iter().any(|&l| l == 7 || l == 8),
        "Should NOT include ChildView which doesn't override render(), got lines: {:?}",
        lines
    );
}

// ─── Server capability test ─────────────────────────────────────────────────

/// The server should advertise `implementationProvider` in its capabilities.
#[tokio::test]
async fn test_server_advertises_implementation_capability() {
    let backend = create_test_backend();

    let init_params = InitializeParams {
        root_uri: None,
        capabilities: ClientCapabilities::default(),
        ..InitializeParams::default()
    };

    let result = backend.initialize(init_params).await.unwrap();

    assert!(
        result.capabilities.implementation_provider.is_some(),
        "Server should advertise implementationProvider capability"
    );
}

// ─── Classmap file scanning (Phase 3) ───────────────────────────────────────

/// Implementors that exist on disk and are referenced by the classmap — but
/// have NOT been opened via `did_open` — should be discovered by Phase 3's
/// file-scanning logic.
#[tokio::test]
async fn test_implementation_classmap_file_scan() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::write(
        dir.path().join("composer.json"),
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
    )
    .expect("failed to write composer.json");

    // Interface file
    let interface_php = concat!(
        "<?php\n",
        "namespace App\\Contracts;\n",
        "interface Cacheable {\n",
        "    public function cacheKey(): string;\n",
        "}\n",
    );

    // Two implementors — these will only exist on disk, NOT opened.
    let redis_cache_php = concat!(
        "<?php\n",
        "namespace App\\Cache;\n",
        "use App\\Contracts\\Cacheable;\n",
        "class RedisCache implements Cacheable {\n",
        "    public function cacheKey(): string { return 'redis'; }\n",
        "}\n",
    );

    let file_cache_php = concat!(
        "<?php\n",
        "namespace App\\Cache;\n",
        "use App\\Contracts\\Cacheable;\n",
        "class FileCache implements Cacheable {\n",
        "    public function cacheKey(): string { return 'file'; }\n",
        "}\n",
    );

    // Write files to disk
    let src = dir.path().join("src");
    fs::create_dir_all(src.join("Contracts")).unwrap();
    fs::create_dir_all(src.join("Cache")).unwrap();
    fs::write(src.join("Contracts/Cacheable.php"), interface_php).unwrap();
    fs::write(src.join("Cache/RedisCache.php"), redis_cache_php).unwrap();
    fs::write(src.join("Cache/FileCache.php"), file_cache_php).unwrap();

    // Build classmap pointing to the on-disk files
    let composer_dir = dir.path().join("vendor").join("composer");
    fs::create_dir_all(&composer_dir).unwrap();
    fs::write(
        composer_dir.join("autoload_classmap.php"),
        concat!(
            "<?php\n",
            "$baseDir = dirname(dirname(__DIR__));\n",
            "return array(\n",
            "    'App\\\\Contracts\\\\Cacheable' => $baseDir . '/src/Contracts/Cacheable.php',\n",
            "    'App\\\\Cache\\\\RedisCache' => $baseDir . '/src/Cache/RedisCache.php',\n",
            "    'App\\\\Cache\\\\FileCache' => $baseDir . '/src/Cache/FileCache.php',\n",
            ");\n",
        ),
    )
    .unwrap();

    let classmap = parse_autoload_classmap(dir.path(), "vendor");
    assert_eq!(classmap.len(), 3, "classmap should have 3 entries");

    let (mappings, _vendor_dir) = phpantom_lsp::composer::parse_composer_json(dir.path());
    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), mappings);
    {
        let mut idx = backend.fqn_uri_index().write();
        for (fqn, path) in &classmap {
            idx.insert(fqn.clone(), Url::from_file_path(path).unwrap().to_string());
        }
    }

    // Only open the interface file — implementors stay on disk only.
    let iface_uri = Url::from_file_path(src.join("Contracts/Cacheable.php")).unwrap();
    open(&backend, &iface_uri, interface_php).await;

    // Go-to-implementation on "Cacheable" (line 2, col 12)
    let locations = implementation_at(&backend, &iface_uri, 2, 12).await;

    assert!(
        locations.len() >= 2,
        "Should find at least 2 implementors of Cacheable via classmap scan, got {}",
        locations.len()
    );
}

// ─── PSR-4 directory scanning (Phase 5) ─────────────────────────────────────

/// Implementors that exist on disk under a PSR-4 root but are NOT in the
/// classmap (the user hasn't run `composer dump-autoload -o`) should be
/// discovered by Phase 5's directory-walking logic.
#[tokio::test]
async fn test_implementation_psr4_directory_scan() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::write(
        dir.path().join("composer.json"),
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
    )
    .expect("failed to write composer.json");

    // Interface file
    let interface_php = concat!(
        "<?php\n",
        "namespace App\\Contracts;\n",
        "interface Notifier {\n",
        "    public function send(string $msg): void;\n",
        "}\n",
    );

    // Implementors — only on disk, NOT opened and NOT in classmap.
    let email_notifier_php = concat!(
        "<?php\n",
        "namespace App\\Notifiers;\n",
        "use App\\Contracts\\Notifier;\n",
        "class EmailNotifier implements Notifier {\n",
        "    public function send(string $msg): void {}\n",
        "}\n",
    );

    let sms_notifier_php = concat!(
        "<?php\n",
        "namespace App\\Notifiers;\n",
        "use App\\Contracts\\Notifier;\n",
        "class SmsNotifier implements Notifier {\n",
        "    public function send(string $msg): void {}\n",
        "}\n",
    );

    // Write files to disk
    let src = dir.path().join("src");
    fs::create_dir_all(src.join("Contracts")).unwrap();
    fs::create_dir_all(src.join("Notifiers")).unwrap();
    fs::write(src.join("Contracts/Notifier.php"), interface_php).unwrap();
    fs::write(src.join("Notifiers/EmailNotifier.php"), email_notifier_php).unwrap();
    fs::write(src.join("Notifiers/SmsNotifier.php"), sms_notifier_php).unwrap();

    // NO classmap — simulate a project without `composer dump-autoload -o`.
    let (mappings, _vendor_dir) = phpantom_lsp::composer::parse_composer_json(dir.path());
    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), mappings);

    // Only open the interface file.
    let iface_uri = Url::from_file_path(src.join("Contracts/Notifier.php")).unwrap();
    open(&backend, &iface_uri, interface_php).await;

    // Go-to-implementation on "Notifier" (line 2, col 12)
    let locations = implementation_at(&backend, &iface_uri, 2, 12).await;

    assert!(
        locations.len() >= 2,
        "Should find at least 2 implementors via PSR-4 directory scan, got {}",
        locations.len()
    );
}

/// PSR-4 directory scanning should not re-process files already covered by
/// the classmap (those are handled by Phase 3).
#[tokio::test]
async fn test_implementation_psr4_scan_skips_classmap_files() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::write(
        dir.path().join("composer.json"),
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
    )
    .expect("failed to write composer.json");

    let interface_php = concat!(
        "<?php\n",
        "namespace App\\Contracts;\n",
        "interface Serializable {\n",
        "    public function serialize(): string;\n",
        "}\n",
    );

    // One implementor in classmap, one only on disk under PSR-4.
    let json_impl_php = concat!(
        "<?php\n",
        "namespace App\\Serializers;\n",
        "use App\\Contracts\\Serializable;\n",
        "class JsonSerializer implements Serializable {\n",
        "    public function serialize(): string { return '{}'; }\n",
        "}\n",
    );

    let xml_impl_php = concat!(
        "<?php\n",
        "namespace App\\Serializers;\n",
        "use App\\Contracts\\Serializable;\n",
        "class XmlSerializer implements Serializable {\n",
        "    public function serialize(): string { return '<xml/>'; }\n",
        "}\n",
    );

    let src = dir.path().join("src");
    fs::create_dir_all(src.join("Contracts")).unwrap();
    fs::create_dir_all(src.join("Serializers")).unwrap();
    fs::write(src.join("Contracts/Serializable.php"), interface_php).unwrap();
    fs::write(src.join("Serializers/JsonSerializer.php"), json_impl_php).unwrap();
    fs::write(src.join("Serializers/XmlSerializer.php"), xml_impl_php).unwrap();

    // Classmap only includes JsonSerializer — XmlSerializer is PSR-4 only.
    let composer_dir = dir.path().join("vendor").join("composer");
    fs::create_dir_all(&composer_dir).unwrap();
    fs::write(
        composer_dir.join("autoload_classmap.php"),
        concat!(
            "<?php\n",
            "$baseDir = dirname(dirname(__DIR__));\n",
            "return array(\n",
            "    'App\\\\Serializers\\\\JsonSerializer' => $baseDir . '/src/Serializers/JsonSerializer.php',\n",
            ");\n",
        ),
    )
    .unwrap();

    let classmap = parse_autoload_classmap(dir.path(), "vendor");
    let (mappings, _vendor_dir) = phpantom_lsp::composer::parse_composer_json(dir.path());
    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), mappings);
    {
        let mut idx = backend.fqn_uri_index().write();
        for (fqn, path) in &classmap {
            idx.insert(fqn.clone(), Url::from_file_path(path).unwrap().to_string());
        }
    }

    // Only open the interface.
    let iface_uri = Url::from_file_path(src.join("Contracts/Serializable.php")).unwrap();
    open(&backend, &iface_uri, interface_php).await;

    let locations = implementation_at(&backend, &iface_uri, 2, 12).await;

    // Both should be found: JsonSerializer via classmap (Phase 3),
    // XmlSerializer via PSR-4 walk (Phase 5).
    assert!(
        locations.len() >= 2,
        "Should find both classmap and PSR-4-only implementors, got {}",
        locations.len()
    );
}

/// Phase 5 should also find implementors of abstract classes via PSR-4 scan.
#[tokio::test]
async fn test_implementation_psr4_scan_abstract_class() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::write(
        dir.path().join("composer.json"),
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
    )
    .expect("failed to write composer.json");

    let abstract_php = concat!(
        "<?php\n",
        "namespace App\\Base;\n",
        "abstract class Handler {\n",
        "    abstract public function handle(): void;\n",
        "}\n",
    );

    let concrete_php = concat!(
        "<?php\n",
        "namespace App\\Handlers;\n",
        "use App\\Base\\Handler;\n",
        "class ConcreteHandler extends Handler {\n",
        "    public function handle(): void {}\n",
        "}\n",
    );

    let src = dir.path().join("src");
    fs::create_dir_all(src.join("Base")).unwrap();
    fs::create_dir_all(src.join("Handlers")).unwrap();
    fs::write(src.join("Base/Handler.php"), abstract_php).unwrap();
    fs::write(src.join("Handlers/ConcreteHandler.php"), concrete_php).unwrap();

    let (mappings, _vendor_dir) = phpantom_lsp::composer::parse_composer_json(dir.path());
    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), mappings);

    let iface_uri = Url::from_file_path(src.join("Base/Handler.php")).unwrap();
    open(&backend, &iface_uri, abstract_php).await;

    let locations = implementation_at(&backend, &iface_uri, 2, 18).await;

    assert!(
        !locations.is_empty(),
        "Should find ConcreteHandler via PSR-4 scan, got {}",
        locations.len()
    );
}

/// Method-level go-to-implementation should work when implementors are
/// discovered via classmap or PSR-4 scanning (not pre-opened).
#[tokio::test]
async fn test_implementation_method_via_psr4_scan() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::write(
        dir.path().join("composer.json"),
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
    )
    .expect("failed to write composer.json");

    let interface_php = concat!(
        "<?php\n",
        "namespace App\\Contracts;\n",
        "interface Repository {\n",
        "    public function find(int $id): object;\n",
        "}\n",
    );

    let service_php = concat!(
        "<?php\n",
        "namespace App\\Services;\n",
        "use App\\Contracts\\Repository;\n",
        "class UserService {\n",
        "    public function get(Repository $repo) {\n",
        "        $repo->find(1);\n",
        "    }\n",
        "}\n",
    );

    let impl_php = concat!(
        "<?php\n",
        "namespace App\\Repos;\n",
        "use App\\Contracts\\Repository;\n",
        "class UserRepository implements Repository {\n",
        "    public function find(int $id): object { return (object)[]; }\n",
        "}\n",
    );

    let src = dir.path().join("src");
    fs::create_dir_all(src.join("Contracts")).unwrap();
    fs::create_dir_all(src.join("Services")).unwrap();
    fs::create_dir_all(src.join("Repos")).unwrap();
    fs::write(src.join("Contracts/Repository.php"), interface_php).unwrap();
    fs::write(src.join("Services/UserService.php"), service_php).unwrap();
    fs::write(src.join("Repos/UserRepository.php"), impl_php).unwrap();

    let (mappings, _vendor_dir) = phpantom_lsp::composer::parse_composer_json(dir.path());
    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), mappings);

    // Open the interface and the service file (but NOT the implementor).
    let iface_uri = Url::from_file_path(src.join("Contracts/Repository.php")).unwrap();
    open(&backend, &iface_uri, interface_php).await;

    let svc_uri = Url::from_file_path(src.join("Services/UserService.php")).unwrap();
    open(&backend, &svc_uri, service_php).await;

    // Cursor on "find" in `$repo->find(1);` — line 5, col 16
    let locations = implementation_at(&backend, &svc_uri, 5, 16).await;

    assert!(
        !locations.is_empty(),
        "Should find UserRepository::find via PSR-4 scan"
    );
}

// ─── Transitive interface inheritance ───────────────────────────────────────

/// If InterfaceB extends InterfaceA and ClassC implements InterfaceB,
/// go-to-implementation on InterfaceA should find ClassC.
#[tokio::test]
async fn test_implementation_transitive_interface_extends() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///transitive_iface.php").unwrap();
    let text = concat!(
        "<?php\n",                                     // 0
        "interface InterfaceA {\n",                    // 1
        "    public function doA(): void;\n",          // 2
        "}\n",                                         // 3
        "interface InterfaceB extends InterfaceA {\n", // 4
        "    public function doB(): void;\n",          // 5
        "}\n",                                         // 6
        "class ConcreteC implements InterfaceB {\n",   // 7
        "    public function doA(): void {}\n",        // 8
        "    public function doB(): void {}\n",        // 9
        "}\n",                                         // 10
        "class DirectImpl implements InterfaceA {\n",  // 11
        "    public function doA(): void {}\n",        // 12
        "}\n",                                         // 13
    );

    open(&backend, &uri, text).await;

    // Cursor on "InterfaceA" on line 1
    let locations = implementation_at(&backend, &uri, 1, 12).await;

    let lines: Vec<u32> = locations.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&7),
        "Should find ConcreteC (line 7) via transitive InterfaceB extends InterfaceA, got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&11),
        "Should find DirectImpl (line 11) via direct implements, got lines: {:?}",
        lines
    );
}

/// Three-level interface chain: C extends B extends A, ClassD implements C.
/// Go-to-implementation on A should find ClassD.
#[tokio::test]
async fn test_implementation_deeply_transitive_interface() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///deep_transitive.php").unwrap();
    let text = concat!(
        "<?php\n",                                           // 0
        "interface BaseContract {\n",                        // 1
        "    public function execute(): void;\n",            // 2
        "}\n",                                               // 3
        "interface MiddleContract extends BaseContract {\n", // 4
        "    public function prepare(): void;\n",            // 5
        "}\n",                                               // 6
        "interface LeafContract extends MiddleContract {\n", // 7
        "    public function finalize(): void;\n",           // 8
        "}\n",                                               // 9
        "class Worker implements LeafContract {\n",          // 10
        "    public function execute(): void {}\n",          // 11
        "    public function prepare(): void {}\n",          // 12
        "    public function finalize(): void {}\n",         // 13
        "}\n",                                               // 14
    );

    open(&backend, &uri, text).await;

    // Cursor on "BaseContract" on line 1
    let locations = implementation_at(&backend, &uri, 1, 12).await;

    let lines: Vec<u32> = locations.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&10),
        "Should find Worker (line 10) via deeply transitive interface chain, got lines: {:?}",
        lines
    );
}

/// Interface extends multiple parent interfaces. A class implementing the
/// child should appear when searching for any of the parents.
#[tokio::test]
async fn test_implementation_multi_extends_interface() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///multi_extends.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "interface Readable {\n",                                // 1
        "    public function read(): string;\n",                 // 2
        "}\n",                                                   // 3
        "interface Writable {\n",                                // 4
        "    public function write(string $data): void;\n",      // 5
        "}\n",                                                   // 6
        "interface ReadWritable extends Readable, Writable {\n", // 7
        "}\n",                                                   // 8
        "class FileStream implements ReadWritable {\n",          // 9
        "    public function read(): string { return ''; }\n",   // 10
        "    public function write(string $data): void {}\n",    // 11
        "}\n",                                                   // 12
    );

    open(&backend, &uri, text).await;

    // Go-to-implementation on "Readable" (line 1) should find FileStream.
    let locations_readable = implementation_at(&backend, &uri, 1, 12).await;
    let lines_readable: Vec<u32> = locations_readable
        .iter()
        .map(|l| l.range.start.line)
        .collect();
    assert!(
        lines_readable.contains(&9),
        "Should find FileStream (line 9) via Readable -> ReadWritable, got lines: {:?}",
        lines_readable
    );

    // Go-to-implementation on "Writable" (line 4) should also find FileStream.
    let locations_writable = implementation_at(&backend, &uri, 4, 12).await;
    let lines_writable: Vec<u32> = locations_writable
        .iter()
        .map(|l| l.range.start.line)
        .collect();
    assert!(
        lines_writable.contains(&9),
        "Should find FileStream (line 9) via Writable -> ReadWritable, got lines: {:?}",
        lines_writable
    );
}

/// Transitive interface via parent class chain: ClassB extends ClassA,
/// ClassA implements InterfaceX which extends InterfaceBase.
/// Go-to-implementation on InterfaceBase should find ClassB.
#[tokio::test]
async fn test_implementation_transitive_interface_via_parent_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trans_via_parent.php").unwrap();
    let text = concat!(
        "<?php\n",                                        // 0
        "interface InterfaceBase {\n",                    // 1
        "    public function base(): void;\n",            // 2
        "}\n",                                            // 3
        "interface InterfaceX extends InterfaceBase {\n", // 4
        "    public function extra(): void;\n",           // 5
        "}\n",                                            // 6
        "class ClassA implements InterfaceX {\n",         // 7
        "    public function base(): void {}\n",          // 8
        "    public function extra(): void {}\n",         // 9
        "}\n",                                            // 10
        "class ClassB extends ClassA {\n",                // 11
        "}\n",                                            // 12
    );

    open(&backend, &uri, text).await;

    // Cursor on "InterfaceBase" on line 1
    let locations = implementation_at(&backend, &uri, 1, 12).await;

    let lines: Vec<u32> = locations.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&7),
        "Should find ClassA (line 7) via InterfaceX extends InterfaceBase, got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&11),
        "Should find ClassB (line 11) via parent ClassA -> InterfaceX -> InterfaceBase, got lines: {:?}",
        lines
    );
}

/// Cross-file transitive interface inheritance via PSR-4.
#[tokio::test]
async fn test_implementation_reverse_jump_to_interface_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///impl_reverse.php").unwrap();
    let text = concat!(
        "<?php\n",                                      // 0
        "interface Handler {\n",                        // 1
        "    public function handle(): void;\n",        // 2
        "}\n",                                          // 3
        "class ConcreteHandler implements Handler {\n", // 4
        "    public function handle(): void {}\n",      // 5
        "}\n",                                          // 6
    );

    open(&backend, &uri, text).await;

    // Cursor on "handle" at the declaration site in ConcreteHandler (line 5).
    // "    public function handle(): void {}"
    //                     ^ col 20
    let locations = implementation_at(&backend, &uri, 5, 20).await;

    assert!(
        !locations.is_empty(),
        "Reverse jump should find the interface method declaration"
    );

    // Should point to the interface method on line 2.
    let lines: Vec<u32> = locations.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&2),
        "Should jump to Handler::handle() on line 2, got lines: {:?}",
        lines
    );
}

// ─── Reverse jump: interface method declaration → concrete implementations ──

#[tokio::test]
async fn test_implementation_forward_jump_from_interface_declaration() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///impl_fwd_decl.php").unwrap();
    let text = concat!(
        "<?php\n",                                     // 0
        "interface Processor {\n",                     // 1
        "    public function process(): void;\n",      // 2
        "}\n",                                         // 3
        "class FooProcessor implements Processor {\n", // 4
        "    public function process(): void {}\n",    // 5
        "}\n",                                         // 6
        "class BarProcessor implements Processor {\n", // 7
        "    public function process(): void {}\n",    // 8
        "}\n",                                         // 9
    );

    open(&backend, &uri, text).await;

    // Cursor on "process" at the declaration site in Processor (line 2).
    // "    public function process(): void;"
    //                     ^ col 20
    let locations = implementation_at(&backend, &uri, 2, 20).await;

    assert!(
        locations.len() >= 2,
        "Forward jump from interface declaration should find concrete implementations, got {}",
        locations.len()
    );

    let lines: Vec<u32> = locations.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&5),
        "Should include FooProcessor::process() on line 5, got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&8),
        "Should include BarProcessor::process() on line 8, got lines: {:?}",
        lines
    );
}

// ─── Reverse jump: abstract class method → concrete implementations ─────────

#[tokio::test]
async fn test_implementation_forward_jump_from_abstract_declaration() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///impl_abstract_decl.php").unwrap();
    let text = concat!(
        "<?php\n",                                              // 0
        "abstract class Shape {\n",                             // 1
        "    abstract public function area(): float;\n",        // 2
        "}\n",                                                  // 3
        "class Circle extends Shape {\n",                       // 4
        "    public function area(): float { return 3.14; }\n", // 5
        "}\n",                                                  // 6
        "class Square extends Shape {\n",                       // 7
        "    public function area(): float { return 1.0; }\n",  // 8
        "}\n",                                                  // 9
    );

    open(&backend, &uri, text).await;

    // Cursor on "area" at the declaration site in Shape (line 2).
    // "    abstract public function area(): float;"
    //                              ^ col 29
    let locations = implementation_at(&backend, &uri, 2, 29).await;

    assert!(
        locations.len() >= 2,
        "Forward jump from abstract declaration should find concrete implementations, got {}",
        locations.len()
    );

    let lines: Vec<u32> = locations.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&5),
        "Should include Circle::area() on line 5, got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&8),
        "Should include Square::area() on line 8, got lines: {:?}",
        lines
    );
}

// ─── Reverse jump: method implementing abstract parent ──────────────────────

#[tokio::test]
async fn test_implementation_reverse_jump_to_abstract_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///impl_reverse_abstract.php").unwrap();
    let text = concat!(
        "<?php\n",                                                // 0
        "abstract class Logger {\n",                              // 1
        "    abstract public function log(string $msg): void;\n", // 2
        "}\n",                                                    // 3
        "class FileLogger extends Logger {\n",                    // 4
        "    public function log(string $msg): void {}\n",        // 5
        "}\n",                                                    // 6
    );

    open(&backend, &uri, text).await;

    // Cursor on "log" at the declaration site in FileLogger (line 5).
    // "    public function log(string $msg): void {}"
    //                     ^ col 20
    let locations = implementation_at(&backend, &uri, 5, 20).await;

    assert!(
        !locations.is_empty(),
        "Reverse jump should find the abstract method declaration"
    );

    let lines: Vec<u32> = locations.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&2),
        "Should jump to Logger::log() on line 2, got lines: {:?}",
        lines
    );
}

// ─── Reverse jump: method implementing interface inherited from parent ──────

#[tokio::test]
async fn test_implementation_reverse_jump_transitive_interface() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///impl_reverse_transitive.php").unwrap();
    let text = concat!(
        "<?php\n",                                                  // 0
        "interface Serializable {\n",                               // 1
        "    public function serialize(): string;\n",               // 2
        "}\n",                                                      // 3
        "abstract class BaseModel implements Serializable {\n",     // 4
        "}\n",                                                      // 5
        "class User extends BaseModel {\n",                         // 6
        "    public function serialize(): string { return ''; }\n", // 7
        "}\n",                                                      // 8
    );

    open(&backend, &uri, text).await;

    // Cursor on "serialize" at the declaration site in User (line 7).
    // "    public function serialize(): string { return ''; }"
    //                     ^ col 20
    let locations = implementation_at(&backend, &uri, 7, 20).await;

    assert!(
        !locations.is_empty(),
        "Reverse jump should find the interface method via transitive inheritance"
    );

    let lines: Vec<u32> = locations.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&2),
        "Should jump to Serializable::serialize() on line 2, got lines: {:?}",
        lines
    );
}

// ─── Reverse jump: concrete class with no interface returns none ─────────────

#[tokio::test]
async fn test_implementation_reverse_jump_no_interface_returns_none() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///impl_reverse_none.php").unwrap();
    let text = concat!(
        "<?php\n",                                  // 0
        "class StandaloneClass {\n",                // 1
        "    public function doStuff(): void {}\n", // 2
        "}\n",                                      // 3
    );

    open(&backend, &uri, text).await;

    // Cursor on "doStuff" at the declaration site (line 2).
    // "    public function doStuff(): void {}"
    //                     ^ col 20
    let locations = implementation_at(&backend, &uri, 2, 20).await;

    assert!(
        locations.is_empty(),
        "No interface or abstract parent — should return empty, got {} locations",
        locations.len()
    );
}

// ─── FQN deduplication: same short name in different namespaces ─────────────

#[tokio::test]
async fn test_implementation_fqn_dedup_different_namespaces() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///impl_fqn_dedup.php").unwrap();
    let text = concat!(
        "<?php\n",                                // 0
        "namespace App;\n",                       // 1
        "interface Logger {\n",                   // 2
        "    public function log(): void;\n",     // 3
        "}\n",                                    // 4
        "class FileLogger implements Logger {\n", // 5
        "    public function log(): void {}\n",   // 6
        "}\n",                                    // 7
    );

    let uri2 = Url::parse("file:///impl_fqn_dedup2.php").unwrap();
    let text2 = concat!(
        "<?php\n",                                   // 0
        "namespace Vendor;\n",                       // 1
        "interface Logger {\n",                      // 2
        "    public function log(): void;\n",        // 3
        "}\n",                                       // 4
        "class ConsoleLogger implements Logger {\n", // 5
        "    public function log(): void {}\n",      // 6
        "}\n",                                       // 7
    );

    open(&backend, &uri, text).await;
    open(&backend, &uri2, text2).await;

    // Go-to-implementation on App\Logger (line 2 of file 1).
    let locations = implementation_at(&backend, &uri, 2, 12).await;

    // Should only find FileLogger, not ConsoleLogger (different namespace).
    let uris: Vec<String> = locations.iter().map(|l| l.uri.to_string()).collect();
    assert!(
        uris.iter().all(|u| u.contains("impl_fqn_dedup.php")),
        "App\\Logger should only find implementors from the App namespace, got: {:?}",
        uris
    );

    // The result should include FileLogger.
    assert!(
        !locations.is_empty(),
        "Should find FileLogger as an implementor of App\\Logger"
    );
}

// ─── Transitive interface cross-file ────────────────────────────────────────

#[tokio::test]
async fn test_implementation_transitive_interface_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Contracts/Loggable.php",
                concat!(
                    "<?php\n",
                    "namespace App\\Contracts;\n",
                    "interface Loggable {\n",
                    "    public function log(string $msg): void;\n",
                    "}\n",
                ),
            ),
            (
                "src/Contracts/AuditLoggable.php",
                concat!(
                    "<?php\n",
                    "namespace App\\Contracts;\n",
                    "interface AuditLoggable extends Loggable {\n",
                    "    public function auditLog(string $action): void;\n",
                    "}\n",
                ),
            ),
            (
                "src/Services/AuditService.php",
                concat!(
                    "<?php\n",
                    "namespace App\\Services;\n",
                    "use App\\Contracts\\AuditLoggable;\n",
                    "class AuditService implements AuditLoggable {\n",
                    "    public function log(string $msg): void {}\n",
                    "    public function auditLog(string $action): void {}\n",
                    "}\n",
                ),
            ),
        ],
    );

    let loggable_uri = Url::from_file_path(_dir.path().join("src/Contracts/Loggable.php")).unwrap();
    let loggable_text =
        std::fs::read_to_string(_dir.path().join("src/Contracts/Loggable.php")).unwrap();
    open(&backend, &loggable_uri, &loggable_text).await;

    // Also open the intermediate interface and concrete class so they
    // are in ast_map.
    let audit_loggable_uri =
        Url::from_file_path(_dir.path().join("src/Contracts/AuditLoggable.php")).unwrap();
    let audit_loggable_text =
        std::fs::read_to_string(_dir.path().join("src/Contracts/AuditLoggable.php")).unwrap();
    open(&backend, &audit_loggable_uri, &audit_loggable_text).await;

    let service_uri =
        Url::from_file_path(_dir.path().join("src/Services/AuditService.php")).unwrap();
    let service_text =
        std::fs::read_to_string(_dir.path().join("src/Services/AuditService.php")).unwrap();
    open(&backend, &service_uri, &service_text).await;

    // Cursor on "Loggable" on line 2 of Loggable.php
    let locations = implementation_at(&backend, &loggable_uri, 2, 12).await;

    assert!(
        !locations.is_empty(),
        "Should find AuditService via transitive AuditLoggable extends Loggable"
    );
}

// ─── Vendor implementors are excluded once the workspace index is ready ─────

/// A vendor class that implements a project interface must NOT appear in
/// go-to-implementation results once the workspace index is ready, even if
/// that vendor class was parsed earlier in the session (which populates the
/// reverse-inheritance index). The full workspace index only parses project
/// files, so results must be user-only and independent of session history.
#[tokio::test]
async fn test_implementation_excludes_loaded_vendor_implementor() {
    let (backend, dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Contracts/Cacheable.php",
                concat!(
                    "<?php\n",
                    "namespace App\\Contracts;\n",
                    "interface Cacheable {\n",
                    "    public function cacheKey(): string;\n",
                    "}\n",
                ),
            ),
            (
                "src/Cache/FileCache.php",
                concat!(
                    "<?php\n",
                    "namespace App\\Cache;\n",
                    "use App\\Contracts\\Cacheable;\n",
                    "class FileCache implements Cacheable {\n",
                    "    public function cacheKey(): string { return 'file'; }\n",
                    "}\n",
                ),
            ),
            // A vendor package that also implements the project interface.
            (
                "vendor/acme/cache/src/AcmeCache.php",
                concat!(
                    "<?php\n",
                    "namespace Acme\\Cache;\n",
                    "use App\\Contracts\\Cacheable;\n",
                    "class AcmeCache implements Cacheable {\n",
                    "    public function cacheKey(): string { return 'acme'; }\n",
                    "}\n",
                ),
            ),
        ],
    );

    // Simulate the vendor class being loaded earlier in the session (e.g.
    // the user hovered or completed something that resolved it). This puts
    // AcmeCache into the reverse-inheritance index under Cacheable and maps
    // its FQN to a `/vendor/` URI in the FQN → URI index.
    let acme_uri = Url::from_file_path(dir.path().join("vendor/acme/cache/src/AcmeCache.php"))
        .expect("vendor uri");
    let acme_text =
        std::fs::read_to_string(dir.path().join("vendor/acme/cache/src/AcmeCache.php")).unwrap();
    open(&backend, &acme_uri, &acme_text).await;

    let iface_uri =
        Url::from_file_path(dir.path().join("src/Contracts/Cacheable.php")).expect("iface uri");
    let iface_text =
        std::fs::read_to_string(dir.path().join("src/Contracts/Cacheable.php")).unwrap();
    open(&backend, &iface_uri, &iface_text).await;

    // Cursor on "Cacheable" (line 2). The first request builds the workspace
    // index (which parses only project files) and then uses the index-ready
    // fast path.
    let locations = implementation_at(&backend, &iface_uri, 2, 12).await;

    let uris: Vec<String> = locations.iter().map(|l| l.uri.to_string()).collect();

    assert!(
        uris.iter().any(|u| u.contains("FileCache")),
        "Should find the project implementor FileCache, got: {uris:?}"
    );
    assert!(
        !uris.iter().any(|u| u.contains("/vendor/")),
        "Vendor implementor AcmeCache must be excluded once the index is ready, got: {uris:?}"
    );
}
