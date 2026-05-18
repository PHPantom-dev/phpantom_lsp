use crate::common::{create_psr4_workspace, create_test_backend};
use tower_lsp::lsp_types::*;

/// Helper: open a file in the backend and return its code lenses.
fn get_code_lenses(backend: &phpantom_lsp::Backend, uri: &str, content: &str) -> Vec<CodeLens> {
    backend.update_ast(uri, content);
    backend.handle_code_lens(uri, content).unwrap_or_default()
}

/// Helper: extract just the titles from a list of code lenses.
fn lens_titles(lenses: &[CodeLens]) -> Vec<&str> {
    lenses
        .iter()
        .filter_map(|l| l.command.as_ref().map(|c| c.title.as_str()))
        .collect()
}

// ─── Basic Override Detection ───────────────────────────────────────────────

#[test]
fn parent_class_method_override() {
    let backend = create_test_backend();
    let content = r#"<?php
class Animal {
    public function speak(): string { return ''; }
    public function eat(): void {}
}

class Dog extends Animal {
    public function speak(): string { return 'woof'; }
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);
    let titles = lens_titles(&lenses);

    assert_eq!(titles.len(), 1);
    assert_eq!(titles[0], "↑ Animal::speak");
}

#[test]
fn interface_method_implementation() {
    let backend = create_test_backend();
    let content = r#"<?php
interface Greetable {
    public function greet(): string;
}

class Greeter implements Greetable {
    public function greet(): string { return 'hello'; }
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);
    let titles = lens_titles(&lenses);

    assert_eq!(titles.len(), 1);
    assert_eq!(titles[0], "◆ Greetable::greet");
}

#[test]
fn no_lens_for_methods_without_prototype() {
    let backend = create_test_backend();
    let content = r#"<?php
class Standalone {
    public function doSomething(): void {}
    public function doMore(): void {}
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);

    assert!(lenses.is_empty());
}

#[test]
fn multiple_overrides_in_one_class() {
    let backend = create_test_backend();
    let content = r#"<?php
class Base {
    public function foo(): void {}
    public function bar(): void {}
    public function baz(): void {}
}

class Child extends Base {
    public function foo(): void {}
    public function bar(): void {}
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);
    let titles = lens_titles(&lenses);

    assert_eq!(titles.len(), 2);
    assert!(titles.contains(&"↑ Base::foo"));
    assert!(titles.contains(&"↑ Base::bar"));
}

// ─── Inheritance Chain ──────────────────────────────────────────────────────

#[test]
fn grandparent_override() {
    let backend = create_test_backend();
    let content = r#"<?php
class GrandParent_ {
    public function legacy(): void {}
}

class Parent_ extends GrandParent_ {
}

class Child extends Parent_ {
    public function legacy(): void {}
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);
    let titles = lens_titles(&lenses);

    assert_eq!(titles.len(), 1);
    // Should point to the grandparent since that's where the method
    // is actually declared.
    assert_eq!(titles[0], "↑ GrandParent_::legacy");
}

#[test]
fn parent_overrides_grandparent_lens_points_to_parent() {
    let backend = create_test_backend();
    let content = r#"<?php
class A {
    public function run(): void {}
}

class B extends A {
    public function run(): void {}
}

class C extends B {
    public function run(): void {}
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);

    // B overrides A::run, C overrides B::run (nearest ancestor wins)
    let b_lens: Vec<_> = lenses
        .iter()
        .filter(|l| {
            let line = l.range.start.line;
            // B::run is around line 7
            line > 5 && line < 9
        })
        .collect();
    let c_lens: Vec<_> = lenses
        .iter()
        .filter(|l| {
            let line = l.range.start.line;
            // C::run is around line 11
            line > 9
        })
        .collect();

    assert_eq!(b_lens.len(), 1);
    assert_eq!(b_lens[0].command.as_ref().unwrap().title, "↑ A::run");

    assert_eq!(c_lens.len(), 1);
    assert_eq!(c_lens[0].command.as_ref().unwrap().title, "↑ B::run");
}

// ─── Trait Methods ──────────────────────────────────────────────────────────

#[test]
fn trait_method_override() {
    let backend = create_test_backend();
    let content = r#"<?php
trait Loggable {
    public function log(string $msg): void {}
}

class Service {
    use Loggable;

    public function log(string $msg): void {
        // custom logging
    }
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);
    let titles = lens_titles(&lenses);

    assert_eq!(titles.len(), 1);
    assert_eq!(titles[0], "↑ Loggable::log");
}

// ─── Interface + Parent Combination ─────────────────────────────────────────

#[test]
fn parent_takes_precedence_over_interface() {
    let backend = create_test_backend();
    let content = r#"<?php
interface Renderable {
    public function render(): string;
}

class BaseView implements Renderable {
    public function render(): string { return ''; }
}

class ChildView extends BaseView {
    public function render(): string { return '<div>child</div>'; }
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);

    // BaseView should get ◆ Renderable::render
    let base_lenses: Vec<_> = lenses.iter().filter(|l| l.range.start.line < 9).collect();
    // ChildView should get ↑ BaseView::render (parent wins over interface)
    let child_lenses: Vec<_> = lenses.iter().filter(|l| l.range.start.line >= 9).collect();

    assert_eq!(base_lenses.len(), 1);
    assert_eq!(
        base_lenses[0].command.as_ref().unwrap().title,
        "◆ Renderable::render"
    );

    assert_eq!(child_lenses.len(), 1);
    assert_eq!(
        child_lenses[0].command.as_ref().unwrap().title,
        "↑ BaseView::render"
    );
}

// ─── Constructor Override ───────────────────────────────────────────────────

#[test]
fn constructor_override() {
    let backend = create_test_backend();
    let content = r#"<?php
class BaseModel {
    public function __construct() {}
}

class User extends BaseModel {
    public function __construct(string $name) {
        parent::__construct();
    }
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);
    let titles = lens_titles(&lenses);

    assert_eq!(titles.len(), 1);
    assert_eq!(titles[0], "↑ BaseModel::__construct");
}

// ─── Interface with no Override ─────────────────────────────────────────────

#[test]
fn interface_itself_has_no_lens() {
    let backend = create_test_backend();
    let content = r#"<?php
interface Cacheable {
    public function getCacheKey(): string;
    public function getCacheTTL(): int;
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);

    assert!(lenses.is_empty());
}

// ─── Code Lens Range ────────────────────────────────────────────────────────

#[test]
fn lens_range_is_on_method_line() {
    let backend = create_test_backend();
    let content = r#"<?php
class Base {
    public function process(): void {}
}

class Handler extends Base {
    public function process(): void {}
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);

    assert_eq!(lenses.len(), 1);
    let lens = &lenses[0];
    // The method `process` in Handler is on line 6 (0-based)
    assert_eq!(lens.range.start.line, 6);
    assert_eq!(lens.range.start.character, 0);
}

// ─── Code Lens Command ─────────────────────────────────────────────────────

#[test]
fn lens_command_uses_show_references_by_default() {
    let backend = create_test_backend();
    let content = r#"<?php
class Parent_ {
    public function action(): void {}
}

class Child extends Parent_ {
    public function action(): void {}
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);

    assert_eq!(lenses.len(), 1);
    let cmd = lenses[0].command.as_ref().unwrap();
    assert_eq!(cmd.command, "editor.action.showReferences");
    assert!(cmd.arguments.is_some());
    let args = cmd.arguments.as_ref().unwrap();
    // Should have 3 arguments: uri, position, locations[]
    assert_eq!(args.len(), 3);
}

// ─── Multiple Interfaces ────────────────────────────────────────────────────

#[test]
fn implements_multiple_interfaces() {
    let backend = create_test_backend();
    let content = r#"<?php
interface Countable_ {
    public function count(): int;
}

interface Serializable_ {
    public function serialize(): string;
}

class Collection implements Countable_, Serializable_ {
    public function count(): int { return 0; }
    public function serialize(): string { return ''; }
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);
    let titles = lens_titles(&lenses);

    assert_eq!(titles.len(), 2);
    assert!(titles.contains(&"◆ Countable_::count"));
    assert!(titles.contains(&"◆ Serializable_::serialize"));
}

// ─── Interface Extends Interface ────────────────────────────────────────────

#[test]
fn interface_extends_interface() {
    let backend = create_test_backend();
    let content = r#"<?php
interface BaseRepo {
    public function find(int $id): ?object;
}

interface UserRepo extends BaseRepo {
    public function findByEmail(string $email): ?object;
}

class EloquentUserRepo implements UserRepo {
    public function find(int $id): ?object { return null; }
    public function findByEmail(string $email): ?object { return null; }
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);
    let titles = lens_titles(&lenses);

    assert_eq!(titles.len(), 2);
    // find() comes from BaseRepo via the extends chain
    assert!(titles.contains(&"◆ BaseRepo::find"));
    assert!(titles.contains(&"◆ UserRepo::findByEmail"));
}

// ─── Cross-File Override ────────────────────────────────────────────────────

#[test]
fn cross_file_parent_class() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[
            (
                "src/Base.php",
                r#"<?php
namespace App;

class Base {
    public function handle(): void {}
}
"#,
            ),
            (
                "src/Handler.php",
                r#"<?php
namespace App;

class Handler extends Base {
    public function handle(): void {}
}
"#,
            ),
        ],
    );

    let base_uri = format!("file://{}", _dir.path().join("src/Base.php").display());
    let handler_uri = format!("file://{}", _dir.path().join("src/Handler.php").display());

    let base_content = std::fs::read_to_string(_dir.path().join("src/Base.php")).unwrap();
    let handler_content = std::fs::read_to_string(_dir.path().join("src/Handler.php")).unwrap();

    backend.update_ast(&base_uri, &base_content);
    backend.update_ast(&handler_uri, &handler_content);

    let lenses = backend
        .handle_code_lens(&handler_uri, &handler_content)
        .unwrap_or_default();
    let titles = lens_titles(&lenses);

    assert_eq!(titles.len(), 1);
    assert_eq!(titles[0], "↑ Base::handle");
}

// ─── Abstract Method Implementation ────────────────────────────────────────

#[test]
fn abstract_method_implementation() {
    let backend = create_test_backend();
    let content = r#"<?php
abstract class Shape {
    abstract public function area(): float;
    abstract public function perimeter(): float;
}

class Circle extends Shape {
    public function area(): float { return 3.14; }
    public function perimeter(): float { return 6.28; }
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);
    let titles = lens_titles(&lenses);

    assert_eq!(titles.len(), 2);
    assert!(titles.contains(&"↑ Shape::area"));
    assert!(titles.contains(&"↑ Shape::perimeter"));
}

// ─── Static Method Override ─────────────────────────────────────────────────

#[test]
fn static_method_override() {
    let backend = create_test_backend();
    let content = r#"<?php
class Factory {
    public static function create(): static { return new static(); }
}

class UserFactory extends Factory {
    public static function create(): static { return new static(); }
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);
    let titles = lens_titles(&lenses);

    assert_eq!(titles.len(), 1);
    assert_eq!(titles[0], "↑ Factory::create");
}

// ─── Empty File / No Classes ────────────────────────────────────────────────

#[test]
fn empty_file_returns_none() {
    let backend = create_test_backend();
    let content = "<?php\n// nothing here\n";
    let uri = "file:///test.php";
    backend.update_ast(uri, content);
    let result = backend.handle_code_lens(uri, content);

    assert!(result.is_none());
}

// ─── Mixed: Some Methods Override, Some Don't ───────────────────────────────

#[test]
fn only_overriding_methods_get_lenses() {
    let backend = create_test_backend();
    let content = r#"<?php
class Transport {
    public function send(): void {}
}

class EmailTransport extends Transport {
    public function send(): void {}
    public function formatBody(): string { return ''; }
    public function addAttachment(): void {}
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);
    let titles = lens_titles(&lenses);

    // Only send() overrides; formatBody and addAttachment are new.
    assert_eq!(titles.len(), 1);
    assert_eq!(titles[0], "↑ Transport::send");
}

// ─── Cross-File Interface Implementation ────────────────────────────────────

#[test]
fn cross_file_interface_implementation() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[
            (
                "src/Printable.php",
                r#"<?php
namespace App;

interface Printable {
    public function print(): string;
}
"#,
            ),
            (
                "src/Document.php",
                r#"<?php
namespace App;

class Document implements Printable {
    public function print(): string { return 'doc'; }
}
"#,
            ),
        ],
    );

    let iface_uri = format!("file://{}", _dir.path().join("src/Printable.php").display());
    let doc_uri = format!("file://{}", _dir.path().join("src/Document.php").display());

    let iface_content = std::fs::read_to_string(_dir.path().join("src/Printable.php")).unwrap();
    let doc_content = std::fs::read_to_string(_dir.path().join("src/Document.php")).unwrap();

    backend.update_ast(&iface_uri, &iface_content);
    backend.update_ast(&doc_uri, &doc_content);

    let lenses = backend
        .handle_code_lens(&doc_uri, &doc_content)
        .unwrap_or_default();
    let titles = lens_titles(&lenses);

    assert_eq!(titles.len(), 1);
    assert_eq!(titles[0], "◆ Printable::print");
}
