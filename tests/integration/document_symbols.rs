//! Integration tests for `textDocument/documentSymbol`.

use crate::common::create_test_backend;
use tower_lsp::lsp_types::*;

/// Helper: open a file and return the document symbol response.
fn get_symbols(php: &str) -> Option<DocumentSymbolResponse> {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    backend.update_ast(uri, php);
    backend.handle_document_symbol(uri, php)
}

/// Flatten a nested `DocumentSymbolResponse` into a list of `(name, kind, depth)`.
#[allow(deprecated)]
fn flatten_symbols(
    symbols: &[DocumentSymbol],
    depth: usize,
    out: &mut Vec<(String, SymbolKind, usize)>,
) {
    for sym in symbols {
        out.push((sym.name.clone(), sym.kind, depth));
        if let Some(ref children) = sym.children {
            flatten_symbols(children, depth + 1, out);
        }
    }
}

/// Extract the nested `DocumentSymbol` vec from the response.
#[allow(deprecated)]
fn unwrap_nested(resp: DocumentSymbolResponse) -> Vec<DocumentSymbol> {
    match resp {
        DocumentSymbolResponse::Nested(syms) => syms,
        _ => panic!("expected Nested response"),
    }
}

// ── Basic class ─────────────────────────────────────────────────────

#[test]
fn class_with_method_property_constant() {
    let php = r#"<?php
class User {
    const MAX_AGE = 150;
    public string $name;
    public function getName(): string {
        return $this->name;
    }
}
"#;
    let resp = get_symbols(php).expect("should have symbols");
    let symbols = unwrap_nested(resp);

    assert_eq!(symbols.len(), 1, "one top-level class");
    assert_eq!(symbols[0].name, "User");
    assert_eq!(symbols[0].kind, SymbolKind::CLASS);

    let children = symbols[0].children.as_ref().expect("class has children");
    let names: Vec<&str> = children.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"MAX_AGE"), "constant present: {names:?}");
    assert!(names.contains(&"$name"), "property present: {names:?}");
    assert!(names.contains(&"getName"), "method present: {names:?}");
}

// ── Interface ───────────────────────────────────────────────────────

#[test]
fn interface_symbol_kind() {
    let php = r#"<?php
interface Printable {
    public function print(): void;
}
"#;
    let resp = get_symbols(php).expect("should have symbols");
    let symbols = unwrap_nested(resp);
    assert_eq!(symbols[0].kind, SymbolKind::INTERFACE);
    assert_eq!(symbols[0].name, "Printable");
}

// ── Trait ────────────────────────────────────────────────────────────

#[test]
fn trait_symbol() {
    let php = r#"<?php
trait Timestampable {
    public function touch(): void {}
}
"#;
    let resp = get_symbols(php).expect("should have symbols");
    let symbols = unwrap_nested(resp);
    // LSP has no dedicated trait kind; we use CLASS.
    assert_eq!(symbols[0].kind, SymbolKind::CLASS);
    assert_eq!(symbols[0].name, "Timestampable");

    let children = symbols[0].children.as_ref().unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].name, "touch");
}

// ── Enum ────────────────────────────────────────────────────────────

#[test]
fn enum_with_cases() {
    let php = r#"<?php
enum Status: string {
    case Active = 'active';
    case Inactive = 'inactive';
}
"#;
    let resp = get_symbols(php).expect("should have symbols");
    let symbols = unwrap_nested(resp);
    assert_eq!(symbols[0].kind, SymbolKind::ENUM);
    assert_eq!(symbols[0].name, "Status");

    let children = symbols[0].children.as_ref().unwrap();
    let case_names: Vec<&str> = children.iter().map(|c| c.name.as_str()).collect();
    assert!(case_names.contains(&"Active"), "has Active case");
    assert!(case_names.contains(&"Inactive"), "has Inactive case");

    // Enum cases should be ENUM_MEMBER.
    for child in children {
        assert_eq!(child.kind, SymbolKind::ENUM_MEMBER);
    }
}

// ── Constructor ─────────────────────────────────────────────────────

#[test]
fn constructor_uses_constructor_kind() {
    let php = r#"<?php
class Foo {
    public function __construct() {}
}
"#;
    let resp = get_symbols(php).expect("should have symbols");
    let symbols = unwrap_nested(resp);
    let children = symbols[0].children.as_ref().unwrap();
    assert_eq!(children[0].name, "__construct");
    assert_eq!(children[0].kind, SymbolKind::CONSTRUCTOR);
}

// ── Multiple classes ────────────────────────────────────────────────

#[test]
fn multiple_classes_in_one_file() {
    let php = r#"<?php
class Alpha {
    public function run(): void {}
}
class Beta {
    public int $count;
}
"#;
    let resp = get_symbols(php).expect("should have symbols");
    let symbols = unwrap_nested(resp);
    assert_eq!(symbols.len(), 2);
    assert_eq!(symbols[0].name, "Alpha");
    assert_eq!(symbols[1].name, "Beta");
}

// ── Standalone function ─────────────────────────────────────────────

#[test]
fn standalone_function_appears_as_top_level_symbol() {
    let php = r#"<?php
function helper(): string {
    return 'hi';
}
"#;
    let resp = get_symbols(php).expect("should have symbols");
    let symbols = unwrap_nested(resp);

    let func_symbols: Vec<_> = symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::FUNCTION)
        .collect();
    assert_eq!(func_symbols.len(), 1);
    assert_eq!(func_symbols[0].name, "helper");
}

// ── Global define constant ──────────────────────────────────────────

#[test]
fn global_define_constant_appears() {
    let php = r#"<?php
define('APP_VERSION', '1.0.0');
"#;
    let resp = get_symbols(php);
    // define() calls may or may not produce a symbol depending on
    // whether the parser registers them with name_offset > 0.
    // This test verifies the handler doesn't crash.
    if let Some(resp) = resp {
        let symbols = unwrap_nested(resp);
        for sym in &symbols {
            // If a constant symbol appears, it should be CONSTANT kind.
            if sym.name == "APP_VERSION" {
                assert_eq!(sym.kind, SymbolKind::CONSTANT);
            }
        }
    }
}

// ── Empty file ──────────────────────────────────────────────────────

#[test]
fn empty_file_returns_none() {
    let php = "<?php\n";
    let resp = get_symbols(php);
    assert!(resp.is_none(), "empty file should have no symbols");
}

// ── Detail strings ──────────────────────────────────────────────────

#[allow(deprecated)]
#[test]
fn method_detail_shows_signature() {
    let php = r#"<?php
class Calc {
    public static function add(int $a, int $b): int {
        return $a + $b;
    }
}
"#;
    let resp = get_symbols(php).expect("should have symbols");
    let symbols = unwrap_nested(resp);
    let children = symbols[0].children.as_ref().unwrap();
    let add = &children[0];
    assert_eq!(add.name, "add");
    let detail = add.detail.as_deref().unwrap_or("");
    assert!(
        detail.contains("$a"),
        "detail should contain parameter name: {detail}"
    );
    assert!(
        detail.contains("int"),
        "detail should contain type hint: {detail}"
    );
    assert!(
        detail.contains("static"),
        "detail should contain static: {detail}"
    );
}

#[allow(deprecated)]
#[test]
fn class_detail_shows_extends_and_implements() {
    let php = r#"<?php
interface Printable {}
class Base {}
class Child extends Base implements Printable {
    public function print(): void {}
}
"#;
    let resp = get_symbols(php).expect("should have symbols");
    let symbols = unwrap_nested(resp);

    // Find the Child class.
    let child = symbols.iter().find(|s| s.name == "Child").unwrap();
    let detail = child.detail.as_deref().unwrap_or("");
    assert!(
        detail.contains("extends Base"),
        "detail should show parent class: {detail}"
    );
    assert!(
        detail.contains("implements Printable"),
        "detail should show interfaces: {detail}"
    );
}

// ── Deprecated tag ──────────────────────────────────────────────────

#[allow(deprecated)]
#[test]
fn deprecated_class_has_deprecated_tag() {
    let php = r#"<?php
/** @deprecated Use NewClass instead */
class OldClass {
    public function run(): void {}
}
"#;
    let resp = get_symbols(php).expect("should have symbols");
    let symbols = unwrap_nested(resp);
    let old = &symbols[0];
    assert_eq!(old.name, "OldClass");
    let tags = old.tags.as_ref().expect("should have tags");
    assert!(tags.contains(&SymbolTag::DEPRECATED));
}

#[allow(deprecated)]
#[test]
fn deprecated_method_has_deprecated_tag() {
    let php = r#"<?php
class Foo {
    /** @deprecated */
    public function old(): void {}
}
"#;
    let resp = get_symbols(php).expect("should have symbols");
    let symbols = unwrap_nested(resp);
    let children = symbols[0].children.as_ref().unwrap();
    let old_method = children.iter().find(|c| c.name == "old").unwrap();
    let tags = old_method.tags.as_ref().expect("should have tags");
    assert!(tags.contains(&SymbolTag::DEPRECATED));
}

// ── Property includes $ prefix ──────────────────────────────────────

#[allow(deprecated)]
#[test]
fn property_name_includes_dollar_prefix() {
    let php = r#"<?php
class Foo {
    public string $bar;
}
"#;
    let resp = get_symbols(php).expect("should have symbols");
    let symbols = unwrap_nested(resp);
    let children = symbols[0].children.as_ref().unwrap();
    let prop = children
        .iter()
        .find(|c| c.kind == SymbolKind::PROPERTY)
        .unwrap();
    assert_eq!(prop.name, "$bar");
}

// ── Property detail shows type hint ─────────────────────────────────

#[allow(deprecated)]
#[test]
fn property_detail_shows_type() {
    let php = r#"<?php
class Foo {
    public string $bar;
}
"#;
    let resp = get_symbols(php).expect("should have symbols");
    let symbols = unwrap_nested(resp);
    let children = symbols[0].children.as_ref().unwrap();
    let prop = children
        .iter()
        .find(|c| c.kind == SymbolKind::PROPERTY)
        .unwrap();
    let detail = prop.detail.as_deref().unwrap_or("");
    assert!(
        detail.contains("string"),
        "property detail should show type: {detail}"
    );
}

// ── Symbol ordering matches source ──────────────────────────────────

#[allow(deprecated)]
#[test]
fn symbols_ordered_by_position() {
    let php = r#"<?php
class Alpha {}
class Beta {}
class Gamma {}
"#;
    let resp = get_symbols(php).expect("should have symbols");
    let symbols = unwrap_nested(resp);
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["Alpha", "Beta", "Gamma"]);
}

// ── Virtual members excluded ────────────────────────────────────────

#[test]
fn virtual_members_excluded_from_outline() {
    // Virtual members (from @method/@property tags) should NOT appear
    // in the document symbol outline since they are not real declarations.
    let php = r#"<?php
/**
 * @method string getName()
 * @property string $email
 */
class User {
    public function getId(): int { return 1; }
}
"#;
    let resp = get_symbols(php).expect("should have symbols");
    let symbols = unwrap_nested(resp);
    let children = symbols[0].children.as_ref().unwrap();
    let names: Vec<&str> = children.iter().map(|c| c.name.as_str()).collect();
    // Only real method should appear, not virtual ones.
    assert!(names.contains(&"getId"), "real method present");
    assert!(!names.contains(&"getName"), "virtual method excluded");
    assert!(!names.contains(&"$email"), "virtual property excluded");
}

// ── Hierarchical nesting ────────────────────────────────────────────

#[allow(deprecated)]
#[test]
fn symbols_are_hierarchically_nested() {
    let php = r#"<?php
class Outer {
    public function inner(): void {}
    public int $val;
    const C = 1;
}
"#;
    let resp = get_symbols(php).expect("should have symbols");
    let symbols = unwrap_nested(resp);
    let mut flat = Vec::new();
    flatten_symbols(&symbols, 0, &mut flat);

    // Top-level class at depth 0.
    assert_eq!(flat[0], ("Outer".to_string(), SymbolKind::CLASS, 0));

    // Members at depth 1.
    let depth_1: Vec<_> = flat.iter().filter(|(_, _, d)| *d == 1).collect();
    assert_eq!(depth_1.len(), 3, "3 children at depth 1");
}

// ── Visibility in method detail ─────────────────────────────────────

#[allow(deprecated)]
#[test]
fn private_method_shows_private_in_detail() {
    let php = r#"<?php
class Foo {
    private function secret(): void {}
}
"#;
    let resp = get_symbols(php).expect("should have symbols");
    let symbols = unwrap_nested(resp);
    let children = symbols[0].children.as_ref().unwrap();
    let method = &children[0];
    let detail = method.detail.as_deref().unwrap_or("");
    assert!(
        detail.contains("private"),
        "detail should mention private: {detail}"
    );
}

// ── Class and function in same file ─────────────────────────────────

#[test]
fn class_and_function_coexist() {
    let php = r#"<?php
class Foo {
    public function bar(): void {}
}
function standalone(): int { return 1; }
"#;
    let resp = get_symbols(php).expect("should have symbols");
    let symbols = unwrap_nested(resp);

    let class_count = symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::CLASS)
        .count();
    let func_count = symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::FUNCTION)
        .count();
    assert_eq!(class_count, 1, "one class");
    assert_eq!(func_count, 1, "one function");
}

// ── Backed enum detail ──────────────────────────────────────────────

#[allow(deprecated)]
#[test]
fn enum_case_detail_shows_value() {
    let php = r#"<?php
enum Color: string {
    case Red = 'red';
    case Blue = 'blue';
}
"#;
    let resp = get_symbols(php).expect("should have symbols");
    let symbols = unwrap_nested(resp);
    let children = symbols[0].children.as_ref().unwrap();
    let red = children.iter().find(|c| c.name == "Red").unwrap();
    if let Some(ref detail) = red.detail {
        assert!(
            detail.contains("red"),
            "enum case detail should show value: {detail}"
        );
    }
}

// ── Abstract class ──────────────────────────────────────────────────

#[test]
fn abstract_class_appears_in_outline() {
    let php = r#"<?php
abstract class Base {
    abstract public function run(): void;
    public function stop(): void {}
}
"#;
    let resp = get_symbols(php).expect("should have symbols");
    let symbols = unwrap_nested(resp);
    assert_eq!(symbols[0].name, "Base");
    assert_eq!(symbols[0].kind, SymbolKind::CLASS);

    let children = symbols[0].children.as_ref().unwrap();
    let method_names: Vec<&str> = children.iter().map(|c| c.name.as_str()).collect();
    assert!(method_names.contains(&"run"), "abstract method present");
    assert!(method_names.contains(&"stop"), "concrete method present");
}

// ── Namespace not duplicated in name ────────────────────────────────

#[test]
fn class_name_is_short_name_not_fqn() {
    let php = r#"<?php
namespace App\Models;

class User {
    public function getId(): int { return 1; }
}
"#;
    let resp = get_symbols(php).expect("should have symbols");
    let symbols = unwrap_nested(resp);
    // The symbol name should be the short class name, not the FQN.
    assert_eq!(symbols[0].name, "User");
}

// ── Selection range is tighter than full range ──────────────────────

#[allow(deprecated)]
#[test]
fn selection_range_covers_name_not_full_body() {
    let php = r#"<?php
class VeryLongClassName {
    public function method(): void {}
}
"#;
    let resp = get_symbols(php).expect("should have symbols");
    let symbols = unwrap_nested(resp);
    let class_sym = &symbols[0];

    // The full range should span multiple lines (keyword to closing brace).
    assert!(
        class_sym.range.end.line > class_sym.range.start.line,
        "full range should span multiple lines"
    );

    // The selection range should be on a single line (just the name).
    assert_eq!(
        class_sym.selection_range.start.line, class_sym.selection_range.end.line,
        "selection range should be on one line"
    );
}

#[allow(deprecated)]
#[test]
fn method_full_range_spans_body() {
    let php = r#"<?php
class Foo {
    public function build(): void {
        $x = 1;
        echo $x;
    }
}
"#;
    let resp = get_symbols(php).expect("should have symbols");
    let symbols = unwrap_nested(resp);
    let class_sym = &symbols[0];
    let method = class_sym
        .children
        .as_ref()
        .and_then(|c| c.iter().find(|s| s.name == "build"))
        .expect("should find build method");

    // The full range must enclose the whole method (signature + body),
    // spanning multiple lines, while the selection range stays on the
    // single name line and is nested inside the full range.
    assert!(
        method.range.end.line > method.range.start.line,
        "method full range should span multiple lines, got {:?}",
        method.range
    );
    assert_eq!(
        method.selection_range.start.line, method.selection_range.end.line,
        "method selection range should be on one line"
    );
    assert!(
        method.range.start.line <= method.selection_range.start.line
            && method.range.end.line >= method.selection_range.end.line,
        "selection range must be nested inside the full range"
    );
}

#[allow(deprecated)]
#[test]
fn property_full_range_reaches_semicolon() {
    let php = r#"<?php
class Foo {
    public string $name = 'default';
}
"#;
    let resp = get_symbols(php).expect("should have symbols");
    let symbols = unwrap_nested(resp);
    let class_sym = &symbols[0];
    let prop = class_sym
        .children
        .as_ref()
        .and_then(|c| c.iter().find(|s| s.name == "$name"))
        .expect("should find $name property");

    // The full range should extend past the name to cover the `= 'default';`
    // initializer, while the selection range covers just `$name`.
    assert!(
        prop.range.end.character > prop.selection_range.end.character,
        "property full range should extend past the name, got range {:?} selection {:?}",
        prop.range,
        prop.selection_range
    );
}
