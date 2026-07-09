//! Integration tests for `textDocument/hover`.

use crate::common::{
    create_psr4_workspace, create_test_backend, create_test_backend_with_closure_stub,
    create_test_backend_with_full_stubs, create_test_backend_with_function_stubs,
    create_test_backend_with_stdclass_stub,
};
use phpantom_lsp::Backend;
use tower_lsp::lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Register file content in the backend (sync) and return the hover result
/// at the given (0-based) line and character.
fn hover_at(
    backend: &Backend,
    uri: &str,
    content: &str,
    line: u32,
    character: u32,
) -> Option<Hover> {
    // Parse and populate ast_map, use_map, namespace_map, symbol_maps
    backend.update_ast(uri, content);

    backend.handle_hover(uri, content, Position { line, character })
}

/// Extract the Markdown text from a Hover response.
fn hover_text(hover: &Hover) -> &str {
    match &hover.contents {
        HoverContents::Markup(markup) => &markup.value,
        _ => panic!("Expected MarkupContent"),
    }
}

// ─── Multi-namespace hover ──────────────────────────────────────────────────

/// Short class names in `@var` annotations resolve through the namespace-aware
/// class loader, not via a first-match scan of all classes in the file.  Without
/// this, multi-namespace files where several blocks define a class with the same
/// short name (e.g. `C`) would resolve to the wrong block's class.
#[test]
fn hover_generic_var_annotation_in_multi_namespace() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace NsOne {
    class A {}
    class B {}

    /**
     * @template T
     */
    class C {
        /** @var T */
        private $t;
        /** @param T $t */
        public function __construct($t) { $this->t = $t; }
        /** @return T */
        public function get() { return $this->t; }
    }

    function doTest(): void {
        /** @var C<A>|C<B> $random_collection **/
        $a_or_b = $random_collection->get();
        $a_or_b;
    }
}
"#;

    // Hover on `$a_or_b` at line 19 (inside function in NsOne namespace)
    let hover = hover_at(&backend, uri, content, 19, 9).expect("expected hover on $a_or_b");
    let text = hover_text(&hover);
    // $a_or_b should be A|B (the return type of C::get() with template substitution)
    assert!(
        text.contains("A") && text.contains("B"),
        "should resolve $a_or_b to A|B: {}",
        text
    );
}

/// Variables with the same name in different namespace blocks must not leak
/// across blocks.  The resolver must only walk the namespace block that
/// contains the cursor.
#[test]
fn hover_variable_shadowing_across_namespace_blocks() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace Block1 {
    class Dog { public function bark(): string { return 'woof'; } }
    $x = new Dog();
}

namespace Block2 {
    class Cat { public function meow(): string { return 'meow'; } }
    $x = new Cat();
    $x;
}
"#;

    // Hover on `$x` at line 9 (inside Block2, not Block1)
    let hover = hover_at(&backend, uri, content, 9, 5).expect("expected hover on $x");
    let text = hover_text(&hover);
    assert!(
        text.contains("Cat"),
        "should resolve to Cat, not Dog: {}",
        text
    );
    assert!(!text.contains("Dog"), "should not resolve to Dog: {}", text);
}

// ─── Variable hover ─────────────────────────────────────────────────────────

#[test]
fn hover_this_variable() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class User {
    public function greet(): string {
        return $this->name();
    }
}
"#;

    // Hover on `$this` (line 3, within the `$this` token)
    let hover = hover_at(&backend, uri, content, 3, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(text.contains("$this"), "should mention $this: {}", text);
    assert!(text.contains("User"), "should resolve to User: {}", text);
}

#[test]
fn hover_variable_with_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Order {
    public string $id;
}
class Service {
    public function run(): void {
        $order = new Order();
        $order->id;
    }
}
"#;

    // Hover on `$order` at line 7 (the usage)
    let hover = hover_at(&backend, uri, content, 7, 9).expect("expected hover");
    let text = hover_text(&hover);
    assert!(text.contains("$order"), "should mention $order: {}", text);
    assert!(text.contains("Order"), "should resolve to Order: {}", text);
}

#[test]
fn hover_variable_without_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function test() {
    $x = 42;
    echo $x;
}
"#;

    // Hover on `$x` at line 3
    let hover = hover_at(&backend, uri, content, 3, 10).expect("expected hover");
    let text = hover_text(&hover);
    assert!(text.contains("$x"), "should mention $x: {}", text);
}

#[test]
fn hover_ambiguous_variable_shows_union_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Lamp {
    public function dim(): void {}
    public function turnOff(): void {}
}

class Faucet {
    public function drip(): void {}
    public function turnOff(): void {}
}

class Consumer {
    public function run(): void {
        if (rand(0, 1)) {
            $ambiguous = new Lamp();
        } else {
            $ambiguous = new Faucet();
        }
        $ambiguous->turnOff();
    }
}
"#;

    // Hover on `$ambiguous` at line 18 (the usage after the if/else)
    let hover = hover_at(&backend, uri, content, 18, 9).expect("expected hover on $ambiguous");
    let text = hover_text(&hover);

    // Both union branches should appear.
    assert!(
        text.contains("Lamp") && text.contains("Faucet"),
        "hover should show both union types Lamp and Faucet, got: {}",
        text
    );

    // The two types should be rendered as separate code blocks
    // separated by a horizontal rule (`---`).
    assert!(
        text.contains("---"),
        "union hover should use a horizontal rule separator, got: {}",
        text
    );
}

#[test]
fn hover_ambiguous_variable_inside_if_branch_shows_single_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Lamp {
    public function dim(): void {}
    public function turnOff(): void {}
}

class Faucet {
    public function drip(): void {}
    public function turnOff(): void {}
}

class Consumer {
    public function run(): void {
        if (rand(0, 1)) {
            $ambiguous = new Lamp();
            $ambiguous->dim();
        } else {
            $ambiguous = new Faucet();
            $ambiguous->drip();
        }
        $ambiguous->turnOff();
    }
}
"#;

    // Hover on `$ambiguous` inside the if branch (line 15, the usage `$ambiguous->dim()`)
    let hover = hover_at(&backend, uri, content, 15, 13).expect("expected hover inside if branch");
    let text = hover_text(&hover);
    assert!(
        text.contains("Lamp"),
        "inside the if branch, should show Lamp: {}",
        text
    );
    assert!(
        !text.contains("Faucet"),
        "inside the if branch, should NOT show Faucet: {}",
        text
    );

    // Hover on `$ambiguous` inside the else branch (line 18, the usage `$ambiguous->drip()`)
    let hover =
        hover_at(&backend, uri, content, 18, 13).expect("expected hover inside else branch");
    let text = hover_text(&hover);
    assert!(
        text.contains("Faucet"),
        "inside the else branch, should show Faucet: {}",
        text
    );
    assert!(
        !text.contains("Lamp"),
        "inside the else branch, should NOT show Lamp: {}",
        text
    );

    // Hover on `$ambiguous` after the if/else (line 20, `$ambiguous->turnOff()`)
    let hover = hover_at(&backend, uri, content, 20, 9).expect("expected hover after if/else");
    let text = hover_text(&hover);
    assert!(
        text.contains("Lamp") && text.contains("Faucet"),
        "after the if/else, should show both Lamp and Faucet: {}",
        text
    );
}

#[test]
fn hover_union_member_access_shows_all_branches() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Lamp {
    public function dim(): void {}
    public function turnOff(): void {}
}

class Faucet {
    public function drip(): void {}
    public function turnOff(): void {}
}

class Consumer {
    public function run(): void {
        if (rand(0, 1)) {
            $ambiguous = new Lamp();
        } else {
            $ambiguous = new Faucet();
        }
        $ambiguous->turnOff();
    }
}
"#;

    // Hover on `turnOff` in `$ambiguous->turnOff()` (line 18)
    let hover = hover_at(&backend, uri, content, 18, 22).expect("expected hover on turnOff");
    let text = hover_text(&hover);

    // Both classes should appear since turnOff is independently declared
    // on each class (no common interface).
    assert!(
        text.contains("Lamp") && text.contains("Faucet"),
        "hover on union member should show both Lamp and Faucet, got: {}",
        text
    );

    // The two branches should be separated by a horizontal rule.
    assert!(
        text.contains("---"),
        "union member hover should use a horizontal rule separator, got: {}",
        text
    );
}

#[test]
fn hover_union_member_access_deduplicates_via_common_interface() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Switchable {
    public function turnOff(): void;
}

class Lamp implements Switchable {
    public function turnOff(): void {}
    public function dim(): void {}
}

class Faucet implements Switchable {
    public function turnOff(): void {}
    public function drip(): void {}
}

class Consumer {
    public function run(): void {
        if (rand(0, 1)) {
            $ambiguous = new Lamp();
        } else {
            $ambiguous = new Faucet();
        }
        $ambiguous->turnOff();
    }
}
"#;

    // Hover on `turnOff` in `$ambiguous->turnOff()` (line 22)
    let hover = hover_at(&backend, uri, content, 22, 22).expect("expected hover on turnOff");
    let text = hover_text(&hover);

    // Both Lamp and Faucet declare turnOff themselves (overriding the
    // interface), so both declaring classes should appear.
    assert!(
        text.contains("Lamp") && text.contains("Faucet"),
        "hover should show both Lamp and Faucet (each declares turnOff), got: {}",
        text
    );
}

#[test]
fn hover_union_member_access_shows_declaring_class_not_access_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class BaseDevice {
    public function turnOff(): void {}
}

class Lamp extends BaseDevice {
    public function dim(): void {}
}

class Faucet extends BaseDevice {
    public function drip(): void {}
}

class Consumer {
    public function run(): void {
        if (rand(0, 1)) {
            $ambiguous = new Lamp();
        } else {
            $ambiguous = new Faucet();
        }
        $ambiguous->turnOff();
    }
}
"#;

    // Hover on `turnOff` in `$ambiguous->turnOff()` (line 20)
    let hover = hover_at(&backend, uri, content, 20, 22).expect("expected hover on turnOff");
    let text = hover_text(&hover);

    // turnOff is declared on BaseDevice, inherited by both Lamp and
    // Faucet.  The hover should show BaseDevice (the declaring class)
    // and should NOT be duplicated since both branches resolve to the
    // same declaring class.
    assert!(
        text.contains("BaseDevice"),
        "hover should show declaring class BaseDevice, got: {}",
        text
    );
    assert!(
        !text.contains("---"),
        "should not have separator when both branches resolve to same declaring class, got: {}",
        text
    );
}

#[test]
fn hover_union_branch_only_member_shows_single_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Lamp {
    public function dim(): void {}
    public function turnOff(): void {}
}

class Faucet {
    public function drip(): void {}
    public function turnOff(): void {}
}

class Consumer {
    public function run(): void {
        if (rand(0, 1)) {
            $ambiguous = new Lamp();
        } else {
            $ambiguous = new Faucet();
        }
        $ambiguous->dim();
    }
}
"#;

    // Hover on `dim` in `$ambiguous->dim()` (line 18) — only Lamp has dim()
    let hover = hover_at(&backend, uri, content, 18, 22).expect("expected hover on dim");
    let text = hover_text(&hover);

    assert!(
        text.contains("Lamp"),
        "hover should show Lamp for branch-only member dim, got: {}",
        text
    );
    assert!(
        !text.contains("Faucet"),
        "hover should NOT show Faucet for dim (only on Lamp), got: {}",
        text
    );
    // No separator needed for a single-branch member.
    assert!(
        !text.contains("---"),
        "single-branch member should not have separator, got: {}",
        text
    );
}

#[test]
fn hover_active_on_parameter_definition_site() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Order { public string $id; }
class Service {
    public function process(Order $query, string $genre): void {
        $query;
    }
}
"#;

    // Hover on `$query` at the parameter definition site (line 3, col on $query)
    let hover = hover_at(&backend, uri, content, 3, 35)
        .expect("hover should be active on parameter $query");
    let text = hover_text(&hover);
    assert!(
        text.contains("$query"),
        "hover should show the parameter name: {}",
        text
    );
    assert!(
        text.contains("Order"),
        "hover should show the resolved type Order: {}",
        text
    );

    // Hover on `$genre` at the parameter definition site (line 3, col on $genre)
    let hover = hover_at(&backend, uri, content, 3, 50)
        .expect("hover should be active on parameter $genre");
    let text = hover_text(&hover);
    assert!(
        text.contains("$genre") && text.contains("string"),
        "hover should show the parameter name and type: {}",
        text
    );
}

#[test]
fn hover_parameter_definition_shows_docblock_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Pen { public function write(): string { return ''; } }
class Drawer {
    /** @param list<Pen> $pens The pens to use. */
    public function fill(array $pens): void {
        $pens;
    }
}
"#;

    // Hover on `$pens` at the parameter definition site (line 4)
    let hover = hover_at(&backend, uri, content, 4, 33)
        .expect("hover should be active on parameter $pens with docblock type");
    let text = hover_text(&hover);
    assert!(
        text.contains("$pens"),
        "hover should show the parameter name: {}",
        text
    );
}

#[test]
fn hover_parameter_definition_standalone_function() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Pen { public function write(): string { return ''; } }
/** @param Pen $tool The writing instrument. */
function draw(Pen $tool): void {
    $tool;
}
"#;

    // Hover on `$tool` at the parameter definition site (line 3)
    let hover = hover_at(&backend, uri, content, 3, 19)
        .expect("hover should be active on standalone function parameter $tool");
    let text = hover_text(&hover);
    assert!(
        text.contains("$tool"),
        "hover should show the parameter name: {}",
        text
    );
    assert!(
        text.contains("Pen"),
        "hover should show the type Pen: {}",
        text
    );
}

#[test]
fn hover_active_on_foreach_variable_definition_site() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Item { public string $name; }
class Service {
    /** @param Item[] $items */
    public function run(array $items): void {
        foreach ($items as $item) {
            $item->name;
        }
    }
}
"#;

    // Hover on `$item` at the foreach binding site (line 5)
    let hover = hover_at(&backend, uri, content, 5, 29)
        .expect("hover should be active on foreach variable $item");
    let text = hover_text(&hover);
    assert!(text.contains("Item"), "should resolve to Item: {}", text);
}

#[test]
fn hover_active_on_catch_variable_definition_site() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function risky(): void {
    try {
        throw new \Exception('oops');
    } catch (\Exception $e) {
        echo $e->getMessage();
    }
}
"#;

    // Hover on `$e` at the catch binding site (line 4)
    let hover = hover_at(&backend, uri, content, 4, 26)
        .expect("hover should be active on catch variable $e");
    let text = hover_text(&hover);
    assert!(
        text.contains("Exception"),
        "should resolve to Exception: {}",
        text
    );
}

#[test]
fn hover_active_on_variable_assignment() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Order { public string $id; }
class Service {
    public function run(): void {
        $order = new Order();
        $order->id;
    }
}
"#;

    // Hover on `$order` at the assignment site (line 4) should still work
    let hover = hover_at(&backend, uri, content, 4, 9)
        .expect("hover should be active on assignment $order");
    let text = hover_text(&hover);
    assert!(text.contains("Order"), "should resolve to Order: {}", text);
}

// ─── Method hover ───────────────────────────────────────────────────────────

#[test]
fn hover_method_call() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Calculator {
    public function add(int $a, int $b): int {
        return $a + $b;
    }
    public function run(): void {
        $this->add(1, 2);
    }
}
"#;

    // Hover on `add` in `$this->add(1, 2)` (line 6)
    let hover = hover_at(&backend, uri, content, 6, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(text.contains("add"), "should contain method name: {}", text);
    assert!(text.contains("int $a"), "should show params: {}", text);
    assert!(text.contains(": int"), "should show return type: {}", text);
    assert!(
        text.contains("Calculator"),
        "should show owner class: {}",
        text
    );
}

#[test]
fn hover_static_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Factory {
    public static function create(string $name): self {
        return new self();
    }
}
class Usage {
    public function run(): void {
        Factory::create('test');
    }
}
"#;

    // Hover on `create` in `Factory::create` (line 8)
    let hover = hover_at(&backend, uri, content, 8, 18).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("create"),
        "should contain method name: {}",
        text
    );
    assert!(text.contains("static"), "should indicate static: {}", text);
    assert!(
        text.contains("string $name"),
        "should show params: {}",
        text
    );
}

// ─── Property hover ─────────────────────────────────────────────────────────

#[test]
fn hover_property_access() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Config {
    public string $name;
    public function show(): void {
        echo $this->name;
    }
}
"#;

    // Hover on `name` in `$this->name` (line 4)
    let hover = hover_at(&backend, uri, content, 4, 21).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("name"),
        "should contain property name: {}",
        text
    );
    assert!(text.contains("string"), "should show type: {}", text);
    assert!(text.contains("Config"), "should show owner: {}", text);
}

#[test]
fn hover_static_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Registry {
    public static int $count;
}
class Usage {
    public function run(): void {
        echo Registry::$count;
    }
}
"#;

    // Hover on `$count` in `Registry::$count` (line 6)
    let hover = hover_at(&backend, uri, content, 6, 24).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("count"),
        "should contain property name: {}",
        text
    );
    assert!(text.contains("static"), "should indicate static: {}", text);
    assert!(text.contains("int"), "should show type: {}", text);
}

// ─── Constant hover ─────────────────────────────────────────────────────────

#[test]
fn hover_class_constant() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Status {
    const ACTIVE = 'active';
}
class Usage {
    public function run(): void {
        echo Status::ACTIVE;
    }
}
"#;

    // Hover on `ACTIVE` in `Status::ACTIVE` (line 6)
    let hover = hover_at(&backend, uri, content, 6, 22).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("ACTIVE"),
        "should contain constant name: {}",
        text
    );
    assert!(text.contains("Status"), "should show owner: {}", text);
}

// ─── Class hover ────────────────────────────────────────────────────────────

#[test]
fn hover_class_reference() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Animal {
    public string $species;
}
class Zoo {
    public function adopt(Animal $pet): void {}
}
"#;

    // Hover on `Animal` in the type hint (line 5)
    let hover = hover_at(&backend, uri, content, 5, 28).expect("expected hover");
    let text = hover_text(&hover);
    assert!(text.contains("class"), "should show class kind: {}", text);
    assert!(text.contains("Animal"), "should show class name: {}", text);
}

#[test]
fn hover_interface_reference() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Printable {
    public function print(): void;
}
class Document implements Printable {
    public function print(): void {}
}
"#;

    // Hover on `Printable` in the implements clause (line 4)
    let hover = hover_at(&backend, uri, content, 4, 32).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("interface"),
        "should show interface kind: {}",
        text
    );
    assert!(
        text.contains("Printable"),
        "should show interface name: {}",
        text
    );
}

#[test]
fn hover_interface_extending_interface_no_duplicate_extends() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @template TKey
 * @template-covariant TValue
 * @template-extends iterable<TKey, TValue>
 */
interface Traversable extends iterable {}

function test(Traversable $t): void {}
"#;

    // Hover on `Traversable` in the function parameter (line 8)
    let hover = hover_at(&backend, uri, content, 8, 17).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("interface Traversable extends iterable"),
        "should show extends once: {}",
        text
    );
    // Must NOT contain the keyword "extends" twice
    let extends_count = text.matches("extends").count();
    assert_eq!(
        extends_count, 1,
        "should contain 'extends' exactly once, got {}: {}",
        extends_count, text
    );
}

#[test]
fn hover_class_declaration_returns_none() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * Represents a blog post.
 */
class BlogPost {
    public string $title;
}
"#;

    // Hover on `BlogPost` at its declaration site should return None —
    // the user is already looking at the definition.
    let hover = hover_at(&backend, uri, content, 4, 8);
    assert!(
        hover.is_none(),
        "should not show hover on class declaration site"
    );
}

#[test]
fn hover_class_declaration_disambiguates_by_namespace_returns_none() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace Demo {
    class Builder {
        public function demo(): void {}
    }
}

namespace Illuminate\Contracts\Database\Eloquent {
    /**
     * @mixin \Illuminate\Database\Eloquent\Builder
     */
    interface Builder {}
}
"#;

    // Hover on declaration sites should return None.
    let hover = hover_at(&backend, uri, content, 11, 16);
    assert!(
        hover.is_none(),
        "should not show hover on interface declaration site"
    );

    let hover = hover_at(&backend, uri, content, 2, 12);
    assert!(
        hover.is_none(),
        "should not show hover on class declaration site"
    );
}

#[test]
fn hover_abstract_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
abstract class Shape {
    abstract public function area(): float;
}
class Circle extends Shape {
    public function area(): float { return 3.14; }
}
"#;

    // Hover on `Shape` in extends clause (line 4)
    let hover = hover_at(&backend, uri, content, 4, 23).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("abstract class"),
        "should show abstract class: {}",
        text
    );
    assert!(text.contains("Shape"), "should show class name: {}", text);
}

#[test]
fn hover_final_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
final class Singleton {
    public static function instance(): self { return new self(); }
}
function test(Singleton $s): void {}
"#;

    // Hover on `Singleton` in function param (line 4)
    let hover = hover_at(&backend, uri, content, 4, 17).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("final class"),
        "should show final class: {}",
        text
    );
}

// ─── Self / static / parent hover ───────────────────────────────────────────

#[test]
fn hover_self_keyword() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public static function make(): self {
        return new self();
    }
}
"#;

    // Hover on `self` at line 3 inside `new self()`
    let hover = hover_at(&backend, uri, content, 3, 20).expect("expected hover");
    let text = hover_text(&hover);
    assert!(text.contains("self"), "should mention self: {}", text);
    assert!(text.contains("Foo"), "should resolve to Foo: {}", text);
}

#[test]
fn hover_parent_keyword() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    public function hello(): string { return 'hi'; }
}
class Child extends Base {
    public function hello(): string {
        return parent::hello();
    }
}
"#;

    // Hover on `parent` at line 6
    let hover = hover_at(&backend, uri, content, 6, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(text.contains("parent"), "should mention parent: {}", text);
    assert!(text.contains("Base"), "should resolve to Base: {}", text);
}

// ─── Function call hover ────────────────────────────────────────────────────

#[test]
fn hover_user_function() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function greet(string $name): string {
    return "Hello, $name!";
}
greet('World');
"#;

    // Hover on `greet` at line 4
    let hover = hover_at(&backend, uri, content, 4, 2).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("greet"),
        "should contain function name: {}",
        text
    );
    assert!(
        text.contains("string $name"),
        "should show params: {}",
        text
    );
    assert!(
        text.contains(": string"),
        "should show return type: {}",
        text
    );
}

// ─── Deprecated marker ──────────────────────────────────────────────────────

#[test]
fn hover_deprecated_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Legacy {
    /**
     * @deprecated Use newMethod() instead.
     */
    public function oldMethod(): void {}
    public function run(): void {
        $this->oldMethod();
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 7, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("oldMethod"),
        "should contain method name: {}",
        text
    );
    assert!(
        text.contains("🪦 **deprecated** Use newMethod() instead."),
        "should show deprecated with message: {}",
        text
    );
}

#[test]
fn hover_deprecated_method_without_message() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Legacy {
    /**
     * @deprecated
     */
    public function oldMethod(): void {}
    public function run(): void {
        $this->oldMethod();
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 7, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("🪦 **deprecated**"),
        "should show bare deprecated: {}",
        text
    );
    // Should NOT contain any message text after the label
    assert!(
        !text.contains("🪦 **deprecated** "),
        "should not have trailing text after deprecated: {}",
        text
    );
}

#[test]
fn hover_deprecated_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @deprecated Use NewApi instead.
 */
class OldApi {
    public function run(): void {}
}
function test(OldApi $api): void {}
"#;

    // Hover on OldApi in function param (line 7)
    let hover = hover_at(&backend, uri, content, 7, 17).expect("expected hover");
    let text = hover_text(&hover);
    assert!(text.contains("OldApi"), "should show class name: {}", text);
    assert!(
        text.contains("🪦 **deprecated** Use NewApi instead."),
        "should show deprecated with message: {}",
        text
    );
}

#[test]
fn hover_deprecated_property_shows_message() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Config {
    /**
     * @deprecated Use getDebugMode() instead.
     */
    public bool $debug = false;

    public function test(): void {
        $this->debug;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 8, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("🪦 **deprecated** Use getDebugMode() instead."),
        "should show deprecated with message: {}",
        text
    );
}

#[test]
fn hover_deprecated_constant_shows_message() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class HttpStatus {
    /**
     * @deprecated Use OK instead.
     */
    const SUCCESS = 200;

    const OK = 200;
}
$x = HttpStatus::SUCCESS;
"#;

    let hover = hover_at(&backend, uri, content, 9, 20).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("🪦 **deprecated** Use OK instead."),
        "should show deprecated with message: {}",
        text
    );
}

#[test]
fn hover_deprecated_function_shows_message() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @deprecated Use newHelper() instead.
 */
function oldHelper(): void {}

oldHelper();
"#;

    let hover = hover_at(&backend, uri, content, 6, 4).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("🪦 **deprecated** Use newHelper() instead."),
        "should show deprecated with message: {}",
        text
    );
}

// ─── Cross-file hover ───────────────────────────────────────────────────────

#[test]
fn hover_cross_file_class() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": { "App\\": "src/" }
            }
        }"#,
        &[
            (
                "src/Models/Product.php",
                r#"<?php
namespace App\Models;
/**
 * Represents a product in the catalog.
 */
class Product {
    public string $name;
    public float $price;
    public function discount(float $percent): float {
        return $this->price * (1 - $percent / 100);
    }
}
"#,
            ),
            (
                "src/Service.php",
                r#"<?php
namespace App;
use App\Models\Product;
class Service {
    public function run(): void {
        $p = new Product();
        $p->discount(10);
    }
}
"#,
            ),
        ],
    );

    let product_uri = format!(
        "file://{}",
        _dir.path().join("src/Models/Product.php").display()
    );
    let product_content =
        std::fs::read_to_string(_dir.path().join("src/Models/Product.php")).unwrap();
    backend.update_ast(&product_uri, &product_content);

    let service_uri = format!("file://{}", _dir.path().join("src/Service.php").display());
    let service_content = std::fs::read_to_string(_dir.path().join("src/Service.php")).unwrap();

    // Hover on `Product` type reference (line 5: `$p = new Product()`)
    let hover = hover_at(&backend, &service_uri, &service_content, 5, 20)
        .expect("expected hover on Product");
    let text = hover_text(&hover);
    assert!(
        text.contains("Product"),
        "should resolve cross-file class: {}",
        text
    );
    assert!(
        text.contains("Represents a product"),
        "should include docblock from cross-file class: {}",
        text
    );
}

#[test]
fn hover_cross_file_method() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": { "App\\": "src/" }
            }
        }"#,
        &[
            (
                "src/Models/Item.php",
                r#"<?php
namespace App\Models;
class Item {
    public function getLabel(): string {
        return 'label';
    }
}
"#,
            ),
            (
                "src/Handler.php",
                r#"<?php
namespace App;
use App\Models\Item;
class Handler {
    public function process(): void {
        $item = new Item();
        $item->getLabel();
    }
}
"#,
            ),
        ],
    );

    let item_uri = format!(
        "file://{}",
        _dir.path().join("src/Models/Item.php").display()
    );
    let item_content = std::fs::read_to_string(_dir.path().join("src/Models/Item.php")).unwrap();
    backend.update_ast(&item_uri, &item_content);

    let handler_uri = format!("file://{}", _dir.path().join("src/Handler.php").display());
    let handler_content = std::fs::read_to_string(_dir.path().join("src/Handler.php")).unwrap();

    // Hover on `getLabel` (line 6)
    let hover = hover_at(&backend, &handler_uri, &handler_content, 6, 16)
        .expect("expected hover on getLabel");
    let text = hover_text(&hover);
    assert!(
        text.contains("getLabel"),
        "should resolve cross-file method: {}",
        text
    );
    assert!(
        text.contains(": string"),
        "should show return type: {}",
        text
    );
    assert!(text.contains("Item"), "should show owner class: {}", text);
}

// ─── Cross-file cache invalidation ─────────────────────────────────────────

#[test]
fn hover_cross_file_docblock_updated_after_edit() {
    // When a cross-file class is loaded via PSR-4 and its docblock is
    // later edited (simulated via update_ast), hover should show the NEW
    // description, not the stale cached version.
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": { "App\\": "src/" }
            }
        }"#,
        &[
            (
                "src/Job.php",
                r#"<?php
namespace App;
class Job {
    /** Original description. */
    public function run(): void {}
}
"#,
            ),
            (
                "src/Worker.php",
                r#"<?php
namespace App;
class Worker {
    public function execute(): void {
        $j = new Job();
        $j->run();
    }
}
"#,
            ),
        ],
    );

    let job_uri = format!("file://{}", _dir.path().join("src/Job.php").display());
    let job_content_v1 = std::fs::read_to_string(_dir.path().join("src/Job.php")).unwrap();
    backend.update_ast(&job_uri, &job_content_v1);

    let worker_uri = format!("file://{}", _dir.path().join("src/Worker.php").display());
    let worker_content = std::fs::read_to_string(_dir.path().join("src/Worker.php")).unwrap();

    // Initial hover shows the original description.
    let hover =
        hover_at(&backend, &worker_uri, &worker_content, 5, 13).expect("expected hover on run()");
    let text = hover_text(&hover);
    assert!(
        text.contains("Original description"),
        "initial hover should show original docblock, got: {}",
        text
    );

    // Simulate editing Job.php: change the docblock description.
    let job_content_v2 = r#"<?php
namespace App;
class Job {
    /** Updated description after edit. */
    public function run(): void {}
}
"#;
    backend.update_ast(&job_uri, job_content_v2);

    // Hover again — should show the updated description.
    let hover = hover_at(&backend, &worker_uri, &worker_content, 5, 13)
        .expect("expected hover on run() after edit");
    let text = hover_text(&hover);
    assert!(
        text.contains("Updated description after edit"),
        "hover should show updated docblock after edit, got: {}",
        text
    );
}

#[test]
fn hover_cross_file_property_type_updated_after_edit() {
    // When a cross-file class @property type changes, hover on a
    // variable accessing that property should reflect the new type.
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": { "App\\": "src/" }
            }
        }"#,
        &[
            (
                "src/Config.php",
                r#"<?php
namespace App;
class Config {
    public string $value = '';
}
"#,
            ),
            (
                "src/Reader.php",
                r#"<?php
namespace App;
class Reader {
    public function read(): void {
        $c = new Config();
        $c->value;
    }
}
"#,
            ),
        ],
    );

    let config_uri = format!("file://{}", _dir.path().join("src/Config.php").display());
    let config_v1 = std::fs::read_to_string(_dir.path().join("src/Config.php")).unwrap();
    backend.update_ast(&config_uri, &config_v1);

    let reader_uri = format!("file://{}", _dir.path().join("src/Reader.php").display());
    let reader_content = std::fs::read_to_string(_dir.path().join("src/Reader.php")).unwrap();

    // Initial hover on $c->value shows string type.
    let hover =
        hover_at(&backend, &reader_uri, &reader_content, 5, 13).expect("expected hover on value");
    let text = hover_text(&hover);
    assert!(
        text.contains("string"),
        "initial hover should show string type, got: {}",
        text
    );

    // Edit Config.php: change property type from string to int.
    let config_v2 = r#"<?php
namespace App;
class Config {
    public int $value = 0;
}
"#;
    backend.update_ast(&config_uri, config_v2);

    // Hover again — should show int, not string.
    let hover = hover_at(&backend, &reader_uri, &reader_content, 5, 13)
        .expect("expected hover on value after edit");
    let text = hover_text(&hover);
    assert!(
        text.contains("int"),
        "hover should show updated type after edit, got: {}",
        text
    );
    assert!(
        !text.contains("string"),
        "hover should NOT show stale string type, got: {}",
        text
    );
}

// ─── Enum hover ─────────────────────────────────────────────────────────────

#[test]
fn hover_enum_declaration() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * Possible statuses for an order.
 */
enum OrderStatus: string {
    case Pending = 'pending';
    case Shipped = 'shipped';
}
function process(OrderStatus $status): void {}
"#;

    // Hover on `OrderStatus` in the function param (line 8)
    let hover = hover_at(&backend, uri, content, 8, 20).expect("expected hover");
    let text = hover_text(&hover);
    assert!(text.contains("enum"), "should show enum kind: {}", text);
    assert!(
        text.contains("OrderStatus"),
        "should show enum name: {}",
        text
    );
    assert!(
        text.contains("Possible statuses"),
        "should include docblock: {}",
        text
    );
}

// ─── Trait hover ────────────────────────────────────────────────────────────

#[test]
fn hover_trait_reference() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * Provides soft-delete functionality.
 */
trait SoftDeletes {
    public function trash(): void {}
}
class Post {
    use SoftDeletes;
}
"#;

    // Hover on `SoftDeletes` in the use statement (line 8)
    let hover = hover_at(&backend, uri, content, 8, 10).expect("expected hover");
    let text = hover_text(&hover);
    assert!(text.contains("trait"), "should show trait kind: {}", text);
    assert!(
        text.contains("SoftDeletes"),
        "should show trait name: {}",
        text
    );
    assert!(
        text.contains("Provides soft-delete"),
        "should include docblock: {}",
        text
    );
}

// ─── Visibility display ─────────────────────────────────────────────────────

#[test]
fn hover_shows_visibility() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Vault {
    private string $secret;
    protected int $level;
    public function getSecret(): string {
        echo $this->secret;
        echo $this->level;
        return $this->secret;
    }
}
"#;

    // Hover on `secret` property (line 5)
    let hover = hover_at(&backend, uri, content, 5, 22).expect("expected hover on secret");
    let text = hover_text(&hover);
    assert!(
        text.contains("private"),
        "should show private visibility: {}",
        text
    );

    // Hover on `level` property (line 6)
    let hover = hover_at(&backend, uri, content, 6, 22).expect("expected hover on level");
    let text = hover_text(&hover);
    assert!(
        text.contains("protected"),
        "should show protected visibility: {}",
        text
    );
}

// ─── Inheritance hover ──────────────────────────────────────────────────────

#[test]
fn hover_inherited_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class BaseRepo {
    public function findAll(): array {
        return [];
    }
}
class UserRepo extends BaseRepo {
    public function run(): void {
        $this->findAll();
    }
}
"#;

    // Hover on `findAll` in the child class (line 8)
    let hover = hover_at(&backend, uri, content, 8, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("findAll"),
        "should show inherited method: {}",
        text
    );
    assert!(
        text.contains(": array"),
        "should show return type: {}",
        text
    );
    // The code block should show the declaring class (BaseRepo),
    // not the class the method was accessed on (UserRepo).
    assert!(
        text.contains("BaseRepo"),
        "should show declaring class BaseRepo, got: {}",
        text
    );
    assert!(
        !text.contains("class UserRepo"),
        "should NOT show UserRepo as the owner class, got: {}",
        text
    );
}

/// Hovering over an inherited static method should show the declaring
/// class in the code block, not the subclass it was called on.
#[test]
fn hover_inherited_static_method_shows_declaring_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
abstract class Model {
    /** @deprecated */
    public static function find(int $id): ?static { return null; }
}
class User extends Model {
    public function toArray(): array { return []; }
}
function demo(): void {
    User::find(1);
}
"#;

    // Hover on `find` (line 9, col 11)
    let hover = hover_at(&backend, uri, content, 9, 11).expect("expected hover");
    let text = hover_text(&hover);
    assert!(text.contains("find"), "should show method name: {}", text);
    assert!(
        text.contains("class Model"),
        "should show declaring class Model, not User, got: {}",
        text
    );
    assert!(
        !text.contains("class User"),
        "should NOT show User as the owner class, got: {}",
        text
    );
}

// ─── Class with parent and implements ───────────────────────────────────────

#[test]
fn hover_class_with_extends_and_implements() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Loggable {
    public function log(): void;
}
class Base {}
class App extends Base implements Loggable {
    public function log(): void {}
}
function test(App $app): void {}
"#;

    // Hover on `App` in the function parameter (line 8)
    let hover = hover_at(&backend, uri, content, 8, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(text.contains("class App"), "should show class: {}", text);
    // Parent/interface names may have a leading `\` from the parser
    assert!(
        text.contains("extends") && text.contains("Base"),
        "should show parent: {}",
        text
    );
    assert!(
        text.contains("implements") && text.contains("Loggable"),
        "should show interfaces: {}",
        text
    );
}

// ─── No hover on whitespace ─────────────────────────────────────────────────

#[test]
fn hover_on_whitespace_returns_none() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php

class Foo {}
"#;

    // Hover on the blank line (line 1)
    let hover = hover_at(&backend, uri, content, 1, 0);
    assert!(hover.is_none(), "should not produce hover on blank line");
}

// ─── Stub function hover ────────────────────────────────────────────────────

#[test]
fn hover_stub_function() {
    let backend = create_test_backend_with_function_stubs();
    let uri = "file:///test.php";
    let content = r#"<?php
$x = str_contains('hello', 'ell');
"#;

    // Hover on `str_contains` (line 1)
    let hover = hover_at(&backend, uri, content, 1, 8).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("str_contains"),
        "should show function name: {}",
        text
    );
    assert!(
        text.contains("string $haystack"),
        "should show params: {}",
        text
    );
    assert!(text.contains(": bool"), "should show return type: {}", text);
}

// ─── Namespaced class hover ─────────────────────────────────────────────────

#[test]
fn hover_shows_fqn() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App\Models;

/**
 * A customer entity.
 */
class Customer {
    public string $email;
}

class Service {
    public function run(): void {
        $c = new Customer();
        $c->email;
    }
}
"#;

    // Hover on Customer reference at line 12
    let hover = hover_at(&backend, uri, content, 12, 18).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("namespace App\\Models;"),
        "should show namespace line: {}",
        text
    );
    assert!(
        text.contains("class Customer"),
        "should show short class name: {}",
        text
    );
    assert!(
        text.contains("A customer entity"),
        "should include docblock: {}",
        text
    );
}

// ─── Method with reference and variadic params ──────────────────────────────

#[test]
fn hover_method_with_reference_param() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Sorter {
    public function sort(array &$items): void {}
    public function run(): void {
        $this->sort([]);
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 4, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("&$items"),
        "should show reference param: {}",
        text
    );
}

#[test]
fn hover_method_with_variadic_param() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Logger {
    public function log(string ...$messages): void {}
    public function run(): void {
        $this->log('a', 'b');
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 4, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("...$messages"),
        "should show variadic param: {}",
        text
    );
}

// ─── Docblock array/object shape type hover ─────────────────────────────────

/// Hovering on a class name inside an array shape value type in a docblock
/// should resolve the class and show hover info.
#[test]
fn hover_class_in_array_shape_value_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Pen {
    public string $color;
}
/**
 * @return array{logger: Pen, debug: bool}
 */
function getAppConfig(): array { return []; }
"#;

    // Hover on `Pen` inside the array shape (line 5, find "Pen" after "logger: ")
    let hover =
        hover_at(&backend, uri, content, 5, 25).expect("expected hover on Pen in array shape");
    let text = hover_text(&hover);
    assert!(
        text.contains("Pen"),
        "should resolve Pen inside array shape, got: {}",
        text
    );
    assert!(
        text.contains("class"),
        "should show class kind for Pen, got: {}",
        text
    );
}

// ─── Docblock callable type hover ───────────────────────────────────────────

/// Hovering on a class name in a callable return type inside a docblock
/// should show the class info, not treat the whole callable as one token.
#[test]
fn hover_class_in_callable_return_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Pencil {
    public string $color;
}
class Factory {
    /** @var \Closure(): Pencil $supplier */
    private $supplier;
}
"#;

    // Hover on `Pencil` in `\Closure(): Pencil` (line 5, character ~29)
    let hover = hover_at(&backend, uri, content, 5, 29).expect("expected hover on Pencil");
    let text = hover_text(&hover);
    assert!(
        text.contains("Pencil"),
        "should show Pencil class: {}",
        text
    );
    assert!(
        !text.contains("Closure(): Pencil"),
        "should not treat whole callable as class name: {}",
        text
    );
}

/// Hovering on a class name used as a callable parameter type in a docblock.
#[test]
fn hover_class_in_callable_param_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Request {
    public string $body;
}
class Response {
    public int $status;
}
class Handler {
    /** @var callable(Request): Response $handler */
    private $handler;
}
"#;

    // Hover on `Request` in `callable(Request)` (line 8)
    let hover = hover_at(&backend, uri, content, 8, 24).expect("expected hover on Request");
    let text = hover_text(&hover);
    assert!(
        text.contains("Request"),
        "should show Request class: {}",
        text
    );

    // Hover on `Response` in callable return type (line 8)
    let hover = hover_at(&backend, uri, content, 8, 34).expect("expected hover on Response");
    let text = hover_text(&hover);
    assert!(
        text.contains("Response"),
        "should show Response class: {}",
        text
    );
}

/// Hovering on `\Closure` itself inside a callable annotation.
#[test]
fn hover_closure_base_in_callable_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Result {}
class Worker {
    /** @param \Closure(int): Result $cb */
    public function run($cb) {}
}
"#;

    // Hover on `Result` in `\Closure(int): Result` (line 3)
    let hover = hover_at(&backend, uri, content, 3, 35).expect("expected hover on Result");
    let text = hover_text(&hover);
    assert!(
        text.contains("Result"),
        "should show Result class: {}",
        text
    );
}

// ─── Docblock description in hover ──────────────────────────────────────────

#[test]
fn hover_property_shows_docblock_description() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Zoo {
    /** @var list<string> The animal names */
    public array $animals;
    public function show(): void {
        echo $this->animals;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 5, 22).expect("expected hover on animals");
    let text = hover_text(&hover);
    assert!(
        text.contains("The animal names"),
        "should include docblock description: {}",
        text
    );
    assert!(
        text.contains("@var list<string>"),
        "should show effective docblock type as @var annotation: {}",
        text
    );
}

#[test]
fn hover_method_shows_docblock_description() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Greeter {
    /**
     * Say hello to someone.
     * @param string $name The person's name
     * @return string
     */
    public function greet(string $name): string {
        return "Hello, $name!";
    }
    public function run(): void {
        $this->greet('World');
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 11, 16).expect("expected hover on greet");
    let text = hover_text(&hover);
    assert!(
        text.contains("Say hello to someone."),
        "should include method docblock description: {}",
        text
    );
}

#[test]
fn hover_constant_shows_docblock_description() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Config {
    /** The maximum retry count. */
    const MAX_RETRIES = 3;
}
class Worker {
    public function run(): void {
        echo Config::MAX_RETRIES;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 7, 22).expect("expected hover on MAX_RETRIES");
    let text = hover_text(&hover);
    assert!(
        text.contains("The maximum retry count."),
        "should include constant docblock description: {}",
        text
    );
}

// ─── Native vs effective type display ───────────────────────────────────────

#[test]
fn hover_property_shows_native_type_in_code_block_and_effective_as_annotation() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace Demo;
class Pen {
    public string $color;
}
class ScaffoldingIteration {
    /** @var list<Pen> The batches */
    public array $batch;
    public function show(): void {
        echo $this->batch;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 9, 22).expect("expected hover on batch");
    let text = hover_text(&hover);

    // The effective (docblock) type should appear as a @var annotation with short names
    assert!(
        text.contains("@var list<Pen>"),
        "should show effective docblock type as @var annotation with short names: {}",
        text
    );
    // The description should appear
    assert!(
        text.contains("The batches"),
        "should show docblock description: {}",
        text
    );
    // The code block should use the native PHP type hint
    assert!(
        text.contains("public array $batch;"),
        "should show native type in PHP code block: {}",
        text
    );
    // The member should be wrapped with namespace line + short class name
    assert!(
        text.contains("namespace Demo;"),
        "should show namespace line: {}",
        text
    );
    assert!(
        text.contains("class ScaffoldingIteration {"),
        "should show short owning class name: {}",
        text
    );
}

#[test]
fn hover_property_without_docblock_type_shows_native_in_both() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Simple {
    public string $name;
    public function show(): void {
        echo $this->name;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 4, 22).expect("expected hover on name");
    let text = hover_text(&hover);
    assert!(
        text.contains("public string $name;"),
        "should show native type in code block: {}",
        text
    );
}

#[test]
fn hover_method_shows_namespace_and_short_names_in_code_block() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App;
class User {
    public string $email;
}
class UserRepo {
    /**
     * Find all users.
     * @return list<User>
     */
    public function findAll(): array {
        return [];
    }
    public function run(): void {
        $this->findAll();
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 14, 16).expect("expected hover on findAll");
    let text = hover_text(&hover);

    // The effective (docblock) return type should appear in the return section
    assert!(
        text.contains("**return** `list<User>`"),
        "should show effective return type with short names in return section: {}",
        text
    );
    // The code block should use the native PHP return type
    assert!(
        text.contains("function findAll(): array;"),
        "should show native return type in PHP code block: {}",
        text
    );
    // Description
    assert!(
        text.contains("Find all users."),
        "should show method docblock description: {}",
        text
    );
    // The method should be wrapped in the owning class
    assert!(
        text.contains("namespace App;"),
        "should show namespace line: {}",
        text
    );
    assert!(
        text.contains("class UserRepo {"),
        "should show short owning class name: {}",
        text
    );
}

#[test]
fn hover_contains_php_open_tag() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Box {
    public int $size;
    public function show(): void {
        echo $this->size;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 4, 22).expect("expected hover on size");
    let text = hover_text(&hover);
    assert!(
        text.contains("<?php"),
        "should contain <?php marker in code block: {}",
        text
    );
}

#[test]
fn hover_function_shows_description_and_native_return() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * Calculate the sum of values.
 * @param list<int> $values
 * @return int
 */
function total(array $values): int {
    return array_sum($values);
}
total([1, 2, 3]);
"#;

    let hover = hover_at(&backend, uri, content, 9, 2).expect("expected hover on total");
    let text = hover_text(&hover);
    assert!(
        text.contains("Calculate the sum of values."),
        "should show function docblock description: {}",
        text
    );
    assert!(
        text.contains("<?php"),
        "should contain <?php marker: {}",
        text
    );
}

// ─── Variable hover format ──────────────────────────────────────────────────

#[test]
fn hover_variable_shows_type_in_code_block() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Order {
    public string $id;
}
class Service {
    public function run(): void {
        $order = new Order();
        $order->id;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 7, 9).expect("expected hover on $order");
    let text = hover_text(&hover);
    // Code block should show variable = type inside <?php block
    assert!(
        text.contains("$order = Order"),
        "should show variable with type in code block: {}",
        text
    );
    assert!(
        text.contains("<?php"),
        "should contain <?php marker: {}",
        text
    );
}

#[test]
fn hover_variable_without_type_shows_php_tag() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function test() {
    $x = 42;
    echo $x;
}
"#;

    let hover = hover_at(&backend, uri, content, 3, 10).expect("expected hover on $x");
    let text = hover_text(&hover);
    assert!(
        text.contains("<?php"),
        "should contain <?php marker for unresolved variable: {}",
        text
    );
}

// ─── self / static / parent / $this hover format ────────────────────────────

#[test]
fn hover_self_shows_namespace_and_short_name_in_code_block() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App;
class Foo {
    public static function make(): self {
        return new self();
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 4, 20).expect("expected hover on self");
    let text = hover_text(&hover);
    assert!(
        text.contains("namespace App;"),
        "should show namespace line: {}",
        text
    );
    assert!(
        text.contains("self = Foo"),
        "should show self = short name in code block: {}",
        text
    );
    assert!(
        text.contains("<?php"),
        "should contain <?php marker: {}",
        text
    );
}

#[test]
fn hover_parent_shows_fqn_in_header_and_code_block() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App;
class Base {
    public function hello(): string { return 'hi'; }
}
class Child extends Base {
    public function hello(): string {
        return parent::hello();
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 7, 16).expect("expected hover on parent");
    let text = hover_text(&hover);
    assert!(text.contains("parent"), "should mention parent: {}", text);
    assert!(
        text.contains("<?php"),
        "should contain <?php marker: {}",
        text
    );
}

#[test]
fn hover_this_shows_namespace_and_short_name_in_code_block() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App;
class Widget {
    public function run(): void {
        $this->run();
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 4, 9).expect("expected hover on $this");
    let text = hover_text(&hover);
    assert!(
        text.contains("namespace App;"),
        "should show namespace line: {}",
        text
    );
    assert!(
        text.contains("$this = Widget"),
        "should show $this = short name in code block: {}",
        text
    );
    assert!(
        text.contains("<?php"),
        "should contain <?php marker: {}",
        text
    );
}

#[test]
fn hover_self_includes_class_docblock() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * A reusable widget.
 */
class Widget {
    public static function make(): self {
        return new self();
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 6, 20).expect("expected hover on self");
    let text = hover_text(&hover);
    assert!(
        text.contains("A reusable widget."),
        "should include class docblock description: {}",
        text
    );
}

#[test]
fn hover_self_shows_deprecated_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @deprecated Use NewWidget instead.
 */
class OldWidget {
    public static function make(): self {
        return new self();
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 6, 20).expect("expected hover on self");
    let text = hover_text(&hover);
    assert!(
        text.contains("🪦 **deprecated** Use NewWidget instead."),
        "should show deprecated with message: {}",
        text
    );
}

// ─── Constant reference hover format ────────────────────────────────────────

#[test]
fn hover_class_constant_shows_php_tag_and_const_syntax() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Config {
    const APP_VERSION = '1.0.0';
}
class Usage {
    public function run(): void {
        echo Config::APP_VERSION;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 6, 24).expect("expected hover on APP_VERSION");
    let text = hover_text(&hover);
    assert!(
        text.contains("<?php"),
        "should contain <?php marker: {}",
        text
    );
    assert!(
        text.contains("const APP_VERSION = '1.0.0';"),
        "should show const declaration with value: {}",
        text
    );
    // Constant should be wrapped in its owning class
    assert!(
        text.contains("class Config {"),
        "should show owning class wrapper: {}",
        text
    );
}

#[test]
fn hover_class_constant_shows_integer_value() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Limits {
    const MAX_RETRIES = 3;
}
$x = Limits::MAX_RETRIES;
"#;

    let hover = hover_at(&backend, uri, content, 4, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("const MAX_RETRIES = 3;"),
        "should show integer value: {}",
        text
    );
}

#[test]
fn hover_class_constant_shows_array_value() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Config {
    const ALLOWED = ['a', 'b', 'c'];
}
$x = Config::ALLOWED;
"#;

    let hover = hover_at(&backend, uri, content, 4, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("const ALLOWED = ['a', 'b', 'c'];"),
        "should show array value: {}",
        text
    );
}

#[test]
fn hover_typed_constant_shows_type_and_value() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Config {
    const string APP_NAME = 'PHPantom';
}
$x = Config::APP_NAME;
"#;

    let hover = hover_at(&backend, uri, content, 4, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("const APP_NAME: string = 'PHPantom';"),
        "should show type hint and value: {}",
        text
    );
}

#[test]
fn hover_constant_via_self_shows_value() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Config {
    const TIMEOUT = 30;
    public function get(): int {
        return self::TIMEOUT;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 4, 22).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("const TIMEOUT = 30;"),
        "should show value via self::: {}",
        text
    );
}

#[test]
fn hover_constant_expression_value() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Math {
    const TWO_PI = 2 * 3.14159;
}
$x = Math::TWO_PI;
"#;

    let hover = hover_at(&backend, uri, content, 4, 14).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("const TWO_PI = 2 * 3.14159;"),
        "should show expression value: {}",
        text
    );
}

// ─── Native param types in code block ───────────────────────────────────────

#[test]
fn hover_method_shows_native_param_types_in_code_block() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App;
class User {
    public string $email;
}
class UserRepo {
    /**
     * Find users by criteria.
     * @param list<User> $criteria
     * @return list<User>
     */
    public function find(array $criteria): array {
        return [];
    }
    public function run(): void {
        $this->find([]);
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 15, 16).expect("expected hover on find");
    let text = hover_text(&hover);
    // The PHP code block should show the native param type (array), not the docblock type
    assert!(
        text.contains("function find(array $criteria)"),
        "should show native param type 'array' in PHP code block: {}",
        text
    );
}

#[test]
fn hover_function_shows_native_param_types_in_code_block() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class User {
    public string $name;
}
/**
 * Process users.
 * @param list<User> $users
 */
function processUsers(array $users): void {}
processUsers([]);
"#;

    let hover = hover_at(&backend, uri, content, 9, 2).expect("expected hover on processUsers");
    let text = hover_text(&hover);
    // The PHP code block should show the native param type (array), not the docblock type
    assert!(
        text.contains("function processUsers(array $users)"),
        "should show native param type 'array' in PHP code block: {}",
        text
    );
}

// ─── Unresolved fallback hover format ───────────────────────────────────────

#[test]
fn hover_unresolved_function_returns_none() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
unknownFunction();
"#;

    backend.update_ast(uri, content);
    let hover = backend.handle_hover(
        uri,
        content,
        Position {
            line: 1,
            character: 5,
        },
    );
    assert!(
        hover.is_none(),
        "hover on unknown function should return None"
    );
}

#[test]
fn hover_unresolved_class_returns_none() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
new IAmNotReal();
"#;

    backend.update_ast(uri, content);
    let hover = backend.handle_hover(
        uri,
        content,
        Position {
            line: 1,
            character: 6,
        },
    );
    assert!(hover.is_none(), "hover on unknown class should return None");
}

// ─── @param description tests ───────────────────────────────────────────────

#[test]
fn hover_function_shows_param_descriptions() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * Process a batch of items.
 * @param list<string> $items The items to process.
 * @param bool $force Whether to force processing.
 */
function processBatch(array $items, bool $force = false): void {}
processBatch([]);
"#;

    let hover = hover_at(&backend, uri, content, 7, 2).expect("expected hover on processBatch");
    let text = hover_text(&hover);
    assert!(
        text.contains("**$items** `list<string>`"),
        "should show param name and effective type: {}",
        text
    );
    assert!(
        text.contains("The items to process."),
        "should show param description: {}",
        text
    );
}

#[test]
fn hover_method_shows_param_descriptions() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Processor {
    /**
     * Process a single item.
     * @param list<int> $ids The IDs to process.
     * @return bool
     */
    public function process(array $ids): bool {
        return true;
    }
    public function run(): void {
        $this->process([]);
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 11, 16).expect("expected hover on process");
    let text = hover_text(&hover);
    assert!(
        text.contains("**$ids** `list<int>`"),
        "should show param name and effective type for method: {}",
        text
    );
    assert!(
        text.contains("The IDs to process."),
        "should show param description for method: {}",
        text
    );
}

// ─── @param suppression tests ───────────────────────────────────────────────

#[test]
fn hover_param_not_shown_when_native_equals_effective_and_no_description() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * Simple function.
 * @param string $name
 */
function greet(string $name): void {}
greet('World');
"#;

    let hover = hover_at(&backend, uri, content, 6, 2).expect("expected hover on greet");
    let text = hover_text(&hover);
    assert!(
        !text.contains("@param"),
        "should NOT show @param when native == effective and no description: {}",
        text
    );
}

#[test]
fn hover_function_param_union_type_on_assignment_lhs() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {}
class Bar {}

/**
 * @param Foo|Bar $x
 */
function doFoo($x)
{
    $y = $x;
}
"#;

    // Hover on `$y` at line 9 — on the LHS of the assignment itself.
    // This mirrors the assertType runner pattern where the hover is
    // on the variable at the assignment line.
    let hover = hover_at(&backend, uri, content, 9, 5).expect("expected hover on $y at assignment");
    let text = hover_text(&hover);
    assert!(
        text.contains("Foo") && text.contains("Bar"),
        "should resolve union param on assignment LHS to Foo|Bar, got: {}",
        text
    );
}

#[test]
fn hover_param_shown_when_type_differs_but_no_description() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * Takes a list.
 * @param list<int> $items
 */
function sum(array $items): int { return 0; }
sum([]);
"#;

    let hover = hover_at(&backend, uri, content, 6, 2).expect("expected hover on sum");
    let text = hover_text(&hover);
    assert!(
        text.contains("**$items** `list<int>`"),
        "should show param when effective type differs from native even without description: {}",
        text
    );
}

// ─── @return description tests ──────────────────────────────────────────────

#[test]
fn hover_method_shows_return_description() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Repo {
    /**
     * Find all records.
     * @return list<string> The matching records.
     */
    public function findAll(): array {
        return [];
    }
    public function run(): void {
        $this->findAll();
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 10, 16).expect("expected hover on findAll");
    let text = hover_text(&hover);
    assert!(
        text.contains("**return** `list<string>`"),
        "should show return type: {}",
        text
    );
    assert!(
        text.contains("The matching records."),
        "should show return description: {}",
        text
    );
}

#[test]
fn hover_function_shows_return_description() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * Get all names.
 * @return list<string> All available names.
 */
function getNames(): array { return []; }
getNames();
"#;

    let hover = hover_at(&backend, uri, content, 6, 2).expect("expected hover on getNames");
    let text = hover_text(&hover);
    assert!(
        text.contains("**return** `list<string>`"),
        "should show return type for standalone function: {}",
        text
    );
    assert!(
        text.contains("All available names."),
        "should show return description for standalone function: {}",
        text
    );
}

// ─── @link URL tests ────────────────────────────────────────────────────────

#[test]
fn hover_function_shows_link_url() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * Map over an array.
 * @link https://php.net/manual/en/function.array-map.php
 * @param callable $callback The callback.
 * @return array The mapped array.
 */
function my_map(callable $callback, array $items): array { return []; }
my_map(fn($x) => $x, []);
"#;

    let hover = hover_at(&backend, uri, content, 8, 2).expect("expected hover on my_map");
    let text = hover_text(&hover);
    assert!(
        text.contains("https://php.net/manual/en/function.array-map.php"),
        "should show @link URL in hover output: {}",
        text
    );
    // The URL should appear outside the code block (before it)
    let url_pos = text
        .find("https://php.net/manual/en/function.array-map.php")
        .unwrap();
    let code_pos = text.find("```php").unwrap();
    assert!(
        url_pos < code_pos,
        "URL should appear before the code block: {}",
        text
    );
}

#[test]
fn hover_method_shows_link_url() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Helper {
    /**
     * Do something useful.
     * @link https://example.com/docs
     */
    public function doStuff(): void {}
    public function run(): void {
        $this->doStuff();
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 8, 16).expect("expected hover on doStuff");
    let text = hover_text(&hover);
    assert!(
        text.contains("https://example.com/docs"),
        "should show @link URL for method hover: {}",
        text
    );
}

// ─── Combined annotations test ──────────────────────────────────────────────

#[test]
fn hover_function_shows_combined_param_and_return_annotations() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * Transform items.
 * @link https://example.com/transform
 * @param list<int> $items The input items.
 * @param callable $fn The transform function.
 * @return list<string> The transformed items.
 */
function transform(array $items, callable $fn): array { return []; }
transform([], fn($x) => (string)$x);
"#;

    let hover = hover_at(&backend, uri, content, 9, 2).expect("expected hover on transform");
    let text = hover_text(&hover);
    assert!(
        text.contains("Transform items."),
        "should show description: {}",
        text
    );
    assert!(
        text.contains("https://example.com/transform"),
        "should show link URL: {}",
        text
    );
    assert!(
        text.contains("**$items** `list<int>`"),
        "should show param for items: {}",
        text
    );
    assert!(
        text.contains("The input items."),
        "should show param description for items: {}",
        text
    );
    assert!(
        text.contains("**return** `list<string>`"),
        "should show return type: {}",
        text
    );
    assert!(
        text.contains("The transformed items."),
        "should show return description: {}",
        text
    );
    assert!(
        text.contains("function transform(array $items, callable $fn): array;"),
        "should show native signature: {}",
        text
    );
}

// ─── Param with description but same type ───────────────────────────────────

#[test]
fn hover_param_shown_when_types_match_but_has_description() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * Say hello.
 * @param string $name The person's name to greet.
 */
function sayHello(string $name): void {}
sayHello('Alice');
"#;

    let hover = hover_at(&backend, uri, content, 6, 2).expect("expected hover on sayHello");
    let text = hover_text(&hover);
    assert!(
        text.contains("**$name** The person's name to greet."),
        "should show param with description when types match: {}",
        text
    );
}

// ─── Docblock type shown even when matching native type ─────────────────────

#[test]
fn hover_shows_docblock_param_and_return_when_types_match_native() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * Applies the callback to the elements of the given arrays
 * @link https://php.net/manual/en/function.array-map.php
 * @param callable|null $callback Callback function to run for each element in each array.
 * @param array $array An array to run through the callback function.
 * @param array ...$arrays
 * @return array an array containing all the elements of arr1
 * after applying the callback function to each one.
 */
function array_map(?callable $callback, array $array, array ...$arrays): array {}
array_map(null, []);
"#;

    let hover = hover_at(&backend, uri, content, 11, 2).expect("expected hover on array_map");
    let text = hover_text(&hover);

    // Description
    assert!(
        text.contains("Applies the callback to the elements of the given arrays"),
        "should show description: {}",
        text
    );

    // Link
    assert!(
        text.contains("https://php.net/manual/en/function.array-map.php"),
        "should show link URL: {}",
        text
    );

    // $callback's docblock type `callable|null` is semantically equivalent to
    // native `?callable`, so types match — description only, no backtick type.
    assert!(
        text.contains("**$callback** Callback function to run for each element in each array."),
        "should show $callback with description (types match after nullable normalisation): {}",
        text
    );
    // $array's types match (array == array), so description only.
    assert!(
        text.contains("**$array** An array to run through the callback function."),
        "should show $array with description (types match): {}",
        text
    );

    // $arrays has a @param tag but no description and types match — should NOT show.
    assert!(
        !text.contains("**$arrays**"),
        "should NOT show $arrays param entry (no description, types match): {}",
        text
    );

    // @return types match (array == array), so description only.
    assert!(
        text.contains("**return** an array containing all the elements of arr1 after applying the callback function to each one."),
        "should show return with description (types match): {}",
        text
    );

    // The code block should use native types.
    assert!(
        text.contains(
            "function array_map(?callable $callback, array $array, array ...$arrays): array;"
        ),
        "should show native signature in code block: {}",
        text
    );
}

// ─── Rich callable signature differs from native ────────────────────────────

#[test]
fn hover_shows_rich_callable_type_when_docblock_refines_native() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * Applies the callback to the elements of the given arrays
 * @param (callable(mixed $item): mixed)|null $callback Callback function to run for each element.
 * @param array $array An array to run through the callback function.
 * @return array the mapped array.
 */
function array_map(?callable $callback, array $array): array {}
array_map(null, []);
"#;

    let hover = hover_at(&backend, uri, content, 8, 2).expect("expected hover on array_map");
    let text = hover_text(&hover);

    // $callback's effective type `(callable(mixed $item): mixed)|null` is richer
    // than native `?callable`, so it shows with backtick type + description.
    assert!(
        text.contains("**$callback** `(callable(mixed): mixed)|null`"),
        "should show $callback with rich effective type: {}",
        text
    );
    assert!(
        text.contains("Callback function to run for each element."),
        "should show $callback description: {}",
        text
    );
}

// ─── @var annotation suppression for equivalent types ───────────────────────

#[test]
fn hover_property_suppresses_var_when_effective_is_fqn_of_native() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace Demo;
class Brush {
    public string $color;
}
class Easel {
    /** @var Brush */
    public Brush $brush;
    public function show(): void {
        echo $this->brush;
    }
}
"#;

    // Hover on `brush` in `$this->brush` (line 9)
    let hover = hover_at(&backend, uri, content, 9, 22).expect("expected hover on brush");
    let text = hover_text(&hover);
    // The effective type is `Demo\Brush` and the native type is `Brush`.
    // These refer to the same class, so the @var annotation should be suppressed.
    assert!(
        !text.contains("@var"),
        "should NOT show @var when effective type is just FQN of native type: {}",
        text
    );
    assert!(
        text.contains("public Brush $brush;"),
        "should still show native type in code block: {}",
        text
    );
}

#[test]
fn hover_property_shows_var_when_effective_genuinely_differs() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace Demo;
class Pen {
    public string $color;
}
class Drawer {
    /** @var list<Pen> */
    public array $pens;
    public function show(): void {
        echo $this->pens;
    }
}
"#;

    // Hover on `pens` in `$this->pens` (line 9)
    let hover = hover_at(&backend, uri, content, 9, 22).expect("expected hover on pens");
    let text = hover_text(&hover);
    // The effective type `list<Demo\Pen>` genuinely differs from the native `array`.
    assert!(
        text.contains("@var list<Pen>"),
        "should show @var with short names when effective type genuinely differs from native: {}",
        text
    );
}

#[test]
fn hover_property_suppresses_var_when_fqn_with_leading_backslash() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App;
class Widget {}
class Factory {
    /** @var \App\Widget */
    public Widget $widget;
    public function show(): void {
        echo $this->widget;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 7, 22).expect("expected hover on widget");
    let text = hover_text(&hover);
    assert!(
        !text.contains("@var"),
        "should suppress @var for FQN with leading backslash: {}",
        text
    );
}

#[test]
fn hover_method_suppresses_return_annotation_when_fqn_matches_native() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace Demo;
class Item {}
class Store {
    /** @return Item */
    public function getItem(): Item { return new Item(); }
    public function run(): void {
        $this->getItem();
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 7, 16).expect("expected hover on getItem");
    let text = hover_text(&hover);
    // The effective return type `Demo\Item` is just FQN of native `Item`.
    // The return annotation should be suppressed.
    assert!(
        !text.contains("**return**"),
        "should suppress return annotation when FQN matches native: {}",
        text
    );
}

#[test]
fn hover_method_shows_return_annotation_when_types_genuinely_differ() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace Demo;
class Item {}
class Store {
    /** @return list<Item> */
    public function getItems(): array { return []; }
    public function run(): void {
        $this->getItems();
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 7, 16).expect("expected hover on getItems");
    let text = hover_text(&hover);
    assert!(
        text.contains("**return** `list<Item>`"),
        "should show return annotation with short names when effective genuinely differs: {}",
        text
    );
}

// ─── new ClassName hover ────────────────────────────────────────────────────

#[test]
fn hover_new_class_shows_constructor() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Widget {
    /**
     * Create a new Widget.
     *
     * @param string $name The widget name
     */
    public function __construct(string $name) {}

    public function run(): void {}
}

function demo(): void {
    $w = new Widget("hello");
}
"#;

    // Hover on `Widget` in `new Widget("hello")` (line 13, "Widget" starts at col 14)
    let hover = hover_at(&backend, uri, content, 13, 15).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("__construct"),
        "should show __construct method, got: {}",
        text
    );
    assert!(
        text.contains("string $name"),
        "should show constructor params: {}",
        text
    );
    assert!(
        text.contains("Create a new Widget"),
        "should show constructor description: {}",
        text
    );
}

#[test]
fn hover_new_class_shows_constructor_default_values() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Zoo {
    public function __construct(
        int $buffalo = 0,
        string $name = 'default',
        ?array $items = null,
        bool $active = true
    ) {}
}

function demo(): void {
    $z = new Zoo();
}
"#;

    // Hover on `Zoo` in `new Zoo()` (line 11)
    let hover = hover_at(&backend, uri, content, 11, 15).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("__construct"),
        "should show __construct: {}",
        text
    );
    assert!(
        text.contains("int $buffalo = 0"),
        "should show int default value, got: {}",
        text
    );
    assert!(
        text.contains("string $name = 'default'"),
        "should show string default value, got: {}",
        text
    );
    assert!(
        text.contains("?array $items = null"),
        "should show null default value, got: {}",
        text
    );
    assert!(
        text.contains("bool $active = true"),
        "should show bool default value, got: {}",
        text
    );
    // Should NOT contain `= ...` placeholder
    assert!(
        !text.contains("= ..."),
        "should not contain placeholder '= ...', got: {}",
        text
    );
}

#[test]
fn hover_method_shows_default_values() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Formatter {
    public function format(string $text, int $indent = 4, string $sep = ', '): string {
        return $text;
    }
    public function run(): void {
        $this->format('hello');
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 6, 16).expect("expected hover on format");
    let text = hover_text(&hover);
    assert!(
        text.contains("int $indent = 4"),
        "should show int default: {}",
        text
    );
    assert!(
        text.contains("string $sep = ', '"),
        "should show string default: {}",
        text
    );
    assert!(
        !text.contains("= ..."),
        "should not contain placeholder: {}",
        text
    );
}

#[test]
fn hover_method_shows_array_default_value() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Config {
    public function load(array $options = []): void {}
    public function run(): void {
        $this->load();
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 4, 16).expect("expected hover on load");
    let text = hover_text(&hover);
    assert!(
        text.contains("array $options = []"),
        "should show empty array default: {}",
        text
    );
}

#[test]
fn hover_class_reference_without_new_shows_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Widget {
    public function __construct(string $name) {}
}

function demo(Widget $w): void {}
"#;

    // Hover on `Widget` in the parameter type hint (line 5, "Widget" starts at col 15)
    let hover = hover_at(&backend, uri, content, 5, 17).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("class"),
        "should show class kind, got: {}",
        text
    );
    assert!(
        !text.contains("__construct"),
        "should NOT show __construct for a type-hint reference, got: {}",
        text
    );
}

#[test]
fn hover_new_class_without_constructor_shows_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class EmptyClass {}

function demo(): void {
    $e = new EmptyClass();
}
"#;

    // Hover on `EmptyClass` in `new EmptyClass()` (line 4)
    let hover = hover_at(&backend, uri, content, 4, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("class"),
        "should fall back to class hover when no __construct: {}",
        text
    );
    assert!(
        text.contains("EmptyClass"),
        "should show class name: {}",
        text
    );
}

#[test]
fn hover_new_class_shows_inherited_constructor() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    /** Build a base instance. */
    public function __construct(int $id) {}
}
class Child extends Base {}

function demo(): void {
    $c = new Child(42);
}
"#;

    // Hover on `Child` in `new Child(42)` (line 8)
    let hover = hover_at(&backend, uri, content, 8, 14).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("__construct"),
        "should show inherited __construct: {}",
        text
    );
    assert!(
        text.contains("int $id"),
        "should show inherited constructor params: {}",
        text
    );
}

#[test]
fn hover_static_method_context_shows_class_not_constructor() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Factory {
    public function __construct(string $name) {}
    public static function create(): self { return new self("x"); }
}

function demo(): void {
    Factory::create();
}
"#;

    // Hover on `Factory` in `Factory::create()` (line 7) — NOT a `new` context
    let hover = hover_at(&backend, uri, content, 7, 5).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("class"),
        "should show class hover for static access, got: {}",
        text
    );
    assert!(
        !text.contains("__construct"),
        "should NOT show __construct for static access context, got: {}",
        text
    );
}

// ─── Class template display ─────────────────────────────────────────────────

/// Hovering a generic class shows its template parameters with variance and bounds.
#[test]
fn hover_class_shows_template_params() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @template TKey
 * @template TValue
 */
class Collection {
    /** @return TValue */
    public function first(): mixed { return null; }
}

function test(Collection $c): void {}
"#;

    // Hover on `Collection` in the function parameter (line 10)
    let hover = hover_at(&backend, uri, content, 10, 17).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("**template** `TKey`"),
        "should show TKey template param, got: {}",
        text
    );
    assert!(
        text.contains("**template** `TValue`"),
        "should show TValue template param, got: {}",
        text
    );
}

#[test]
fn hover_class_shows_covariant_template_with_bound() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @template TKey of array-key
 * @template-covariant TValue of object
 */
class TypedMap {}

function test(TypedMap $m): void {}
"#;

    // Hover on `TypedMap` in the function parameter (line 7)
    let hover = hover_at(&backend, uri, content, 7, 17).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("**template** `TKey` of `array-key`"),
        "should show TKey with bound, got: {}",
        text
    );
    assert!(
        text.contains("**template-covariant** `TValue` of `object`"),
        "should show TValue as covariant with bound, got: {}",
        text
    );
}

#[test]
fn hover_class_shows_contravariant_template() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @template-contravariant TInput
 */
class Consumer {}

function test(Consumer $c): void {}
"#;

    // Hover on `Consumer` in the function parameter (line 6)
    let hover = hover_at(&backend, uri, content, 6, 17).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("**template-contravariant** `TInput`"),
        "should show TInput as contravariant, got: {}",
        text
    );
}

#[test]
fn hover_interface_shows_template_params() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @template TKey
 * @template-covariant TValue
 * @template-extends iterable<TKey, TValue>
 */
interface Traversable extends iterable {}

function test(Traversable $t): void {}
"#;

    // Hover on `Traversable` in the function parameter (line 8)
    let hover = hover_at(&backend, uri, content, 8, 17).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("**template** `TKey`"),
        "should show TKey template param, got: {}",
        text
    );
    assert!(
        text.contains("**template-covariant** `TValue`"),
        "should show TValue as covariant, got: {}",
        text
    );
}

#[test]
fn hover_template_param_shows_covariant_variance() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @template-covariant TValue
 */
class Box {
    /** @return TValue */
    public function get(): mixed { return null; }
}
"#;

    // Hover on `TValue` in `@return TValue` (line 5)
    let hover = hover_at(&backend, uri, content, 5, 19).expect("expected hover on TValue");
    let text = hover_text(&hover);
    assert!(
        text.contains("**template-covariant**"),
        "should show covariant variance, got: {}",
        text
    );
    assert!(
        text.contains("`TValue`"),
        "should show the template name, got: {}",
        text
    );
}

// ─── Template parameter hover ───────────────────────────────────────────────

/// Hovering a template parameter name in a docblock type position should
/// show `**template** \`TKey\` of \`array-key\`` rather than `class TKey`.
#[test]
fn hover_template_param_in_callable_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @template TKey of array-key
 * @template TValue
 */
class Collection {
    /**
     * @param callable(TValue, TKey): mixed $callback
     * @return static
     */
    public function each(callable $callback): static { return $this; }
}
"#;

    // Hover on `TKey` inside the callable param type (line 7)
    // `callable(TValue, TKey): mixed` — TKey starts around character 30
    let hover = hover_at(&backend, uri, content, 7, 31).expect("expected hover on TKey");
    let text = hover_text(&hover);
    assert!(
        text.contains("**template**"),
        "should show template hover, got: {}",
        text
    );
    assert!(
        text.contains("`TKey`"),
        "should show the template name, got: {}",
        text
    );
    assert!(
        text.contains("`array-key`"),
        "should show the bound type, got: {}",
        text
    );
    assert!(
        !text.contains("class TKey"),
        "should NOT show 'class TKey', got: {}",
        text
    );
}

/// Template parameter without an `of` bound shows just the name.
#[test]
fn hover_template_param_without_bound() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @template TValue
 */
class Box {
    /** @return TValue */
    public function get(): mixed { return null; }
}
"#;

    // Hover on `TValue` in `@return TValue` (line 5)
    let hover = hover_at(&backend, uri, content, 5, 19).expect("expected hover on TValue");
    let text = hover_text(&hover);
    assert!(
        text.contains("**template**"),
        "should show template hover, got: {}",
        text
    );
    assert!(
        text.contains("`TValue`"),
        "should show the template name, got: {}",
        text
    );
    assert!(
        !text.contains(" of "),
        "should NOT show 'of' when there is no bound, got: {}",
        text
    );
}

/// Template parameter with a class-like bound shows the bound.
#[test]
fn hover_template_param_with_class_bound() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Animal {}
/**
 * @template T of Animal
 */
class Zoo {
    /** @return T */
    public function first(): mixed { return null; }
}
"#;

    // Hover on `T` in `@return T` (line 6)
    let hover = hover_at(&backend, uri, content, 6, 16).expect("expected hover on T");
    let text = hover_text(&hover);
    assert!(
        text.contains("**template**"),
        "should show template hover, got: {}",
        text
    );
    assert!(
        text.contains("`T`"),
        "should show the template name, got: {}",
        text
    );
    assert!(
        text.contains("`Animal`"),
        "should show the bound class, got: {}",
        text
    );
}

/// Method-level template parameter shows hover within the method body.
#[test]
fn hover_method_level_template_param() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Util {
    /**
     * @template TItem of object
     * @param TItem $item
     * @return TItem
     */
    public function identity(object $item): object { return $item; }
}
"#;

    // Hover on `TItem` in `@param TItem $item` (line 4)
    let hover = hover_at(&backend, uri, content, 4, 14).expect("expected hover on TItem");
    let text = hover_text(&hover);
    assert!(
        text.contains("**template**"),
        "should show template hover, got: {}",
        text
    );
    assert!(
        text.contains("`TItem`"),
        "should show the template name, got: {}",
        text
    );
    assert!(
        text.contains("`object`"),
        "should show the bound, got: {}",
        text
    );
}

/// Hovering a fully-qualified class name (`\stdClass`) inside a docblock
/// in a namespaced file should resolve the class via the FQN path, not
/// prepend the current namespace.
#[test]
fn hover_fqn_class_in_docblock_resolves_stub() {
    let backend = create_test_backend_with_stdclass_stub();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App\Models;

class Repo {
    /** @return \stdClass */
    public function find(): \stdClass { return new \stdClass(); }
}
"#;

    // Hover on `\stdClass` in the @return tag (line 4, on "stdClass" portion)
    let hover = hover_at(&backend, uri, content, 4, 19).expect("expected hover on \\stdClass");
    let text = hover_text(&hover);
    assert!(
        text.contains("class stdClass"),
        "should resolve stdClass from stubs, got: {}",
        text
    );
    assert!(
        !text.contains("class stdClass;"),
        "should not show the unknown-class fallback (with semicolon), got: {}",
        text
    );
    // The stub docblock has @link — verify it appears in hover.
    assert!(
        text.contains("php.net"),
        "should include the @link URL from the stub docblock, got: {}",
        text
    );
    // The stub docblock has a description — verify it appears in hover.
    assert!(
        text.contains("Created by typecasting to object"),
        "should include the docblock description from the stub, got: {}",
        text
    );
}

/// Same as above but with a FQN inside a generic type argument:
/// `Collection<int, \stdClass>`.
#[test]
fn hover_fqn_class_in_generic_arg_resolves_stub() {
    let backend = create_test_backend_with_stdclass_stub();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App\Models;

class Repo {
    /** @return array<int, \stdClass> */
    public function all(): array { return []; }
}
"#;

    // Hover on `\stdClass` inside the generic (line 4)
    let hover = hover_at(&backend, uri, content, 4, 30).expect("expected hover on \\stdClass");
    let text = hover_text(&hover);
    assert!(
        text.contains("class stdClass"),
        "should resolve stdClass from stubs inside generic arg, got: {}",
        text
    );
    assert!(
        !text.contains("class stdClass;"),
        "should not show the unknown-class fallback, got: {}",
        text
    );
}

/// A user-defined class with a `@link` tag should display the URL in hover.
#[test]
fn hover_class_with_link_tag() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * Handles user authentication.
 * @link https://example.com/docs/auth
 */
class AuthService {}

function demo(): void {
    $a = new AuthService();
}
"#;

    // Hover on `AuthService` in `new AuthService()` — but since there's
    // no constructor, it falls through to class hover.
    let hover = hover_at(&backend, uri, content, 8, 14).expect("expected hover on AuthService");
    let text = hover_text(&hover);
    assert!(
        text.contains("class AuthService"),
        "should show class name, got: {}",
        text
    );
    assert!(
        text.contains("Handles user authentication"),
        "should show docblock description, got: {}",
        text
    );
    assert!(
        text.contains("https://example.com/docs/auth"),
        "should show @link URL, got: {}",
        text
    );
}

#[test]
fn hover_function_shows_see_symbol_reference() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @param string $tz
 *
 * @deprecated So old, how old is it!
 * @see UnsetDemo
 * @see https://google.com/
 */
function formatUtfDate(string $tz): void {}

formatUtfDate('');
"#;

    let hover = hover_at(&backend, uri, content, 10, 2).expect("expected hover on formatUtfDate");
    let text = hover_text(&hover);
    // Deprecation message should NOT contain @see references
    assert!(
        text.contains("So old, how old is it!"),
        "should show deprecation message, got: {}",
        text
    );
    assert!(
        !text.contains("(see:"),
        "deprecation line should not contain inline @see, got: {}",
        text
    );
    // Symbol @see should be rendered with inline code
    assert!(
        text.contains("`UnsetDemo`"),
        "should show @see symbol reference as inline code, got: {}",
        text
    );
    // URL @see should be rendered as a clickable link
    assert!(
        text.contains("[https://google.com/](https://google.com/)"),
        "should show @see URL as clickable link, got: {}",
        text
    );
    // Both @see entries should appear before the code block
    let see_pos = text.find("`UnsetDemo`").unwrap();
    let code_pos = text.find("```php").unwrap();
    assert!(
        see_pos < code_pos,
        "@see should appear before the code block: {}",
        text
    );
}

#[test]
fn hover_method_shows_see_reference() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Formatter {
    /**
     * Format a date.
     * @see OtherFormatter::format()
     */
    public function formatDate(): string { return ''; }
    public function run(): void {
        $this->formatDate();
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 8, 16).expect("expected hover on formatDate");
    let text = hover_text(&hover);
    assert!(
        text.contains("`OtherFormatter::format()`"),
        "should show @see symbol for method hover, got: {}",
        text
    );
}

#[test]
fn hover_class_shows_see_reference() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * Old auth handler.
 * @see NewAuthService
 * @see https://docs.example.com/auth
 */
class OldAuth {}

function demo(): void {
    $a = new OldAuth();
}
"#;

    let hover = hover_at(&backend, uri, content, 9, 14).expect("expected hover on OldAuth");
    let text = hover_text(&hover);
    assert!(
        text.contains("`NewAuthService`"),
        "should show @see symbol for class hover, got: {}",
        text
    );
    assert!(
        text.contains("[https://docs.example.com/auth](https://docs.example.com/auth)"),
        "should show @see URL for class hover, got: {}",
        text
    );
}

#[test]
fn hover_see_with_description_shows_description() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @see MyClass::newMethod() Use this instead.
 */
function oldFunc(): void {}

oldFunc();
"#;

    let hover = hover_at(&backend, uri, content, 6, 2).expect("expected hover on oldFunc");
    let text = hover_text(&hover);
    assert!(
        text.contains("`MyClass::newMethod()` Use this instead."),
        "should show @see symbol with trailing description, got: {}",
        text
    );
}

#[test]
fn hover_see_url_not_duplicated_in_link_section() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @link https://php.net/manual/en/function.array-map.php
 * @see https://example.com/docs
 */
function myFunc(): void {}

myFunc();
"#;

    let hover = hover_at(&backend, uri, content, 7, 2).expect("expected hover on myFunc");
    let text = hover_text(&hover);
    // @link should appear as a plain link
    assert!(
        text.contains("[https://php.net/manual/en/function.array-map.php](https://php.net/manual/en/function.array-map.php)"),
        "should show @link URL, got: {}",
        text
    );
    // @see URL (different from @link) should also appear as a plain link
    assert!(
        text.contains("[https://example.com/docs](https://example.com/docs)"),
        "should show @see URL as plain link, got: {}",
        text
    );
    // The @see URL should appear exactly once (no duplication)
    let plain_link_count = text.matches("[https://example.com/docs]").count();
    assert_eq!(
        plain_link_count, 1,
        "@see URL should appear exactly once, got {} occurrences in: {}",
        plain_link_count, text
    );
}

#[test]
fn hover_see_symbol_renders_clickable_file_link() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class UnsetDemo {
    public function reset(): void {}
}

/**
 * @param string $tz
 *
 * @deprecated So old, how old is it!
 * @see UnsetDemo
 */
function formatUtfDate(string $tz): void {}

formatUtfDate('');
"#;

    let hover = hover_at(&backend, uri, content, 13, 2).expect("expected hover on formatUtfDate");
    let text = hover_text(&hover);
    // The @see reference to UnsetDemo should be a clickable link
    // because the class exists in the workspace.
    assert!(
        text.contains("[`UnsetDemo`](file:///test.php#L2)"),
        "should render @see symbol as clickable file link, got: {}",
        text
    );
}

#[test]
fn hover_see_class_member_renders_clickable_file_link() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class NewFormatter {
    public function format(): string { return ''; }
}

/**
 * @see NewFormatter::format()
 */
function oldFormat(): string { return ''; }

oldFormat();
"#;

    let hover = hover_at(&backend, uri, content, 10, 2).expect("expected hover on oldFormat");
    let text = hover_text(&hover);
    // The @see reference to NewFormatter::format() should be a clickable
    // link that points to the method's definition line.
    assert!(
        text.contains("[`NewFormatter::format()`](file:///test.php#L3)"),
        "should render @see class::method as clickable file link, got: {}",
        text
    );
}

#[test]
fn hover_see_unresolvable_symbol_falls_back_to_inline_code() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @see NonExistentClass
 */
function myFunc(): void {}

myFunc();
"#;

    let hover = hover_at(&backend, uri, content, 6, 2).expect("expected hover on myFunc");
    let text = hover_text(&hover);
    // When the class can't be found, fall back to inline code (no link).
    assert!(
        text.contains("`NonExistentClass`"),
        "unresolvable @see should render as inline code, got: {}",
        text
    );
    // Make sure it's NOT rendered as a link.
    assert!(
        !text.contains("](file://"),
        "unresolvable @see should not have a file link, got: {}",
        text
    );
}

#[test]
fn hover_see_url_deduplicated_when_same_as_link() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @param string $tz
 *
 * @deprecated So old, how old is it!
 * @see http://google.com/
 * @link http://google.com/
 */
function formatUtfDate(string $tz): void {}

formatUtfDate('');
"#;

    let hover = hover_at(&backend, uri, content, 10, 2).expect("expected hover on formatUtfDate");
    let text = hover_text(&hover);
    // The @link URL should appear as a clickable link
    assert!(
        text.contains("[http://google.com/](http://google.com/)"),
        "should show @link URL, got: {}",
        text
    );
    // The same URL from @see should NOT appear a second time
    let link_count = text
        .matches("[http://google.com/](http://google.com/)")
        .count();
    assert_eq!(
        link_count, 1,
        "URL appearing in both @link and @see should render only once, got {} in: {}",
        link_count, text
    );
}

#[test]
fn hover_closure_in_parenthesized_callable_union() {
    let backend = create_test_backend_with_closure_stub();
    let uri = "file:///test.php";
    let content = r#"<?php
class Builder {
    /**
     * @param  (\Closure(static): mixed)|string|array  $column
     * @return $this
     */
    public function where($column) {}
}
"#;

    // Hover on `\Closure` inside `(\Closure(static): mixed)` (line 3).
    // The `\` is at column 15, `Closure` spans columns 16–22.
    let hover = hover_at(&backend, uri, content, 3, 16).expect("expected hover on \\Closure");
    let text = hover_text(&hover);
    assert!(
        text.contains("class Closure"),
        "should show Closure class info, got: {}",
        text
    );
    // Must NOT contain the leading `(` in the class name.
    assert!(
        !text.contains("(\\Closure"),
        "should not include leading paren in class name, got: {}",
        text
    );
}

#[test]
fn hover_template_param_in_use_tag_generic_arg() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @template TModel of \stdClass
 */
class Builder {
    /** @use SomeTrait<TModel> */
    use SomeTrait;
}
"#;

    // Hover on `TModel` inside `@use SomeTrait<TModel>` (line 5).
    let hover = hover_at(&backend, uri, content, 5, 24).expect("expected hover on TModel");
    let text = hover_text(&hover);
    assert!(
        text.contains("template") && text.contains("TModel"),
        "should show template param info for TModel, got: {}",
        text
    );
}

#[test]
fn hover_static_in_docblock_generic_arg() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Model {
    /** @return Builder<static> */
    public static function query() {}
}
"#;

    // Hover on `static` inside `Builder<static>` (line 2).
    let hover = hover_at(&backend, uri, content, 2, 25).expect("expected hover on static");
    let text = hover_text(&hover);
    assert!(
        text.contains("Model"),
        "should resolve static to the enclosing class Model, got: {}",
        text
    );
}

#[test]
fn hover_backed_enum_case_shows_case_syntax_and_value() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
enum OrderStatus: string {
    case Pending = 'pending';
    case Processing = 'processing';

    public function isPending(): bool { return $this === self::Pending; }
}
"#;

    // Hover on `Pending` in `self::Pending` (line 5).
    let hover = hover_at(&backend, uri, content, 5, 63).expect("expected hover on Pending");
    let text = hover_text(&hover);
    assert!(
        text.contains("case Pending = 'pending';"),
        "should show enum case syntax with value, got: {}",
        text
    );
    assert!(
        text.contains("enum OrderStatus: string"),
        "should show enum keyword with backing type, got: {}",
        text
    );
    assert!(
        !text.contains("class "),
        "should not show 'class' for an enum, got: {}",
        text
    );
    assert!(
        !text.contains("const "),
        "should not show 'const' for an enum case, got: {}",
        text
    );
}

#[test]
fn hover_unit_enum_case_shows_case_syntax_without_value() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
enum Suit {
    case Hearts;
    case Diamonds;

    public function isRed(): bool { return $this === self::Hearts; }
}
"#;

    // Hover on `Hearts` in `self::Hearts` (line 5).
    let hover = hover_at(&backend, uri, content, 5, 59).expect("expected hover on Hearts");
    let text = hover_text(&hover);
    assert!(
        text.contains("case Hearts;"),
        "should show enum case syntax without value, got: {}",
        text
    );
    assert!(
        text.contains("enum Suit"),
        "should show enum keyword without backing type, got: {}",
        text
    );
    assert!(
        !text.contains("enum Suit:"),
        "should not show colon for unit enum, got: {}",
        text
    );
    assert!(
        !text.contains("const "),
        "should not show 'const' for a unit enum case, got: {}",
        text
    );
}

#[test]
fn hover_method_without_native_param_types_omits_docblock_types_from_code_block() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Builder {
    /**
     * @param  (\Closure(static): mixed)|string|array  $column
     * @return $this
     */
    public function where($column, $operator = null, $value = null, $boolean = 'and') {}

    public function run(): void {
        $this->where('active', true);
    }
}
"#;

    // Hover on `where` in `$this->where(...)` (line 9).
    let hover = hover_at(&backend, uri, content, 9, 16).expect("expected hover on where");
    let text = hover_text(&hover);
    // The code block should show untyped params (no native types exist),
    // NOT the docblock type `(\Closure(static): mixed)|string|array`.
    assert!(
        text.contains("function where($column, $operator = null, $value = null, $boolean = 'and')"),
        "should show untyped params in PHP code block, got: {}",
        text
    );
    // The code block (between ```php fences) must not contain the docblock type.
    let code_block = text
        .split("```php")
        .nth(1)
        .and_then(|s| s.split("```").next())
        .unwrap_or("");
    assert!(
        !code_block.contains("Closure"),
        "code block should not contain docblock Closure type, got code block: {}",
        code_block
    );
}

#[test]
fn hover_class_reference_in_property_default() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class FrostingCast {}
class Bread {
    protected $casts = [
        'icing' => FrostingCast::class,
    ];
}
"#;

    // Hover on `FrostingCast` in `FrostingCast::class` (line 4).
    let hover = hover_at(&backend, uri, content, 4, 20).expect("expected hover on FrostingCast");
    let text = hover_text(&hover);
    assert!(
        text.contains("FrostingCast"),
        "should show FrostingCast class info, got: {}",
        text
    );
}

#[test]
fn hover_class_in_multiline_docblock_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class SomeCollection {}
class Demo {
    /**
     * @return array<
     *   string,
     *   SomeCollection<int>
     * >
     */
    public function grouped() {}

    public function run(): void {
        $this->grouped();
    }
}
"#;

    // Hover on `SomeCollection` inside the multiline @return type (line 6).
    let hover = hover_at(&backend, uri, content, 6, 10).expect("expected hover on SomeCollection");
    let text = hover_text(&hover);
    assert!(
        text.contains("SomeCollection"),
        "should show SomeCollection class info, got: {}",
        text
    );
}

#[test]
fn hover_template_param_in_multiline_docblock_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @template TValue
 */
class FluentCollection {
    /**
     * @return array<
     *   string,
     *   FluentCollection<int, TValue>
     * >
     */
    public function grouped() {}
}
"#;

    // Hover on `TValue` inside the multiline @return type (line 8).
    let hover = hover_at(&backend, uri, content, 8, 32).expect("expected hover on TValue");
    let text = hover_text(&hover);
    assert!(
        text.contains("template") && text.contains("TValue"),
        "should show template param info for TValue, got: {}",
        text
    );
}

// ── Anonymous class ─────────────────────────────────────────────────────────

#[test]
fn hover_anonymous_class_extends() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Animal {
    public string $species;
}
function make() {
    return new class extends Animal {};
}
"#;

    // Hover on `Animal` in `new class extends Animal` (line 5, col ~30).
    let hover = hover_at(&backend, uri, content, 5, 31).expect("expected hover on Animal");
    let text = hover_text(&hover);
    assert!(
        text.contains("Animal"),
        "should show Animal class info, got: {}",
        text
    );
}

#[test]
fn hover_anonymous_class_implements() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Runnable {
    public function run(): void;
}
function make() {
    return new class implements Runnable {
        public function run(): void {}
    };
}
"#;

    // Hover on `Runnable` in `new class implements Runnable` (line 5).
    let hover = hover_at(&backend, uri, content, 5, 34).expect("expected hover on Runnable");
    let text = hover_text(&hover);
    assert!(
        text.contains("Runnable"),
        "should show Runnable interface info, got: {}",
        text
    );
}

#[test]
fn hover_anonymous_class_method_param_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Widget {}
function make() {
    return new class {
        public function process(Widget $w): void {}
    };
}
"#;

    // Hover on `Widget` in anonymous class method param (line 4).
    let hover = hover_at(&backend, uri, content, 4, 32).expect("expected hover on Widget");
    let text = hover_text(&hover);
    assert!(
        text.contains("Widget"),
        "should show Widget class info, got: {}",
        text
    );
}

// ── Top-level const ─────────────────────────────────────────────────────────

#[test]
fn hover_class_in_top_level_const_value() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Handler {}
const DEFAULT_HANDLER = Handler::class;
"#;

    // Hover on `Handler` in `Handler::class` (line 2, col ~24).
    let hover = hover_at(&backend, uri, content, 2, 24).expect("expected hover on Handler");
    let text = hover_text(&hover);
    assert!(
        text.contains("Handler"),
        "should show Handler class info, got: {}",
        text
    );
}

#[test]
fn hover_define_constant_shows_value() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
define('APP_VERSION', '1.0.0');
echo APP_VERSION;
"#;

    backend.update_ast(uri, content);
    let hover = hover_at(&backend, uri, content, 2, 7).expect("expected hover on APP_VERSION");
    let text = hover_text(&hover);
    assert!(
        text.contains("'1.0.0'"),
        "hover should show the constant value '1.0.0', got: {}",
        text
    );
    assert!(
        text.contains("APP_VERSION"),
        "hover should show the constant name, got: {}",
        text
    );
}

#[test]
fn hover_define_constant_integer_value() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
define('MAX_RETRIES', 5);
echo MAX_RETRIES;
"#;

    backend.update_ast(uri, content);
    let hover = hover_at(&backend, uri, content, 2, 7).expect("expected hover on MAX_RETRIES");
    let text = hover_text(&hover);
    assert!(
        text.contains("= 5"),
        "hover should show 'const MAX_RETRIES = 5;', got: {}",
        text
    );
}

#[test]
fn hover_top_level_const_shows_value() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
const DB_HOST = 'localhost';
echo DB_HOST;
"#;

    backend.update_ast(uri, content);
    let hover = hover_at(&backend, uri, content, 2, 7).expect("expected hover on DB_HOST");
    let text = hover_text(&hover);
    assert!(
        text.contains("'localhost'"),
        "hover should show the constant value, got: {}",
        text
    );
    assert!(
        text.contains("DB_HOST"),
        "hover should show the constant name, got: {}",
        text
    );
}

#[test]
fn hover_define_constant_no_value_still_works() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    // Register a constant without a value (e.g. from autoload discovery).
    {
        let mut dmap = backend.global_defines().write();
        dmap.insert(
            "LEGACY_CONST".to_string(),
            phpantom_lsp::DefineInfo {
                file_uri: "file:///legacy.php".to_string(),
                name_offset: 0,
                value: None,
            },
        );
    }

    let content = r#"<?php
echo LEGACY_CONST;
"#;

    backend.update_ast(uri, content);
    let hover = hover_at(&backend, uri, content, 1, 7).expect("expected hover on LEGACY_CONST");
    let text = hover_text(&hover);
    assert!(
        text.contains("LEGACY_CONST"),
        "hover should show the constant name, got: {}",
        text
    );
    // No value available, so it should show just `const LEGACY_CONST;`
    assert!(
        !text.contains('='),
        "hover should not show '=' when value is unknown, got: {}",
        text
    );
}

#[test]
fn hover_stub_constant_shows_value() {
    let backend = create_test_backend_with_function_stubs();
    let uri = "file:///test.php";
    let content = r#"<?php
echo PHP_INT_MAX;
"#;

    backend.update_ast(uri, content);
    let hover = hover_at(&backend, uri, content, 1, 7).expect("expected hover on PHP_INT_MAX");
    let text = hover_text(&hover);
    assert!(
        text.contains("PHP_INT_MAX"),
        "hover should show the constant name, got: {}",
        text
    );
    assert!(
        text.contains('='),
        "hover should show a value for the stub constant, got: {}",
        text
    );
}

#[test]
fn hover_stub_constant_php_eol_shows_value() {
    let backend = create_test_backend_with_function_stubs();
    let uri = "file:///test.php";
    let content = r#"<?php
echo PHP_EOL;
"#;

    backend.update_ast(uri, content);
    let hover = hover_at(&backend, uri, content, 1, 7).expect("expected hover on PHP_EOL");
    let text = hover_text(&hover);
    assert!(
        text.contains("PHP_EOL"),
        "hover should show the constant name, got: {}",
        text
    );
}

// ── Language constructs ─────────────────────────────────────────────────────

#[test]
fn hover_variable_inside_isset() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Config {
    public string $key;
}
function check(Config $cfg) {
    isset($cfg->key);
}
"#;

    // Hover on `key` inside `isset($cfg->key)` (line 5).
    let hover = hover_at(&backend, uri, content, 5, 16).expect("expected hover on key");
    let text = hover_text(&hover);
    assert!(
        text.contains("key"),
        "should show property info for key, got: {}",
        text
    );
}

#[test]
fn hover_variable_inside_empty() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Box {
    public string $label;
}
function check(Box $b) {
    empty($b->label);
}
"#;

    // Hover on `label` inside `empty($b->label)` (line 5).
    let hover = hover_at(&backend, uri, content, 5, 15).expect("expected hover on label");
    let text = hover_text(&hover);
    assert!(
        text.contains("label"),
        "should show property info for label, got: {}",
        text
    );
}

// ── String interpolation ────────────────────────────────────────────────────

#[test]
fn hover_variable_inside_interpolated_string() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Greeter {
    public string $name;
}
function greet(Greeter $g) {
    echo "Hello {$g->name}!";
}
"#;

    // Hover on `name` inside the interpolated string (line 5).
    let hover = hover_at(&backend, uri, content, 5, 22).expect("expected hover on name");
    let text = hover_text(&hover);
    assert!(
        text.contains("name"),
        "should show property info for name, got: {}",
        text
    );
}

// ── First-class callable ────────────────────────────────────────────────────

#[test]
fn hover_first_class_callable_static_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Formatter {
    public static function bold(string $text): string {
        return "<b>$text</b>";
    }
}
function test() {
    $fn = Formatter::bold(...);
}
"#;

    // Hover on `Formatter` in `Formatter::bold(...)` (line 7).
    let hover = hover_at(&backend, uri, content, 7, 10).expect("expected hover on Formatter");
    let text = hover_text(&hover);
    assert!(
        text.contains("Formatter"),
        "should show Formatter class info, got: {}",
        text
    );

    // Hover on `bold` in `Formatter::bold(...)` (line 7).
    let hover2 = hover_at(&backend, uri, content, 7, 22).expect("expected hover on bold");
    let text2 = hover_text(&hover2);
    assert!(
        text2.contains("bold"),
        "should show bold method info, got: {}",
        text2
    );
}

#[test]
fn hover_first_class_callable_instance_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Printer {
    public function printLine(string $line): void {}
}
function test(Printer $p) {
    $fn = $p->printLine(...);
}
"#;

    // Hover on `printLine` in `$p->printLine(...)` (line 5).
    let hover = hover_at(&backend, uri, content, 5, 15).expect("expected hover on printLine");
    let text = hover_text(&hover);
    assert!(
        text.contains("printLine"),
        "should show printLine method info, got: {}",
        text
    );
}

// ─── Origin indicator tests ─────────────────────────────────────────────────

#[test]
fn hover_method_override_shows_indicator() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Animal {
    public function speak(): string { return ''; }
}
class Dog extends Animal {
    public function speak(): string { return 'woof'; }
    public function run(): void {
        $this->speak();
    }
}
"#;

    // Hover on `speak` called on `$this` inside Dog (line 7).
    let hover = hover_at(&backend, uri, content, 7, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("overrides **Animal**"),
        "should show override indicator, got: {}",
        text
    );
}

#[test]
fn hover_method_implements_shows_indicator() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Loggable {
    public function log(string $msg): void;
}
class FileLogger implements Loggable {
    public function log(string $msg): void {}
    public function run(): void {
        $this->log('hi');
    }
}
"#;

    // Hover on `log` called on `$this` inside FileLogger (line 7).
    let hover = hover_at(&backend, uri, content, 7, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("implements **Loggable**"),
        "should show implements indicator, got: {}",
        text
    );
}

#[test]
fn hover_method_override_and_implements_shows_both() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Renderable {
    public function render(): string;
}
class BaseView {
    public function render(): string { return ''; }
}
class HtmlView extends BaseView implements Renderable {
    public function render(): string { return '<html>'; }
    public function test(): void {
        $this->render();
    }
}
"#;

    // Hover on `render` called on `$this` inside HtmlView (line 10).
    let hover = hover_at(&backend, uri, content, 10, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("overrides **BaseView**"),
        "should show override indicator, got: {}",
        text
    );
    assert!(
        text.contains("implements **Renderable**"),
        "should show implements indicator, got: {}",
        text
    );
}

#[test]
fn hover_virtual_method_shows_indicator() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @method string getName()
 */
class Magic {
    public function test(): void {
        $this->getName();
    }
}
"#;

    // Hover on `getName` called on `$this` inside Magic (line 6).
    let hover = hover_at(&backend, uri, content, 6, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("virtual"),
        "should show virtual indicator, got: {}",
        text
    );
}

#[test]
fn hover_virtual_property_shows_indicator() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @property string $title
 */
class Document {
    public function test(): void {
        $this->title;
    }
}
"#;

    // Hover on `title` accessed on `$this` inside Document (line 6).
    let hover = hover_at(&backend, uri, content, 6, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("virtual"),
        "should show virtual indicator for property, got: {}",
        text
    );
}

#[test]
fn hover_non_overriding_method_has_no_indicator() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Simple {
    public function doStuff(): void {}
    public function test(): void {
        $this->doStuff();
    }
}
"#;

    // Hover on `doStuff` called on `$this` (line 4).
    let hover = hover_at(&backend, uri, content, 4, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        !text.contains("overrides"),
        "should NOT show override indicator, got: {}",
        text
    );
    assert!(
        !text.contains("implements"),
        "should NOT show implements indicator, got: {}",
        text
    );
    assert!(
        !text.contains("virtual"),
        "should NOT show virtual indicator, got: {}",
        text
    );
}

#[test]
fn hover_constant_implements_interface_shows_indicator() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface HasVersion {
    const VERSION = '1.0';
}
class App implements HasVersion {
    const VERSION = '2.0';
    public function test(): void {
        self::VERSION;
    }
}
"#;

    // Hover on `VERSION` via `self::VERSION` (line 7).
    let hover = hover_at(&backend, uri, content, 7, 15).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("implements **HasVersion**"),
        "should show implements indicator for constant, got: {}",
        text
    );
}

#[test]
fn hover_property_override_shows_indicator() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    public string $label = '';
}
class Child extends Base {
    public string $label = 'child';
    public function test(): void {
        $this->label;
    }
}
"#;

    // Hover on `label` on `$this` inside Child (line 7).
    let hover = hover_at(&backend, uri, content, 7, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("overrides **Base**"),
        "should show override indicator for property, got: {}",
        text
    );
}

#[test]
fn hover_inherited_method_no_override_indicator() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class ParentClass {
    public function inherited(): void {}
}
class ChildClass extends ParentClass {
    public function test(): void {
        $this->inherited();
    }
}
"#;

    // Hover on `inherited` called on `$this` in ChildClass (line 6).
    // The method is inherited (not overridden), so no indicator.
    let hover = hover_at(&backend, uri, content, 6, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        !text.contains("overrides"),
        "inherited method should NOT show override, got: {}",
        text
    );
}

#[test]
fn hover_cross_file_method_override_shows_indicator() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": { "App\\": "src/" }
            }
        }"#,
        &[
            (
                "src/Base.php",
                r#"<?php
namespace App;
class Base {
    public function process(): void {}
}
"#,
            ),
            (
                "src/Child.php",
                r#"<?php
namespace App;
class Child extends Base {
    public function process(): void {}
    public function test(): void {
        $this->process();
    }
}
"#,
            ),
        ],
    );

    let base_uri = format!("file://{}", _dir.path().join("src/Base.php").display());
    let base_content = std::fs::read_to_string(_dir.path().join("src/Base.php")).unwrap();
    backend.update_ast(&base_uri, &base_content);

    let child_uri = format!("file://{}", _dir.path().join("src/Child.php").display());
    let child_content = std::fs::read_to_string(_dir.path().join("src/Child.php")).unwrap();

    // Hover on `process` called on `$this` inside Child (line 5).
    let hover = hover_at(&backend, &child_uri, &child_content, 5, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("overrides **Base**"),
        "cross-file override should show indicator, got: {}",
        text
    );
}

#[test]
fn hover_cross_file_method_implements_shows_indicator() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": { "App\\": "src/" }
            }
        }"#,
        &[
            (
                "src/Loggable.php",
                r#"<?php
namespace App;
interface Loggable {
    public function log(string $msg): void;
}
"#,
            ),
            (
                "src/FileLogger.php",
                r#"<?php
namespace App;
class FileLogger implements Loggable {
    public function log(string $msg): void {}
    public function test(): void {
        $this->log('hi');
    }
}
"#,
            ),
        ],
    );

    let iface_uri = format!("file://{}", _dir.path().join("src/Loggable.php").display());
    let iface_content = std::fs::read_to_string(_dir.path().join("src/Loggable.php")).unwrap();
    backend.update_ast(&iface_uri, &iface_content);

    let impl_uri = format!(
        "file://{}",
        _dir.path().join("src/FileLogger.php").display()
    );
    let impl_content = std::fs::read_to_string(_dir.path().join("src/FileLogger.php")).unwrap();

    // Hover on `log` called on `$this` inside FileLogger (line 5).
    let hover = hover_at(&backend, &impl_uri, &impl_content, 5, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("implements **Loggable**"),
        "cross-file implements should show indicator, got: {}",
        text
    );
}

#[test]
fn hover_implements_indicator_same_file_with_namespace() {
    // Mimics example.php's HoverOriginsDemo scenario: interface and class
    // in the same namespace block of the same file.
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace Demo;

interface Renderable {
    public function format(string $template): string;
}

abstract class Model {
    abstract public function toArray(): array;
}

class HoverOriginsDemo extends Model implements Renderable {
    public function format(string $template): string { return ''; }
    public function toArray(): array { return []; }
    public function demo(): void {
        $this->format('x');
        $this->toArray();
    }
}
"#;

    // Hover on `format` called on `$this` (line 15).
    let hover = hover_at(&backend, uri, content, 15, 16).expect("expected hover on format");
    let text = hover_text(&hover);
    assert!(
        text.contains("implements **Renderable**"),
        "should show implements indicator for format(), got: {}",
        text
    );

    // Hover on `toArray` called on `$this` (line 16).
    let hover = hover_at(&backend, uri, content, 16, 16).expect("expected hover on toArray");
    let text = hover_text(&hover);
    assert!(
        text.contains("overrides **Model**"),
        "should show overrides indicator for toArray(), got: {}",
        text
    );
}

#[test]
fn hover_implements_indicator_multi_namespace_block_file() {
    // Mirrors example.php's structure: a single file with one big
    // `namespace Demo { ... }` block containing both the interface
    // and the implementing class.
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace Demo {

interface Renderable {
    public function format(string $template): string;
}

abstract class Model {
    abstract public function toArray(): array;
    public function getName(): string { return ''; }
}

class HoverOriginsDemo extends Model implements Renderable {
    public function format(string $template): string { return ''; }
    public function toArray(): array { return []; }
    public function demo(): void {
        $this->format('x');
        $this->toArray();
        $this->getName();
    }
}

}
"#;

    // Hover on `format` called on `$this` (line 16).
    // `format` is declared on Renderable, so should show implements indicator.
    let hover = hover_at(&backend, uri, content, 16, 16).expect("expected hover on format");
    let text = hover_text(&hover);
    assert!(
        text.contains("implements **Renderable**"),
        "should show implements indicator for format(), got: {}",
        text
    );

    // Hover on `toArray` called on `$this` (line 17).
    // `toArray` is declared on Model, so should show overrides indicator.
    let hover = hover_at(&backend, uri, content, 17, 16).expect("expected hover on toArray");
    let text = hover_text(&hover);
    assert!(
        text.contains("overrides **Model**"),
        "should show overrides indicator for toArray(), got: {}",
        text
    );

    // Hover on `getName` called on `$this` (line 18).
    // `getName` is inherited from Model (not overridden), so NO indicator.
    let hover = hover_at(&backend, uri, content, 18, 16).expect("expected hover on getName");
    let text = hover_text(&hover);
    assert!(
        !text.contains("overrides"),
        "inherited method should NOT show overrides, got: {}",
        text
    );
    assert!(
        !text.contains("implements"),
        "inherited method should NOT show implements, got: {}",
        text
    );
}

// ─── Enum case listing tests ────────────────────────────────────────────────

#[test]
fn hover_enum_shows_cases_in_code_block() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
enum Color {
    case Red;
    case Green;
    case Blue;
}
function paint(Color $c): void {}
"#;

    // Hover on `Color` in the function parameter (line 6).
    let hover = hover_at(&backend, uri, content, 6, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("case Red;"),
        "should list Red case, got: {}",
        text
    );
    assert!(
        text.contains("case Green;"),
        "should list Green case, got: {}",
        text
    );
    assert!(
        text.contains("case Blue;"),
        "should list Blue case, got: {}",
        text
    );
}

#[test]
fn hover_backed_enum_shows_cases_with_values() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
enum Status: string {
    case Active = 'active';
    case Inactive = 'inactive';
}
function check(Status $s): void {}
"#;

    // Hover on `Status` in the function parameter (line 5).
    let hover = hover_at(&backend, uri, content, 5, 16).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("case Active = 'active';"),
        "should list Active case with value, got: {}",
        text
    );
    assert!(
        text.contains("case Inactive = 'inactive';"),
        "should list Inactive case with value, got: {}",
        text
    );
}

#[test]
fn hover_enum_does_not_show_regular_constants() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
enum Suit: string {
    const TABLE = 'suits';
    case Hearts = 'H';
    case Diamonds = 'D';
}
function deal(Suit $s): void {}
"#;

    // Hover on `Suit` in the function parameter (line 6).
    let hover = hover_at(&backend, uri, content, 6, 14).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("case Hearts"),
        "should list Hearts case, got: {}",
        text
    );
    assert!(
        !text.contains("const TABLE"),
        "should NOT list regular constant TABLE in enum body, got: {}",
        text
    );
}

#[test]
fn hover_enum_with_no_cases_shows_plain_signature() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
enum Permission {
    const ADMIN_ROLE = 'admin';
    public function label(): string { return ''; }
}
function f(Permission $p): void {}
"#;

    // Hover on `Permission` in the function parameter (line 5).
    let hover = hover_at(&backend, uri, content, 5, 13).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("enum Permission"),
        "should show enum signature, got: {}",
        text
    );
    // No enum cases exist, so no curly-brace body should be rendered.
    assert!(
        !text.contains('{'),
        "should not have curly brace body when no cases, got: {}",
        text
    );
}

// ─── Trait method signature listing tests ───────────────────────────────────

#[test]
fn hover_trait_shows_public_method_signatures() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
trait Cacheable {
    public function getCacheKey(): string { return ''; }
    public static function flushCache(): void {}
    protected function internalCache(): void {}
}
class Item {
    use Cacheable;
}
"#;

    // Hover on `Cacheable` in the use statement (line 7).
    let hover = hover_at(&backend, uri, content, 7, 9).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("function getCacheKey(): string;"),
        "should show getCacheKey signature, got: {}",
        text
    );
    assert!(
        text.contains("static function flushCache(): void;"),
        "should show static flushCache signature, got: {}",
        text
    );
    assert!(
        !text.contains("internalCache"),
        "should NOT show protected method, got: {}",
        text
    );
}

#[test]
fn hover_trait_shows_public_properties() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
trait HasName {
    public string $name;
    protected int $id;
    public function getName(): string { return $this->name; }
}
class Person {
    use HasName;
}
"#;

    // Hover on `HasName` in the use statement (line 7).
    let hover = hover_at(&backend, uri, content, 7, 9).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("public string $name;"),
        "should show public property, got: {}",
        text
    );
    assert!(
        !text.contains("$id"),
        "should NOT show protected property, got: {}",
        text
    );
    assert!(
        text.contains("function getName(): string;"),
        "should show public method, got: {}",
        text
    );
}

#[test]
fn hover_trait_with_no_public_members_shows_plain_signature() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
trait Internal {
    protected function secret(): void {}
}
class Box {
    use Internal;
}
"#;

    // Hover on `Internal` in the use statement (line 5).
    let hover = hover_at(&backend, uri, content, 5, 9).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("trait Internal"),
        "should show trait signature, got: {}",
        text
    );
    assert!(
        !text.contains('{'),
        "should not have curly brace body when no public members, got: {}",
        text
    );
}

#[test]
fn hover_class_does_not_show_member_body() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Widget {
    public string $label;
    public function render(): string { return ''; }
}
function f(Widget $w): void {}
"#;

    // Hover on `Widget` in the function parameter (line 5).
    let hover = hover_at(&backend, uri, content, 5, 12).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("class Widget"),
        "should show class name, got: {}",
        text
    );
    // Regular classes should NOT show a member body.
    assert!(
        !text.contains("$label"),
        "should NOT list properties for a regular class, got: {}",
        text
    );
    assert!(
        !text.contains("function render"),
        "should NOT list methods for a regular class, got: {}",
        text
    );
}

/// Variable hover namespace is derived from the type string, not the file.
/// A parameter typed as `\Generator<int, Pencil>` in a `Demo` namespace
/// file should show no namespace line (Generator is global).
#[test]
fn hover_variable_namespace_from_type_not_file() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace Demo;
class Pencil { public function sketch(): void {} }
class Demo {
    /**
     * @param \Generator<int, Pencil> $pencils
     */
    public function foreachGeneratorParam(\Generator $pencils): void
    {
        foreach ($pencils as $pencil) {
            $pencil->sketch();
        }
    }
}
"#;

    // Hover on `$pencils` inside the foreach header (line 9)
    let hover =
        hover_at(&backend, uri, content, 9, 18).expect("hover should be active on $pencils");
    let text = hover_text(&hover);
    assert!(
        !text.contains("namespace Demo"),
        "should not show file namespace for global Generator type, got: {}",
        text
    );
    assert!(
        text.contains("Generator<int, Pencil>"),
        "should show full generic type, got: {}",
        text
    );
}

/// Catch variable hover should not show the enclosing file's namespace
/// when the exception type is global (e.g. `\RuntimeException`).
#[test]
fn hover_catch_variable_namespace_from_type_not_file() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace Demo;
class Demo {
    public function risky(): void
    {
        try {
            throw new \RuntimeException('oops');
        } catch (\RuntimeException $e) {
            echo $e->getMessage();
        }
    }
}
"#;

    // Hover on `$e` at the catch binding (line 7)
    let hover =
        hover_at(&backend, uri, content, 7, 36).expect("hover should be active on catch $e");
    let text = hover_text(&hover);
    assert!(
        !text.contains("namespace Demo"),
        "should not show file namespace for global RuntimeException, got: {}",
        text
    );
    assert!(
        text.contains("RuntimeException"),
        "should show RuntimeException, got: {}",
        text
    );
}

/// When the type is in a real namespace (e.g. `\App\Models\User`),
/// the hover should show `namespace App\Models;` and the short name.
#[test]
fn hover_variable_namespaced_type_shows_type_namespace() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App\Services;
class MyService {
    /**
     * @param \App\Models\User $user
     */
    public function process($user): void
    {
        echo $user;
    }
}
"#;

    // Hover on `$user` in the method body (line 8)
    let hover = hover_at(&backend, uri, content, 8, 14).expect("hover should be active on $user");
    let text = hover_text(&hover);
    assert!(
        text.contains("namespace App\\Models"),
        "should show the type's namespace, got: {}",
        text
    );
    assert!(
        !text.contains("namespace App\\Services"),
        "should not show the file's namespace, got: {}",
        text
    );
}

/// When a variable's type is a `@template` parameter, hover should show
/// the template variance and bound above the code block.
#[test]
fn hover_variable_with_template_type_shows_template_info() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @template-covariant TNode of AstNode
 */
class NodeList {
    /**
     * @param TNode $node
     */
    public function add($node): void {
        echo $node;
    }
}
class AstNode {}
"#;

    // Hover on `$node` in `echo $node;` (line 9)
    let hover = hover_at(&backend, uri, content, 9, 14).expect("hover should be active on $node");
    let text = hover_text(&hover);
    assert!(
        text.contains("template-covariant"),
        "should show template variance, got: {}",
        text
    );
    assert!(
        text.contains("TNode"),
        "should show template name, got: {}",
        text
    );
    assert!(
        text.contains("AstNode"),
        "should show template bound, got: {}",
        text
    );
    assert!(
        text.contains("$node = TNode"),
        "should still show type assignment, got: {}",
        text
    );
}

/// An invariant `@template` without a bound shows just the variance and name.
#[test]
fn hover_variable_with_unbounded_template_shows_variance() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @template T
 */
class Box {
    /**
     * @param T $value
     */
    public function set($value): void {
        echo $value;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 9, 14).expect("hover should be active on $value");
    let text = hover_text(&hover);
    assert!(
        text.contains("template"),
        "should show template variance, got: {}",
        text
    );
    assert!(
        text.contains("`T`"),
        "should show template name, got: {}",
        text
    );
    assert!(
        text.contains("$value = T"),
        "should still show type assignment, got: {}",
        text
    );
}

/// When a property's `@var` type is a `@template` parameter on the owning
/// class, hover should show the template variance and bound.
#[test]
fn hover_property_with_template_type_shows_template_info() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @template-covariant TNode of AstNode
 */
class NodeList {
    /** @var TNode */
    public $node;

    public function demo(): void {
        $this->node;
    }
}
class AstNode {
    public function getChildren(): array { return []; }
}
"#;

    // Hover on `node` in `$this->node` (line 9)
    let hover = hover_at(&backend, uri, content, 9, 16).expect("expected hover on node property");
    let text = hover_text(&hover);
    assert!(
        text.contains("template-covariant"),
        "should show template variance, got: {}",
        text
    );
    assert!(
        text.contains("TNode"),
        "should show template name, got: {}",
        text
    );
    assert!(
        text.contains("AstNode"),
        "should show template bound, got: {}",
        text
    );
}

/// A property typed with a `@template` parameter that has no bound
/// should still show the variance and name.
#[test]
fn hover_property_with_unbounded_template_shows_variance() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @template T
 */
class Box {
    /** @var T */
    public $value;

    public function demo(): void {
        $this->value;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 9, 16).expect("expected hover on value property");
    let text = hover_text(&hover);
    assert!(
        text.contains("template"),
        "should show template variance, got: {}",
        text
    );
    assert!(
        text.contains("`T`"),
        "should show template name, got: {}",
        text
    );
}

/// When a method's `@return` type is a `@template` parameter on the owning
/// class, hover should show the template variance and bound.
#[test]
fn hover_method_with_template_return_type_shows_template_info() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @template-covariant TNode of AstNode
 */
class NodeList {
    /**
     * @return TNode
     */
    public function first() {}

    public function demo(): void {
        $this->first();
    }
}
class AstNode {}
"#;

    // Hover on `first` in `$this->first()` (line 11)
    let hover = hover_at(&backend, uri, content, 11, 16).expect("expected hover on first()");
    let text = hover_text(&hover);
    assert!(
        text.contains("template-covariant"),
        "should show template variance, got: {}",
        text
    );
    assert!(
        text.contains("TNode"),
        "should show template name, got: {}",
        text
    );
    assert!(
        text.contains("AstNode"),
        "should show template bound, got: {}",
        text
    );
}

/// When a method's `@param` type is a `@template` parameter, hover should
/// show the template info.  Duplicate templates (same name in both param
/// and return) should appear only once.
#[test]
fn hover_method_with_template_param_type_shows_template_info() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @template T
 * @template-covariant TNode of AstNode
 */
class NodeList {
    /**
     * @param TNode $node
     * @param T $extra
     * @return TNode
     */
    public function add($node, $extra) {}

    public function demo(): void {
        $this->add(null, null);
    }
}
class AstNode {}
"#;

    // Hover on `add` in `$this->add(...)` (line 14)
    let hover = hover_at(&backend, uri, content, 14, 15).expect("expected hover on add()");
    let text = hover_text(&hover);
    assert!(
        text.contains("template-covariant"),
        "should show covariant variance for TNode, got: {}",
        text
    );
    assert!(
        text.contains("`T`"),
        "should show template T info, got: {}",
        text
    );
    // TNode appears in both @return and @param — should only show once
    let count = text.matches("template-covariant").count();
    assert_eq!(
        count, 1,
        "TNode template info should appear exactly once, got {} in: {}",
        count, text
    );
}

// ─── @var scope isolation ───────────────────────────────────────────────────

/// An inline `/** @var Type $var */` annotation in one method must not
/// leak into a different method that uses the same variable name.
#[test]
fn hover_var_annotation_does_not_leak_across_method_scopes() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class ObjectShapeDemo {
    public function demo(): void {
        /** @var object{title: string, score: float} $item */
        $item = getUnknownValue();
        $item->title;
    }
}

class ObjectMapper {
    /**
     * @template T
     * @param T $item
     * @return T
     */
    public function identity(mixed $item): mixed {
        return $item;
    }
}
"#;

    // Hover on `$item` in ObjectMapper::identity (line 16, `return $item;`)
    let hover = hover_at(&backend, uri, content, 16, 16).expect("expected hover on $item");
    let text = hover_text(&hover);
    assert!(
        !text.contains("object{"),
        "should NOT leak @var from ObjectShapeDemo into ObjectMapper, got: {}",
        text
    );
    assert!(
        text.contains("$item = T"),
        "should resolve $item to template param T, got: {}",
        text
    );
}

/// Same-method `/** @var */` still works after the scope fix.
#[test]
fn hover_var_annotation_within_same_method_still_works() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Demo {
    public function run(): void {
        /** @var object{name: string} $thing */
        $thing = getUnknown();
        echo $thing;
    }
}
"#;

    // Hover on `$thing` in `echo $thing;` (line 5)
    let hover = hover_at(&backend, uri, content, 5, 14).expect("expected hover on $thing");
    let text = hover_text(&hover);
    assert!(
        text.contains("object{name: string}"),
        "should still resolve @var in the same method, got: {}",
        text
    );
}

// ─── Method-level template info in hover ────────────────────────────────────

/// When a method declares its own `@template T` and uses it in `@return`,
/// hover should show `**template** \`T\`` (method-level, always invariant).
#[test]
fn hover_method_level_template_in_return_shows_info() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class ObjectMapper {
    /**
     * @template T
     * @param T $item
     * @return T
     */
    public function identity(mixed $item): mixed {
        return $item;
    }

    public function demo(): void {
        $this->identity(null);
    }
}
"#;

    // Hover on `identity` in `$this->identity(null)` (line 12)
    let hover = hover_at(&backend, uri, content, 12, 16).expect("expected hover on identity()");
    let text = hover_text(&hover);
    assert!(
        text.contains("**template** `T`"),
        "should show method-level template info for T, got: {}",
        text
    );
}

/// Method-level `@template T of Model` should show the bound.
#[test]
fn hover_method_level_template_with_bound_shows_info() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Model {}
class Repo {
    /**
     * @template T of Model
     * @param class-string<T> $class
     * @return T
     */
    public function find(string $class): mixed {
        return new $class();
    }

    public function demo(): void {
        $this->find(Model::class);
    }
}
"#;

    // Hover on `find` in `$this->find(...)` (line 13)
    let hover = hover_at(&backend, uri, content, 13, 16).expect("expected hover on find()");
    let text = hover_text(&hover);
    assert!(
        text.contains("**template** `T` of `Model`"),
        "should show method-level template with bound, got: {}",
        text
    );
}

/// Method-level template takes priority over a same-named class-level template.
#[test]
fn hover_method_level_template_takes_priority_over_class_level() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Animal {}
class Plant {}
/**
 * @template T of Animal
 */
class Container {
    /**
     * @template T of Plant
     * @param T $item
     * @return T
     */
    public function wrap($item) {}

    public function demo(): void {
        $this->wrap(null);
    }
}
"#;

    // Hover on `wrap` in `$this->wrap(null)` (line 15)
    let hover = hover_at(&backend, uri, content, 15, 16).expect("expected hover on wrap()");
    let text = hover_text(&hover);
    assert!(
        text.contains("of `Plant`"),
        "should show method-level bound (Plant), not class-level (Animal), got: {}",
        text
    );
    assert!(
        !text.contains("of `Animal`"),
        "should NOT show class-level bound, got: {}",
        text
    );
}

/// When the method has no templates but the class does, class-level
/// template info should still appear (existing behavior preserved).
#[test]
fn hover_class_level_template_still_shown_when_method_has_none() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @template-covariant TValue of object
 */
class TypedBox {
    /**
     * @return TValue
     */
    public function get() {}

    public function demo(): void {
        $this->get();
    }
}
"#;

    // Hover on `get` in `$this->get()` (line 11)
    let hover = hover_at(&backend, uri, content, 11, 16).expect("expected hover on get()");
    let text = hover_text(&hover);
    assert!(
        text.contains("**template-covariant** `TValue` of `object`"),
        "should show class-level template info when method has no own templates, got: {}",
        text
    );
}

// ─── Scope method hover on Builder instances ────────────────────────────────

// Minimal Laravel framework stubs for hover tests.
// These mirror the stubs in completion_laravel.rs but are kept here to avoid
// cross-test-file dependencies.

const HOVER_LARAVEL_COMPOSER: &str = r#"{
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "Illuminate\\Database\\Eloquent\\": "vendor/illuminate/Eloquent/",
            "Illuminate\\Database\\Query\\": "vendor/illuminate/Query/",
            "Illuminate\\Database\\Concerns\\": "vendor/illuminate/Concerns/"
        }
    }
}"#;

const HOVER_MODEL_PHP: &str = "\
<?php
namespace Illuminate\\Database\\Eloquent;
class Model {
    /** @return \\Illuminate\\Database\\Eloquent\\Builder<static> */
    public static function with(mixed $relations): Builder { return new Builder(); }
}
";

const HOVER_BUILDER_PHP: &str = "\
<?php
namespace Illuminate\\Database\\Eloquent;
use Illuminate\\Database\\Concerns\\BuildsQueries;
/**
 * @template TModel of \\Illuminate\\Database\\Eloquent\\Model
 * @mixin \\Illuminate\\Database\\Query\\Builder
 */
class Builder {
    /** @use BuildsQueries<TModel> */
    use BuildsQueries;
    /** @return static */
    public function where(string $column, mixed $operator = null, mixed $value = null): static { return $this; }
    /** @return static */
    public function orderBy(string $column, string $direction = 'asc'): static { return $this; }
    /** @return \\Illuminate\\Database\\Eloquent\\Collection<int, TModel> */
    public function get(): Collection { return new Collection(); }
    /** @return static */
    public function limit(int $value): static { return $this; }
}
";

const HOVER_QUERY_BUILDER_PHP: &str = "\
<?php
namespace Illuminate\\Database\\Query;
class Builder {
    /** @return static */
    public function whereIn(string $column, array $values): static { return $this; }
    /** @return static */
    public function groupBy(string ...$groups): static { return $this; }
}
";

const HOVER_BUILDS_QUERIES_PHP: &str = "\
<?php
namespace Illuminate\\Database\\Concerns;
/**
 * @template TValue
 */
trait BuildsQueries {
    /** @return TValue|null */
    public function first(): mixed { return null; }
}
";

const HOVER_COLLECTION_PHP: &str = "\
<?php
namespace Illuminate\\Database\\Eloquent;
/**
 * @template TKey of array-key
 * @template TModel
 */
class Collection {
    /** @return TModel|null */
    public function first(): mixed { return null; }
}
";

/// Build a PSR-4 workspace with the minimal Laravel framework stubs
/// plus extra application files.
fn make_laravel_hover_workspace(app_files: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let mut files: Vec<(&str, &str)> = vec![
        ("vendor/illuminate/Eloquent/Model.php", HOVER_MODEL_PHP),
        ("vendor/illuminate/Eloquent/Builder.php", HOVER_BUILDER_PHP),
        (
            "vendor/illuminate/Eloquent/Collection.php",
            HOVER_COLLECTION_PHP,
        ),
        (
            "vendor/illuminate/Query/Builder.php",
            HOVER_QUERY_BUILDER_PHP,
        ),
        (
            "vendor/illuminate/Concerns/BuildsQueries.php",
            HOVER_BUILDS_QUERIES_PHP,
        ),
    ];
    files.extend_from_slice(app_files);
    create_psr4_workspace(HOVER_LARAVEL_COMPOSER, &files)
}

/// Hovering on a scope method (or any Builder method) called on a variable
/// that holds a Builder instance should show the method hover.
///
/// Reproduces the user's exact case:
///   $query = BlogAuthor::where('genre', 'fiction');
///   $query->active();          // ← hover on `active` should work
///   $query->orderBy('name');   // ← hover on `orderBy` should work
#[test]
fn hover_scope_method_on_builder_variable() {
    let brand_php = "\
<?php
namespace App;
use Illuminate\\Database\\Eloquent\\Model;
use Illuminate\\Database\\Eloquent\\Builder;
class Brand extends Model {
    public function scopeActive(Builder $query): void {}
    public function test() {
        $query = Brand::where('genre', 'fiction');
        $query->active();
        $query->orderBy('name')->get();
    }
}
";
    let (backend, _dir) = make_laravel_hover_workspace(&[("src/Brand.php", brand_php)]);

    let uri = format!("file://{}", _dir.path().join("src/Brand.php").display());
    backend.update_ast(&uri, brand_php);

    // Line 8:  "        $query->active();"
    //           0         1
    //           0123456789012345678
    // `active` starts at col 16

    // Hover on `active` — a scope method on the Builder variable
    let hover = hover_at(&backend, &uri, brand_php, 8, 17);
    assert!(
        hover.is_some(),
        "hover should be shown on scope method active() called on $query (Builder variable)"
    );
    let text = hover_text(hover.as_ref().unwrap());
    assert!(
        text.contains("active"),
        "hover should mention active, got: {}",
        text
    );

    // Line 9:  "        $query->orderBy('name')->get();"
    //           0         1         2         3
    //           01234567890123456789012345678901234567890
    // `orderBy` starts at col 16, `get` starts at col 35

    // Hover on `orderBy` — a Builder-forwarded method
    let hover_ob = hover_at(&backend, &uri, brand_php, 9, 18);
    assert!(
        hover_ob.is_some(),
        "hover should be shown on orderBy() called on $query (Builder variable)"
    );

    // Hover on `get` — chained after orderBy()
    let hover_get = hover_at(&backend, &uri, brand_php, 9, 36);
    assert!(
        hover_get.is_some(),
        "hover should be shown on get() chained after $query->orderBy()"
    );
}

/// Hovering on scope methods after an inline Builder chain
/// (e.g. `Brand::where('id', 1)->active()->get()`) should work.
#[test]
fn hover_scope_method_after_inline_builder_chain() {
    let brand_php = "\
<?php
namespace App;
use Illuminate\\Database\\Eloquent\\Model;
use Illuminate\\Database\\Eloquent\\Builder;
class Brand extends Model {
    public function scopeActive(Builder $query): void {}
    public function scopeOfGenre(Builder $query, string $genre): void {}
    public function test() {
        Brand::where('active', 1)->active()->ofGenre('sci-fi')->get();
    }
}
";
    let (backend, _dir) = make_laravel_hover_workspace(&[("src/Brand.php", brand_php)]);

    let uri = format!("file://{}", _dir.path().join("src/Brand.php").display());
    backend.update_ast(&uri, brand_php);

    // Line 8: "        Brand::where('active', 1)->active()->ofGenre('sci-fi')->get();"
    //          0         1         2         3         4         5         6
    //          0123456789012345678901234567890123456789012345678901234567890123456789

    // Hover on `where` (col ~15)
    let h_where = hover_at(&backend, &uri, brand_php, 8, 16);
    assert!(
        h_where.is_some(),
        "hover should work on where() in Brand::where()"
    );

    // Hover on `active` (col ~35)
    let h_active = hover_at(&backend, &uri, brand_php, 8, 36);
    assert!(
        h_active.is_some(),
        "hover should work on scope method active() after Brand::where() chain"
    );

    // Hover on `ofGenre` (col ~46)
    let h_genre = hover_at(&backend, &uri, brand_php, 8, 47);
    assert!(
        h_genre.is_some(),
        "hover should work on scope method ofGenre() after chained scope"
    );
}

/// Hover on scope/Builder methods in a single-file with multiple namespace
/// blocks — simulates example.php where Eloquent stubs and user code live
/// in the same file under separate `namespace { }` blocks.
#[test]
fn hover_scope_method_multi_namespace_single_file() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace Illuminate\Database\Eloquent {
   abstract class Model {
       /** @return \Illuminate\Database\Eloquent\Builder<static> */
       public static function query() {}
   }

   /**
    * @template TModel of \Illuminate\Database\Eloquent\Model
    *
    * @mixin \Illuminate\Database\Query\Builder
    */
   class Builder {
       /**
        * @param  (\Closure(static): mixed)|string|array  $column
        * @return $this
        */
       public function where($column, $operator = null, $value = null, $boolean = 'and') {}

       /** @return \Illuminate\Database\Eloquent\Collection<int, TModel> */
       public function get($columns = ['*']) { return new Collection(); }
   }

   /**
    * @template TKey of array-key
    * @template TModel of \Illuminate\Database\Eloquent\Model
    */
   class Collection {}
}

namespace Illuminate\Database\Query {
   class Builder {
       /** @return $this */
       public function orderBy($column, $direction = 'asc') { return $this; }
       /** @return $this */
       public function limit($value) { return $this; }
   }
}

namespace Demo {
   use Illuminate\Database\Eloquent\Model;
   use Illuminate\Database\Eloquent\Builder;

   class BlogAuthor extends Model {
       public function scopeActive(Builder $query): void {}
       public function scopeOfGenre(Builder $query, string $genre): void {}
   }

   class EloquentDemo {
       public function run(): void {
           $author = new BlogAuthor();
           $author->active();
           BlogAuthor::active();

           BlogAuthor::where('active', 1)->active()->ofGenre('sci-fi')->get();

           $query = BlogAuthor::where('genre', 'fiction');
           $query->active();
           $query->orderBy('name')->get();
       }
   }
}
"#;

    // Find the actual line numbers dynamically
    let author_active_line = content
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("$author->active()"))
        .map(|(i, _)| i as u32)
        .expect("should find $author->active() line");

    let static_active_line = content
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("BlogAuthor::active()"))
        .map(|(i, _)| i as u32)
        .expect("should find BlogAuthor::active() line");

    let inline_chain_line = content
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("BlogAuthor::where('active', 1)->active"))
        .map(|(i, _)| i as u32)
        .expect("should find inline chain line");

    let query_active_line = content
        .lines()
        .enumerate()
        .find(|(_, l)| l.trim() == "$query->active();")
        .map(|(i, _)| i as u32)
        .expect("should find $query->active() line");

    let query_orderby_line = content
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("$query->orderBy"))
        .map(|(i, _)| i as u32)
        .expect("should find $query->orderBy line");

    // $author->active()
    let h_instance_scope = hover_at(&backend, uri, content, author_active_line, 23);
    assert!(
        h_instance_scope.is_some(),
        "hover should work on $author->active() (scope on model instance)"
    );

    // BlogAuthor::active()
    let h_static_scope = hover_at(&backend, uri, content, static_active_line, 25);
    assert!(
        h_static_scope.is_some(),
        "hover should work on BlogAuthor::active() (scope as static)"
    );

    // BlogAuthor::where('active', 1)->...
    let h_where = hover_at(&backend, uri, content, inline_chain_line, 26);
    assert!(
        h_where.is_some(),
        "hover should work on where() in BlogAuthor::where() (builder-forwarded)"
    );

    // $query->active()
    let h_var_scope = hover_at(&backend, uri, content, query_active_line, 21);
    assert!(
        h_var_scope.is_some(),
        "hover should work on $query->active() (scope on Builder variable)"
    );

    // $query->orderBy('name')->get()
    let h_order_by = hover_at(&backend, uri, content, query_orderby_line, 22);
    assert!(
        h_order_by.is_some(),
        "hover should work on $query->orderBy() (Builder method on variable)"
    );
}

/// Reproduces the Builder cache-poisoning scenario.
///
/// When `resolve_class_fully_cached` is called with a plain Builder
/// (without model-specific scope methods), the cached entry for
/// `Illuminate\Database\Eloquent\Builder` has no scopes.  If a
/// subsequent hover resolves a Builder<Model> with injected scopes
/// but then re-resolves via the cache, it gets the stale entry and
/// the scope method is not found.
///
/// This test forces a cache entry for plain Builder first (by hovering
/// on a Builder method via a different model that has no scopes), then
/// hovers on a scope method on a second model's Builder chain.
#[test]
fn hover_scope_survives_builder_cache_poisoning() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace Illuminate\Database\Eloquent {
    abstract class Model {
        /** @return \Illuminate\Database\Eloquent\Builder<static> */
        public static function query() {}
    }

    /**
     * @template TModel of \Illuminate\Database\Eloquent\Model
     * @mixin \Illuminate\Database\Query\Builder
     */
    class Builder {
        /** @return $this */
        public function where($column, $operator = null, $value = null) {}
        /** @return \Illuminate\Database\Eloquent\Collection<int, TModel> */
        public function get($columns = ['*']) { return new Collection(); }
        /** @return static */
        public function orderBy(string $column, string $direction = 'asc'): static { return $this; }
    }

    /** @template TKey @template TModel */
    class Collection {}
}

namespace Illuminate\Database\Query {
    class Builder {
        /** @return $this */
        public function limit(int $value) { return $this; }
    }
}

namespace App {
    use Illuminate\Database\Eloquent\Model;
    use Illuminate\Database\Eloquent\Builder;

    // Model with NO scope methods — hovering on its Builder chain
    // populates the cache with a plain Builder entry.
    class PlainModel extends Model {}

    // Model WITH scope methods.
    class ScopedModel extends Model {
        public function scopeFeatured(Builder $query): void {}
        public function scopeRecent(Builder $query): void {}
    }

    class Demo {
        public function run(): void {
            // Step 1: hover on get() here populates the Builder cache
            // with a plain Builder (no scope methods from PlainModel).
            PlainModel::where('id', 1)->get();

            // Step 2: hover on featured() here must still work even
            // though the Builder cache was seeded without scopes.
            ScopedModel::where('active', 1)->featured()->recent()->get();

            // Also test $variable path.
            $q = ScopedModel::where('status', 'draft');
            $q->featured();
        }
    }
}
"#;

    // Parse ONCE — do NOT re-parse between hovers.
    backend.update_ast(uri, content);

    let lines: Vec<&str> = content.lines().collect();

    // Helper to find a line and column.
    let find = |pattern: &str, token: &str| -> (u32, u32) {
        let idx = lines
            .iter()
            .enumerate()
            .find(|(_, l)| l.contains(pattern))
            .map(|(i, _)| i)
            .unwrap_or_else(|| panic!("should find {:?}", pattern));
        let col = lines[idx]
            .find(token)
            .unwrap_or_else(|| panic!("should find token {:?} on line {:?}", token, lines[idx]))
            as u32;
        (idx as u32, col + 1)
    };

    // ── Step 1: Hover on `get()` after PlainModel::where() ──────────
    // This forces `resolve_class_fully_cached` to cache the Builder
    // FQN with PlainModel's (empty) scope set.
    let (line, col) = find("PlainModel::where('id', 1)->get()", "get()");
    let h_get = backend.handle_hover(
        uri,
        content,
        Position {
            line,
            character: col,
        },
    );
    assert!(
        h_get.is_some(),
        "hover should work on get() after PlainModel::where() (line {})",
        line
    );

    // ── Step 2: Hover on `featured()` after ScopedModel::where() ────
    // The Builder cache now has an entry WITHOUT ScopedModel's scopes.
    // Before the fix, this would return None because the cached Builder
    // was missing the `featured` scope method.
    let (line, col) = find("ScopedModel::where('active', 1)->featured()", "featured()");
    let h_featured = backend.handle_hover(
        uri,
        content,
        Position {
            line,
            character: col,
        },
    );
    assert!(
        h_featured.is_some(),
        "hover should work on featured() after ScopedModel::where() even when Builder cache was seeded by PlainModel (line {})",
        line
    );
    let text = hover_text(h_featured.as_ref().unwrap());
    assert!(
        text.contains("featured"),
        "hover text should mention featured, got: {}",
        text
    );

    // ── Step 3: Hover on `recent()` chained after featured() ────────
    let chain_line = lines
        .iter()
        .enumerate()
        .find(|(_, l)| l.contains("->featured()->recent()"))
        .map(|(i, _)| i)
        .expect("should find chain line");
    let recent_col = lines[chain_line]
        .find("recent()")
        .expect("should find recent()") as u32
        + 1;
    let h_recent = backend.handle_hover(
        uri,
        content,
        Position {
            line: chain_line as u32,
            character: recent_col,
        },
    );
    assert!(
        h_recent.is_some(),
        "hover should work on recent() chained after featured() (line {})",
        chain_line
    );

    // ── Step 4: Hover on $q->featured() via variable ────────────────
    let (line, col) = find("$q->featured();", "featured();");
    let h_var = backend.handle_hover(
        uri,
        content,
        Position {
            line,
            character: col,
        },
    );
    assert!(
        h_var.is_some(),
        "hover should work on $q->featured() (Builder variable, line {})",
        line
    );
}

// ── Crash regressions ───────────────────────────────────────────────────────

/// Hovering on `$q` inside a nested closure that reuses the same variable
/// name as the outer closure used to cause infinite recursion in the hover
/// handler.  Fixed by a thread-local recursion depth guard in
/// `infer_callable_params_from_receiver`.
#[test]
fn hover_nested_closure_reused_variable_does_not_crash() {
    let backend = create_test_backend();
    let uri = "file:///nested_closure.php";
    let content = r#"<?php
namespace App;

class QueryBuilder {
    public function where(string $col, mixed $val = null): static { return $this; }
    public function whereNull(string $col): static { return $this; }
    public function orWhere(mixed ...$args): static { return $this; }
    public function whereHas(string $rel, \Closure $cb): static { return $this; }
}

class Repo {
    public function list(): void {
        $query = new QueryBuilder();
        $query->where(function ($q) {
            $q->whereNull('user_id')
                ->orWhere('user_id', 1)
                ->orWhere(function ($q): void {
                    $q->where('is_public', 1)
                        ->where('is_verified', 1);
                });
        });
    }
}
"#;
    backend.update_ast(uri, content);

    // Line 17 is `->orWhere(function ($q): void {` — the crashing line.
    // Hover at the midpoint which lands on `function` or `($q)`.
    let line_text = content.lines().nth(17).unwrap();
    let col = (line_text.len() / 2) as u32;
    let position = Position {
        line: 17,
        character: col,
    };

    // This must not stack-overflow.
    backend.handle_hover(uri, content, position);
}

/// When a variable is narrowed via `/** @var Provider */` or
/// `assert($var instanceof Provider)` where `Provider` is imported
/// via a `use` statement from another file, hover should resolve
/// the type through the use-map to the concrete class, not to a
/// different class that happens to share the same short name.
#[test]
fn hover_variable_narrowed_by_var_and_instanceof_with_use_import() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Contracts\\": "contracts/",
                    "Concrete\\": "concrete/"
                }
            }
        }"#,
        &[
            (
                "contracts/Provider.php",
                concat!(
                    "<?php\n",
                    "namespace Contracts;\n",
                    "interface Provider {\n",
                    "    public function redirect(): string;\n",
                    "}\n",
                ),
            ),
            (
                "concrete/Provider.php",
                concat!(
                    "<?php\n",
                    "namespace Concrete;\n",
                    "class Provider implements \\Contracts\\Provider {\n",
                    "    public function redirect(): string { return ''; }\n",
                    "    public function stateless(): static { return $this; }\n",
                    "}\n",
                ),
            ),
        ],
    );

    // --- @var narrowing ---
    let var_uri = "file:///test_var.php";
    let var_content = concat!(
        "<?php\n",
        "use Concrete\\Provider;\n",
        "class VarTest {\n",
        "    public function run(): void {\n",
        "        /** @var Provider $provider */\n",
        "        $provider = $this->getProvider();\n",
        "        $provider;\n",
        "    }\n",
        "}\n",
    );

    // Parse the cross-file classes so they're in the classmap.
    let contracts_uri = format!(
        "file://{}",
        _dir.path().join("contracts/Provider.php").display()
    );
    let contracts_content =
        std::fs::read_to_string(_dir.path().join("contracts/Provider.php")).unwrap();
    backend.update_ast(&contracts_uri, &contracts_content);

    let concrete_uri = format!(
        "file://{}",
        _dir.path().join("concrete/Provider.php").display()
    );
    let concrete_content =
        std::fs::read_to_string(_dir.path().join("concrete/Provider.php")).unwrap();
    backend.update_ast(&concrete_uri, &concrete_content);

    // Hover on `$provider` at line 6 (the bare `$provider;` usage)
    let hover = hover_at(&backend, var_uri, var_content, 6, 9);
    assert!(
        hover.is_some(),
        "hover should resolve @var-narrowed variable"
    );
    let text = hover_text(hover.as_ref().unwrap());
    assert!(
        text.contains("namespace Concrete"),
        "@var Provider should resolve to Concrete\\Provider via use-map, got: {}",
        text
    );
    assert!(
        !text.contains("namespace Contracts"),
        "@var Provider should NOT resolve to Contracts\\Provider, got: {}",
        text
    );

    // --- instanceof narrowing ---
    let instanceof_uri = "file:///test_instanceof.php";
    let instanceof_content = concat!(
        "<?php\n",
        "use Concrete\\Provider;\n",
        "class InstanceofTest {\n",
        "    /** @return \\Contracts\\Provider */\n",
        "    public function getProvider() {}\n",
        "    public function run(): void {\n",
        "        $provider = $this->getProvider();\n",
        "        assert($provider instanceof Provider);\n",
        "        $provider;\n",
        "    }\n",
        "}\n",
    );

    // Hover on `$provider` at line 8 (after the assert)
    let hover2 = hover_at(&backend, instanceof_uri, instanceof_content, 8, 9);
    assert!(
        hover2.is_some(),
        "hover should resolve instanceof-narrowed variable"
    );
    let text2 = hover_text(hover2.as_ref().unwrap());
    assert!(
        text2.contains("namespace Concrete"),
        "instanceof Provider should resolve to Concrete\\Provider via use-map, got: {}",
        text2
    );
    assert!(
        !text2.contains("namespace Contracts"),
        "instanceof Provider should NOT resolve to Contracts\\Provider, got: {}",
        text2
    );
}

// ─── Ternary / elvis / null-coalesce variable type inference ────────────────

#[test]
fn hover_variable_assigned_via_elvis_operator_with_static_call() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
enum Country: string {
    case US = 'us';
    case UK = 'uk';
    /** @return array<int, self> */
    public static function getActiveCountries(): array {
        return [self::US, self::UK];
    }
}
class Indexer {
    public function index(array $markets = [], bool $shouldDelete = false): void {
        $markets = $markets ?: Country::getActiveCountries();
        $markets;
    }
}
"#;

    // Hover on `$markets` at line 12 (the bare `$markets;` usage after reassignment)
    let hover = hover_at(&backend, uri, content, 12, 9).expect("expected hover on $markets");
    let text = hover_text(&hover);
    assert!(
        text.contains("array<int, Country>"),
        "should resolve elvis RHS static call return type, got: {}",
        text
    );
}

#[test]
fn hover_variable_assigned_via_ternary_operator() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Bucket {}
class Factory {
    /** @return list<Bucket> */
    public static function makeBuckets(): array { return []; }
}
class Demo {
    public function run(bool $flag): void {
        $items = $flag ? Factory::makeBuckets() : Factory::makeBuckets();
        $items;
    }
}
"#;

    // Hover on `$items` at line 9
    let hover = hover_at(&backend, uri, content, 9, 9).expect("expected hover on $items");
    let text = hover_text(&hover);
    assert!(
        text.contains("list<Bucket>"),
        "should resolve ternary branches to list<Bucket>, got: {}",
        text
    );
}

#[test]
fn hover_variable_assigned_via_null_coalesce_operator() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Widget {}
class Store {
    /** @return list<Widget> */
    public function getWidgets(): array { return []; }
    public function run(): void {
        $widgets = $this->getWidgets() ?? [];
        $widgets;
    }
}
"#;

    // Hover on `$widgets` at line 7
    let hover = hover_at(&backend, uri, content, 7, 9).expect("expected hover on $widgets");
    let text = hover_text(&hover);
    assert!(
        text.contains("list<Widget>"),
        "should resolve null coalesce LHS to list<Widget>, got: {}",
        text
    );
}

#[test]
fn hover_variable_assigned_via_match_expression() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Alpha {}
class Beta {}
class Demo {
    public function run(int $mode): void {
        $result = match($mode) {
            1 => new Alpha(),
            2 => new Beta(),
        };
        $result;
    }
}
"#;

    // Hover on `$result` at line 9
    let hover = hover_at(&backend, uri, content, 9, 9).expect("expected hover on $result");
    let text = hover_text(&hover);
    assert!(
        text.contains("Alpha") || text.contains("Beta"),
        "should resolve match expression arm types, got: {}",
        text
    );
}

#[test]
fn hover_variable_assigned_via_elvis_with_identical_branches() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Item {}
class Repo {
    /** @return list<Item> */
    public function getItems(): array { return []; }
    /** @return list<Item> */
    public function getDefaultItems(): array { return []; }
    public function run(): void {
        $items = $this->getItems() ?: $this->getDefaultItems();
        $items;
    }
}
"#;

    // Hover on `$items` at line 9
    let hover = hover_at(&backend, uri, content, 9, 9).expect("expected hover on $items");
    let text = hover_text(&hover);
    // Both branches return list<Item>, so the union should be just list<Item>
    assert!(
        text.contains("list<Item>"),
        "should resolve elvis with identical branch types to list<Item>, got: {}",
        text
    );
    // Should NOT have a duplicated union like list<Item>|list<Item>
    assert!(
        !text.contains("list<Item>|list<Item>"),
        "should not duplicate identical types in union, got: {}",
        text
    );
}

#[test]
fn hover_foreach_over_variable_assigned_via_elvis_with_static_call() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
enum Country: string {
    case US = 'us';
    case UK = 'uk';
    public function label(): string { return $this->value; }
    /** @return array<int, self> */
    public static function getActiveCountries(): array {
        return [self::US, self::UK];
    }
}
class Indexer {
    public function index(array $markets = []): void {
        $markets = $markets ?: Country::getActiveCountries();
        foreach ($markets as $market) {
            $market->label();
        }
    }
}
"#;

    // Hover on `$market` inside the foreach body (line 14)
    let hover = hover_at(&backend, uri, content, 14, 13).expect("expected hover on $market");
    let text = hover_text(&hover);
    assert!(
        text.contains("Country"),
        "foreach value variable should resolve to Country, got: {}",
        text
    );
}

// ─── Closure parameter: inferred subclass wins over explicit parent ─────────

/// When a closure parameter has an explicit parent type hint (e.g. `Model`)
/// but the callable signature infers a more specific subclass (e.g.
/// `BrandTranslation extends Model`), hover should show the subclass type.
#[test]
fn hover_closure_param_inferred_subclass_wins_over_explicit_parent() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Model {
    public function save(): bool { return true; }
}
class BrandTranslation extends Model {
    public function getLangCode(): string { return ''; }
}
/**
 * @template TKey
 * @template TValue
 */
class Collection {
    /**
     * @param callable(TValue): mixed $callback
     * @return static
     */
    public function each(callable $callback): static {}
}
class BrandService {
    /** @return Collection<int, BrandTranslation> */
    public function getTranslations(): Collection {}
    public function run(): void {
        $translations = $this->getTranslations();
        $translations->each(function (Model $brandTranslation) {
            $brandTranslation->getLangCode();
        });
    }
}
"#;

    // Hover on `$brandTranslation` inside the closure body (line 24)
    let hover =
        hover_at(&backend, uri, content, 24, 13).expect("expected hover on $brandTranslation");
    let text = hover_text(&hover);
    assert!(
        text.contains("BrandTranslation"),
        "Hover should show inferred subclass BrandTranslation, not explicit Model, got: {}",
        text
    );
}

/// Inverse: when the explicit type hint is already a subclass of the inferred
/// type, the explicit type should still win for hover.
#[test]
fn hover_closure_param_explicit_subclass_wins_over_inferred_parent() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Animal {
    public function speak(): string { return ''; }
}
class Cat extends Animal {
    public function purr(): void {}
}
/**
 * @template TKey
 * @template TValue
 */
class Collection {
    /**
     * @param callable(TValue): mixed $callback
     * @return static
     */
    public function each(callable $callback): static {}
}
class Shelter {
    /** @return Collection<int, Animal> */
    public function getAnimals(): Collection {}
    public function run(): void {
        $animals = $this->getAnimals();
        $animals->each(function (Cat $c) {
            $c->purr();
        });
    }
}
"#;

    // Hover on `$c` inside the closure body (line 24)
    let hover = hover_at(&backend, uri, content, 24, 13).expect("expected hover on $c");
    let text = hover_text(&hover);
    assert!(
        text.contains("Cat"),
        "Hover should keep the explicit Cat type, got: {}",
        text
    );
}

// ─── Array shape element type inference ─────────────────────────────────────

/// Hovering on a variable assigned from an array literal should show
/// resolved types for parameter variables, property accesses, and method
/// calls instead of `mixed`.
#[test]
fn hover_array_shape_infers_parameter_and_property_types() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Decimal {
    public function toFixed(int $decimals): string { return ''; }
}
class Tracker {
    private string $websiteUuid = 'abc';
    public function cartTracking(string $trackingUserId, string $url, Decimal $total, array $productIds): void {
        $params = [
            'websiteUuid'    => $this->websiteUuid,
            'trackingUserId' => $trackingUserId,
            'total'          => $total->toFixed(2),
            'url'            => $url,
        ];
        $params;
    }
}
"#;

    // Hover on `$params` at line 13 (the bare `$params;` usage)
    let hover = hover_at(&backend, uri, content, 13, 9).expect("expected hover on $params");
    let text = hover_text(&hover);
    assert!(
        text.contains("array{"),
        "Hover should show array shape, got: {}",
        text
    );
    assert!(
        text.contains("websiteUuid: string"),
        "websiteUuid should be string (from property), got: {}",
        text
    );
    assert!(
        text.contains("trackingUserId: string"),
        "trackingUserId should be string (from parameter), got: {}",
        text
    );
    assert!(
        text.contains("total: string"),
        "total should be string (from toFixed return type), got: {}",
        text
    );
    assert!(
        text.contains("url: string"),
        "url should be string (from parameter), got: {}",
        text
    );
    assert!(
        !text.contains("mixed"),
        "No values should be mixed, got: {}",
        text
    );
}

/// Property access on `$this` inside an array literal value should resolve
/// to the property's declared type.
#[test]
fn hover_array_shape_infers_this_property_access() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Config {
    private int $retries = 3;
    private string $host = 'localhost';
    public function toArray(): array {
        $data = [
            'retries' => $this->retries,
            'host'    => $this->host,
        ];
        $data;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 9, 9).expect("expected hover on $data");
    let text = hover_text(&hover);
    assert!(
        text.contains("retries: int"),
        "retries should be int, got: {}",
        text
    );
    assert!(
        text.contains("host: string"),
        "host should be string, got: {}",
        text
    );
}

/// Method call return types used as array literal values should be
/// resolved in the array shape.
#[test]
fn hover_array_shape_infers_method_call_return_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Clock {
    public function now(): int { return time(); }
}
class Logger {
    public function build(Clock $clock): void {
        $meta = [
            'timestamp' => $clock->now(),
            'level'     => 'info',
        ];
        $meta;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 10, 9).expect("expected hover on $meta");
    let text = hover_text(&hover);
    assert!(
        text.contains("timestamp: int"),
        "timestamp should be int (from Clock::now()), got: {}",
        text
    );
    assert!(
        text.contains("level: string"),
        "level should be string (from literal), got: {}",
        text
    );
}

#[test]
fn hover_empty_array_literal_shows_array_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function test() {
    $items = [];
    $items;
}
"#;

    let hover = hover_at(&backend, uri, content, 3, 5).expect("expected hover on $items");
    let text = hover_text(&hover);
    assert!(
        text.contains("array"),
        "empty array literal should resolve to array type, got: {}",
        text
    );
    assert!(
        text.contains("$items"),
        "should mention the variable name, got: {}",
        text
    );
}

#[test]
fn hover_empty_legacy_array_literal_shows_array_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function test() {
    $items = array();
    $items;
}
"#;

    let hover = hover_at(&backend, uri, content, 3, 5).expect("expected hover on $items");
    let text = hover_text(&hover);
    assert!(
        text.contains("array"),
        "empty array() literal should resolve to array type, got: {}",
        text
    );
}

// ─── Extra @param tags & pseudo-type refinements ────────────────────────────

#[test]
fn hover_function_shows_extra_param_from_docblock() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @param class-string $tz
 * @param string $tz2 Deprecated
 */
function formatUtfDate(string $tz): void {
    $tz2 = func_get_args()[1];
}
formatUtfDate('foo');
"#;

    // Hover on function call should show both the native param and the extra docblock param.
    let hover = hover_at(&backend, uri, content, 8, 2).expect("expected hover on formatUtfDate");
    let text = hover_text(&hover);
    assert!(
        text.contains("$tz2"),
        "should show extra @param $tz2 from docblock: {}",
        text
    );
    assert!(
        text.contains("Deprecated"),
        "should show description for extra @param $tz2: {}",
        text
    );
}

#[test]
fn hover_method_shows_extra_param_from_docblock() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class DateHelper {
    /**
     * @param string $format
     * @param string $extra Optional extra arg
     */
    public function format(string $format): string {
        return '';
    }
}
$d = new DateHelper();
$d->format('Y-m-d');
"#;

    let hover = hover_at(&backend, uri, content, 11, 6).expect("expected hover on format");
    let text = hover_text(&hover);
    assert!(
        text.contains("$extra"),
        "should show extra @param $extra from method docblock: {}",
        text
    );
    assert!(
        text.contains("Optional extra arg"),
        "should show description for extra @param $extra: {}",
        text
    );
}

#[test]
fn hover_variable_shows_class_string_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @param class-string $tz
 */
function formatUtfDate(string $tz): void {
    $tz;
}
"#;

    // Hover on $tz inside the function body should show class-string, not string.
    let hover = hover_at(&backend, uri, content, 5, 5).expect("expected hover on $tz");
    let text = hover_text(&hover);
    assert!(
        text.contains("class-string"),
        "should show refined class-string type, got: {}",
        text
    );
}

#[test]
fn hover_function_shows_class_string_param_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @param class-string $tz
 */
function formatUtfDate(string $tz): void {}
formatUtfDate('foo');
"#;

    // Hover on function call should show class-string as param annotation.
    let hover = hover_at(&backend, uri, content, 5, 2).expect("expected hover on formatUtfDate");
    let text = hover_text(&hover);
    assert!(
        text.contains("class-string"),
        "should show class-string annotation for $tz param: {}",
        text
    );
}

#[test]
fn hover_variable_shows_non_empty_string_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @param non-empty-string $name
 */
function greet(string $name): void {
    $name;
}
"#;

    let hover = hover_at(&backend, uri, content, 5, 5).expect("expected hover on $name");
    let text = hover_text(&hover);
    assert!(
        text.contains("non-empty-string"),
        "should show refined non-empty-string type, got: {}",
        text
    );
}

#[test]
fn hover_variable_shows_positive_int_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @param positive-int $count
 */
function repeat(int $count): void {
    $count;
}
"#;

    let hover = hover_at(&backend, uri, content, 5, 5).expect("expected hover on $count");
    let text = hover_text(&hover);
    assert!(
        text.contains("positive-int"),
        "should show refined positive-int type, got: {}",
        text
    );
}

#[test]
fn hover_extra_param_does_not_duplicate_native_params() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @param string $a First arg
 * @param string $b Second arg
 */
function test(string $a, string $b): void {}
test('x', 'y');
"#;

    // Both params are native — no extra params should be appended.
    // The signature code block contains `$a` and the param description
    // section contains `**$a**`, so exactly two occurrences are expected.
    let hover = hover_at(&backend, uri, content, 6, 2).expect("expected hover on test");
    let text = hover_text(&hover);
    let sig_matches = text.matches("$a").count();
    assert_eq!(
        sig_matches, 2,
        "should have exactly two occurrences of $a (signature + description, not duplicated): {}",
        text
    );
}

// ─── Array shape string key access variable hover ───────────────────────────

#[test]
fn hover_variable_assigned_from_array_shape_string_key() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class User {
    public function getName(): string {}
}
class Demo {
    public function test(): void {
        /** @var array{name: User, age: int} $data */
        $data = getData();
        $name = $data['name'];
        $name->getName();
    }
}
"#;

    // Hover on $name should show User type
    let hover = hover_at(&backend, uri, content, 9, 9).expect("expected hover on $name");
    let text = hover_text(&hover);
    assert!(
        text.contains("User"),
        "$name should resolve to User from array shape key access, got: {}",
        text
    );
}

#[test]
fn hover_variable_assigned_from_chained_bracket_access() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Gift {
    public function open(): string {}
}
class Demo {
    public function test(): void {
        /** @var array{items: list<Gift>} $result */
        $result = getResult();
        $first = $result['items'][0];
        $first->open();
    }
}
"#;

    // Hover on $first should show Gift type
    let hover = hover_at(&backend, uri, content, 9, 9).expect("expected hover on $first");
    let text = hover_text(&hover);
    assert!(
        text.contains("Gift"),
        "$first should resolve to Gift from chained bracket access, got: {}",
        text
    );
}

#[test]
fn hover_variable_type_from_shape_shows_no_namespace_corruption() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Demo {
    public function test(): void {
        /** @var array{data: string, items: list<int>} $result */
        $result = getResult();
        $result;
    }
}
"#;

    // Hover on $result should show the shape type without a corrupted
    // namespace line (the `{` in `array{...}` must not bleed into the
    // namespace extraction).
    let hover = hover_at(&backend, uri, content, 5, 9).expect("expected hover on $result");
    let text = hover_text(&hover);
    assert!(
        !text.contains("namespace array"),
        "array shape type should not produce a 'namespace array' line, got: {}",
        text
    );
    assert!(
        text.contains("array{"),
        "hover should show the array shape type, got: {}",
        text
    );
}

// ─── Ternary / null-coalesce subject extraction ─────────────────────────────

#[test]
fn hover_short_ternary_member_access() {
    let backend = create_test_backend();
    let uri = "file:///b6_short_ternary.php";
    let content = r#"<?php
class Gadget {
    public string $label = '';
}
class B6Demo {
    public function run(?Gadget $a, Gadget $b): void {
        ($a ?: $b)->label;
    }
}
"#;

    // Hover on `label` in `($a ?: $b)->label` (line 6, character 21)
    let hover = hover_at(&backend, uri, content, 6, 21);
    assert!(
        hover.is_some(),
        "hover should resolve the member through short ternary subject"
    );
    let text = hover_text(hover.as_ref().unwrap());
    assert!(
        text.contains("label"),
        "hover should mention 'label', got: {}",
        text
    );
}

#[test]
fn hover_null_coalesce_member_access() {
    let backend = create_test_backend();
    let uri = "file:///b6_null_coalesce.php";
    let content = r#"<?php
class Sensor {
    public int $value = 0;
}
class B6Demo2 {
    public function run(?Sensor $a, Sensor $b): void {
        ($a ?? $b)->value;
    }
}
"#;

    // Hover on `value` in `($a ?? $b)->value` (line 6, character 21)
    let hover = hover_at(&backend, uri, content, 6, 21);
    assert!(
        hover.is_some(),
        "hover should resolve the member through null-coalesce subject"
    );
    let text = hover_text(hover.as_ref().unwrap());
    assert!(
        text.contains("value"),
        "hover should mention 'value', got: {}",
        text
    );
}

#[test]
fn hover_full_ternary_member_access() {
    let backend = create_test_backend();
    let uri = "file:///b6_full_ternary.php";
    let content = r#"<?php
class Engine {
    public function start(): void {}
}
class B6Demo3 {
    public function run(bool $flag, Engine $a, Engine $b): void {
        ($flag ? $a : $b)->start();
    }
}
"#;

    // Hover on `start` in `($flag ? $a : $b)->start()` (line 6, character 28)
    let hover = hover_at(&backend, uri, content, 6, 28);
    assert!(
        hover.is_some(),
        "hover should resolve the member through full ternary subject"
    );
    let text = hover_text(hover.as_ref().unwrap());
    assert!(
        text.contains("start"),
        "hover should mention 'start', got: {}",
        text
    );
}

// ── Null coalesce (`??`) type refinement ────────────────────────────────────

#[test]
fn hover_null_coalesce_non_nullable_lhs_shows_only_lhs_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Pen {
    public function write(): void {}
}
class Marker {
    public function draw(): void {}
}
class Svc {
    public function test(): void {
        $a = new Pen() ?? new Marker();
        $a->write();
    }
}
"#;

    // Hover on `$a` at line 10 (the usage `$a->write()`)
    let hover = hover_at(&backend, uri, content, 10, 9).expect("expected hover on $a");
    let text = hover_text(&hover);
    assert!(
        text.contains("Pen"),
        "hover should show Pen (non-nullable LHS of ??), got: {}",
        text
    );
    assert!(
        !text.contains("Marker"),
        "hover should NOT show Marker (RHS is dead code), got: {}",
        text
    );
}

#[test]
fn hover_null_coalesce_nullable_lhs_shows_union() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Pen {
    public function write(): void {}
}
class Marker {
    public function draw(): void {}
}
class Svc {
    /** @return ?Pen */
    public function maybePen(): ?Pen { return null; }
    public function test(): void {
        $b = $this->maybePen() ?? new Marker();
        $b->write();
    }
}
"#;

    // Hover on `$b` at line 12 (the usage `$b->write()`)
    let hover = hover_at(&backend, uri, content, 12, 9).expect("expected hover on $b");
    let text = hover_text(&hover);
    assert!(
        text.contains("Pen"),
        "hover should show Pen (nullable LHS stripped of null), got: {}",
        text
    );
    assert!(
        text.contains("Marker"),
        "hover should show Marker (RHS of ?? when LHS is nullable), got: {}",
        text
    );
}

#[test]
fn hover_null_coalesce_clone_lhs_shows_only_cloned_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Pen {
    public function write(): void {}
}
class Marker {
    public function draw(): void {}
}
class Svc {
    public function test(Pen $p): void {
        $c = clone $p ?? new Marker();
        $c->write();
    }
}
"#;

    // Hover on `$c` at line 10 (the usage `$c->write()`)
    let hover = hover_at(&backend, uri, content, 10, 9).expect("expected hover on $c");
    let text = hover_text(&hover);
    assert!(
        text.contains("Pen"),
        "hover should show Pen (clone is non-nullable), got: {}",
        text
    );
    assert!(
        !text.contains("Marker"),
        "hover should NOT show Marker (RHS is dead code after clone), got: {}",
        text
    );
}

/// Verify that hover agrees with completion for `??` when the LHS is a
/// method call.  `getWidget()` returns non-nullable `Widget`, so the RHS
/// is dead code and hover should show `Widget` only.
#[test]
fn hover_null_coalesce_method_call_lhs_not_lost() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Widget {
    public function render(): void {}
}
class DefaultWidget {
    public function render(): void {}
}
class Service {
    public function getWidget(): Widget { return new Widget(); }
}
class App {
    public function test(): void {
        $svc = new Service();
        $w = $svc->getWidget() ?? new DefaultWidget();
        $w->render();
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 14, 9).expect("expected hover on $w");
    let text = hover_text(&hover);
    // getWidget() returns Widget (non-nullable), so $w should be Widget only
    assert!(
        text.contains("Widget"),
        "hover should show Widget, got: {}",
        text
    );
}

/// The `??` null-coalesce divergence: when the raw-type engine cannot resolve
/// the LHS (returns `None`), it falls through to RHS-only.  The ClassInfo
/// engine checks whether the LHS AST node is *syntactically* non-nullable
/// (e.g. `clone`, `new`, literal) and skips the RHS.  This test uses `clone`
/// on a variable whose type comes from a *method call return* — the raw-type
/// engine's simple `resolve_rhs_raw_type(clone_expr.object)` recurse may not
/// resolve the inner method call, causing the `None` path in `??` to fire and
/// show only the RHS type.
#[test]
fn hover_null_coalesce_clone_of_method_call_shows_lhs() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Config {
    public function get(): string { return ''; }
}
class Fallback {
    public function get(): string { return ''; }
}
class Factory {
    public function makeConfig(): Config { return new Config(); }
}
class App {
    public function test(): void {
        $factory = new Factory();
        $cfg = clone $factory->makeConfig() ?? new Fallback();
        $cfg->get();
    }
}
"#;

    // Hover on `$cfg` at line 14 (`$cfg->get()`)
    let hover = hover_at(&backend, uri, content, 14, 9).expect("expected hover on $cfg");
    let text = hover_text(&hover);
    // `clone` is syntactically non-nullable, so the result should be Config,
    // not Fallback.  If hover shows Fallback (or Fallback only), the raw-type
    // engine's `??` handler is incorrectly falling through to the RHS.
    assert!(
        text.contains("Config"),
        "hover should show Config (clone is non-nullable LHS of ??), got: {}",
        text
    );
    assert!(
        !text.contains("Fallback"),
        "hover should NOT show Fallback (RHS is dead code after clone), got: {}",
        text
    );
}

/// When the LHS of `??` is an immediately-invoked closure (which produces a
/// non-nullable result), the raw-type engine may not resolve it and fall
/// through to RHS-only.  The ClassInfo engine does not have this problem
/// because closures/arrow-fns are explicitly matched as non-nullable.
/// This test verifies hover shows the correct type.
#[test]
fn hover_null_coalesce_closure_invocation_lhs() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Alpha {
    public function run(): void {}
}
class Beta {
    public function run(): void {}
}
class Svc {
    public function test(): void {
        $x = (function(): Alpha { return new Alpha(); })() ?? new Beta();
        $x->run();
    }
}
"#;

    // Hover on `$x` at line 10 (`$x->run()`)
    let hover = hover_at(&backend, uri, content, 10, 9).expect("expected hover on $x");
    let text = hover_text(&hover);
    // The invoked closure returns Alpha (non-nullable).
    // At minimum, hover should include Alpha.
    assert!(
        text.contains("Alpha"),
        "hover should include Alpha from the closure return type, got: {}",
        text
    );
}

/// Verify that hover and completion agree for a variable assigned from a
/// `clone` of another variable typed by a parameter hint.  This is the
/// simplest clone divergence scenario.
#[test]
fn hover_clone_of_typed_variable() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Document {
    public function save(): void {}
}
class Editor {
    public function test(Document $doc): void {
        $copy = clone $doc;
        $copy->save();
    }
}
"#;

    // Hover on `$copy` at line 7 (`$copy->save()`)
    let hover = hover_at(&backend, uri, content, 7, 9).expect("expected hover on $copy");
    let text = hover_text(&hover);
    assert!(
        text.contains("Document"),
        "hover on clone of typed param should show Document, got: {}",
        text
    );
}

// ── Constant type inference ─────────────────────────────────────────────────

#[test]
fn hover_variable_assigned_from_global_constant() {
    let backend = create_test_backend();
    let uri = "file:///test.php";

    // Register a file that defines a global constant via `define()`.
    let const_file = r#"<?php
define('MY_TIMEOUT', 30);
"#;
    backend.update_ast("file:///constants.php", const_file);

    let content = r#"<?php
function test() {
    $timeout = MY_TIMEOUT;
    echo $timeout;
}
"#;

    // Hover on `$timeout` at line 3 (the `echo $timeout` usage).
    let hover = hover_at(&backend, uri, content, 3, 10).expect("expected hover on $timeout");
    let text = hover_text(&hover);
    assert!(
        text.contains("int"),
        "variable assigned from integer constant should resolve to int, got: {}",
        text
    );
}

#[test]
fn hover_variable_assigned_from_string_constant() {
    let backend = create_test_backend();
    let uri = "file:///test.php";

    let const_file = r#"<?php
define('APP_NAME', 'PHPantom');
"#;
    backend.update_ast("file:///constants.php", const_file);

    let content = r#"<?php
function test() {
    $name = APP_NAME;
    echo $name;
}
"#;

    let hover = hover_at(&backend, uri, content, 3, 10).expect("expected hover on $name");
    let text = hover_text(&hover);
    assert!(
        text.contains("string"),
        "variable assigned from string constant should resolve to string, got: {}",
        text
    );
}

#[test]
fn hover_variable_assigned_from_top_level_const() {
    let backend = create_test_backend();
    let uri = "file:///test.php";

    let const_file = r#"<?php
const MAX_RETRIES = 5;
"#;
    backend.update_ast("file:///constants.php", const_file);

    let content = r#"<?php
function test() {
    $retries = MAX_RETRIES;
    echo $retries;
}
"#;

    let hover = hover_at(&backend, uri, content, 3, 10).expect("expected hover on $retries");
    let text = hover_text(&hover);
    assert!(
        text.contains("int"),
        "variable assigned from top-level const int should resolve to int, got: {}",
        text
    );
}

#[test]
fn hover_variable_assigned_from_class_constant_without_type_hint() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Config {
    const TIMEOUT = 30;
    const NAME = 'app';
    const RATE = 3.14;
    const ENABLED = true;
}
function test() {
    $a = Config::TIMEOUT;
    $b = Config::NAME;
    $c = Config::RATE;
    $d = Config::ENABLED;
    echo $a;
    echo $b;
    echo $c;
    echo $d;
}
"#;

    let hover_a = hover_at(&backend, uri, content, 12, 10).expect("expected hover on $a");
    let text_a = hover_text(&hover_a);
    assert!(
        text_a.contains("int"),
        "Config::TIMEOUT (int literal) should infer int, got: {}",
        text_a
    );

    let hover_b = hover_at(&backend, uri, content, 13, 10).expect("expected hover on $b");
    let text_b = hover_text(&hover_b);
    assert!(
        text_b.contains("string"),
        "Config::NAME (string literal) should infer string, got: {}",
        text_b
    );

    let hover_c = hover_at(&backend, uri, content, 14, 10).expect("expected hover on $c");
    let text_c = hover_text(&hover_c);
    assert!(
        text_c.contains("float"),
        "Config::RATE (float literal) should infer float, got: {}",
        text_c
    );

    let hover_d = hover_at(&backend, uri, content, 15, 10).expect("expected hover on $d");
    let text_d = hover_text(&hover_d);
    assert!(
        text_d.contains("bool"),
        "Config::ENABLED (bool literal) should infer bool, got: {}",
        text_d
    );
}

#[test]
fn hover_variable_assigned_from_class_constant_with_type_hint_takes_precedence() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Settings {
    public const string VERSION = '1.0';
}
function test() {
    $v = Settings::VERSION;
    echo $v;
}
"#;

    // When a typed class constant exists, the explicit type hint should
    // be used (not the value-based inference).
    let hover = hover_at(&backend, uri, content, 6, 10).expect("expected hover on $v");
    let text = hover_text(&hover);
    assert!(
        text.contains("string"),
        "typed class constant should use the type hint, got: {}",
        text
    );
}

#[test]
fn hover_variable_assigned_from_bool_constant() {
    let backend = create_test_backend();
    let uri = "file:///test.php";

    let const_file = r#"<?php
define('DEBUG_MODE', false);
"#;
    backend.update_ast("file:///constants.php", const_file);

    let content = r#"<?php
function test() {
    $debug = DEBUG_MODE;
    echo $debug;
}
"#;

    let hover = hover_at(&backend, uri, content, 3, 10).expect("expected hover on $debug");
    let text = hover_text(&hover);
    assert!(
        text.contains("bool"),
        "variable assigned from bool constant should resolve to bool, got: {}",
        text
    );
}

#[test]
fn hover_variable_assigned_from_array_constant() {
    let backend = create_test_backend();
    let uri = "file:///test.php";

    let const_file = r#"<?php
define('ALLOWED_HOSTS', ['localhost', '127.0.0.1']);
"#;
    backend.update_ast("file:///constants.php", const_file);

    let content = r#"<?php
function test() {
    $hosts = ALLOWED_HOSTS;
    echo $hosts;
}
"#;

    let hover = hover_at(&backend, uri, content, 3, 10).expect("expected hover on $hosts");
    let text = hover_text(&hover);
    assert!(
        text.contains("array"),
        "variable assigned from array constant should resolve to array, got: {}",
        text
    );
}

// ─── Guard clause null narrowing in hover ───────────────────────────────────

#[test]
fn hover_guard_clause_falsy_continue_narrows_null() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class OrderLine {
    public int $actualAmount = 0;
}
class Svc {
    /** @param array<int, OrderLine|null> $lines */
    public function test(array $lines): void {
        foreach ($lines as $line) {
            if (!$line) {
                continue;
            }
            $line->actualAmount;
        }
    }
}
"#;

    // Hover on `$line` at line 11 (after the guard clause)
    let hover = hover_at(&backend, uri, content, 11, 13).expect("expected hover on $line");
    let text = hover_text(&hover);
    assert!(
        text.contains("OrderLine") && !text.contains("null"),
        "after `if (!$line) {{ continue; }}`, hover should show OrderLine without null, got: {}",
        text
    );
}

#[test]
fn hover_guard_clause_null_identity_return_narrows() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Formatter {
    public function format(string $s): string { return $s; }
}
class Svc {
    public function test(?Formatter $fmt): void {
        if ($fmt === null) {
            return;
        }
        $fmt->format('hello');
    }
}
"#;

    // Hover on `$fmt` at line 9 (after the guard clause)
    let hover = hover_at(&backend, uri, content, 9, 9).expect("expected hover on $fmt");
    let text = hover_text(&hover);
    assert!(
        text.contains("Formatter") && !text.contains("null"),
        "after `if ($fmt === null) {{ return; }}`, hover should show Formatter without null, got: {}",
        text
    );
}

#[test]
fn hover_guard_clause_null_coalesce_then_falsy_continue() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class OrderLine {
    public int $actualAmount = 0;
    public int $amount = 0;
}
class Svc {
    /** @param array<int, OrderLine> $warehouseOrderLines */
    public function test(array $warehouseOrderLines): void {
        foreach ($warehouseOrderLines as $key => $val) {
            $warehouseOrderline = $warehouseOrderLines[$key] ?? null;
            if (!$warehouseOrderline) {
                continue;
            }
            $warehouseOrderline->actualAmount;
        }
    }
}
"#;

    // Hover on `$warehouseOrderline` at line 13 (after the guard clause)
    let hover =
        hover_at(&backend, uri, content, 13, 17).expect("expected hover on $warehouseOrderline");
    let text = hover_text(&hover);
    assert!(
        text.contains("OrderLine") && !text.contains("null"),
        "after null coalesce + falsy guard, hover should show OrderLine without null, got: {}",
        text
    );
}

// ─── Inline @var cast should not override variable type in RHS ──────────────

#[test]
fn hover_inline_var_cast_does_not_override_rhs_without_varname() {
    let backend = create_test_backend();
    let uri = "file:///b15_hover_no_varname.php";
    let content = r#"<?php
class Data {
    public function toArray(): array { return []; }
    public function count(): int { return 0; }
}
class Test {
    public function run(Data $data): array {
        /** @var array<string, mixed> */
        $data = $data->toArray();
        return $data;
    }
}
"#;

    // Hover on `$data` in the RHS (line 8, the second $data after `= `)
    // `        $data = $data->toArray();`
    //                  ^~~~~ cursor here (character 16)
    let hover = hover_at(&backend, uri, content, 8, 17).expect("expected hover on RHS $data");
    let text = hover_text(&hover);
    assert!(
        text.contains("Data"),
        "RHS $data should resolve to Data (the previous type), got: {}",
        text
    );
    assert!(
        !text.contains("array<string, mixed>"),
        "RHS $data should NOT show the @var cast type, got: {}",
        text
    );
}

#[test]
fn hover_inline_var_cast_does_not_override_rhs_with_varname() {
    let backend = create_test_backend();
    let uri = "file:///b15_hover_varname.php";
    let content = r#"<?php
class ApiResponse {
    public function getBody(): string { return ''; }
    public function json(): array { return []; }
}
class Test {
    public function handle(ApiResponse $response): array {
        /** @var array<string, mixed> $response */
        $response = $response->json();
        return $response;
    }
}
"#;

    // Hover on `$response` in the RHS (line 8, the second $response after `= `)
    // `        $response = $response->json();`
    //                      ^~~~~~~~~ cursor here (character 20)
    let hover = hover_at(&backend, uri, content, 8, 21).expect("expected hover on RHS $response");
    let text = hover_text(&hover);
    assert!(
        text.contains("ApiResponse"),
        "RHS $response should resolve to ApiResponse (the previous type), got: {}",
        text
    );
    assert!(
        !text.contains("array<string, mixed>"),
        "RHS $response should NOT show the @var cast type, got: {}",
        text
    );
}

#[test]
fn hover_inline_var_cast_applies_after_assignment() {
    let backend = create_test_backend();
    let uri = "file:///b15_hover_after.php";
    let content = r#"<?php
class Data {
    public function toArray(): array { return []; }
}
class Wrapper {
    public string $name;
}
class Test {
    public function run(Data $data): void {
        /** @var Wrapper */
        $data = $data->toArray();
        $data->name;
    }
}
"#;

    // Hover on `$data` on line 11 (after the assignment) — @var should apply
    let hover =
        hover_at(&backend, uri, content, 11, 9).expect("expected hover on $data after assignment");
    let text = hover_text(&hover);
    assert!(
        text.contains("Wrapper"),
        "@var override should apply after the assignment, got: {}",
        text
    );
}

#[test]
fn hover_standalone_var_annotation_still_applies() {
    let backend = create_test_backend();
    let uri = "file:///b15_hover_standalone.php";
    let content = r#"<?php
class Formatter {
    public function format(string $s): string { return $s; }
}
class Test {
    public function run(): void {
        $data = get_data();
        /** @var Formatter $data */
        $data->format('hello');
    }
}
"#;

    // Hover on `$data` on line 8 (after standalone @var annotation)
    // The @var annotation is standalone (no assignment), so it should apply.
    let hover = hover_at(&backend, uri, content, 8, 9)
        .expect("expected hover on $data after standalone @var");
    let text = hover_text(&hover);
    assert!(
        text.contains("Formatter"),
        "standalone @var annotation should apply, got: {}",
        text
    );
}

/// When a property is typed as `Collection<SectionTranslation>` (single
/// generic arg on a class with `@template TKey of array-key` and
/// `@template TValue`), calling `->where()->first()` should resolve
/// TValue to `SectionTranslation`, not leave it as a raw template param.
///
/// This is the common PHP pattern of writing `Collection<Model>` instead
/// of `Collection<int, Model>`.  The single arg should right-align to
/// bind to TValue.
#[test]
fn hover_collection_single_generic_arg_resolves_value_type() {
    let backend = create_test_backend();
    let uri = "file:///test_collection_single_arg.php";
    let content = r#"<?php
/**
 * @template TKey of array-key
 * @template-covariant TValue
 */
class Collection {
    /**
     * @param callable|string $key
     * @return static
     */
    public function where($key, $operator = null, $value = null) { return $this; }

    /**
     * @template TFirstDefault
     * @param (callable(TValue, TKey): bool)|null $callback
     * @param TFirstDefault|(\Closure(): TFirstDefault) $default
     * @return TValue|TFirstDefault
     */
    public function first(?callable $callback = null, $default = null) { return null; }
}

class SectionTranslation {
    public string $title = '';
    public bool $enabled = false;
}

class Section {
    /** @var Collection<SectionTranslation> */
    public $translations;
}

class Test {
    public function run(Section $section): void {
        $result = $section->translations->where('lang_code', 'en')->first();
        $result;
    }
}
"#;

    // Hover on `$result` on line 34 (the standalone reference, after assignment)
    let hover = hover_at(&backend, uri, content, 34, 9).expect("expected hover on $result");
    let text = hover_text(&hover);
    assert!(
        text.contains("SectionTranslation"),
        "Hover should resolve TValue to SectionTranslation via right-aligned generic arg, got: {}",
        text
    );
    assert!(
        !text.contains("TValue"),
        "Hover should not show raw template param TValue, got: {}",
        text
    );
    // TFirstDefault should resolve to `null` because $default = null and
    // no second argument was passed to first().
    assert!(
        !text.contains("TFirstDefault"),
        "TFirstDefault should resolve to null (parameter default), got: {}",
        text
    );
    assert!(
        text.contains("null"),
        "Hover should include null from the resolved TFirstDefault default, got: {}",
        text
    );
}

// ── Assignment inside if-condition ─────────────────────────────────────

#[test]
fn hover_variable_assigned_in_if_condition() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let php = r#"<?php
class AdminUser {
    public function assignRole(string $role): void {}
    /** @return ?static */
    public static function first(): ?static { return new static(); }
}
function test(string $role): void {
    if ($admin = AdminUser::first()) {
        $admin->assignRole($role);
    }
}
"#;
    // Hover on `$admin` inside the if-body (line 8, the `$admin->assignRole` line)
    let hover = hover_at(&backend, uri, php, 8, 9);
    assert!(hover.is_some(), "should produce hover for $admin");
    let text = hover_text(hover.as_ref().unwrap());
    eprintln!("if-condition assignment hover text: {}", text);
    assert!(
        text.contains("AdminUser"),
        "hover should resolve $admin to AdminUser inside if-body, got: {}",
        text
    );
    assert!(
        !text.contains("null"),
        "hover should not include null inside truthy if-body, got: {}",
        text
    );
}

/// When a method returns `TValue|null` and `TValue` is substituted with
/// a concrete class, the `|null` component must be preserved in hover output.
#[test]
fn hover_nullable_template_return_type_preserves_null() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @template TValue
 */
class Builder {
    /** @return TValue|null */
    public function first() {}

    /** @return static */
    public static function query(): static { return new static(); }
}

/**
 * @extends Builder<AdminUser>
 */
class AdminUser extends Builder {
    public function assignRole(string $role): void {}
}

function test(): void {
    $builder = AdminUser::query();
    $admin = $builder->first();
    $admin;
}
"#;

    // Hover on `$admin` at the standalone `$admin;` line (line 22)
    let hover = hover_at(&backend, uri, content, 22, 5);
    assert!(hover.is_some(), "should produce hover for $admin");
    let text = hover_text(hover.as_ref().unwrap());
    eprintln!("nullable template hover text: {}", text);
    assert!(
        text.contains("AdminUser"),
        "hover should resolve $admin to AdminUser, got: {}",
        text
    );
    assert!(
        text.contains("null"),
        "hover should preserve |null from TValue|null after substitution, got: {}",
        text
    );
}

/// Nullable shorthand `?TValue` should also preserve nullability after template substitution.
#[test]
fn hover_nullable_shorthand_template_return_type_preserves_null() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @template TValue
 */
class Builder2 {
    /** @return ?TValue */
    public function first() {}

    /** @return static */
    public static function query(): static { return new static(); }
}

/**
 * @extends Builder2<AdminUser2>
 */
class AdminUser2 extends Builder2 {}

function test2(): void {
    $builder = AdminUser2::query();
    $admin = $builder->first();
    $admin;
}
"#;

    // Hover on `$admin` at the standalone `$admin;` line (line 20)
    let hover = hover_at(&backend, uri, content, 20, 5);
    assert!(hover.is_some(), "should produce hover for $admin");
    let text = hover_text(hover.as_ref().unwrap());
    eprintln!("nullable ?TValue hover text: {}", text);
    assert!(
        text.contains("AdminUser2"),
        "hover should resolve $admin to AdminUser2, got: {}",
        text
    );
    assert!(
        text.contains("?") || text.contains("null"),
        "hover should preserve nullability from ?TValue after substitution, got: {}",
        text
    );
}

/// Non-generic `@return Foo|null` should preserve `|null`.
#[test]
fn hover_non_generic_nullable_return_type_preserves_null() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Widget {
    public function name(): string { return ''; }
}

class WidgetFactory {
    /** @return Widget|null */
    public function find(): ?Widget { return null; }
}

function test3(): void {
    $factory = new WidgetFactory();
    $w = $factory->find();
    $w;
}
"#;

    // Hover on `$w` at the standalone `$w;` line (line 13)
    let hover = hover_at(&backend, uri, content, 13, 5);
    assert!(hover.is_some(), "should produce hover for $w");
    let text = hover_text(hover.as_ref().unwrap());
    eprintln!("non-generic nullable hover text: {}", text);
    assert!(
        text.contains("Widget"),
        "hover should resolve $w to Widget, got: {}",
        text
    );
    assert!(
        text.contains("null") || text.contains("?"),
        "hover should preserve nullability from Widget|null, got: {}",
        text
    );
}

/// When a closure parameter has an explicit bare class type hint and the
/// callable signature infers the same class WITH generic arguments, hover
/// should show the generic version (e.g. `Builder<Order>`) instead of
/// the bare class name (`Builder`).  This verifies that the
/// `from_classes_with_hint` path preserves the inferred type string.
#[test]
fn hover_closure_param_inferred_generic_args_preserved_in_type_string() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Order {
    public function getTotal(): float { return 0.0; }
}

/**
 * @template T
 */
class Builder {
    /** @return static */
    public function where(string $col, mixed $val = null): static { return $this; }
}

class Processor {
    /**
     * @param callable(Builder<Order>): mixed $callback
     * @return void
     */
    public function apply(callable $callback): void {}

    public function run(): void {
        $this->apply(function (Builder $q) {
            $q;
        });
    }
}
"#;

    // Hover on `$q` at the standalone `$q;` statement (line 22)
    let hover = hover_at(&backend, uri, content, 22, 13).expect("expected hover on $q");
    let text = hover_text(&hover);
    assert!(
        text.contains("Builder<"),
        "Hover should show Builder<…> with generic param, not bare Builder, got: {}",
        text
    );
}

#[test]
fn hover_variable_assigned_from_chained_method_preserves_generic_params() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Order {
    public function getTotal(): float { return 0.0; }
}

/**
 * @template T
 */
class Builder {
    /** @return static */
    public function where(string $col, mixed $val = null): static { return $this; }
}

class Processor {
    /**
     * @param callable(Builder<Order>): mixed $callback
     * @return void
     */
    public function apply(callable $callback): void {}

    public function run(): void {
        $this->apply(function (Builder $q) {
            $a = $q->where('published', 1);
            $a;
        });
    }
}
"#;

    // Hover on `$a` at the standalone `$a;` statement (line 23)
    let hover = hover_at(&backend, uri, content, 23, 13).expect("expected hover on $a");
    let text = hover_text(&hover);
    assert!(
        text.contains("Builder<"),
        "Hover on $a should show Builder<…> with generic param (not bare Builder), got: {}",
        text
    );
}

#[test]
fn hover_variable_assigned_from_multi_step_chain_preserves_generic_params() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Article {
    public function getTitle(): string { return ''; }
}

/**
 * @template T
 */
class Builder {
    /** @return static */
    public function where(string $col, mixed $val = null): static { return $this; }
    /** @return static */
    public function orderBy(string $col): static { return $this; }
}

class Service {
    /**
     * @param callable(Builder<Article>): mixed $cb
     */
    public function query(callable $cb): void {}

    public function run(): void {
        $this->query(function (Builder $q) {
            $b = $q->where('published', 1)->orderBy('title');
            $b;
        });
    }
}
"#;

    // Hover on `$b` at the standalone `$b;` statement (line 23)
    let hover = hover_at(&backend, uri, content, 23, 13).expect("expected hover on $b");
    let text = hover_text(&hover);
    assert!(
        text.contains("Builder<"),
        "Hover on $b (multi-step chain) should show Builder<…>, got: {}",
        text
    );
}

#[test]
fn hover_variable_assigned_from_method_on_generic_variable_preserves_params() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class User {
    public int $id;
}

/**
 * @template TModel
 */
class Collection {
    /** @return static */
    public function filter(callable $cb): static { return $this; }
    /** @return static */
    public function values(): static { return $this; }
}

class Handler {
    /**
     * @param Collection<User> $users
     */
    public function handle(Collection $users): void {
        $filtered = $users->filter(fn($u) => $u->id > 0);
        $filtered;
    }
}
"#;

    // Hover on `$filtered` at the standalone `$filtered;` statement (line 21)
    let hover = hover_at(&backend, uri, content, 21, 9).expect("expected hover on $filtered");
    let text = hover_text(&hover);
    assert!(
        text.contains("Collection<"),
        "Hover on $filtered should show Collection<…> with generic param, got: {}",
        text
    );
}

#[test]
fn hover_variable_generic_preserved_after_prior_member_hover() {
    // Regression: hovering on `$q->` (member access) first, then hovering
    // on `$a` (variable) showed bare `$a` with no type.  The first hover
    // must not poison any cache or depth counter that prevents the second
    // hover from resolving.
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Article {
    public function getTitle(): string { return ''; }
}

/**
 * @template T
 */
class Builder {
    /** @return static */
    public function where(string $col, mixed $val = null): static { return $this; }
    /** @return static */
    public function whereLanguage(string $lang): static { return $this; }
}

class Repo {
    /**
     * @param callable(Builder<Article>): mixed $cb
     */
    public function query(callable $cb): void {}

    public function run(): void {
        $this->query(function (Builder $q) {
            $a = $q->where('published', 1);
            $a->whereLanguage('en');
        });
    }
}
"#;

    // Line 23 (0-based): "            $a = $q->where('published', 1);"
    //   $a starts at col 12, $q starts at col 17

    // 1. Hover on `$q` variable (line 23, col 18) — simulates the user
    //    first resolving $q, which exercises the closure-param inference
    //    path and may populate caches.
    let hover_q = hover_at(&backend, uri, content, 23, 18);
    let q_text = hover_q
        .as_ref()
        .map(|h| hover_text(h).to_string())
        .unwrap_or_else(|| "(none)".to_string());
    assert!(hover_q.is_some(), "hover on $q should resolve");

    // 2. Now hover on `$a` at the assignment site (line 23, col 13).
    //    This must still show Builder<Article>, not bare `$a`.
    let hover_var = hover_at(&backend, uri, content, 23, 13);
    let a_text = hover_var
        .as_ref()
        .map(|h| hover_text(h).to_string())
        .unwrap_or_else(|| "(none)".to_string());
    assert!(
        a_text.contains("Builder<"),
        "Hover on $a (after prior $q hover) should show Builder<…> with generic param, not bare Builder.\n\
         $q hover returned: {}\n\
         $a hover returned: {}",
        q_text,
        a_text
    );
}

#[test]
fn hover_variable_at_dollar_sign_resolves_assignment_type() {
    // Regression: hovering on the `$` sign of a variable at its assignment
    // site returned no type, while hovering on the letter after `$` worked.
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Order { public string $id; }
class Service {
    public function run(): void {
        $order = new Order();
        $order->id;
    }
}
"#;

    // Line 4: "        $order = new Order();"
    //   col 8 is `$`, col 9 is `o`

    // Hover on `o` (col 9) — baseline, should work.
    let hover_letter =
        hover_at(&backend, uri, content, 4, 9).expect("hover on variable letter should resolve");
    let text_letter = hover_text(&hover_letter);
    assert!(
        text_letter.contains("Order"),
        "Hover on `o` of `$order` should show Order, got: {}",
        text_letter
    );

    // Hover on `$` (col 8) — must also work.
    let hover_dollar = hover_at(&backend, uri, content, 4, 8);
    let text_dollar = hover_dollar
        .as_ref()
        .map(|h| hover_text(h).to_string())
        .unwrap_or_else(|| "(none)".to_string());
    assert!(
        text_dollar.contains("Order"),
        "Hover on `$` of `$order` should show Order, got: {}",
        text_dollar
    );
}

// ─── Variable-key array assignment type strings ─────────────────────────────

#[test]
fn hover_variable_key_string_produces_array_string_value() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Pen {
    public function write(): string { return ''; }
    public function color(): string { return ''; }
}
class Svc {
    /** @param list<Pen> $pens */
    public function run(array $pens): void {
        $indexed = [];
        foreach ($pens as $pen) {
            $key = $pen->color();
            $indexed[$key] = $pen;
        }
        $indexed;
    }
}
"#;

    // Hover on `$indexed` at line 13 (the usage after the loop).
    // $key is string (from color()), so type should be array<string, Pen>.
    let hover = hover_at(&backend, uri, content, 13, 9).expect("expected hover on $indexed");
    let text = hover_text(&hover);
    assert!(
        text.contains("array<string, Pen>"),
        "Variable-key assignment with string key should produce array<string, Pen>, got: {}",
        text
    );
}

#[test]
fn hover_variable_key_int_produces_array_int_value() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Pen {
    public function write(): string { return ''; }
}
function run(int $id): void {
    $indexed = [];
    $indexed[$id] = new Pen();
    $indexed;
}
"#;

    // Hover on `$indexed` at line 7. $id is int (parameter type),
    // so type should be array<int, Pen>.
    let hover = hover_at(&backend, uri, content, 7, 5).expect("expected hover on $indexed");
    let text = hover_text(&hover);
    assert!(
        text.contains("array<int, Pen>"),
        "Variable-key assignment with int key should produce array<int, Pen>, got: {}",
        text
    );
}

#[test]
fn hover_variable_key_unknown_produces_array_intstring_value() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Pen {
    public function write(): string { return ''; }
}
function run(mixed $key): void {
    $indexed = [];
    $indexed[$key] = new Pen();
    $indexed;
}
"#;

    // Hover on `$indexed` at line 7. $key is mixed,
    // so type should be array<int|string, Pen>.
    let hover = hover_at(&backend, uri, content, 7, 5).expect("expected hover on $indexed");
    let text = hover_text(&hover);
    assert!(
        text.contains("array<int|string, Pen>"),
        "Variable-key assignment with mixed key should produce array<int|string, Pen>, got: {}",
        text
    );
}

#[test]
fn hover_push_style_produces_list() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Pen {
    public function write(): string { return ''; }
}
function run(): void {
    $items = [];
    $items[] = new Pen();
    $items;
}
"#;

    // Hover on `$items` at line 7. Push-style should produce list<Pen>.
    let hover = hover_at(&backend, uri, content, 7, 5).expect("expected hover on $items");
    let text = hover_text(&hover);
    assert!(
        text.contains("list<Pen>"),
        "Push-style assignment should produce list<Pen>, got: {}",
        text
    );
}

#[test]
fn hover_variable_assigned_from_conditional_return_shows_resolved_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class SubmissionProducerMessage {
    public function getId(): int {}
}

class Serializer {
    /**
     * @template TObject of object
     * @template TType of string|class-string<TObject>
     *
     * @param TType                $type
     * @param array<string, mixed> $context
     *
     * @phpstan-return ($type is class-string<TObject> ? TObject : mixed)
     * @psalm-return (TType is class-string<TObject> ? TObject : mixed)
     */
    public function deserialize(mixed $data, string $type, string $format, array $context = []): mixed {}
}

class Consumer {
    private Serializer $serializer;

    public function handle(string $buffer): void {
        $message = $this->serializer->deserialize($buffer, SubmissionProducerMessage::class, 'json');
        $message->getId();
    }
}
"#;

    // Hover on `$message` at its usage site (line 23, character 9)
    let hover = hover_at(&backend, uri, content, 23, 9).expect("expected hover on $message");
    let text = hover_text(&hover);
    // The type should be SubmissionProducerMessage, NOT SubmissionProducerMessage|mixed
    assert!(
        text.contains("SubmissionProducerMessage"),
        "Should show SubmissionProducerMessage, got: {}",
        text
    );
    assert!(
        !text.contains("mixed"),
        "Should NOT show mixed when conditional resolves to a concrete class, got: {}",
        text
    );
}

// ── Hover on generic method shows substituted return type ───────────────────

#[test]
fn hover_generic_trait_method_shows_concrete_return_type() {
    // A generic class uses a trait with a template param.  After trait
    // merging the method's return type contains the class's template
    // param.  When hovering on the method via a concrete instantiation,
    // the hover should show the substituted type, not the raw param.
    let content = r#"<?php
/** @template TItem */
trait Fetchable {
    /** @return TItem|null */
    public function first() { return null; }
}

/**
 * @template TElement
 */
class Box {
    /** @use Fetchable<TElement> */
    use Fetchable;

    /** @return static */
    public function filter(): static { return $this; }
}

class Pen {
    public function write(): string { return ''; }
}

function demo(): void {
    /** @var Box<Pen> $box */
    $box = new Box();
    $box->first();
}
"#;

    let uri = "file:///test.php";
    let backend = create_test_backend();
    backend.update_ast(uri, content);

    // Hover on `first` at line 25 (0-based): `$box->first();`
    let hover = hover_at(&backend, uri, content, 25, 11).expect("expected hover on first()");
    let text = hover_text(&hover);

    assert!(
        text.contains("Pen|null") || text.contains("Pen | null"),
        "Hover on first() should show substituted return type 'Pen|null', got:\n{}",
        text
    );
    assert!(
        !text.contains("TElement"),
        "Hover should NOT show raw template param 'TElement', got:\n{}",
        text
    );
}

#[test]
fn hover_class_string_from_class_constant() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Pen {
    public function write(): void {}
}
class Service {
    public function run(): void {
        $cls = Pen::class;
        $cls;
    }
}
"#;

    // Hover on $cls at line 7 (the standalone $cls; statement)
    let hover = hover_at(&backend, uri, content, 7, 9).expect("expected hover on $cls");
    let text = hover_text(&hover);
    assert!(
        text.contains("class-string"),
        "should show class-string<Pen> type for Pen::class assignment, got: {}",
        text
    );
}

#[test]
fn hover_is_int_narrows_union_to_int() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function test(int|string $x): void {
    if (is_int($x)) {
        return;
    }
    $x;
}
"#;
    let hover = hover_at(&backend, uri, content, 5, 4).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("string"),
        "should contain string, got: {}",
        text
    );
    assert!(
        !text.contains("int|string"),
        "should not contain int|string, got: {}",
        text
    );
}

#[test]
fn hover_array_shape_key_null_guard_clause() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function test(): void {
    /** @var array{test: ?int} $a */
    $a = ["test" => null];
    if ($a["test"] === null) {
        return;
    }
    $a;
}
"#;
    // Hover on $a after the guard clause to check if shape was narrowed
    let hover = hover_at(&backend, uri, content, 7, 4).expect("expected hover");
    let text = hover_text(&hover);
    eprintln!("DEBUG: $a type after guard clause: {}", text);
    assert!(
        text.contains("array{test: int}"),
        "after null guard clause, $a should be array{{test: int}}, got: {}",
        text
    );
}

#[test]
fn hover_class_hierarchy_union_simplified_after_instanceof_reassignment() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class ClassResolvesBack {
    public static function getA(): self { return new self(); }
}
class ClassResolvesBackChild extends ClassResolvesBack {}
function test(): void {
    $a = ClassResolvesBack::getA();
    if ($a instanceof ClassResolvesBackChild) {
        $a = new ClassResolvesBackChild;
    }
    $a;
}
"#;
    // After the if block, both branches produce a type assignable to
    // ClassResolvesBack, so the union should collapse to the parent.
    let hover = hover_at(&backend, uri, content, 10, 4).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("ClassResolvesBack") && !text.contains("ClassResolvesBackChild"),
        "after instanceof + reassignment, $a should be ClassResolvesBack, got: {}",
        text
    );
}

#[test]
fn hover_is_string_narrows_union_to_string() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function test(int|string $x): void {
    if (is_string($x)) {
        $x;
    }
}
"#;
    let hover = hover_at(&backend, uri, content, 3, 8).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("string"),
        "should contain string, got: {}",
        text
    );
    assert!(
        !text.contains("int|string"),
        "should not contain int|string, got: {}",
        text
    );
}

#[test]
fn hover_is_bool_narrows_union() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function test(string|bool $x): void {
    if (is_bool($x)) {
        $x;
    }
}
"#;
    let hover = hover_at(&backend, uri, content, 3, 8).expect("expected hover");
    let text = hover_text(&hover);
    assert!(text.contains("bool"), "should contain bool, got: {}", text);
    assert!(
        !text.contains("string"),
        "should not contain string, got: {}",
        text
    );
}

#[test]
fn hover_is_float_narrows_int_float_union() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function test(int|float $x): void {
    if (is_float($x)) {
        return;
    }
    $x;
}
"#;
    let hover = hover_at(&backend, uri, content, 5, 4).expect("expected hover");
    let text = hover_text(&hover);
    assert!(text.contains("int"), "should contain int, got: {}", text);
    assert!(
        !text.contains("float"),
        "should not contain float, got: {}",
        text
    );
}

#[test]
fn hover_instanceof_interface_narrows_object() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Loggable {
    public function log(): void;
}
function test(object $x): void {
    if (!$x instanceof Loggable) {
        return;
    }
    $x;
}
"#;
    let hover = hover_at(&backend, uri, content, 8, 4).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("Loggable"),
        "should contain Loggable, got: {}",
        text
    );
}

#[test]
fn hover_ternary_is_int_narrows() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function test(int|string $a): void {
    $b = is_int($a) ? $a : strlen($a);
    $b;
}
"#;
    let hover = hover_at(&backend, uri, content, 3, 4).expect("expected hover");
    let text = hover_text(&hover);
    assert!(text.contains("int"), "should contain int, got: {}", text);
}

#[test]
fn hover_nullsafe_chain_short_circuits_to_nullable() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Template {
    public function getName(): string { return ''; }
}
class Configuration {
    public function getTemplate(): Template { return new Template(); }
}
class FormItem {
    public readonly ?Configuration $configuration;
}
function test(FormItem $item): void {
    $result = $item->configuration?->getTemplate()->getName();
    $result;
}
"#;
    let hover = hover_at(&backend, uri, content, 12, 4).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("string"),
        "should contain string, got: {}",
        text
    );
}

#[test]
fn hover_null_check_on_getter_return_narrows() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Account {
    public function getType(): ?string { return null; }
}
function test(Account $account): void {
    $type = $account->getType();
    if ($type === null) {
        return;
    }
    $type;
}
"#;
    let hover = hover_at(&backend, uri, content, 9, 4).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("string"),
        "should contain string, got: {}",
        text
    );
    assert!(
        !text.contains("null"),
        "should not contain null, got: {}",
        text
    );
}

#[test]
fn hover_null_coalescing_on_nullable_method_return() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class ApiResponse {
    /** @return array<string, string>|null */
    public function getHeaders(): ?array { return null; }
}
function test(ApiResponse $response): void {
    $headers = $response->getHeaders() ?? [];
    $headers;
}
"#;
    let hover = hover_at(&backend, uri, content, 7, 4).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("array"),
        "should contain array, got: {}",
        text
    );
    assert!(
        !text.contains("null"),
        "should not contain null, got: {}",
        text
    );
}

#[test]
fn hover_docblock_var_override_changes_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class A {
    public function aMethod(): void {}
}
class B {
    public function bMethod(): void {}
}
function test(): void {
    $a = new B();
    /** @var A $a */
    $a;
}
"#;
    let hover = hover_at(&backend, uri, content, 10, 4).expect("expected hover");
    let text = hover_text(&hover);
    assert!(text.contains("A"), "should contain A, got: {}", text);
}

#[test]
fn hover_multi_branch_narrowing_null_then_is_string() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function test(int|string|null $value): void {
    if ($value === null) {
        return;
    }
    if (is_string($value)) {
        return;
    }
    $value;
}
"#;
    let hover = hover_at(&backend, uri, content, 8, 4).expect("expected hover");
    let text = hover_text(&hover);
    assert!(text.contains("int"), "should contain int, got: {}", text);
    assert!(
        !text.contains("string"),
        "should not contain string, got: {}",
        text
    );
    assert!(
        !text.contains("null"),
        "should not contain null, got: {}",
        text
    );
}

#[test]
fn hover_nullsafe_method_chain_with_non_nullable_return() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class X {
    public function nonNullable(): X { return $this; }
    public function getName(): string { return ''; }
}
function test(?X $a): void {
    $result = $a?->nonNullable()->getName();
    $result;
}
"#;
    let hover = hover_at(&backend, uri, content, 7, 4).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("string"),
        "should contain string, got: {}",
        text
    );
}

#[test]
fn hover_isset_guard_narrows_nullable_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class X {
    public ?string $a = null;
}
function test(X $x): void {
    if (!isset($x->a)) {
        return;
    }
    $val = $x->a;
    $val;
}
"#;
    let hover = hover_at(&backend, uri, content, 9, 4).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("string"),
        "should contain string, got: {}",
        text
    );
}

#[test]
fn hover_template_narrowed_by_is_string() {
    let backend = create_test_backend();
    let uri = "file:///template_narrowed.php";
    let content = r#"<?php
/**
 * @template K of array-key
 * @param K $key
 */
function test(int|string $key): void {
    if (is_string($key)) {
        $key;
    }
}
"#;
    let hover = hover_at(&backend, uri, content, 7, 8).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("string"),
        "should contain string after is_string narrowing, got: {}",
        text
    );
}

#[test]
fn hover_interface_template_substitution_return_type() {
    let backend = create_test_backend();
    let uri = "file:///interface_template_sub.php";
    let content = r#"<?php
class User {
    public function getName(): string { return ''; }
}
/**
 * @template K
 * @template V
 */
interface Collection {
    /** @return V|null */
    public function get(mixed $key): mixed;
}
/**
 * @implements Collection<string, User>
 */
class UserCollection implements Collection {
    public function get(mixed $key): mixed { return null; }
}
function test(): void {
    $coll = new UserCollection();
    $result = $coll->get('foo');
    $result;
}
"#;
    let hover = hover_at(&backend, uri, content, 21, 4).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("User"),
        "should contain User from interface template substitution, got: {}",
        text
    );
}

#[test]
fn hover_nested_interface_template_substitution() {
    let backend = create_test_backend();
    let uri = "file:///nested_interface_template.php";
    let content = r#"<?php
/**
 * @template T
 */
interface Container {
    /** @return T */
    public function value(): mixed;
}
/**
 * @template U
 * @implements Container<array<U>>
 */
interface ListContainer extends Container {
    /** @return array<U> */
    public function value(): mixed;
}
/**
 * @implements ListContainer<string>
 */
class StringList implements ListContainer {
    public function value(): mixed { return []; }
}
function test(): void {
    $list = new StringList();
    $result = $list->value();
    $result;
}
"#;
    let hover = hover_at(&backend, uri, content, 25, 4).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("array"),
        "should contain array from nested interface template substitution, got: {}",
        text
    );
}

#[test]
fn hover_inheritdoc_inherits_parent_return_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class ParentClass {
    /**
     * @return array<int, string> List of items
     */
    public function getItems(): array { return []; }
}
class ChildClass extends ParentClass {
    /** @inheritDoc */
    public function getItems(): array { return []; }
}
class Svc {
    public function test(ChildClass $child): void {
        $child->getItems();
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 13, 17)
        .expect("should return hover for inherited method call");
    let text = hover_text(&hover);
    assert!(
        text.contains("array"),
        "inherited return type should contain 'array', got: {}",
        text
    );
}

#[test]
fn hover_inheritdoc_inherits_param_descriptions() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    /**
     * Process the input.
     * @param string $name The user's name
     * @return bool True if ok
     */
    public function process(string $name): bool { return true; }
}
class Child extends Base {
    /** @inheritDoc */
    public function process(string $name): bool { return false; }
}
class Svc {
    public function test(Child $c): void {
        $c->process('hi');
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 15, 12)
        .expect("should return hover for inherited method call");
    let text = hover_text(&hover);
    assert!(
        text.contains("Process the input") || text.contains("name"),
        "inherited docs should contain description or param name, got: {}",
        text
    );
}

// ─── Mago-inspired type inference tests ─────────────────────────────────────

#[test]
fn hover_array_key_coalesce_removes_null() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/** @param array{name?: string, age: int} $item */
function test(array $item): void {
    $name = $item['name'] ?? 'Unknown';
    $name;
}
"#;

    // Hover on `$name` at line 4, character 4
    let hover = hover_at(&backend, uri, content, 4, 4).expect("expected hover on $name");
    let text = hover_text(&hover);
    assert!(
        text.contains("string"),
        "null-coalesced optional array key should resolve to string, got: {}",
        text
    );
}

#[test]
fn hover_spread_into_variadic_does_not_lose_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Number {
    public function getValue(): int { return 0; }
}
class Calculator {
    public static function sum(Number $first, Number ...$rest): Number {
        return new Number();
    }
}
function test(): void {
    $a = new Number();
    $b = new Number();
    $result = Calculator::sum($a, ...[$b]);
    $result;
}
"#;

    // Hover on `$result` at line 13, character 4
    let hover = hover_at(&backend, uri, content, 13, 4).expect("expected hover on $result");
    let text = hover_text(&hover);
    assert!(
        text.contains("Number"),
        "spread into variadic should preserve return type Number, got: {}",
        text
    );
}

#[test]
fn hover_list_element_type_from_generic_array() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class User {
    public function getName(): string { return ''; }
}
/** @param list<User> $users */
function test(array $users): void {
    $first = $users[0];
    $first;
}
"#;

    // Hover on `$first` at line 7, character 4
    let hover = hover_at(&backend, uri, content, 7, 4).expect("expected hover on $first");
    let text = hover_text(&hover);
    assert!(
        text.contains("User"),
        "indexing into list<User> should resolve element to User, got: {}",
        text
    );
}

// ─── Chain assignments ($a = $b = expr) ─────────────────────────────────────

#[test]
fn hover_chain_assignment_first_var() {
    let backend = create_test_backend();
    let uri = "file:///chain_assign.php";
    let content = r#"<?php
class Foo {
    public function bar(): string { return ''; }
}
function test(): void {
    $a = $b = new Foo();
    $a;
}
"#;

    let hover = hover_at(&backend, uri, content, 6, 4).expect("expected hover on $a");
    let text = hover_text(&hover);
    assert!(
        text.contains("Foo"),
        "chain assignment: $a should resolve to Foo, got: {}",
        text
    );
}

#[test]
fn hover_chain_assignment_second_var() {
    let backend = create_test_backend();
    let uri = "file:///chain_assign2.php";
    let content = r#"<?php
class Foo {
    public function bar(): string { return ''; }
}
function test(): void {
    $a = $b = new Foo();
    $b;
}
"#;

    let hover = hover_at(&backend, uri, content, 6, 4).expect("expected hover on $b");
    let text = hover_text(&hover);
    assert!(
        text.contains("Foo"),
        "chain assignment: $b should resolve to Foo, got: {}",
        text
    );
}

#[test]
fn hover_chain_assignment_three_vars() {
    let backend = create_test_backend();
    let uri = "file:///chain_assign3.php";
    let content = r#"<?php
class Baz {
    public function hello(): int { return 0; }
}
function test(): void {
    $a = $b = $c = new Baz();
    $a;
    $b;
    $c;
}
"#;

    for (line, var) in [(6, "$a"), (7, "$b"), (8, "$c")] {
        let hover = hover_at(&backend, uri, content, line, 4)
            .unwrap_or_else(|| panic!("expected hover on {var}"));
        let text = hover_text(&hover);
        assert!(
            text.contains("Baz"),
            "triple chain assignment: {var} should resolve to Baz, got: {text}",
        );
    }
}

#[test]
fn hover_chain_assignment_function_call_rhs() {
    let backend = create_test_backend();
    let uri = "file:///chain_assign_fn.php";
    let content = r#"<?php
class Widget {
    public function render(): string { return ''; }
}
function makeWidget(): Widget { return new Widget(); }
function test(): void {
    $a = $b = makeWidget();
    $a;
    $b;
}
"#;

    for (line, var) in [(7, "$a"), (8, "$b")] {
        let hover = hover_at(&backend, uri, content, line, 4)
            .unwrap_or_else(|| panic!("expected hover on {var}"));
        let text = hover_text(&hover);
        assert!(
            text.contains("Widget"),
            "chain assignment with function call: {var} should resolve to Widget, got: {text}",
        );
    }
}

// ─── Inherited class constants via self:: ─────────────────────────────

#[test]
fn hover_inherited_constant_via_self() {
    let backend = create_test_backend();
    let uri = "file:///inherited_const.php";
    let content = r#"<?php
class ParentClass {
    const FOO = 42;
}
class ChildClass extends ParentClass {
    public function test(): void {
        $x = self::FOO;
        $x;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 7, 8).expect("expected hover on $x");
    let text = hover_text(&hover);
    assert!(
        text.contains("int"),
        "inherited constant via self:: should resolve to int, got: {}",
        text
    );
}

#[test]
fn hover_own_constant_via_self() {
    let backend = create_test_backend();
    let uri = "file:///own_const.php";
    let content = r#"<?php
class MyClass {
    const BAR = 'hello';
    public function test(): void {
        $x = self::BAR;
        $x;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 5, 8).expect("expected hover on $x");
    let text = hover_text(&hover);
    assert!(
        text.contains("string"),
        "own constant via self:: should resolve to string, got: {}",
        text
    );
}

#[test]
fn hover_grandparent_constant_via_self() {
    let backend = create_test_backend();
    let uri = "file:///grandparent_const.php";
    let content = r#"<?php
class GrandParent_ {
    const LEVEL = 3;
}
class Parent_ extends GrandParent_ {}
class Child extends Parent_ {
    public function test(): void {
        $x = self::LEVEL;
        $x;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 8, 8).expect("expected hover on $x");
    let text = hover_text(&hover);
    assert!(
        text.contains("int"),
        "grandparent constant via self:: should resolve to int, got: {}",
        text
    );
}

#[test]
fn hover_inherited_constant_via_class_name() {
    let backend = create_test_backend();
    let uri = "file:///inherited_const_classname.php";
    let content = r#"<?php
class Base {
    const STATUS = 'active';
}
class Derived extends Base {}
function test(): void {
    $x = Derived::STATUS;
    $x;
}
"#;

    let hover = hover_at(&backend, uri, content, 7, 4).expect("expected hover on $x");
    let text = hover_text(&hover);
    assert!(
        text.contains("string"),
        "inherited constant via class name should resolve to string, got: {}",
        text
    );
}

// ─── instanceof on nullable strips null ───────────────────────────────

#[test]
fn hover_instanceof_strips_null_from_nullable() {
    let backend = create_test_backend();
    let uri = "file:///instanceof_nullable.php";
    let content = r#"<?php
class Foo {
    public function fooMethod(): string { return ''; }
}
/** @param Foo|null $x */
function test(?Foo $x): void {
    if ($x instanceof Foo) {
        $x;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 7, 8).expect("expected hover on $x");
    let text = hover_text(&hover);
    assert!(
        !text.contains("null") && text.contains("Foo"),
        "instanceof should strip null from ?Foo, got: {}",
        text
    );
}

#[test]
fn hover_instanceof_strips_null_from_union_with_null() {
    let backend = create_test_backend();
    let uri = "file:///instanceof_union_null.php";
    let content = r#"<?php
class Bar {
    public function barMethod(): int { return 0; }
}
class Baz {
    public function bazMethod(): float { return 0.0; }
}
/** @param Bar|Baz|null $x */
function test(Bar|Baz|null $x): void {
    if ($x instanceof Bar) {
        $x;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 10, 8).expect("expected hover on $x");
    let text = hover_text(&hover);
    assert!(
        text.contains("Bar") && !text.contains("null"),
        "instanceof should narrow Bar|Baz|null to Bar, got: {}",
        text
    );
}

#[test]
fn hover_instanceof_non_nullable_unchanged() {
    let backend = create_test_backend();
    let uri = "file:///instanceof_non_nullable.php";
    let content = r#"<?php
class Animal {
    public function speak(): string { return ''; }
}
class Dog extends Animal {
    public function fetch(): bool { return true; }
}
/** @param Animal $x */
function test(Animal $x): void {
    if ($x instanceof Dog) {
        $x;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 10, 8).expect("expected hover on $x");
    let text = hover_text(&hover);
    assert!(
        text.contains("Dog"),
        "instanceof should narrow Animal to Dog, got: {}",
        text
    );
}

// ─── is_object() on multi-class union ─────────────────────────────────

#[test]
fn hover_is_object_narrows_multi_class_union() {
    let backend = create_test_backend();
    let uri = "file:///is_object_union.php";
    let content = r#"<?php
class Foo {
    public function fooMethod(): void {}
}
class Bar {
    public function barMethod(): void {}
}
/**
 * @param Foo|Bar|int $value
 */
function test($value): void {
    if (is_object($value)) {
        $value;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 12, 8).expect("expected hover on $value");
    let text = hover_text(&hover);
    assert!(
        text.contains("Foo") && text.contains("Bar") && !text.contains("int"),
        "is_object should narrow Foo|Bar|int to Foo|Bar, got: {}",
        text
    );
}

#[test]
fn hover_is_object_narrows_multi_class_union_namespaced() {
    let backend = create_test_backend();
    let uri = "file:///is_object_union_ns.php";
    let content = r#"<?php
namespace TestNs;

class Foo {
    public function fooMethod(): void {}
}
class Bar {
    public function barMethod(): void {}
}
/**
 * @param Foo|Bar|int $value
 */
function test($value): void {
    if (is_object($value)) {
        $value;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 14, 8).expect("expected hover on $value");
    let text = hover_text(&hover);
    assert!(
        text.contains("Foo") && text.contains("Bar") && !text.contains("int"),
        "namespaced is_object should narrow Foo|Bar|int to Foo|Bar, got: {}",
        text
    );
}

#[test]
fn hover_is_int_narrows_class_union_to_scalar() {
    let backend = create_test_backend();
    let uri = "file:///is_int_union.php";
    let content = r#"<?php
class Foo {
    public function fooMethod(): void {}
}
class Bar {
    public function barMethod(): void {}
}
/**
 * @param Foo|Bar|int $value
 */
function test($value): void {
    if (is_int($value)) {
        $value;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 12, 8).expect("expected hover on $value");
    let text = hover_text(&hover);
    assert!(
        text.contains("int") && !text.contains("Foo") && !text.contains("Bar"),
        "is_int should narrow Foo|Bar|int to int, got: {}",
        text
    );
}

/// Inline `@var` with a use-imported short name must resolve to FQN in hover.
#[test]
fn hover_inline_var_docblock_resolves_short_name_to_fqn() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": { "App\\": "src/" }
            }
        }"#,
        &[
            (
                "src/Models/Order.php",
                r#"<?php
namespace App\Models;
class Order {
    public string $id;
}
"#,
            ),
            (
                "src/Service.php",
                r#"<?php
namespace App;
use App\Models\Order;
class Service {
    public function run(): void {
        /** @var Order $order */
        $order = $this->fetchOrder();
        $order->id;
    }
    private function fetchOrder(): mixed { return null; }
}
"#,
            ),
        ],
    );

    let order_uri = format!(
        "file://{}",
        _dir.path().join("src/Models/Order.php").display()
    );
    let order_content = std::fs::read_to_string(_dir.path().join("src/Models/Order.php")).unwrap();
    backend.update_ast(&order_uri, &order_content);

    let service_uri = format!("file://{}", _dir.path().join("src/Service.php").display());
    let service_content = std::fs::read_to_string(_dir.path().join("src/Service.php")).unwrap();

    // Hover on `$order` usage at line 7 (`$order->id`, 0-indexed)
    let hover =
        hover_at(&backend, &service_uri, &service_content, 7, 9).expect("expected hover on $order");
    let text = hover_text(&hover);
    // The type must be resolved: the namespace line shows where the class
    // lives and the short name is used for display.  Before the fix, the
    // namespace line was missing because the type was never resolved to FQN.
    assert!(
        text.contains("App\\Models") && text.contains("Order"),
        "inline @var should resolve short name to FQN (namespace + short name), got: {}",
        text
    );
}

/// `new ClassName()` with a use-imported short name must show FQN in hover.
#[test]
fn hover_instantiation_resolves_short_name_to_fqn() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": { "App\\": "src/" }
            }
        }"#,
        &[
            (
                "src/Models/Invoice.php",
                r#"<?php
namespace App\Models;
class Invoice {
    public string $number;
}
"#,
            ),
            (
                "src/Handler.php",
                r#"<?php
namespace App;
use App\Models\Invoice;
class Handler {
    public function handle(): void {
        $inv = new Invoice();
        $inv->number;
    }
}
"#,
            ),
        ],
    );

    let invoice_uri = format!(
        "file://{}",
        _dir.path().join("src/Models/Invoice.php").display()
    );
    let invoice_content =
        std::fs::read_to_string(_dir.path().join("src/Models/Invoice.php")).unwrap();
    backend.update_ast(&invoice_uri, &invoice_content);

    let handler_uri = format!("file://{}", _dir.path().join("src/Handler.php").display());
    let handler_content = std::fs::read_to_string(_dir.path().join("src/Handler.php")).unwrap();

    // Hover on `$inv` usage at line 6 (`$inv->number`, 0-indexed)
    let hover =
        hover_at(&backend, &handler_uri, &handler_content, 6, 9).expect("expected hover on $inv");
    let text = hover_text(&hover);
    assert!(
        text.contains("App\\Models") && text.contains("Invoice"),
        "new ClassName() should resolve short name to FQN (namespace + short name), got: {}",
        text
    );
}

/// Self-referencing array key assignment (`$arr['k'] = f($arr['k'])`) must
/// not hang or cause infinite re-entry in the forward walker.
#[test]
fn hover_array_key_self_referencing_assignment_does_not_hang() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function transform(string $v): string { return strtoupper($v); }

function process(): void {
    $data = ['name' => 'alice', 'count' => 0];
    $data['name'] = transform($data['name']);
    $data['count'] = count($data);
    echo $data['name'];
}
"#;

    // The test passes if it completes without hanging.  Hover on
    // `$data` at the echo statement (line 7, 0-indexed) to exercise
    // the full forward-walk pipeline including array shape merging.
    let hover = hover_at(&backend, uri, content, 7, 10).expect("expected hover on $data");
    let text = hover_text(&hover);
    assert!(
        text.contains("data"),
        "should produce hover for $data: {}",
        text
    );
}

// ─── Virtual method: colon return type syntax ───────────────────────────────

#[test]
fn hover_virtual_method_colon_return_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @method getBool(string $foo) : bool some description
 */
class Child {
    public function __call(string $name, array $args) {}
}
class Demo {
    public function test(): void {
        $child = new Child();
        $result = $child->getBool("hello");
        $result;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 11, 9).expect("expected hover on $result");
    let text = hover_text(&hover);
    assert!(
        text.contains("bool"),
        "colon return type @method should resolve to bool, got: {}",
        text
    );
}

// ─── Virtual method: grouped union array return type ────────────────────────

#[test]
fn hover_virtual_method_grouped_union_array_return() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @method (string|int)[] getArray() description
 */
class Child {
    public function __call(string $name, array $args) {}
}
class Demo {
    public function test(): void {
        $child = new Child();
        $result = $child->getArray();
        $result;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 11, 9).expect("expected hover on $result");
    let text = hover_text(&hover);
    assert!(
        text.contains("string") || text.contains("int") || text.contains("array"),
        "grouped union array should resolve, got: {}",
        text
    );
}

// ─── Virtual method: callable return type ───────────────────────────────────

#[test]
fn hover_virtual_method_callable_return() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @method (callable(): string) getCallable() desc
 */
class Child {
    public function __call(string $name, array $args) {}
}
class Demo {
    public function test(): void {
        $child = new Child();
        $result = $child->getCallable();
        $result;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 11, 9).expect("expected hover on $result");
    let text = hover_text(&hover);
    assert!(
        text.contains("callable"),
        "callable return type should resolve, got: {}",
        text
    );
}

// ─── Virtual method: `static` return type on instance ───────────────────────

#[test]
fn hover_virtual_method_returning_static_on_instance() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/** @method static getStatic() */
class C {
    public function __call(string $name, array $args) {}
}
class Demo {
    public function test(): void {
        $c = (new C)->getStatic();
        $c;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 8, 9).expect("expected hover on $c");
    let text = hover_text(&hover);
    assert!(
        text.contains("C"),
        "virtual @method returning static should resolve to class C, got: {}",
        text
    );
}

#[test]
fn hover_virtual_method_returning_static_on_subclass() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/** @method static getStatic() */
class C {
    public function __call(string $name, array $args) {}
}
class D extends C {}
class Demo {
    public function test(): void {
        $d = (new D)->getStatic();
        $d;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 9, 9).expect("expected hover on $d");
    let text = hover_text(&hover);
    assert!(
        text.contains("D"),
        "virtual @method returning static on subclass should resolve to D, got: {}",
        text
    );
}

// ─── Virtual method: duplicate calls to same method ─────────────────────────

#[test]
fn hover_virtual_method_duplicate_calls_both_resolve() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @method setBool(string $foo, string|bool $bar): bool
 */
class Child {
    public function __call(string $name, array $args) {}
}
class Demo {
    public function test(): void {
        $child = new Child();
        $b = $child->setBool("hello", true);
        $c = $child->setBool("hello", "true");
        $b;
        $c;
    }
}
"#;

    let hover_b = hover_at(&backend, uri, content, 12, 9).expect("expected hover on $b");
    let text_b = hover_text(&hover_b);
    assert!(
        text_b.contains("bool"),
        "first call to virtual method should resolve to bool, got: {}",
        text_b
    );

    let hover_c = hover_at(&backend, uri, content, 13, 9).expect("expected hover on $c");
    let text_c = hover_text(&hover_c);
    assert!(
        text_c.contains("bool"),
        "second call to same virtual method should also resolve to bool, got: {}",
        text_c
    );
}

// ─── Virtual method: static call returning static ───────────────────────────

#[test]
fn hover_static_virtual_method_call_returning_static() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @method static static getInstance()
 */
class Child {
    public static function __callStatic(string $name, array $args) {}
}
class Demo {
    public function test(): void {
        $f = Child::getInstance();
        $f;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 10, 9).expect("expected hover on $f");
    let text = hover_text(&hover);
    assert!(
        text.contains("Child"),
        "static virtual @method returning static should resolve to Child, got: {}",
        text
    );
}

// ─── Virtual method: generic substitution through @extends ──────────────────

#[test]
fn hover_virtual_method_generic_substitution_through_extends() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @template T
 * @method T get()
 */
class ParentBox {
    public function __call(string $name, array $args) {}
}

/**
 * @extends ParentBox<string>
 */
class StringBox extends ParentBox {}

class Demo {
    public function test(): void {
        $box = new StringBox();
        $result = $box->get();
        $result;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 18, 9).expect("expected hover on $result");
    let text = hover_text(&hover);
    assert!(
        text.contains("string"),
        "@method generic T should be substituted to string through @extends, got: {}",
        text
    );
}

// ─── Virtual method: generic substitution through @implements ────────────────

#[test]
fn hover_virtual_method_generic_substitution_through_implements() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @template T
 * @method T get()
 */
interface Container {}

/**
 * @implements Container<string>
 */
class StringContainer implements Container {
    public function __call(string $name, array $args) {}
}

class Demo {
    public function test(): void {
        $c = new StringContainer();
        $result = $c->get();
        $result;
    }
}
"#;

    let hover = hover_at(&backend, uri, content, 18, 9).expect("expected hover on $result");
    let text = hover_text(&hover);
    assert!(
        text.contains("string"),
        "@method generic T should be substituted to string through @implements, got: {}",
        text
    );
}

#[test]
fn hover_property_through_or_instanceof() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class PropA {
    /** @var int */
    public $foo = 0;
}
class PropB {
    /** @var string */
    public $foo = "";
}

/** @var PropA|PropB|null $a */
$a = null;
$b = null;

if ($a instanceof PropA || $a instanceof PropB) {
    $b = $a->foo;
}
"#;

    // Hover on `foo` in `$a->foo` (line 15, char 14)
    let hover = hover_at(&backend, uri, content, 15, 14).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("int") && text.contains("string"),
        "should contain both int and string from union property: {}",
        text
    );
}

#[test]
fn hover_static_mixin_method_on_instance() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class MixProvider {
    public static function getInt(): int {
        return 5;
    }
}

/** @mixin MixProvider */
class MixChild {
    public function __call(string $name, array $args) {}
    public static function __callStatic(string $name, array $args) {}
}

$child = new MixChild();
$b = $child::getInt();
"#;

    // Hover on `$b` (line 14, char 0)
    let hover = hover_at(&backend, uri, content, 14, 1).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("int"),
        "should resolve static mixin method return type: {}",
        text
    );
}

#[test]
fn hover_mixin_this_return_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @method $this active()
 */
class MixinBase {
    public function __call(string $name, array $arguments) {}
}

/**
 * @mixin MixinBase
 */
class MixConsumer {
    public function __call(string $name, array $arguments) {}
}

$b = new MixConsumer;
$c = $b->active();
"#;

    // Hover on `$c` (line 16, char 1)
    let hover = hover_at(&backend, uri, content, 16, 1).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("MixConsumer"),
        "$this on mixin method should resolve to the consumer class: {}",
        text
    );
}

#[test]
fn hover_iterator_iterator_mixin_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Subject implements Iterator {
    public function index(int $idx): bool {
        return true;
    }
    public function current(): int { return 2; }
    public function next(): void {}
    public function key(): int { return 1; }
    public function valid(): bool { return false; }
    public function rewind(): void {}
}

/**
 * @template TKey
 * @template TValue
 * @template TIterator of Traversable
 * @mixin TIterator
 */
class IteratorIterator {
    /** @param TIterator $iterator */
    public function __construct(Traversable $iterator) {}
}

$iter = new IteratorIterator(new Subject());
$b = $iter->index(0);
"#;

    // Hover on `$b` (line 24, char 1)
    let hover = hover_at(&backend, uri, content, 24, 1).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("bool"),
        "should resolve mixin method return type from wrapped iterator: {}",
        text
    );
}

#[test]
fn hover_method_return_type_on_child_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";

    // Test 1: array<int, static> substitution
    let content = r#"<?php
abstract class ParentClass {
    /** @return array<int, static> */
    public static function loadMultiple() {
        return [new static()];
    }
}
class ChildClass extends ParentClass {}
$items = ChildClass::loadMultiple();
$test = $items;
"#;
    let target_line = content
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("$test = $items"))
        .map(|(i, _)| i as u32)
        .unwrap();
    let hover = hover_at(&backend, uri, content, target_line, 1).expect("hover 1");
    let text = hover_text(&hover);
    assert!(text.contains("ChildClass"), "Test 1 failed: {}", text);

    // Test 2: overridden return type
    let content2 = r#"<?php
class A {
    /** @return string|null */
    public function blah() { return null; }
}
class B extends A {
    /** @return string */
    public function blah() { return "blah"; }
}
$blah = (new B())->blah();
$test2 = $blah;
"#;
    let target_line = content2
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("$test2 = $blah"))
        .map(|(i, _)| i as u32)
        .unwrap();
    let hover = hover_at(&backend, uri, content2, target_line, 1).expect("hover 2");
    let text = hover_text(&hover);
    assert!(text.contains("string"), "Test 2 failed: {}", text);

    // Test 3: interface method return type
    let content3 = r#"<?php
interface Iface {
    /** @return string|null */
    public function blah();
}
class Impl implements Iface {
    /** @return string|null */
    public function blah() { return null; }
}
$blah = (new Impl())->blah();
$test3 = $blah;
"#;
    let target_line = content3
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("$test3 = $blah"))
        .map(|(i, _)| i as u32)
        .unwrap();
    let hover = hover_at(&backend, uri, content3, target_line, 1).expect("hover 3");
    let text = hover_text(&hover);
    assert!(
        text.contains("string") || text.contains("null"),
        "Test 3 failed: {}",
        text
    );
}

#[test]
fn hover_while_loop_exit_narrows_to_null() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Node {
    /** @var ?Node */
    public $parent;
}
function makeNode(): Node { return new Node(); }

$a = makeNode();
while ($a) {
    $a = $a->parent;
}
$result = $a;
"#;
    // Hover on `$result` which should be `null` after the while loop.
    let target_line = content
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("$result = $a"))
        .map(|(i, _)| i as u32)
        .unwrap();
    let hover =
        hover_at(&backend, uri, content, target_line, 1).expect("expected hover on $result");
    let text = hover_text(&hover);
    assert!(
        text.contains("null") && !text.contains("Node"),
        "After while($a) loop, $a should be null, got: {}",
        text
    );
}

// ─── __get magic method template resolution ─────────────────────────────────

#[test]
fn hover_magic_get_key_of_index_access() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @template TData as array
 */
abstract class DataBag {
    /** @var TData */
    protected $data;

    /** @param TData $data */
    public function __construct(array $data) {
        $this->data = $data;
    }

    /**
     * @template K as key-of<TData>
     * @param K $property
     * @return TData[K]
     */
    public function __get(string $property) {
        return $this->data[$property];
    }
}

/** @extends DataBag<array{a: int, b: string}> */
class FooBag extends DataBag {}

function test(): void {
    $foo = new FooBag(['a' => 5, 'b' => 'hello']);
    $a = $foo->a;
    $b = $foo->b;
}
"#;
    // $a should be int (line with `$a = $foo->a;`)
    let hover = hover_at(&backend, uri, content, 28, 5).expect("expected hover on $a");
    let text = hover_text(&hover);
    assert!(
        text.contains("int"),
        "Expected $a to be int via __get template resolution, got: {}",
        text
    );

    // $b should be string (line with `$b = $foo->b;`)
    let hover = hover_at(&backend, uri, content, 29, 5).expect("expected hover on $b");
    let text = hover_text(&hover);
    assert!(
        text.contains("string"),
        "Expected $b to be string via __get template resolution, got: {}",
        text
    );
}

#[test]
fn hover_while_loop_exit_narrows_to_null_multi_namespace() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace NS1 {
    class A {
        /** @var ?A */
        public $parent;
    }
}
namespace NS2 {
    class B {}
    class A {
        /** @var A|B */
        public $parent;
    }
}
namespace NS3 {
    class B {}
    class A {
        /** @var A|B */
        public $parent;
    }
}
namespace NS4 {
    class A {
        /** @var ?A */
        public $parent;

        public function __construct() {
            $this->parent = rand(0, 1) ? new A() : null;
        }
    }

    function makeA(): A {
        return new A();
    }

    $a = makeA();

    while ($a) {
        $a = $a->parent;
    }
    $result = $a;
}
namespace NS5 {
    class A {
        /** @var ?A */
        public $parent;
    }
}
"#;
    let target_line = content
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("$result = $a"))
        .map(|(i, _)| i as u32)
        .unwrap();
    let hover =
        hover_at(&backend, uri, content, target_line, 5).expect("expected hover on $result");
    let text = hover_text(&hover);
    assert!(
        text.contains("null") && !text.contains("A"),
        "After while($a) loop in multi-ns file, $a should be null, got: {}",
        text
    );
}

#[test]
fn hover_while_loop_exit_multi_ns_with_function() {
    // Reproduce the exact psalm test structure with assertion-runner transform
    let backend = create_test_backend();
    let uri = "file:///test.php";
    // This mimics the assertion runner's transformed source for test 4.
    // Preceding namespaces have class A with different property types.
    let content = r#"<?php
namespace PsalmTest_loop_while_1 {
    $worked = false;
    while (rand(0,100) === 10) {
        $worked = true;
    }
    $__phpantom_assert_0 = $worked;
}
namespace PsalmTest_loop_while_2 {
    class B {}
    class A {
        /** @var A|B */
        public $parent;
        public function __construct() {
            $this->parent = rand(0, 1) ? new A() : new B();
        }
    }
    function makeA(): A {
        return new A();
    }
    $a = makeA();
    while ($a instanceof A) {
        $a = $a->parent;
    }
    $__phpantom_assert_1 = $a;
}
namespace PsalmTest_loop_while_4 {
    class A {
        /** @var ?A */
        public $parent;
        public function __construct() {
            $this->parent = rand(0, 1) ? new A() : null;
        }
    }
    function makeA(): A {
        return new A();
    }
    $a = makeA();
    while ($a) {
        $a = $a->parent;
    }
    $__phpantom_assert_3 = $a;
}
"#;
    let target_line = content
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("$__phpantom_assert_3 = $a"))
        .map(|(i, _)| i as u32)
        .unwrap();
    let hover =
        hover_at(&backend, uri, content, target_line, 5).expect("expected hover on assert var");
    let text = hover_text(&hover);
    assert!(
        text.contains("null") && !text.contains("A"),
        "After while($a) in psalm-like multi-ns, $a should be null, got: {}",
        text
    );
}

#[test]
fn hover_multi_namespace_function_return_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace NS1 {
    class Foo {
        public string $x;
    }
}
namespace NS2 {
    class Foo {
        public int $x;
    }
    function makeFoo(): Foo { return new Foo(); }
    $f = makeFoo();
    $result = $f->x;
}
"#;
    let target_line = content
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("$result = $f->x"))
        .map(|(i, _)| i as u32)
        .unwrap();
    let hover =
        hover_at(&backend, uri, content, target_line, 5).expect("expected hover on $result");
    let text = hover_text(&hover);
    assert!(
        text.contains("int"),
        "makeFoo()->x in NS2 should be int, got: {}",
        text
    );
}

#[test]
fn hover_multi_namespace_property_resolution() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace NS1 {
    class Foo {
        public string $x;
    }
}
namespace NS2 {
    class Foo {
        public int $x;
    }
    $f = new Foo();
    $result = $f->x;
}
"#;
    let target_line = content
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("$result = $f->x"))
        .map(|(i, _)| i as u32)
        .unwrap();
    let hover =
        hover_at(&backend, uri, content, target_line, 5).expect("expected hover on $result");
    let text = hover_text(&hover);
    assert!(
        text.contains("int"),
        "$f->x in NS2 should be int (from NS2\\Foo), got: {}",
        text
    );
}

#[test]
fn hover_while_loop_instanceof_exit_narrows_away_matched_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Animal {}
class Dog extends Animal {}
class Cat extends Animal {}

/** @var Dog|Cat $a */
$a = rand(0,1) ? new Dog() : new Cat();

while ($a instanceof Dog) {
    break;
}
$result = $a;
"#;
    let target_line = content
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("$result = $a"))
        .map(|(i, _)| i as u32)
        .unwrap();
    let hover =
        hover_at(&backend, uri, content, target_line, 5).expect("expected hover on $result");
    let text = hover_text(&hover);
    assert!(
        text.contains("Cat") && !text.contains("Dog"),
        "After while($a instanceof Dog), $a should be Cat, got: {}",
        text
    );
}

#[test]
fn hover_while_loop_exit_property_access_on_narrowed_var() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Node {
    /** @var ?Node */
    public $parent;
    public string $name;
}
function makeNode(): Node { return new Node(); }

$a = makeNode();
while ($a) {
    $a = $a->parent;
}
// $a is null here, so property access should not resolve
$result = $a;
"#;
    let target_line = content
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("$result = $a"))
        .map(|(i, _)| i as u32)
        .unwrap();
    let hover =
        hover_at(&backend, uri, content, target_line, 5).expect("expected hover on $result");
    let text = hover_text(&hover);
    assert!(
        text.contains("null"),
        "After while($a) loop, $a should be null, got: {}",
        text
    );
}

#[test]
fn hover_multi_namespace_template_foo_collision() {
    // Regression: when `@var Foo<A>` is followed by extra tags
    // (e.g. `@psalm-suppress`) in the docblock, the type parser
    // would include the extra lines in the type string, breaking
    // resolution. This test verifies the fix works with multiple
    // namespace blocks and Foo collisions across them.
    let backend = create_test_backend_with_full_stubs();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace NS6 {
    class A {}

    /**
     * @template T as object
     */
    class Foo {
        /** @var class-string<T> */
        public $T;
        /**
         * @param class-string<T> $T
         */
        public function __construct(string $T) {
            $this->T = $T;
        }
        /**
         * @return T
         */
        public function bar() {
            $t = $this->T;
            return new $t();
        }
    }

    /**
     * @var Foo<A>
     * @psalm-suppress ArgumentTypeCoercion
     */
    $afoo = new Foo('A');
    $afoo_bar = $afoo->bar();
}
namespace NS7 {
    /**
     * @template T as object
     */
    class Foo {
        /** @var T */
        public $item;
    }
}
"#;
    let target_line = content
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("$afoo_bar = $afoo->bar()"))
        .map(|(i, _)| i as u32)
        .unwrap();
    let line_text = content.lines().nth(target_line as usize).unwrap();
    let col = line_text.find("$afoo_bar").unwrap() as u32;
    let hover = hover_at(&backend, uri, content, target_line, col + 1)
        .expect("expected hover on $afoo_bar");
    let text = hover_text(&hover);
    assert!(
        text.contains("A"),
        "$afoo_bar should resolve to A via Foo<A>::bar(), got: {}",
        text
    );
}

#[test]
fn hover_top_level_variable_in_namespace_block() {
    // Variable assigned at namespace top level (not inside any class or
    // function) should resolve via the forward walker's top-level path.
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace TopLevelNs {
    class Foo {
        public function bar(): void {}
    }
    $x = new Foo();
    $y = $x;
}
"#;

    backend.update_ast(uri, content);

    // Hover on $y — should resolve to Foo via the top-level forward walk.
    let target_line = content
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("$y = $x"))
        .map(|(i, _)| i as u32)
        .unwrap();
    let line_text = content.lines().nth(target_line as usize).unwrap();
    let col = line_text.find("$y").unwrap() as u32;
    let hover =
        hover_at(&backend, uri, content, target_line, col + 1).expect("expected hover on $y");
    let text = hover_text(&hover);
    assert!(
        text.contains("Foo"),
        "$y should resolve to Foo at namespace top level, got: {}",
        text
    );
}

#[test]
fn hover_top_level_variable_multi_ns_same_class_name() {
    // When two namespace blocks define a class with the same short name,
    // variables in each block should resolve to their own namespace's class.
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace Ns1 {
    class ParentClass {
        public function __callStatic(string $name, array $args) {}
    }
    /**
     * @method static static getInstance()
     */
    class Child extends ParentClass {}
    $a = Child::getInstance();
}
namespace Ns2 {
    class ParentClass {
        public function __callStatic(string $name, array $args) {}
    }
    /**
     * @method static static getInstance()
     */
    class Child extends ParentClass {}
    $f = Child::getInstance();
    $g = $f;
}
"#;

    backend.update_ast(uri, content);

    // Hover on $g in Ns2 — should resolve to Ns2\Child, not Ns1\Child.
    let target_line = content
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("$g = $f"))
        .map(|(i, _)| i as u32)
        .unwrap();
    let line_text = content.lines().nth(target_line as usize).unwrap();
    let col = line_text.find("$g").unwrap() as u32;
    let hover =
        hover_at(&backend, uri, content, target_line, col + 1).expect("expected hover on $g");
    let text = hover_text(&hover);
    assert!(
        text.contains("Child"),
        "$g should resolve to Child in multi-namespace file, got: {}",
        text
    );
}

#[test]
fn hover_top_level_variable_multi_ns_same_class_assert_runner_pattern() {
    // Mimics the assert runner transform: assertType('Child', $f) becomes
    // $__phpantom_assert_10 = $f; and we hover on $__phpantom_assert_10.
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace Ns1 {
    class ParentClass {
        public function __callStatic(string $name, array $args) {}
    }
    /**
     * @method static string getString()
     */
    class Child extends ParentClass {}
    $a = Child::getString();
    $__phpantom_assert_0 = $a;
}
namespace Ns2 {
    class ParentClass {
        public function __callStatic(string $name, array $args) {}
    }
    /**
     * @method static static getInstance()
     */
    class Child extends ParentClass {}
    $f = Child::getInstance();
    $__phpantom_assert_10 = $f;
}
"#;

    backend.update_ast(uri, content);

    // Hover on $__phpantom_assert_10 in PsalmTest_2.
    let target_line = content
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("$__phpantom_assert_10 = $f"))
        .map(|(i, _)| i as u32)
        .unwrap();
    let line_text = content.lines().nth(target_line as usize).unwrap();
    let col = line_text.find("$__phpantom_assert_10").unwrap() as u32;
    let hover = hover_at(&backend, uri, content, target_line, col + 1)
        .expect("expected hover on $__phpantom_assert_10");
    let text = hover_text(&hover);
    assert!(
        text.contains("Child"),
        "$__phpantom_assert_10 should resolve to Child via $f, got: {}",
        text
    );
}

#[test]
fn hover_variable_reassigned_inside_method_shows_new_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php

final class OrderController
{
    private function getOrder()
    {
        $ship = null;
        if (true) {
            $ship = new OrderController();
        }

        return $ship;
    }
}
"#;

    // Hover on $ship at line 8 col 12 (the LHS of the reassignment inside if)
    // `$` is at column 12 (12 spaces indentation).
    let hover = hover_at(&backend, uri, content, 8, 12).expect("expected hover on $ship LHS");
    let text = hover_text(&hover);
    assert!(
        text.contains("OrderController") && !text.contains("null"),
        "$ship on LHS of reassignment should be OrderController, got: {}",
        text
    );
}

#[test]
fn hover_variable_reassigned_inside_if_shows_new_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class ShipmentData {}
function test(?object $shipment): ?ShipmentData {
    $ship = null;
    if ($shipment) {
        $ship = new ShipmentData();
    }
    return $ship;
}
"#;

    // Hover on $ship at line 5 col 8 (the LHS of the reassignment inside if)
    // Should show `ShipmentData`, not `null`.
    let hover = hover_at(&backend, uri, content, 5, 9).expect("expected hover on $ship LHS");
    let text = hover_text(&hover);
    assert!(
        text.contains("ShipmentData") && !text.contains("null"),
        "$ship on LHS of reassignment inside if should be ShipmentData, got: {}",
        text
    );
}

#[test]
fn hover_variable_reassigned_shows_new_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    // Mirrors the real-world pattern: method call returns a union,
    // assert() narrows, then reassignment to string.
    let content = r#"<?php
class CarbonLike {
    public function locale(string $l): self { return $this; }
    public function isoFormat(string $format): string { return ''; }
}
function test(): void {
    $date = new CarbonLike();

    assert($date instanceof CarbonLike);

    $date = $date->isoFormat('D. MMMM YYYY');
    echo $date;
}
"#;

    // Hover on $date at line 10 (the LHS of the reassignment)
    // Should show `string`, not `CarbonLike`.
    let hover = hover_at(&backend, uri, content, 10, 5).expect("expected hover on $date LHS");
    let text = hover_text(&hover);
    assert!(
        text.contains("string") && !text.contains("CarbonLike"),
        "$date on LHS of reassignment should be string, got: {}",
        text
    );

    // Hover on $date at line 11 (the echo usage) should also be string.
    let hover = hover_at(&backend, uri, content, 11, 9).expect("expected hover on $date usage");
    let text = hover_text(&hover);
    assert!(
        text.contains("string") && !text.contains("CarbonLike"),
        "$date after reassignment should be string, got: {}",
        text
    );
}

/// `PDOStatement::fetch()` carries a PHPStan conditional return type keyed
/// on the fetch-mode class constant. Passing the mode directly selects the
/// matching branch (object for `FETCH_OBJ`, array for `FETCH_ASSOC`, and
/// `mixed` for modes with no dedicated branch or when no mode is passed).
#[test]
fn hover_pdo_fetch_mode_dependent_return_type() {
    let backend = create_test_backend_with_full_stubs();
    let uri = "file:///pdo_fetch.php";
    let content = r#"<?php
function probe(\PDOStatement $stmt): void {
    $obj = $stmt->fetch(\PDO::FETCH_OBJ);
    $obj;
    $assoc = $stmt->fetch(\PDO::FETCH_ASSOC);
    $assoc;
    $col = $stmt->fetch(\PDO::FETCH_COLUMN);
    $col;
    $default = $stmt->fetch();
    $default;
}
"#;

    // FETCH_OBJ selects the `\stdClass|false` branch.
    let obj = hover_text(&hover_at(&backend, uri, content, 3, 6).expect("hover $obj")).to_string();
    assert!(
        obj.contains("stdClass"),
        "FETCH_OBJ should be stdClass: {obj}"
    );

    // FETCH_ASSOC selects the associative-array branch.
    let assoc =
        hover_text(&hover_at(&backend, uri, content, 5, 6).expect("hover $assoc")).to_string();
    assert!(
        assoc.contains("array"),
        "FETCH_ASSOC should be an array: {assoc}"
    );
    assert!(
        !assoc.contains("stdClass"),
        "FETCH_ASSOC should not select the object branch: {assoc}"
    );

    // FETCH_COLUMN has no dedicated branch → falls through to `mixed`.
    let col = hover_text(&hover_at(&backend, uri, content, 7, 6).expect("hover $col")).to_string();
    assert!(
        !col.contains("stdClass") && !col.contains("array<"),
        "FETCH_COLUMN should not resolve to a specific branch: {col}"
    );

    // No mode argument → the conditional cannot be resolved statically.
    let default =
        hover_text(&hover_at(&backend, uri, content, 9, 6).expect("hover $default")).to_string();
    assert!(
        !default.contains("stdClass") && !default.contains("array<"),
        "fetch() without a mode should not resolve to a branch: {default}"
    );
}

/// `PDOStatement::fetchAll()` resolves to a `list<...>` whose element type
/// depends on the fetch mode. The element type flows through to `foreach`.
#[test]
fn hover_pdo_fetch_all_mode_dependent_element_type() {
    let backend = create_test_backend_with_full_stubs();
    let uri = "file:///pdo_fetch_all.php";
    let content = r#"<?php
function probe(\PDOStatement $stmt): void {
    foreach ($stmt->fetchAll(\PDO::FETCH_OBJ) as $item) {
        $item;
    }
}
"#;

    let item =
        hover_text(&hover_at(&backend, uri, content, 3, 9).expect("hover $item")).to_string();
    assert!(
        item.contains("stdClass"),
        "fetchAll(FETCH_OBJ) elements should be stdClass: {item}"
    );
}

/// `+=` on arrays is an array union in PHP, not numeric addition.
/// The inferred type must be `array`, not `int|float`.
#[test]
fn hover_array_plus_assign_infers_array() {
    let backend = create_test_backend();
    let uri = "file:///array_plus_assign.php";
    let content = r#"<?php
function test(): void {
    $array = ['a' => 1];
    $array += ['foo' => 'bar'];
    $array;
}
"#;

    let result =
        hover_text(&hover_at(&backend, uri, content, 4, 6).expect("hover $array")).to_string();
    assert!(
        result.contains("array"),
        "$array after += with array literal should be array, got: {result}"
    );
    assert!(
        !result.contains("int|float"),
        "$array after += should not be int|float: {result}"
    );
}

/// Numeric `+=` should still infer `int|float` (regression guard).
#[test]
fn hover_numeric_plus_assign_still_infers_numeric() {
    let backend = create_test_backend();
    let uri = "file:///numeric_plus_assign.php";
    let content = r#"<?php
function test(): void {
    $n = 1;
    $n += 2;
    $n;
}
"#;

    let result = hover_text(&hover_at(&backend, uri, content, 4, 6).expect("hover $n")).to_string();
    assert!(
        result.contains("int"),
        "$n after numeric += should contain int, got: {result}"
    );
    assert!(
        !result.contains("array"),
        "$n after numeric += should not be array: {result}"
    );
}

/// `ReflectionFunctionAbstract::getAttributes()` has a docblock return type
/// of `ReflectionAttribute<T>[]`.  The `[]` suffix after a generic type must
/// be preserved so the result is inferred as an array, not a single
/// `ReflectionAttribute`.
#[test]
fn hover_reflection_get_attributes_returns_array() {
    let backend = create_test_backend();
    let uri = "file:///reflection_attrs.php";

    // Provide a minimal stub inline so the test is self-contained.
    let stub_uri = "file:///reflection_stub.php";
    let stub = r#"<?php
/**
 * @template T
 */
class ReflectionAttribute {}
class ReflectionFunctionAbstract {
    /**
     * @template T
     * @param class-string<T>|null $name
     * @return ReflectionAttribute<T>[]
     */
    public function getAttributes(?string $name = null, int $flags = 0): array {}
}
class ReflectionMethod extends ReflectionFunctionAbstract {}
"#;
    backend.update_ast(stub_uri, stub);

    let content = r#"<?php
function test(ReflectionMethod $ref): void {
    $attrs = $ref->getAttributes();
    $attrs;
}
"#;

    let result =
        hover_text(&hover_at(&backend, uri, content, 3, 6).expect("hover $attrs")).to_string();
    assert!(
        result.contains("array") || result.contains("[]"),
        "$attrs from getAttributes() should be an array type, got: {result}"
    );
    assert!(
        !result.contains("ReflectionAttribute<")
            || result.contains("[]")
            || result.contains("array"),
        "$attrs should not be a bare ReflectionAttribute without array wrapper: {result}"
    );
}

/// Foreach key type should be extracted from a class's
/// `implements_generics` when the iterable is a bare class name
/// (e.g. Finder implementing `IteratorAggregate<non-empty-string, SplFileInfo>`).
#[test]
fn hover_foreach_key_from_iterator_aggregate_generics() {
    let backend = create_test_backend();
    let uri = "file:///foreach_key_iface.php";

    let stub_uri = "file:///finder_stub.php";
    let stub = r#"<?php
class SplFileInfo {}

/** @implements \IteratorAggregate<non-empty-string, SplFileInfo> */
class Finder implements \IteratorAggregate, \Countable {
    public function getIterator(): \Iterator {}
    public function count(): int {}
}
"#;
    backend.update_ast(stub_uri, stub);

    let content = r#"<?php
function test(Finder $files): void {
    foreach ($files as $filePath => $file) {
        $filePath;
    }
}
"#;

    let result =
        hover_text(&hover_at(&backend, uri, content, 3, 10).expect("hover $filePath")).to_string();
    assert!(
        result.contains("non-empty-string") || result.contains("string"),
        "$filePath should be non-empty-string (or string), not int|string, got: {result}"
    );
    assert!(
        !result.contains("int|string") && !result.contains("int | string"),
        "$filePath should not fall back to int|string: {result}"
    );
}
