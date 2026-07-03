use crate::Backend;
use crate::virtual_members::laravel::extract_macro_registrations;
use std::sync::atomic::Ordering;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// Helper: open a file in the backend and return the URI.
async fn open_file(backend: &Backend, uri: &Url, text: &str) {
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;
}

/// Helper: send a find-references request and return the locations.
async fn find_references(
    backend: &Backend,
    uri: &Url,
    line: u32,
    character: u32,
    include_declaration: bool,
) -> Vec<Location> {
    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: ReferenceContext {
            include_declaration,
        },
    };

    backend
        .references(params)
        .await
        .unwrap()
        .unwrap_or_default()
}

fn seed_macro_index(backend: &Backend, uri: &Url, text: &str) {
    let mut index = backend.laravel_macros.write();
    index.set_file(
        uri.to_string(),
        extract_macro_registrations(text, Some(*backend.php_version.lock())),
    );
    index.rebuild();
    backend
        .laravel_has_macros
        .store(!index.is_empty(), Ordering::Relaxed);
}

fn line_char_of(haystack: &str, needle: &str) -> (u32, u32) {
    for (line_idx, line) in haystack.lines().enumerate() {
        if let Some(char_idx) = line.find(needle) {
            return (line_idx as u32, char_idx as u32);
        }
    }
    panic!("needle not found: {needle}");
}

// ─── Variable References ────────────────────────────────────────────────────

#[tokio::test]
async fn test_variable_references_same_scope() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                      // L0
        "function demo(): void {\n",    // L1
        "    $user = new User();\n",    // L2
        "    $user->name = 'Alice';\n", // L3
        "    echo $user->name;\n",      // L4
        "}\n",                          // L5
    );

    open_file(&backend, &uri, text).await;

    // Click on $user at line 3
    let locs = find_references(&backend, &uri, 3, 5, true).await;
    assert!(
        locs.len() >= 3,
        "Expected at least 3 references to $user, got {}",
        locs.len()
    );
    // All references should be in the same file.
    for loc in &locs {
        assert_eq!(loc.uri, uri);
    }
}

#[tokio::test]
async fn test_variable_references_excludes_other_scope() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                    // L0
        "function alpha(): void {\n", // L1
        "    $x = 1;\n",              // L2
        "    echo $x;\n",             // L3
        "}\n",                        // L4
        "function beta(): void {\n",  // L5
        "    $x = 2;\n",              // L6
        "    echo $x;\n",             // L7
        "}\n",                        // L8
    );

    open_file(&backend, &uri, text).await;

    // References to $x in alpha() should NOT include $x in beta().
    let locs = find_references(&backend, &uri, 2, 5, true).await;
    for loc in &locs {
        assert!(
            loc.range.start.line <= 4,
            "Reference to $x in alpha() should not appear in beta() (line {})",
            loc.range.start.line
        );
    }
}

#[tokio::test]
async fn test_variable_references_exclude_declaration() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                   // L0
        "function demo(): void {\n", // L1
        "    $val = 42;\n",          // L2
        "    echo $val;\n",          // L3
        "    $val = 99;\n",          // L4
        "}\n",                       // L5
    );

    open_file(&backend, &uri, text).await;

    // include_declaration = false: should still include usage sites
    let locs_no_decl = find_references(&backend, &uri, 3, 10, false).await;
    let locs_with_decl = find_references(&backend, &uri, 3, 10, true).await;
    // With declaration should have at least as many as without.
    assert!(
        locs_with_decl.len() >= locs_no_decl.len(),
        "with_decl ({}) should be >= no_decl ({})",
        locs_with_decl.len(),
        locs_no_decl.len()
    );
}

#[tokio::test]
async fn test_variable_references_include_compact_string() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo(): array {\n",
        "    $user = 'alice';\n",
        "    return compact('user');\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    let locs = find_references(&backend, &uri, 2, 6, true).await;
    assert!(
        locs.iter().any(|loc| {
            loc.range.start.line == 3
                && loc.range.start.character == 20
                && loc.range.end.character == 24
        }),
        "Expected compact('user') string contents to be included in variable references: {locs:?}"
    );
}

#[tokio::test]
async fn test_variable_references_include_compact_array_string() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo(): array {\n",
        "    $user = 'alice';\n",
        "    return compact(['user']);\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    let locs = find_references(&backend, &uri, 2, 6, true).await;
    assert!(
        locs.iter().any(|loc| {
            loc.range.start.line == 3
                && loc.range.start.character == 21
                && loc.range.end.character == 25
        }),
        "Expected compact(['user']) string contents to be included in variable references: {locs:?}"
    );
}

// ─── Class References ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_class_references_same_file() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                      // L0
        "class Logger {\n",                             // L1
        "    public function info(): void {}\n",        // L2
        "}\n",                                          // L3
        "class Service {\n",                            // L4
        "    public function run(Logger $l): void {\n", // L5
        "        $x = new Logger();\n",                 // L6
        "    }\n",                                      // L7
        "}\n",                                          // L8
    );

    open_file(&backend, &uri, text).await;

    // Click on "Logger" on line 5 (type hint).
    let locs = find_references(&backend, &uri, 5, 27, true).await;
    // Should find: declaration (L1), type hint (L5), new (L6) = at least 3.
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to Logger, got {}",
        locs.len()
    );
}

#[tokio::test]
async fn test_class_references_exclude_declaration() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                   // L0
        "class Foo {}\n",                            // L1
        "class Bar {\n",                             // L2
        "    public function test(Foo $f): Foo {\n", // L3
        "        return new Foo();\n",               // L4
        "    }\n",                                   // L5
        "}\n",                                       // L6
    );

    open_file(&backend, &uri, text).await;

    // Without declaration: should not include the `class Foo` declaration site.
    let locs = find_references(&backend, &uri, 3, 25, false).await;
    for loc in &locs {
        // Line 1 is the declaration of class Foo.
        assert_ne!(
            loc.range.start.line, 1,
            "Should not include declaration site when include_declaration=false"
        );
    }

    // With declaration: should include line 1.
    let locs_decl = find_references(&backend, &uri, 3, 25, true).await;
    let has_decl = locs_decl.iter().any(|l| l.range.start.line == 1);
    assert!(
        has_decl,
        "Should include declaration site when include_declaration=true"
    );
}

#[tokio::test]
async fn test_class_declaration_finds_references() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                     // L0
        "class Widget {}\n",           // L1
        "function make(): Widget {\n", // L2
        "    return new Widget();\n",  // L3
        "}\n",                         // L4
    );

    open_file(&backend, &uri, text).await;

    // Click on "Widget" at the declaration (line 1).
    let locs = find_references(&backend, &uri, 1, 7, true).await;
    assert!(
        locs.len() >= 3,
        "Expected at least 3 references (decl + 2 usages), got {}",
        locs.len()
    );
}

// ─── Member Access References ───────────────────────────────────────────────

#[tokio::test]
async fn test_method_references_same_file() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                      // L0
        "class Repo {\n",                               // L1
        "    public function find(int $id): void {}\n", // L2
        "}\n",                                          // L3
        "class Controller {\n",                         // L4
        "    public function index(Repo $r): void {\n", // L5
        "        $r->find(1);\n",                       // L6
        "        $r->find(2);\n",                       // L7
        "    }\n",                                      // L8
        "}\n",                                          // L9
    );

    open_file(&backend, &uri, text).await;

    // Click on "find" at line 6 (method call).
    let locs = find_references(&backend, &uri, 6, 14, false).await;
    // Should find at least 2 call sites (L6, L7).
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to find(), got {}",
        locs.len()
    );
}

#[tokio::test]
async fn test_method_references_include_declaration() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                               // L0
        "class Repo {\n",                        // L1
        "    public function save(): void {}\n", // L2
        "}\n",                                   // L3
        "function demo(Repo $r): void {\n",      // L4
        "    $r->save();\n",                     // L5
        "}\n",                                   // L6
    );

    open_file(&backend, &uri, text).await;

    // With declaration should also include the method definition on L2.
    let locs = find_references(&backend, &uri, 5, 10, true).await;
    let has_def = locs.iter().any(|l| l.range.start.line == 2);
    assert!(
        has_def,
        "Should include method declaration when include_declaration=true"
    );
}

#[tokio::test]
async fn test_static_method_references() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                        // L0
        "class Factory {\n",                              // L1
        "    public static function create(): void {}\n", // L2
        "}\n",                                            // L3
        "function demo(): void {\n",                      // L4
        "    Factory::create();\n",                       // L5
        "    Factory::create();\n",                       // L6
        "}\n",                                            // L7
    );

    open_file(&backend, &uri, text).await;

    // Click on "create" at line 5.
    let locs = find_references(&backend, &uri, 5, 15, false).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to create(), got {}",
        locs.len()
    );
}

#[tokio::test]
async fn test_property_references() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                            // L0
        "class Config {\n",                   // L1
        "    public string $name = '';\n",    // L2
        "}\n",                                // L3
        "function demo(Config $c): void {\n", // L4
        "    echo $c->name;\n",               // L5
        "    $c->name = 'test';\n",           // L6
        "}\n",                                // L7
    );

    open_file(&backend, &uri, text).await;

    // Click on "name" at line 5 (property access).
    let locs = find_references(&backend, &uri, 5, 15, false).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to ->name, got {}",
        locs.len()
    );
}

// ─── Function Call References ───────────────────────────────────────────────

#[tokio::test]
async fn test_function_call_references() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                      // L0
        "function helper(): void {}\n", // L1
        "function main(): void {\n",    // L2
        "    helper();\n",              // L3
        "    helper();\n",              // L4
        "}\n",                          // L5
    );

    open_file(&backend, &uri, text).await;

    // Click on "helper" at line 3.
    let locs = find_references(&backend, &uri, 3, 6, false).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to helper(), got {}",
        locs.len()
    );
}

#[tokio::test]
async fn test_function_references_include_declaration() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                // L0
        "function myFunc(): int { return 1; }\n", // L1
        "function demo(): void {\n",              // L2
        "    $x = myFunc();\n",                   // L3
        "}\n",                                    // L4
    );

    open_file(&backend, &uri, text).await;

    // With declaration should include the function definition on L1.
    let locs = find_references(&backend, &uri, 3, 11, true).await;
    let has_def = locs.iter().any(|l| l.range.start.line == 1);
    assert!(
        has_def,
        "Should include function declaration when include_declaration=true"
    );
}

// ─── Constant References ────────────────────────────────────────────────────

#[tokio::test]
async fn test_constant_references() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                    // L0
        "class Status {\n",           // L1
        "    const ACTIVE = 1;\n",    // L2
        "}\n",                        // L3
        "function demo(): void {\n",  // L4
        "    echo Status::ACTIVE;\n", // L5
        "    $x = Status::ACTIVE;\n", // L6
        "}\n",                        // L7
    );

    open_file(&backend, &uri, text).await;

    // Click on "ACTIVE" at line 5.
    let locs = find_references(&backend, &uri, 5, 20, false).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to ACTIVE, got {}",
        locs.len()
    );
}

#[tokio::test]
async fn test_define_constant_references_use_reference_index_snapshot() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///constants.php").unwrap();
    let text = concat!(
        "<?php\n",                        // L0
        "define('APP_FLAG', true);\n",    // L1
        "if (APP_FLAG) { echo 'on'; }\n"  // L2
    );

    open_file(&backend, &uri, text).await;

    let locs = find_references(&backend, &uri, 2, 4, false).await;
    assert!(
        locs.iter().any(|loc| loc.range.start.line == 2),
        "Expected bare APP_FLAG usage to be found through constant references, got {locs:?}"
    );
}

// ─── self / static / parent References ──────────────────────────────────────

#[tokio::test]
async fn test_self_references() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                       // L0
        "class Item {\n",                                // L1
        "    public static function create(): self {\n", // L2
        "        return new self();\n",                  // L3
        "    }\n",                                       // L4
        "}\n",                                           // L5
        "function demo(): void {\n",                     // L6
        "    $x = new Item();\n",                        // L7
        "}\n",                                           // L8
    );

    open_file(&backend, &uri, text).await;

    // Click on "self" at line 3.  This should resolve to class Item
    // and find references to Item across the file.
    let locs = find_references(&backend, &uri, 3, 20, true).await;
    assert!(
        !locs.is_empty(),
        "Expected references when clicking on self"
    );
}

// ─── Cross-File References ──────────────────────────────────────────────────

#[tokio::test]
async fn test_class_references_cross_file() {
    let backend = Backend::new_test();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!(
        "<?php\n",           // L0
        "class Animal {}\n", // L1
    );
    let text_b = concat!(
        "<?php\n",                                       // L0
        "class Zoo {\n",                                 // L1
        "    public function add(Animal $a): void {}\n", // L2
        "    public function get(): Animal {\n",         // L3
        "        return new Animal();\n",                // L4
        "    }\n",                                       // L5
        "}\n",                                           // L6
    );

    open_file(&backend, &uri_a, text_a).await;
    open_file(&backend, &uri_b, text_b).await;

    // Find references to Animal from file a.
    let locs = find_references(&backend, &uri_a, 1, 7, true).await;
    // Should find references in both files.
    let in_a = locs.iter().filter(|l| l.uri == uri_a).count();
    let in_b = locs.iter().filter(|l| l.uri == uri_b).count();
    assert!(
        in_a >= 1,
        "Expected at least 1 reference in a.php, got {}",
        in_a
    );
    assert!(
        in_b >= 1,
        "Expected at least 1 reference in b.php, got {}",
        in_b
    );
}

#[tokio::test]
async fn test_member_references_cross_file() {
    let backend = Backend::new_test();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!(
        "<?php\n",                                // L0
        "class Printer {\n",                      // L1
        "    public function print(): void {}\n", // L2
        "}\n",                                    // L3
        "function useA(Printer $p): void {\n",    // L4
        "    $p->print();\n",                     // L5
        "}\n",                                    // L6
    );
    let text_b = concat!(
        "<?php\n",                             // L0
        "function useB(Printer $p): void {\n", // L1
        "    $p->print();\n",                  // L2
        "}\n",                                 // L3
    );

    open_file(&backend, &uri_a, text_a).await;
    open_file(&backend, &uri_b, text_b).await;

    // Find references to print() from file a.
    let locs = find_references(&backend, &uri_a, 5, 10, false).await;
    let in_a = locs.iter().filter(|l| l.uri == uri_a).count();
    let in_b = locs.iter().filter(|l| l.uri == uri_b).count();
    assert!(
        in_a >= 1,
        "Expected at least 1 reference in a.php, got {}",
        in_a
    );
    assert!(
        in_b >= 1,
        "Expected at least 1 reference in b.php, got {}",
        in_b
    );
}

// ─── Namespaced References ──────────────────────────────────────────────────

#[tokio::test]
async fn test_namespaced_class_references() {
    let backend = Backend::new_test();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!(
        "<?php\n",                  // L0
        "namespace App\\Models;\n", // L1
        "class User {}\n",          // L2
    );
    let text_b = concat!(
        "<?php\n",                              // L0
        "namespace App\\Services;\n",           // L1
        "use App\\Models\\User;\n",             // L2
        "class UserService {\n",                // L3
        "    public function find(): User {\n", // L4
        "        return new User();\n",         // L5
        "    }\n",                              // L6
        "}\n",                                  // L7
    );

    open_file(&backend, &uri_a, text_a).await;
    open_file(&backend, &uri_b, text_b).await;

    // Find references to App\Models\User from declaration in a.php.
    let locs = find_references(&backend, &uri_a, 2, 7, true).await;
    let in_b = locs.iter().filter(|l| l.uri == uri_b).count();
    assert!(
        in_b >= 1,
        "Expected at least 1 cross-file namespaced reference in b.php, got {}",
        in_b
    );
}

// ─── Edge Cases ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_no_references_on_whitespace() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",        // L0
        "\n",             // L1
        "class Foo {}\n", // L2
    );

    open_file(&backend, &uri, text).await;

    // Click on empty line — should return None / empty.
    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 1,
                character: 0,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let result = backend.references(params).await.unwrap();
    assert!(
        result.is_none() || result.as_ref().unwrap().is_empty(),
        "Expected no references on whitespace"
    );
}

#[tokio::test]
async fn test_variable_parameter_reference() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                  // L0
        "function greet(string $name): string {\n", // L1
        "    return 'Hello ' . $name;\n",           // L2
        "}\n",                                      // L3
    );

    open_file(&backend, &uri, text).await;

    // Click on $name at usage (line 2).
    let locs = find_references(&backend, &uri, 2, 25, true).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references (param + usage), got {}",
        locs.len()
    );
}

#[tokio::test]
async fn test_results_sorted_by_position() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                   // L0
        "class X {}\n",                              // L1
        "function a(X $x): X { return new X(); }\n", // L2
        "function b(X $x): X { return new X(); }\n", // L3
    );

    open_file(&backend, &uri, text).await;

    let locs = find_references(&backend, &uri, 2, 12, true).await;
    // Verify results are sorted by line then character.
    for window in locs.windows(2) {
        let a = &window[0];
        let b = &window[1];
        let a_before_b = (a.uri.as_str(), a.range.start.line, a.range.start.character)
            <= (b.uri.as_str(), b.range.start.line, b.range.start.character);
        assert!(
            a_before_b,
            "Results should be sorted: {:?} should come before {:?}",
            a.range.start, b.range.start
        );
    }
}

#[tokio::test]
async fn test_class_extends_references() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                           // L0
        "class Base {}\n",                   // L1
        "class Child extends Base {}\n",     // L2
        "function demo(Base $b): void {}\n", // L3
    );

    open_file(&backend, &uri, text).await;

    // Find references to Base — should include extends clause and type hint.
    let locs = find_references(&backend, &uri, 1, 7, true).await;
    assert!(
        locs.len() >= 3,
        "Expected at least 3 references (decl + extends + param), got {}",
        locs.len()
    );
}

#[tokio::test]
async fn test_interface_implements_references() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                   // L0
        "interface Loggable {}\n",                   // L1
        "class FileLogger implements Loggable {}\n", // L2
        "function log(Loggable $l): void {}\n",      // L3
    );

    open_file(&backend, &uri, text).await;

    let locs = find_references(&backend, &uri, 1, 12, true).await;
    assert!(
        locs.len() >= 3,
        "Expected at least 3 references (decl + implements + param), got {}",
        locs.len()
    );
}

#[tokio::test]
async fn test_foreach_variable_references() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                           // L0
        "function demo(): void {\n",         // L1
        "    $items = [1, 2, 3];\n",         // L2
        "    foreach ($items as $item) {\n", // L3
        "        echo $item;\n",             // L4
        "        echo $item + 1;\n",         // L5
        "    }\n",                           // L6
        "}\n",                               // L7
    );

    open_file(&backend, &uri, text).await;

    // Click on $item at line 4.
    let locs = find_references(&backend, &uri, 4, 14, true).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to $item (foreach var + usages), got {}",
        locs.len()
    );
}

#[tokio::test]
async fn test_static_property_references() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                          // L0
        "class Counter {\n",                                // L1
        "    public static int $count = 0;\n",              // L2
        "    public static function increment(): void {\n", // L3
        "        self::$count++;\n",                        // L4
        "    }\n",                                          // L5
        "}\n",                                              // L6
        "function demo(): void {\n",                        // L7
        "    Counter::$count = 5;\n",                       // L8
        "}\n",                                              // L9
    );

    open_file(&backend, &uri, text).await;

    // Click on $count at line 4.
    let locs = find_references(&backend, &uri, 4, 16, false).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to static $count, got {}",
        locs.len()
    );
}

#[tokio::test]
async fn test_this_property_references() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // L0
        "class Person {\n",                                      // L1
        "    public string $email = '';\n",                      // L2
        "    public function setEmail(string $email): void {\n", // L3
        "        $this->email = $email;\n",                      // L4
        "    }\n",                                               // L5
        "    public function getEmail(): string {\n",            // L6
        "        return $this->email;\n",                        // L7
        "    }\n",                                               // L8
        "}\n",                                                   // L9
    );

    open_file(&backend, &uri, text).await;

    // Click on "email" at line 4 (property access via $this->).
    let locs = find_references(&backend, &uri, 4, 17, false).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to ->email, got {}",
        locs.len()
    );
}

#[tokio::test]
async fn test_multiple_files_function_references() {
    let backend = Backend::new_test();
    let uri_a = Url::parse("file:///helpers.php").unwrap();
    let uri_b = Url::parse("file:///main.php").unwrap();

    let text_a = concat!(
        "<?php\n",                                        // L0
        "function format_name(string $name): string {\n", // L1
        "    return ucfirst($name);\n",                   // L2
        "}\n",                                            // L3
    );
    let text_b = concat!(
        "<?php\n",                          // L0
        "function demo(): void {\n",        // L1
        "    $x = format_name('alice');\n", // L2
        "    $y = format_name('bob');\n",   // L3
        "}\n",                              // L4
    );

    open_file(&backend, &uri_a, text_a).await;
    open_file(&backend, &uri_b, text_b).await;

    // Find references to format_name from file b.
    let locs = find_references(&backend, &uri_b, 2, 11, false).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 call-site references across files, got {}",
        locs.len()
    );
}

// ─── $this References (file-local, not cross-file class search) ─────────────

#[tokio::test]
async fn test_this_is_file_local_not_cross_file() {
    let backend = Backend::new_test();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!(
        "<?php\n",                             // L0
        "class Foo {\n",                       // L1
        "    public function bar(): void {\n", // L2
        "        $this->baz();\n",             // L3
        "    }\n",                             // L4
        "}\n",                                 // L5
    );
    let text_b = concat!(
        "<?php\n",                         // L0
        "function demo(Foo $f): void {\n", // L1
        "    $f->baz();\n",                // L2
        "}\n",                             // L3
    );

    open_file(&backend, &uri_a, text_a).await;
    open_file(&backend, &uri_b, text_b).await;

    // Click on $this at line 3 of a.php.
    let locs = find_references(&backend, &uri_a, 3, 9, true).await;

    // All results must be in the same file — $this is not a cross-file
    // class reference.
    for loc in &locs {
        assert_eq!(
            loc.uri, uri_a,
            "$this references should stay within the current file, but found one in {}",
            loc.uri
        );
    }
}

#[tokio::test]
async fn test_this_references_within_class() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                          // L0
        "class Account {\n",                                // L1
        "    public string $name = '';\n",                  // L2
        "    public function getName(): string {\n",        // L3
        "        return $this->name;\n",                    // L4
        "    }\n",                                          // L5
        "    public function setName(string $n): void {\n", // L6
        "        $this->name = $n;\n",                      // L7
        "    }\n",                                          // L8
        "    public function self_ref(): self {\n",         // L9
        "        return $this;\n",                          // L10
        "    }\n",                                          // L11
        "}\n",                                              // L12
    );

    open_file(&backend, &uri, text).await;

    // Click on $this at line 4.
    let locs = find_references(&backend, &uri, 4, 16, true).await;
    // Should find at least 3 occurrences of $this (L4, L7, L10).
    assert!(
        locs.len() >= 3,
        "Expected at least 3 $this references in Account, got {}",
        locs.len()
    );
    for loc in &locs {
        assert_eq!(loc.uri, uri);
    }
}

#[tokio::test]
async fn test_this_scoped_to_enclosing_class() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                            // L0
        "class Alpha {\n",                    // L1
        "    public function go(): void {\n", // L2
        "        $this->run();\n",            // L3
        "    }\n",                            // L4
        "}\n",                                // L5
        "class Beta {\n",                     // L6
        "    public function go(): void {\n", // L7
        "        $this->run();\n",            // L8
        "    }\n",                            // L9
        "}\n",                                // L10
    );

    open_file(&backend, &uri, text).await;

    // Click on $this inside Alpha (line 3).
    let locs = find_references(&backend, &uri, 3, 9, true).await;
    // Should NOT include $this from Beta on line 8.
    for loc in &locs {
        assert!(
            loc.range.start.line < 5,
            "$this in Alpha should not include Beta's $this on line {}",
            loc.range.start.line
        );
    }
    assert!(!locs.is_empty(), "Should find at least one $this in Alpha");
}

// ─── Method Declaration Triggers Find References ────────────────────────────

#[tokio::test]
async fn test_method_declaration_triggers_references() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                              // L0
        "class Converter {\n",                                                  // L1
        "    public static function toListOfString(iterable $values): array\n", // L2
        "    {\n",                                                              // L3
        "        self::toListOfString($values);\n",                             // L4
        "    }\n",                                                              // L5
        "}\n",                                                                  // L6
    );

    open_file(&backend, &uri, text).await;

    // Click on the method NAME at the declaration site (line 2).
    // "    public static function toListOfString(..."
    // "toListOfString" starts at character 27.
    let locs = find_references(&backend, &uri, 2, 30, true).await;
    assert!(
        locs.len() >= 2,
        "Clicking on method declaration should find references; got {} locations",
        locs.len()
    );
    // Should include the call site on L4.
    let has_call = locs.iter().any(|l| l.range.start.line == 4);
    assert!(
        has_call,
        "Should include the self::toListOfString call on line 4"
    );
}

#[tokio::test]
async fn test_property_declaration_triggers_references() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                              // L0
        "class Box {\n",                        // L1
        "    public int $size = 0;\n",          // L2
        "    public function grow(): void {\n", // L3
        "        $this->size++;\n",             // L4
        "    }\n",                              // L5
        "}\n",                                  // L6
    );

    open_file(&backend, &uri, text).await;

    // Click on the property name at the declaration (line 2).
    // "    public int $size = 0;"
    // "$size" starts at character 15.
    let locs = find_references(&backend, &uri, 2, 16, true).await;
    assert!(
        locs.len() >= 2,
        "Clicking on property declaration should find references; got {} locations",
        locs.len()
    );
    let has_usage = locs.iter().any(|l| l.range.start.line == 4);
    assert!(has_usage, "Should include the $this->size usage on line 4");
}

#[tokio::test]
async fn test_constant_declaration_triggers_references() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                               // L0
        "class Limit {\n",                       // L1
        "    const MAX = 100;\n",                // L2
        "    public function check(): bool {\n", // L3
        "        return self::MAX > 0;\n",       // L4
        "    }\n",                               // L5
        "}\n",                                   // L6
    );

    open_file(&backend, &uri, text).await;

    // Click on the constant name at the declaration (line 2).
    // "    const MAX = 100;"
    // "MAX" starts at character 10.
    let locs = find_references(&backend, &uri, 2, 11, true).await;
    assert!(
        locs.len() >= 2,
        "Clicking on constant declaration should find references; got {} locations",
        locs.len()
    );
    let has_usage = locs.iter().any(|l| l.range.start.line == 4);
    assert!(has_usage, "Should include the self::MAX usage on line 4");
}

#[tokio::test]
async fn test_method_declaration_cross_file() {
    let backend = Backend::new_test();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!(
        "<?php\n",                                         // L0
        "class Formatter {\n",                             // L1
        "    public function format(string $s): string\n", // L2
        "    {\n",                                         // L3
        "        return $s;\n",                            // L4
        "    }\n",                                         // L5
        "}\n",                                             // L6
    );
    let text_b = concat!(
        "<?php\n",                               // L0
        "function demo(Formatter $f): void {\n", // L1
        "    $f->format('hello');\n",            // L2
        "}\n",                                   // L3
    );

    open_file(&backend, &uri_a, text_a).await;
    open_file(&backend, &uri_b, text_b).await;

    // Click on method name at the declaration in a.php (line 2).
    let locs = find_references(&backend, &uri_a, 2, 23, true).await;
    let in_b = locs.iter().filter(|l| l.uri == uri_b).count();
    assert!(
        in_b >= 1,
        "Method declaration should find cross-file call site; got {} in b.php",
        in_b
    );
}

// ─── Class-Aware Member Filtering ───────────────────────────────────────────

#[tokio::test]
async fn test_unrelated_class_same_method_excluded() {
    // Two unrelated classes with the same method name.  Find References
    // on one should NOT return results from the other.
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                            // L0
        "class MyClass {\n",                                  // L1
        "    public function save(): void {}\n",              // L2
        "}\n",                                                // L3
        "class OtherClass {\n",                               // L4
        "    public function save(): void {}\n",              // L5
        "}\n",                                                // L6
        "function demo(MyClass $a, OtherClass $b): void {\n", // L7
        "    $a->save();\n",                                  // L8
        "    $b->save();\n",                                  // L9
        "}\n",                                                // L10
    );

    open_file(&backend, &uri, text).await;

    // Click on save() at L8 ($a->save(), where $a: MyClass).
    let locs = find_references(&backend, &uri, 8, 10, false).await;

    // Should include L8 ($a->save()) but NOT L9 ($b->save()).
    let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&8),
        "Should find $a->save() on L8; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&9),
        "Should NOT find $b->save() on L9 (unrelated class); got lines: {:?}",
        lines
    );
}

#[tokio::test]
async fn test_unrelated_class_same_method_excluded_cross_file() {
    // Cross-file: two unrelated classes with the same method name.
    let backend = Backend::new_test();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!(
        "<?php\n",                               // L0
        "class MyClass {\n",                     // L1
        "    public function save(): void {}\n", // L2
        "}\n",                                   // L3
        "class OtherClass {\n",                  // L4
        "    public function save(): void {}\n", // L5
        "}\n",                                   // L6
    );
    let text_b = concat!(
        "<?php\n",                                         // L0
        "function useMyClass(MyClass $m): void {\n",       // L1
        "    $m->save();\n",                               // L2
        "}\n",                                             // L3
        "function useOtherClass(OtherClass $o): void {\n", // L4
        "    $o->save();\n",                               // L5
        "}\n",                                             // L6
    );

    open_file(&backend, &uri_a, text_a).await;
    open_file(&backend, &uri_b, text_b).await;

    // Find references to MyClass::save() from its declaration (L2 in a.php).
    // "save" starts at character 20 in "    public function save(): void {}"
    let locs = find_references(&backend, &uri_a, 2, 21, true).await;

    // b.php should have $m->save() (L2) but NOT $o->save() (L5).
    let b_lines: Vec<u32> = locs
        .iter()
        .filter(|l| l.uri == uri_b)
        .map(|l| l.range.start.line)
        .collect();
    assert!(
        b_lines.contains(&2),
        "Should find $m->save() on L2 of b.php; got lines: {:?}",
        b_lines
    );
    assert!(
        !b_lines.contains(&5),
        "Should NOT find $o->save() on L5 of b.php (unrelated class); got lines: {:?}",
        b_lines
    );

    // The declaration of OtherClass::save() (L5 in a.php) should also be excluded.
    let a_lines: Vec<u32> = locs
        .iter()
        .filter(|l| l.uri == uri_a)
        .map(|l| l.range.start.line)
        .collect();
    assert!(
        a_lines.contains(&2),
        "Should include MyClass::save() declaration on L2 of a.php; got: {:?}",
        a_lines
    );
    assert!(
        !a_lines.contains(&5),
        "Should NOT include OtherClass::save() declaration on L5 of a.php; got: {:?}",
        a_lines
    );
}

#[tokio::test]
async fn test_inherited_method_references_included() {
    // A child class inherits a method from its parent.  Find References
    // on the parent's method should include calls via the child.
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                    // L0
        "class Base {\n",                             // L1
        "    public function save(): void {}\n",      // L2
        "}\n",                                        // L3
        "class Child extends Base {}\n",              // L4
        "function demo(Base $a, Child $b): void {\n", // L5
        "    $a->save();\n",                          // L6
        "    $b->save();\n",                          // L7
        "}\n",                                        // L8
    );

    open_file(&backend, &uri, text).await;

    // Click on save() at L6 ($a->save(), $a: Base).
    let locs = find_references(&backend, &uri, 6, 10, false).await;

    let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&6),
        "Should find $a->save() on L6; got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&7),
        "Should find $b->save() on L7 (Child extends Base); got lines: {:?}",
        lines
    );
}

#[tokio::test]
async fn test_interface_method_references_included() {
    // A class implements an interface.  Find References on the interface's
    // method should include calls via the implementing class.
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                         // L0
        "interface Saveable {\n",                          // L1
        "    public function save(): void;\n",             // L2
        "}\n",                                             // L3
        "class Record implements Saveable {\n",            // L4
        "    public function save(): void {}\n",           // L5
        "}\n",                                             // L6
        "function demo(Saveable $s, Record $r): void {\n", // L7
        "    $s->save();\n",                               // L8
        "    $r->save();\n",                               // L9
        "}\n",                                             // L10
    );

    open_file(&backend, &uri, text).await;

    // Click on save() at L8 ($s->save(), $s: Saveable).
    let locs = find_references(&backend, &uri, 8, 10, false).await;

    let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&8),
        "Should find $s->save() on L8; got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&9),
        "Should find $r->save() on L9 (Record implements Saveable); got lines: {:?}",
        lines
    );
}

#[tokio::test]
async fn test_static_method_unrelated_class_excluded() {
    // Two unrelated classes with the same static method name.
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                        // L0
        "class Alpha {\n",                                // L1
        "    public static function create(): void {}\n", // L2
        "}\n",                                            // L3
        "class Beta {\n",                                 // L4
        "    public static function create(): void {}\n", // L5
        "}\n",                                            // L6
        "function demo(): void {\n",                      // L7
        "    Alpha::create();\n",                         // L8
        "    Beta::create();\n",                          // L9
        "}\n",                                            // L10
    );

    open_file(&backend, &uri, text).await;

    // Click on create() at L8 (Alpha::create()).
    let locs = find_references(&backend, &uri, 8, 14, false).await;

    let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&8),
        "Should find Alpha::create() on L8; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&9),
        "Should NOT find Beta::create() on L9 (unrelated class); got lines: {:?}",
        lines
    );
}

#[tokio::test]
async fn test_self_static_method_references_scoped() {
    // self:: and static:: calls should be scoped to the enclosing class.
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                       // L0
        "class Foo {\n",                                 // L1
        "    public static function build(): void {}\n", // L2
        "    public function demo(): void {\n",          // L3
        "        self::build();\n",                      // L4
        "    }\n",                                       // L5
        "}\n",                                           // L6
        "class Bar {\n",                                 // L7
        "    public static function build(): void {}\n", // L8
        "    public function demo(): void {\n",          // L9
        "        self::build();\n",                      // L10
        "    }\n",                                       // L11
        "}\n",                                           // L12
    );

    open_file(&backend, &uri, text).await;

    // Click on build() at L4 (self::build() inside Foo).
    let locs = find_references(&backend, &uri, 4, 16, false).await;

    let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&4),
        "Should find self::build() on L4 (inside Foo); got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&10),
        "Should NOT find self::build() on L10 (inside Bar, unrelated); got lines: {:?}",
        lines
    );
}

#[tokio::test]
async fn test_unresolvable_variable_excluded_when_member_scope_known() {
    // Once a member search has a resolved receiver scope, unresolved
    // receivers with the same member name should not be included. In large
    // projects, common methods such as `find` otherwise match unrelated
    // untyped services and repositories.
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                       // L0
        "class MyClass {\n",                             // L1
        "    public function save(): void {}\n",         // L2
        "}\n",                                           // L3
        "function demo(MyClass $a, $unknown): void {\n", // L4
        "    $a->save();\n",                             // L5
        "    $unknown->save();\n",                       // L6
        "}\n",                                           // L7
    );

    open_file(&backend, &uri, text).await;

    // Click on save() at L5 ($a->save(), $a: MyClass).
    let locs = find_references(&backend, &uri, 5, 10, false).await;

    let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&5),
        "Should find $a->save() on L5; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&6),
        "Should NOT include unresolved $unknown->save() on L6; got lines: {:?}",
        lines
    );
}

#[tokio::test]
async fn test_overridden_find_excludes_base_repository_and_unresolved_calls() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                          // L0
        "class ServiceEntityRepository {\n",                                // L1
        "    public function find(int $id): object {}\n",                   // L2
        "}\n",                                                              // L3
        "class NotificationRepository extends ServiceEntityRepository {\n", // L4
        "    public function find(int $id): object {}\n",                   // L5
        "}\n",                                                              // L6
        "class UserRepository extends ServiceEntityRepository {\n",         // L7
        "    public function find(int $id): object {}\n",                   // L8
        "}\n",                                                              // L9
        "function demo(NotificationRepository $notifications, ServiceEntityRepository $base, UserRepository $users, $managerRegistry, $unknown): void {\n", // L10
        "    $notifications->find(1);\n", // L11
        "    $base->find(2);\n",          // L12
        "    $users->find(3);\n",         // L13
        "    $notificationRepository = $managerRegistry->getManager()->getRepository(NotificationImpl::class);\n", // L14
        "    $notificationRepository->find(4);\n", // L15
        "    $unknown->find(5);\n",                // L16
        "}\n",                                     // L17
    );

    open_file(&backend, &uri, text).await;

    let locs = find_references(&backend, &uri, 5, 21, true).await;
    let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();

    assert!(
        lines.contains(&5),
        "Should include NotificationRepository::find declaration on L5; got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&11),
        "Should include $notifications->find() on L11; got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&15),
        "Should include unresolved but clearly named $notificationRepository->find() on L15; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&2),
        "Should NOT include base ServiceEntityRepository::find declaration on L2; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&8),
        "Should NOT include sibling UserRepository::find declaration on L8; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&12),
        "Should NOT include base-typed $base->find() on L12; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&13),
        "Should NOT include sibling $users->find() on L13; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&16),
        "Should NOT include unresolved $unknown->find() on L16; got lines: {:?}",
        lines
    );
}

#[tokio::test]
async fn test_concrete_method_references_include_interface_typed_calls() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                           // L0
        "namespace App;\n",                                                  // L1
        "class Notification {}\n",                                           // L2
        "interface NotificationGateway {\n",                                 // L3
        "    public function insert(Notification $notification): void;\n",   // L4
        "}\n",                                                               // L5
        "interface UserGateway {\n",                                         // L6
        "    public function insert(Notification $notification): void;\n",   // L7
        "}\n",                                                               // L8
        "class NotificationRepository implements NotificationGateway {\n",   // L9
        "    public function insert(Notification $notification): void {}\n", // L10
        "}\n",                                                               // L11
        "class AddNotification {\n",                                         // L12
        "    public function __construct(private readonly NotificationGateway $notificationGateway) {}\n", // L13
        "    public function execute(Notification $notification): void {\n", // L14
        "        $this->notificationGateway->insert($notification);\n",      // L15
        "    }\n",                                                           // L16
        "}\n",                                                               // L17
        "class AddUser {\n",                                                 // L18
        "    public function __construct(private readonly UserGateway $userGateway) {}\n", // L19
        "    public function execute(Notification $notification): void {\n", // L20
        "        $this->userGateway->insert($notification);\n",              // L21
        "    }\n",                                                           // L22
        "}\n",                                                               // L23
    );

    open_file(&backend, &uri, text).await;

    let locs = find_references(&backend, &uri, 10, 21, true).await;
    let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();

    assert!(
        lines.contains(&4),
        "Should include NotificationGateway::insert declaration on L4; got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&10),
        "Should include NotificationRepository::insert declaration on L10; got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&15),
        "Should include interface-typed $notificationGateway->insert() on L15; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&7),
        "Should NOT include unrelated UserGateway::insert declaration on L7; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&21),
        "Should NOT include unrelated $userGateway->insert() on L21; got lines: {:?}",
        lines
    );
}

#[tokio::test]
async fn test_this_method_references_excludes_unrelated() {
    // $this->method() inside one class should not match $this->method()
    // inside an unrelated class with the same method name.
    // Note: $this references are currently file-local, but the member
    // reference search is cross-file.  This test checks the member
    // name filtering when triggered from a $this-> call site.
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                    // L0
        "class Dog {\n",                              // L1
        "    public function speak(): void {}\n",     // L2
        "    public function demo(): void {\n",       // L3
        "        $this->speak();\n",                  // L4
        "    }\n",                                    // L5
        "}\n",                                        // L6
        "class Cat {\n",                              // L7
        "    public function speak(): void {}\n",     // L8
        "    public function demo(): void {\n",       // L9
        "        $this->speak();\n",                  // L10
        "    }\n",                                    // L11
        "}\n",                                        // L12
        "function outside(Dog $d, Cat $c): void {\n", // L13
        "    $d->speak();\n",                         // L14
        "    $c->speak();\n",                         // L15
        "}\n",                                        // L16
    );

    open_file(&backend, &uri, text).await;

    // Click on speak() at L14 ($d->speak(), $d: Dog).
    let locs = find_references(&backend, &uri, 14, 10, true).await;

    let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&14),
        "Should find $d->speak() on L14; got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&2),
        "Should include Dog::speak() declaration on L2; got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&4),
        "Should include $this->speak() inside Dog on L4; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&8),
        "Should NOT include Cat::speak() declaration on L8; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&10),
        "Should NOT include $this->speak() inside Cat on L10; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&15),
        "Should NOT include $c->speak() on L15 (unrelated class); got lines: {:?}",
        lines
    );
}

// ─── PHPDoc @property and @method References ────────────────────────────────

#[tokio::test]
async fn test_phpdoc_property_references_from_usage() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                              // L0
        "/**\n",                                // L1
        " * @property string $email\n",         // L2
        " */\n",                                // L3
        "class User {\n",                       // L4
        "    public function demo(): void {\n", // L5
        "        echo $this->email;\n",         // L6
        "    }\n",                              // L7
        "}\n",                                  // L8
        "$u = new User();\n",                   // L9
        "echo $u->email;\n",                    // L10
    );

    open_file(&backend, &uri, text).await;

    // Click on "email" at line 10 ($u->email).
    let locs = find_references(&backend, &uri, 10, 13, true).await;
    assert!(
        locs.len() >= 3,
        "Expected at least 3 references to email (declaration + 2 usages), got {}",
        locs.len()
    );

    // Should include the @property declaration (line 2).
    let has_declaration = locs.iter().any(|l| l.range.start.line == 2);
    assert!(
        has_declaration,
        "Should include the @property declaration on line 2"
    );

    // Should include the $this->email usage (line 6).
    let has_this_usage = locs.iter().any(|l| l.range.start.line == 6);
    assert!(
        has_this_usage,
        "Should include the $this->email usage on line 6"
    );

    // Should include the $u->email usage (line 10).
    let has_external_usage = locs.iter().any(|l| l.range.start.line == 10);
    assert!(
        has_external_usage,
        "Should include the $u->email usage on line 10"
    );
}

#[tokio::test]
async fn test_phpdoc_property_references_from_declaration() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                              // L0
        "/**\n",                                // L1
        " * @property string $email\n",         // L2
        " */\n",                                // L3
        "class User {\n",                       // L4
        "    public function demo(): void {\n", // L5
        "        echo $this->email;\n",         // L6
        "    }\n",                              // L7
        "}\n",                                  // L8
        "$u = new User();\n",                   // L9
        "echo $u->email;\n",                    // L10
    );

    open_file(&backend, &uri, text).await;

    // Click on "email" in the @property tag (line 2).
    // Line: " * @property string $email"
    // The MemberDeclaration span covers "email" (without $) starting at char 22.
    let locs = find_references(&backend, &uri, 2, 22, true).await;
    assert!(
        locs.len() >= 3,
        "Expected at least 3 references from @property declaration, got {}",
        locs.len()
    );
}

#[tokio::test]
async fn test_phpdoc_method_references_from_usage() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                        // L0
        "/**\n",                          // L1
        " * @method string getEmail()\n", // L2
        " */\n",                          // L3
        "class User {\n",                 // L4
        "}\n",                            // L5
        "$u = new User();\n",             // L6
        "echo $u->getEmail();\n",         // L7
    );

    open_file(&backend, &uri, text).await;

    // Click on "getEmail" at line 7 ($u->getEmail()).
    let locs = find_references(&backend, &uri, 7, 10, true).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to getEmail (declaration + usage), got {}",
        locs.len()
    );

    // Should include the @method declaration (line 2).
    let has_declaration = locs.iter().any(|l| l.range.start.line == 2);
    assert!(
        has_declaration,
        "Should include the @method declaration on line 2"
    );
}

#[tokio::test]
async fn test_phpdoc_property_references_exclude_unrelated_class() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                      // L0
        "/**\n",                        // L1
        " * @property string $email\n", // L2
        " */\n",                        // L3
        "class User {}\n",              // L4
        "/**\n",                        // L5
        " * @property int $email\n",    // L6
        " */\n",                        // L7
        "class Order {}\n",             // L8
        "$u = new User();\n",           // L9
        "echo $u->email;\n",            // L10
        "$o = new Order();\n",          // L11
        "echo $o->email;\n",            // L12
    );

    open_file(&backend, &uri, text).await;

    // Click on "email" at line 10 ($u->email).
    let locs = find_references(&backend, &uri, 10, 13, true).await;

    // Should include User's @property and $u->email, but NOT Order's @property or $o->email.
    let has_user_declaration = locs.iter().any(|l| l.range.start.line == 2);
    let has_user_usage = locs.iter().any(|l| l.range.start.line == 10);
    let has_order_declaration = locs.iter().any(|l| l.range.start.line == 6);
    let has_order_usage = locs.iter().any(|l| l.range.start.line == 12);

    assert!(
        has_user_declaration,
        "Should include User's @property declaration"
    );
    assert!(has_user_usage, "Should include $u->email usage");
    assert!(
        !has_order_declaration,
        "Should NOT include Order's @property declaration"
    );
    assert!(!has_order_usage, "Should NOT include $o->email usage");
}

#[tokio::test]
async fn test_phpdoc_property_multiple_properties() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                      // L0
        "/**\n",                        // L1
        " * @property int $id\n",       // L2
        " * @property string $email\n", // L3
        " * @property string $name\n",  // L4
        " */\n",                        // L5
        "class User {}\n",              // L6
        "$u = new User();\n",           // L7
        "echo $u->email;\n",            // L8
    );

    open_file(&backend, &uri, text).await;

    // Click on "email" at line 8 ($u->email).
    let locs = find_references(&backend, &uri, 8, 13, true).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to email, got {}",
        locs.len()
    );

    // Should include only the @property string $email declaration (line 3), not id or name.
    let has_email_decl = locs.iter().any(|l| l.range.start.line == 3);
    let has_id_decl = locs.iter().any(|l| l.range.start.line == 2);
    let has_name_decl = locs.iter().any(|l| l.range.start.line == 4);
    assert!(
        has_email_decl,
        "Should include @property string $email declaration"
    );
    assert!(
        !has_id_decl,
        "Should NOT include @property int $id declaration"
    );
    assert!(
        !has_name_decl,
        "Should NOT include @property string $name declaration"
    );
}

#[tokio::test]
async fn test_phpdoc_property_read_write_variants() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                          // L0
        "/**\n",                            // L1
        " * @property-read string $name\n", // L2
        " * @property-write int $age\n",    // L3
        " */\n",                            // L4
        "class User {}\n",                  // L5
        "$u = new User();\n",               // L6
        "echo $u->name;\n",                 // L7
    );

    open_file(&backend, &uri, text).await;

    // Click on "name" at line 7 ($u->name).
    let locs = find_references(&backend, &uri, 7, 13, true).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to name (property-read declaration + usage), got {}",
        locs.len()
    );

    // Should include the @property-read declaration (line 2).
    let has_read_decl = locs.iter().any(|l| l.range.start.line == 2);
    assert!(
        has_read_decl,
        "Should include the @property-read declaration"
    );
}

#[tokio::test]
async fn test_phpdoc_method_references_from_declaration() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                        // L0
        "/**\n",                          // L1
        " * @method string getEmail()\n", // L2
        " */\n",                          // L3
        "class User {}\n",                // L4
        "$u = new User();\n",             // L5
        "echo $u->getEmail();\n",         // L6
    );

    open_file(&backend, &uri, text).await;

    // Click on "getEmail" in the @method tag (line 2).
    // Line: " * @method string getEmail()"
    // The MemberDeclaration span covers "getEmail" starting at char 19.
    let locs = find_references(&backend, &uri, 2, 19, true).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references from @method declaration, got {}",
        locs.len()
    );

    let has_usage = locs.iter().any(|l| l.range.start.line == 6);
    assert!(has_usage, "Should include $u->getEmail() usage on line 6");
}

#[tokio::test]
async fn test_phpdoc_method_no_return_type_references() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                 // L0
        "/**\n",                   // L1
        " * @method getEmail()\n", // L2
        " */\n",                   // L3
        "class User {}\n",         // L4
        "$u = new User();\n",      // L5
        "echo $u->getEmail();\n",  // L6
    );

    open_file(&backend, &uri, text).await;

    // Click on "getEmail" at line 6 ($u->getEmail()).
    let locs = find_references(&backend, &uri, 6, 10, true).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to getEmail (declaration + usage), got {}",
        locs.len()
    );

    // Should include the @method declaration (line 2).
    let has_declaration = locs.iter().any(|l| l.range.start.line == 2);
    assert!(
        has_declaration,
        "Should include the @method declaration on line 2"
    );
}

// ─── Constructor References ──────────────────────────────────────────

#[tokio::test]
async fn test_constructor_references_finds_instantiations() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///ctor.php").unwrap();
    let text = concat!(
        "<?php\n",                                // L0
        "class Service {\n",                      // L1
        "    public function __construct() {}\n", // L2
        "}\n",                                    // L3
        "$a = new Service();\n",                  // L4
        "$b = new Service();\n",                  // L5
    );

    open_file(&backend, &uri, text).await;

    // Click on "__construct" at line 2.
    let locs = find_references(&backend, &uri, 2, 25, true).await;

    // Both `new Service()` sites should be found.
    let has_l4 = locs.iter().any(|l| l.range.start.line == 4);
    let has_l5 = locs.iter().any(|l| l.range.start.line == 5);
    assert!(
        has_l4 && has_l5,
        "Expected both `new Service()` instantiations (L4 + L5), got {:?}",
        locs
    );
}

#[tokio::test]
async fn test_constructor_references_includes_inheriting_subclass() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///ctor_inherit.php").unwrap();
    let text = concat!(
        "<?php\n",                                // L0
        "class Base {\n",                         // L1
        "    public function __construct() {}\n", // L2
        "}\n",                                    // L3
        "class Child extends Base {}\n",          // L4
        "$a = new Base();\n",                     // L5
        "$b = new Child();\n",                    // L6
    );

    open_file(&backend, &uri, text).await;

    // Click on "__construct" at line 2.
    let locs = find_references(&backend, &uri, 2, 25, true).await;

    // `new Child()` inherits Base's constructor, so it counts.
    let has_base = locs.iter().any(|l| l.range.start.line == 5);
    let has_child = locs.iter().any(|l| l.range.start.line == 6);
    assert!(
        has_base && has_child,
        "Expected `new Base()` (L5) and inherited `new Child()` (L6), got {:?}",
        locs
    );
}

#[tokio::test]
async fn test_constructor_references_excludes_overriding_subclass() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///ctor_override.php").unwrap();
    let text = concat!(
        "<?php\n",                                // L0
        "class Base {\n",                         // L1
        "    public function __construct() {}\n", // L2
        "}\n",                                    // L3
        "class Child extends Base {\n",           // L4
        "    public function __construct() {}\n", // L5
        "}\n",                                    // L6
        "$a = new Base();\n",                     // L7
        "$b = new Child();\n",                    // L8
    );

    open_file(&backend, &uri, text).await;

    // Click on Base's "__construct" at line 2.
    let locs = find_references(&backend, &uri, 2, 25, true).await;

    // `new Child()` invokes Child's OWN constructor, so it must be excluded.
    let has_base = locs.iter().any(|l| l.range.start.line == 7);
    let has_child = locs.iter().any(|l| l.range.start.line == 8);
    assert!(has_base, "Expected `new Base()` (L7), got {:?}", locs);
    assert!(
        !has_child,
        "`new Child()` (L8) overrides the constructor and must be excluded, got {:?}",
        locs
    );
}

#[tokio::test]
async fn test_constructor_references_finds_attribute_usage() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///ctor_attr.php").unwrap();
    let text = concat!(
        "<?php\n",                                          // L0
        "#[\\Attribute]\n",                                 // L1
        "class MyAttr {\n",                                 // L2
        "    public function __construct(int $x = 0) {}\n", // L3
        "}\n",                                              // L4
        "#[MyAttr(1)]\n",                                   // L5
        "class Target {}\n",                                // L6
    );

    open_file(&backend, &uri, text).await;

    // Click on MyAttr's "__construct" at line 3.
    let locs = find_references(&backend, &uri, 3, 25, true).await;

    // The `#[MyAttr(1)]` attribute usage on line 5 invokes the constructor.
    let has_attr_usage = locs.iter().any(|l| l.range.start.line == 5);
    assert!(
        has_attr_usage,
        "Expected the `#[MyAttr(1)]` attribute usage (L5) to be a constructor reference, got {:?}",
        locs
    );
}

#[tokio::test]
async fn test_macro_registration_string_references_include_call_sites() {
    let backend = Backend::new_test();
    let class_uri = Url::parse("file:///Widget.php").unwrap();
    let provider_uri = Url::parse("file:///Provider.php").unwrap();
    let caller_uri = Url::parse("file:///Caller.php").unwrap();

    let class_text = concat!("<?php\n", "namespace App\\Support;\n", "class Widget {}\n",);
    let provider_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Support\\Widget;\n",
        "Widget::macro('shine', function (): string { return 'ok'; });\n",
    );
    let caller_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Support\\Widget;\n",
        "function demo(Widget $widget): void {\n",
        "    Widget::shine();\n",
        "    $widget->shine();\n",
        "}\n",
    );

    open_file(&backend, &class_uri, class_text).await;
    open_file(&backend, &provider_uri, provider_text).await;
    open_file(&backend, &caller_uri, caller_text).await;
    seed_macro_index(&backend, &provider_uri, provider_text);

    let (line, character) = line_char_of(provider_text, "shine");
    let locs = find_references(&backend, &provider_uri, line, character, true).await;
    assert_eq!(
        locs.len(),
        3,
        "expected declaration + static + instance call: {locs:?}"
    );
    assert!(locs.iter().any(|loc| loc.uri == provider_uri));
    assert!(locs.iter().any(|loc| {
        loc.uri == caller_uri && loc.range.start.line == 4 && loc.range.start.character == 12
    }));
    assert!(locs.iter().any(|loc| {
        loc.uri == caller_uri && loc.range.start.line == 5 && loc.range.start.character == 13
    }));
}

#[tokio::test]
async fn test_macro_references_from_descendant_call_include_ancestor_and_sibling_calls() {
    let backend = Backend::new_test();
    let base_uri = Url::parse("file:///BaseCollection.php").unwrap();
    let child_uri = Url::parse("file:///EloquentCollection.php").unwrap();
    let provider_uri = Url::parse("file:///Provider.php").unwrap();
    let caller_uri = Url::parse("file:///Caller.php").unwrap();

    let base_text = concat!(
        "<?php\n",
        "namespace App\\Support;\n",
        "class BaseCollection {}\n",
    );
    let child_text = concat!(
        "<?php\n",
        "namespace App\\Support;\n",
        "class EloquentCollection extends BaseCollection {}\n",
    );
    let provider_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Support\\BaseCollection;\n",
        "BaseCollection::macro('shine', function (): string { return 'ok'; });\n",
    );
    let caller_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Support\\BaseCollection;\n",
        "use App\\Support\\EloquentCollection;\n",
        "function demo(BaseCollection $base, EloquentCollection $eloquent): void {\n",
        "    $base->shine();\n",
        "    $eloquent->shine();\n",
        "}\n",
    );

    open_file(&backend, &base_uri, base_text).await;
    open_file(&backend, &child_uri, child_text).await;
    open_file(&backend, &provider_uri, provider_text).await;
    open_file(&backend, &caller_uri, caller_text).await;
    seed_macro_index(&backend, &provider_uri, provider_text);

    let locs = find_references(&backend, &caller_uri, 6, 16, true).await;
    assert_eq!(
        locs.len(),
        3,
        "expected registration + base + descendant call: {locs:?}"
    );
    assert!(locs.iter().any(|loc| loc.uri == provider_uri));
    assert!(locs.iter().any(|loc| {
        loc.uri == caller_uri && loc.range.start.line == 5 && loc.range.start.character == 11
    }));
    assert!(locs.iter().any(|loc| {
        loc.uri == caller_uri && loc.range.start.line == 6 && loc.range.start.character == 15
    }));
}

#[tokio::test]
async fn test_macro_registration_references_include_unresolved_chain_call() {
    let backend = Backend::new_test();
    let base_uri = Url::parse("file:///BaseCollection.php").unwrap();
    let provider_uri = Url::parse("file:///Provider.php").unwrap();
    let caller_uri = Url::parse("file:///Caller.php").unwrap();

    let base_text = concat!(
        "<?php\n",
        "namespace App\\Support;\n",
        "class BaseCollection {}\n",
    );
    let provider_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Support\\BaseCollection;\n",
        "BaseCollection::macro('shine', function (): string { return 'ok'; });\n",
    );
    let caller_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "function demo($query): void {\n",
        "    $query->pluck('name', 'id')->shine();\n",
        "}\n",
    );

    open_file(&backend, &base_uri, base_text).await;
    open_file(&backend, &provider_uri, provider_text).await;
    open_file(&backend, &caller_uri, caller_text).await;
    seed_macro_index(&backend, &provider_uri, provider_text);

    let shine_subject = backend
        .symbol_maps
        .read()
        .get(caller_uri.as_str())
        .expect("caller symbol map should exist")
        .spans
        .iter()
        .find_map(|span| match &span.kind {
            crate::symbol_map::SymbolKind::MemberAccess {
                member_name,
                subject_text,
                ..
            } if member_name == "shine" => Some(subject_text.clone()),
            _ => None,
        })
        .expect("expected member-access span for unresolved chain call");
    assert!(
        shine_subject.contains("pluck"),
        "expected chain subject text, got {shine_subject:?}"
    );
    assert!(
        shine_subject.contains('('),
        "expected call-chain subject text, got {shine_subject:?}"
    );

    let (line, character) = line_char_of(provider_text, "shine");
    let locs = find_references(&backend, &provider_uri, line, character, true).await;
    assert!(
        locs.iter().any(|loc| {
            loc.uri == caller_uri && loc.range.start.line == 3 && loc.range.start.character == 33
        }),
        "expected unresolved chain macro call to be included: {locs:?}"
    );
}

#[test]
fn workspace_indexing_batch_merges_disk_files() {
    use crate::reference_index::ReferenceIndexKey;

    let dir = tempfile::tempdir().expect("temp dir");
    let src = dir.path().join("src");
    std::fs::create_dir_all(src.join("Contracts")).expect("contracts dir");
    std::fs::create_dir_all(src.join("Impl")).expect("impl dir");

    std::fs::write(
        src.join("Contracts/Service.php"),
        "<?php\nnamespace App\\Contracts;\ninterface Service {}\n",
    )
    .expect("service file");
    std::fs::write(
        src.join("Impl/A.php"),
        "<?php\nnamespace App\\Impl;\nuse App\\Contracts\\Service;\nclass A implements Service { public function run(): void {} }\n",
    )
    .expect("a file");
    std::fs::write(
        src.join("Impl/B.php"),
        "<?php\nnamespace App\\Impl;\nclass B extends A {}\n",
    )
    .expect("b file");
    std::fs::write(
        src.join("Use.php"),
        "<?php\nnamespace App;\nuse App\\Impl\\A;\nfunction helper(): void {}\ndefine('APP_FLAG', 'yes');\n$a = new A();\n$a->run();\nhelper();\n",
    )
    .expect("use file");

    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), Vec::new());
    backend.ensure_workspace_indexed();

    assert!(
        backend
            .workspace_indexed
            .load(std::sync::atomic::Ordering::Acquire)
    );
    assert_eq!(
        backend.symbol_maps.read().len(),
        4,
        "all disk files should publish symbol maps through the batch merge"
    );
    assert!(
        backend
            .fqn_class_index
            .read()
            .contains_key("App\\Contracts\\Service")
    );
    assert!(backend.fqn_class_index.read().contains_key("App\\Impl\\A"));
    assert!(backend.global_functions.read().contains_key("App\\helper"));
    assert!(backend.global_defines.read().contains_key("APP_FLAG"));

    let service_children = backend
        .gti_index
        .read()
        .get("App\\Contracts\\Service")
        .cloned()
        .unwrap_or_default();
    assert!(service_children.contains(&"App\\Impl\\A".to_string()));

    let use_uri = crate::util::path_to_uri(&src.join("Use.php"));
    let class_candidates = backend
        .reference_candidate_uris_for_keys(&[ReferenceIndexKey::Class("App\\Impl\\A".to_string())])
        .expect("reference index should be active after workspace indexing");
    assert!(class_candidates.contains(&use_uri));

    let member_candidates = backend
        .reference_candidate_uris_for_keys(&[ReferenceIndexKey::Member {
            name: "run".to_string(),
            is_static: false,
        }])
        .expect("reference index should be active after workspace indexing");
    assert!(member_candidates.contains(&use_uri));

    let function_snapshot =
        backend.user_file_symbol_maps_for_reference_keys(&[ReferenceIndexKey::Function(
            "App\\helper".to_string(),
        )]);
    assert_eq!(
        function_snapshot.len(),
        1,
        "reference-key snapshots should use the reference index instead of cloning every user file"
    );
    assert_eq!(function_snapshot[0].0, use_uri);
}

#[test]
fn indexing_work_order_processes_largest_files_first() {
    assert_eq!(
        super::largest_first_work_order(&[10, 1, 50, 3]),
        vec![2, 0, 3, 1]
    );
}

#[test]
fn reference_key_snapshot_falls_back_until_workspace_index_ready() {
    use crate::reference_index::ReferenceIndexKey;

    let backend = Backend::new_test();
    let matching_uri = "file:///project/src/Use.php";
    let unrelated_uri = "file:///project/src/Other.php";

    backend.update_ast(
        matching_uri,
        "<?php\nnamespace App;\nfunction helper(): void {}\nhelper();\n",
    );
    backend.update_ast(unrelated_uri, "<?php\nnamespace App;\nclass Other {}\n");

    let snapshot =
        backend.user_file_symbol_maps_for_reference_keys(&[ReferenceIndexKey::Function(
            "App\\helper".to_string(),
        )]);
    let uris: std::collections::HashSet<_> = snapshot.into_iter().map(|(uri, _)| uri).collect();

    assert!(
        !backend
            .workspace_indexed
            .load(std::sync::atomic::Ordering::Acquire)
    );
    assert!(uris.contains(matching_uri));
    assert!(
        uris.contains(unrelated_uri),
        "before the full-index flag is ready, reference scans must fall back to all user files"
    );
}

#[test]
fn user_file_symbol_maps_exclude_vendor_and_stubs() {
    let dir = tempfile::tempdir().expect("temp dir");
    let vendor = dir.path().join("vendor");
    std::fs::create_dir_all(&vendor).expect("vendor dir");

    let backend = Backend::new_test();
    backend.add_vendor_dir(&vendor);

    let user_uri = "file:///project/src/User.php";
    let vendor_uri = crate::util::path_to_uri(&vendor.join("Package.php"));
    backend.update_ast(user_uri, "<?php\nnamespace App;\nclass User {}\n");
    backend.update_ast(&vendor_uri, "<?php\nnamespace Vendor;\nclass Package {}\n");
    backend.update_ast("phpantom-stub://core.php", "<?php\nclass StubClass {}\n");
    backend.update_ast(
        "phpantom-stub-fn://core.php",
        "<?php\nfunction stub_fn(): void {}\n",
    );

    let snapshot = backend.user_file_symbol_maps();
    let uris: std::collections::HashSet<_> = snapshot.into_iter().map(|(uri, _)| uri).collect();

    assert!(uris.contains(user_uri));
    assert!(!uris.contains(&vendor_uri));
    assert!(!uris.contains("phpantom-stub://core.php"));
    assert!(!uris.contains("phpantom-stub-fn://core.php"));
}

#[test]
fn workspace_index_progress_covers_known_files_and_refresh_walks() {
    let dir = tempfile::tempdir().expect("temp dir");
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).expect("src dir");

    let known_path = src.join("Known.php");
    let disk_path = src.join("Disk.php");
    std::fs::write(&known_path, "<?php\nnamespace App;\nclass Known {}\n").expect("known file");
    std::fs::write(&disk_path, "<?php\nnamespace App;\nclass Disk {}\n").expect("disk file");

    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), Vec::new());
    backend.fqn_uri_index.write().insert(
        "App\\Known".to_string(),
        crate::util::path_to_uri(&known_path),
    );

    let progress = std::sync::Mutex::new(Vec::new());
    backend.ensure_workspace_indexed_with_progress(Some(&|percentage, message| {
        progress
            .lock()
            .expect("progress lock")
            .push((percentage, message));
    }));

    let messages: Vec<String> = progress
        .lock()
        .expect("progress lock")
        .iter()
        .map(|(_, message)| message.clone())
        .collect();
    assert!(
        messages
            .iter()
            .any(|message| message == "Preparing workspace index")
    );
    assert!(
        messages
            .iter()
            .any(|message| message.starts_with("Parsing indexed files"))
    );
    assert!(
        messages
            .iter()
            .any(|message| message.starts_with("Parsing workspace files"))
    );
    assert_eq!(
        progress
            .lock()
            .expect("progress lock")
            .last()
            .map(|(pct, _)| *pct),
        Some(100)
    );
    assert!(backend.fqn_class_index.read().contains_key("App\\Known"));
    assert!(backend.fqn_class_index.read().contains_key("App\\Disk"));

    let refresh_path = src.join("Refresh.php");
    std::fs::write(&refresh_path, "<?php\nnamespace App;\nclass Refresh {}\n")
        .expect("refresh file");
    backend.ensure_workspace_indexed_with_progress(None);
    assert!(backend.fqn_class_index.read().contains_key("App\\Refresh"));
}

#[test]
fn parse_files_parallel_with_progress_merges_large_batches() {
    let backend = Backend::new_test();
    let files = (0..3)
        .map(|idx| {
            (
                format!("file:///project/src/File{idx}.php"),
                Some(format!("<?php\nnamespace App;\nclass File{idx} {{}}\n")),
            )
        })
        .collect();
    let progress = std::sync::Mutex::new(Vec::new());

    backend.parse_files_parallel_with_progress(
        files,
        Some(&|done, total, done_units, total_units| {
            progress
                .lock()
                .expect("progress lock")
                .push((done, total, done_units, total_units));
        }),
    );

    for idx in 0..3 {
        assert!(
            backend
                .fqn_class_index
                .read()
                .contains_key(format!("App\\File{idx}").as_str())
        );
    }
    assert!(progress.lock().expect("progress lock").iter().any(
        |(done, total, done_units, total_units)| {
            *done == 3 && *total == 3 && *done_units == *total_units
        }
    ));
}

#[test]
fn parse_paths_parallel_with_progress_handles_small_batches_and_missing_files() {
    let dir = tempfile::tempdir().expect("temp dir");
    let first = dir.path().join("First.php");
    let missing = dir.path().join("Missing.php");
    std::fs::write(&first, "<?php\nnamespace App;\nclass First {}\n").expect("first file");

    let backend = Backend::new_test();
    let work = vec![
        (crate::util::path_to_uri(&first), first),
        (crate::util::path_to_uri(&missing), missing),
    ];
    let progress = std::sync::Mutex::new(Vec::new());
    backend.parse_paths_parallel_with_progress(
        &work,
        Some(&|done, total, done_units, total_units| {
            progress
                .lock()
                .expect("progress lock")
                .push((done, total, done_units, total_units));
        }),
    );

    assert!(backend.fqn_class_index.read().contains_key("App\\First"));
    assert!(progress.lock().expect("progress lock").iter().any(
        |(done, total, done_units, total_units)| {
            *done == 2 && *total == 2 && *done_units == *total_units
        }
    ));
}

#[test]
fn workspace_parse_percentage_handles_empty_and_weighted_totals() {
    assert_eq!(super::workspace_parse_percentage(0, 0), 95);
    assert_eq!(super::workspace_parse_percentage(0, 200), 5);
    assert_eq!(super::workspace_parse_percentage(100, 200), 50);
    assert_eq!(super::workspace_parse_percentage(200, 200), 95);
    assert_eq!(super::workspace_parse_percentage(500, 200), 95);
}

#[test]
fn index_progress_weight_prefers_supplied_and_open_file_content() {
    let backend = Backend::new_test();
    let uri = "file:///project/src/Open.php";

    assert_eq!(backend.index_progress_weight_for_uri(uri, Some("")), 1);

    backend
        .open_files
        .write()
        .insert(uri.to_string(), std::sync::Arc::new("abcdef".to_string()));
    assert_eq!(backend.index_progress_weight_for_uri(uri, None), 6);
}
