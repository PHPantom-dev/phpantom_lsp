//! Integration tests for the "Implement missing methods" code action.
//!
//! These tests exercise the full pipeline: parsing PHP source, resolving
//! the class hierarchy, detecting missing methods, and generating the
//! `WorkspaceEdit` with method stubs.

use crate::common::{create_psr4_workspace, create_test_backend, get_code_actions_at};
use tower_lsp::lsp_types::*;

/// Extract the "Implement" code action from a list of actions.
fn find_implement_action(actions: &[CodeActionOrCommand]) -> Option<&CodeAction> {
    actions.iter().find_map(|a| match a {
        CodeActionOrCommand::CodeAction(ca) if ca.title.starts_with("Implement ") => Some(ca),
        _ => None,
    })
}

/// Extract the inserted text from a code action's workspace edit.
fn extract_edit_text(action: &CodeAction) -> String {
    let edit = action.edit.as_ref().expect("action should have an edit");
    let changes = edit.changes.as_ref().expect("edit should have changes");
    let edits: Vec<&TextEdit> = changes.values().flat_map(|v| v.iter()).collect();
    edits
        .iter()
        .map(|e| e.new_text.as_str())
        .collect::<Vec<_>>()
        .join("")
}

// ─── Basic interface implementation ─────────────────────────────────────────

#[test]
fn implements_single_interface_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Renderable {
    public function render(): string;
}

class Page implements Renderable {
}
"#;

    backend.update_ast(uri, content);

    // Cursor inside the Page class body (line 6, the closing brace line).
    let actions = get_code_actions_at(&backend, uri, content, 6, 0);
    let action = find_implement_action(&actions);
    assert!(action.is_some(), "Should offer implement action");

    let action = action.unwrap();
    assert_eq!(action.title, "Implement `render`");

    let text = extract_edit_text(action);
    assert!(
        text.contains("public function render(): string"),
        "Stub should have correct signature. Got:\n{}",
        text
    );
}

#[test]
fn triggers_on_class_declaration_line() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Renderable {
    public function render(): string;
}

class Page implements Renderable {
}
"#;

    backend.update_ast(uri, content);

    // Cursor on the `class Page implements Renderable {` line (line 5),
    // before the opening brace.
    let actions = get_code_actions_at(&backend, uri, content, 5, 6);
    let action = find_implement_action(&actions);
    assert!(
        action.is_some(),
        "Should offer implement action when cursor is on the class declaration line"
    );
    assert_eq!(action.unwrap().title, "Implement `render`");
}

#[test]
fn implements_multiple_interface_methods() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Serializable {
    public function serialize(): string;
    public function unserialize(string $data): void;
}

class MyClass implements Serializable {
}
"#;

    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 7, 0);
    let action = find_implement_action(&actions);
    assert!(action.is_some(), "Should offer implement action");

    let action = action.unwrap();
    assert!(
        action.title.contains("2 missing methods"),
        "Title should mention count. Got: {}",
        action.title
    );

    let text = extract_edit_text(action);
    assert!(
        text.contains("public function serialize(): string"),
        "Should contain serialize stub. Got:\n{}",
        text
    );
    assert!(
        text.contains("public function unserialize(string $data): void"),
        "Should contain unserialize stub. Got:\n{}",
        text
    );
}

// ─── Abstract parent class ──────────────────────────────────────────────────

#[test]
fn implements_abstract_parent_methods() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
abstract class Shape {
    abstract public function area(): float;
    abstract protected function perimeter(): float;

    public function describe(): string {
        return "I am a shape";
    }
}

class Circle extends Shape {
}
"#;

    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 11, 0);
    let action = find_implement_action(&actions);
    assert!(
        action.is_some(),
        "Should offer implement action for abstract methods"
    );

    let action = action.unwrap();
    assert!(
        action.title.contains("2 missing methods"),
        "Title should mention 2 methods. Got: {}",
        action.title
    );

    let text = extract_edit_text(action);
    assert!(
        text.contains("public function area(): float"),
        "Should implement area(). Got:\n{}",
        text
    );
    assert!(
        text.contains("protected function perimeter(): float"),
        "Should keep protected visibility. Got:\n{}",
        text
    );
    // The concrete method describe() should NOT be in the stubs.
    assert!(
        !text.contains("describe"),
        "Should not stub concrete methods. Got:\n{}",
        text
    );
}

// ─── Already implemented methods are skipped ────────────────────────────────

#[test]
fn skips_already_implemented_methods() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Renderable {
    public function render(): string;
    public function toHtml(): string;
}

class Page implements Renderable {
    public function render(): string {
        return '<p>Page</p>';
    }
}
"#;

    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 8, 0);
    let action = find_implement_action(&actions);
    assert!(action.is_some(), "Should offer action for toHtml");

    let action = action.unwrap();
    assert_eq!(action.title, "Implement `toHtml`");

    let text = extract_edit_text(action);
    assert!(
        !text.contains("render"),
        "Should not re-implement render(). Got:\n{}",
        text
    );
    assert!(
        text.contains("public function toHtml(): string"),
        "Should implement toHtml(). Got:\n{}",
        text
    );
}

// ─── No action when all methods are implemented ─────────────────────────────

#[test]
fn no_action_when_fully_implemented() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Renderable {
    public function render(): string;
}

class Page implements Renderable {
    public function render(): string {
        return '<p>Page</p>';
    }
}
"#;

    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 7, 0);
    let action = find_implement_action(&actions);
    assert!(
        action.is_none(),
        "Should not offer implement action when all methods are implemented"
    );
}

// ─── No action for abstract classes ─────────────────────────────────────────

#[test]
fn no_action_for_abstract_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Renderable {
    public function render(): string;
}

abstract class AbstractPage implements Renderable {
}
"#;

    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 6, 0);
    let action = find_implement_action(&actions);
    assert!(
        action.is_none(),
        "Should not offer implement action for abstract classes"
    );
}

// ─── No action for interfaces ───────────────────────────────────────────────

#[test]
fn no_action_for_interface() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Renderable {
    public function render(): string;
}

interface ExtendedRenderable extends Renderable {
}
"#;

    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 6, 0);
    let action = find_implement_action(&actions);
    assert!(
        action.is_none(),
        "Should not offer implement action for interfaces"
    );
}

// ─── No action for traits ───────────────────────────────────────────────────

#[test]
fn no_action_for_trait() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
trait MyTrait {
    abstract public function doWork(): void;
}
"#;

    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 2, 0);
    let action = find_implement_action(&actions);
    assert!(
        action.is_none(),
        "Should not offer implement action for traits"
    );
}

// ─── Parameters with defaults and type hints ────────────────────────────────

#[test]
fn stub_includes_parameter_details() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Processor {
    public function process(string $name, array $options = [], bool $force = false): void;
}

class MyProcessor implements Processor {
}
"#;

    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 6, 0);
    let action = find_implement_action(&actions);
    assert!(action.is_some(), "Should offer implement action");

    let text = extract_edit_text(action.unwrap());
    assert!(
        text.contains("string $name"),
        "Should include type hint. Got:\n{}",
        text
    );
    assert!(
        text.contains("array $options = []"),
        "Should include default value. Got:\n{}",
        text
    );
    assert!(
        text.contains("bool $force = false"),
        "Should include bool default. Got:\n{}",
        text
    );
}

// ─── Static methods ─────────────────────────────────────────────────────────

#[test]
fn stub_preserves_static_modifier() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Factory {
    public static function create(): static;
}

class UserFactory implements Factory {
}
"#;

    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 6, 0);
    let action = find_implement_action(&actions);
    assert!(action.is_some(), "Should offer implement action");

    let text = extract_edit_text(action.unwrap());
    assert!(
        text.contains("public static function create(): static"),
        "Should preserve static modifier. Got:\n{}",
        text
    );
}

// ─── Multiple interfaces ────────────────────────────────────────────────────

#[test]
fn implements_methods_from_multiple_interfaces() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Renderable {
    public function render(): string;
}

interface Stringable {
    public function __toString(): string;
}

class Widget implements Renderable, Stringable {
}
"#;

    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 10, 0);
    let action = find_implement_action(&actions);
    assert!(action.is_some(), "Should offer implement action");

    let action = action.unwrap();
    assert!(
        action.title.contains("2 missing methods"),
        "Title should mention 2. Got: {}",
        action.title
    );

    let text = extract_edit_text(action);
    assert!(text.contains("function render()"), "Got:\n{}", text);
    assert!(text.contains("function __toString()"), "Got:\n{}", text);
}

// ─── Interface extends interface ────────────────────────────────────────────

#[test]
fn implements_methods_from_parent_interface() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Base {
    public function baseMethod(): void;
}

interface Child extends Base {
    public function childMethod(): string;
}

class MyClass implements Child {
}
"#;

    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 10, 0);
    let action = find_implement_action(&actions);
    assert!(action.is_some(), "Should offer implement action");

    let text = extract_edit_text(action.unwrap());
    assert!(
        text.contains("function childMethod()"),
        "Should implement childMethod. Got:\n{}",
        text
    );
    assert!(
        text.contains("function baseMethod()"),
        "Should implement inherited baseMethod. Got:\n{}",
        text
    );
}

// ─── Case-insensitive method matching ───────────────────────────────────────

#[test]
fn case_insensitive_method_check() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Logger {
    public function Log(string $message): void;
}

class FileLogger implements Logger {
    public function log(string $message): void {
        // PHP method names are case-insensitive
    }
}
"#;

    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 7, 0);
    let action = find_implement_action(&actions);
    assert!(
        action.is_none(),
        "Should recognize log/Log as the same method (case-insensitive)"
    );
}

// ─── No action when cursor is outside class ─────────────────────────────────

#[test]
fn no_action_outside_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Renderable {
    public function render(): string;
}

class Page implements Renderable {
}

$x = 1;
"#;

    backend.update_ast(uri, content);

    // Cursor on the $x = 1 line, outside any class.
    let actions = get_code_actions_at(&backend, uri, content, 8, 0);
    let action = find_implement_action(&actions);
    assert!(
        action.is_none(),
        "Should not offer action when cursor is outside a class"
    );
}

// ─── Indentation detection ──────────────────────────────────────────────────

#[test]
fn respects_tab_indentation() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\ninterface Renderable {\n\tpublic function render(): string;\n}\n\nclass Page implements Renderable {\n\tpublic $name = 'test';\n}\n";

    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 6, 0);
    let action = find_implement_action(&actions);
    assert!(action.is_some(), "Should offer implement action");

    let text = extract_edit_text(action.unwrap());
    // The stub should use tab indentation to match the class body.
    assert!(
        text.contains("\tpublic function render(): string"),
        "Should use tab indentation. Got:\n{}",
        text
    );
}

// ─── Cross-file with PSR-4 ──────────────────────────────────────────────────

#[test]
fn implements_interface_from_another_file() {
    let composer = r#"{
        "autoload": {
            "psr-4": {
                "App\\": "src/"
            }
        }
    }"#;

    let interface_file = "<?php\nnamespace App\\Contracts;\n\ninterface Renderable {\n    public function render(): string;\n    public function toHtml(): string;\n}\n";

    let class_file = "<?php\nnamespace App\\Views;\n\nuse App\\Contracts\\Renderable;\n\nclass Page implements Renderable {\n}\n";

    let (backend, dir) = create_psr4_workspace(
        composer,
        &[
            ("src/Contracts/Renderable.php", interface_file),
            ("src/Views/Page.php", class_file),
        ],
    );

    // Build URIs from the actual temp-directory paths.
    let iface_uri = Url::from_file_path(dir.path().join("src/Contracts/Renderable.php"))
        .unwrap()
        .to_string();
    let class_uri = Url::from_file_path(dir.path().join("src/Views/Page.php"))
        .unwrap()
        .to_string();

    // Load the interface file first so it's in the AST map.
    backend.update_ast(&iface_uri, interface_file);

    // Load the class file.
    backend.update_ast(&class_uri, class_file);

    let actions = get_code_actions_at(&backend, &class_uri, class_file, 6, 0);
    let action = find_implement_action(&actions);
    assert!(
        action.is_some(),
        "Should offer implement action for cross-file interface"
    );

    let action = action.unwrap();
    assert!(
        action.title.contains("2 missing methods"),
        "Title should mention 2 methods. Got: {}",
        action.title
    );

    let text = extract_edit_text(action);
    assert!(
        text.contains("function render()"),
        "Should contain render stub. Got:\n{}",
        text
    );
    assert!(
        text.contains("function toHtml()"),
        "Should contain toHtml stub. Got:\n{}",
        text
    );
}

// ─── Variadic and reference parameters ──────────────────────────────────────

#[test]
fn stub_handles_variadic_and_reference_params() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Collector {
    public function collect(string ...$items): array;
    public function fill(array &$target): void;
}

class MyCollector implements Collector {
}
"#;

    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 7, 0);
    let action = find_implement_action(&actions);
    assert!(action.is_some(), "Should offer implement action");

    let text = extract_edit_text(action.unwrap());
    assert!(
        text.contains("string ...$items"),
        "Should preserve variadic. Got:\n{}",
        text
    );
    assert!(
        text.contains("array &$target"),
        "Should preserve reference. Got:\n{}",
        text
    );
}

// ─── Nullable return types ──────────────────────────────────────────────────

#[test]
fn stub_preserves_nullable_return_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Repository {
    public function find(int $id): ?string;
}

class UserRepository implements Repository {
}
"#;

    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 6, 0);
    let action = find_implement_action(&actions);
    assert!(action.is_some());

    let text = extract_edit_text(action.unwrap());
    assert!(
        text.contains("?string"),
        "Should preserve nullable return type. Got:\n{}",
        text
    );
}

// ─── Union return types ─────────────────────────────────────────────────────

#[test]
fn stub_preserves_union_return_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Parser {
    public function parse(string $input): int|false;
}

class MyParser implements Parser {
}
"#;

    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 6, 0);
    let action = find_implement_action(&actions);
    assert!(action.is_some());

    let text = extract_edit_text(action.unwrap());
    assert!(
        text.contains("int|false"),
        "Should preserve union return type. Got:\n{}",
        text
    );
}

// ─── No return type ─────────────────────────────────────────────────────────

#[test]
fn stub_without_return_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Handler {
    public function handle();
}

class MyHandler implements Handler {
}
"#;

    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 6, 0);
    let action = find_implement_action(&actions);
    assert!(action.is_some());

    let text = extract_edit_text(action.unwrap());
    // No `: type` after the parentheses.
    assert!(
        text.contains("public function handle()\n"),
        "Should have no return type. Got:\n{}",
        text
    );
}

// ─── Mixed abstract + interface ─────────────────────────────────────────────

#[test]
fn implements_both_abstract_and_interface_methods() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Renderable {
    public function render(): string;
}

abstract class Component implements Renderable {
    abstract protected function setup(): void;
}

class Button extends Component {
}
"#;

    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 10, 0);
    let action = find_implement_action(&actions);
    assert!(action.is_some(), "Should offer action");

    let text = extract_edit_text(action.unwrap());
    assert!(
        text.contains("function render()"),
        "Should implement interface method. Got:\n{}",
        text
    );
    assert!(
        text.contains("protected function setup(): void"),
        "Should implement abstract method. Got:\n{}",
        text
    );
}

// ─── Deep inheritance chain ─────────────────────────────────────────────────

#[test]
fn implements_methods_from_deep_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
abstract class GrandParent {
    abstract public function grandParentMethod(): void;
}

abstract class ParentClass extends GrandParent {
    abstract public function parentMethod(): string;
}

class Child extends ParentClass {
}
"#;

    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 10, 0);
    let action = find_implement_action(&actions);
    assert!(action.is_some(), "Should offer action for deep chain");

    let text = extract_edit_text(action.unwrap());
    assert!(
        text.contains("function parentMethod()"),
        "Should implement parentMethod. Got:\n{}",
        text
    );
    assert!(
        text.contains("function grandParentMethod()"),
        "Should implement grandParentMethod. Got:\n{}",
        text
    );
}

// ─── Already partially implemented from parent ──────────────────────────────

#[test]
fn skips_methods_implemented_by_parent() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface FullService {
    public function methodA(): void;
    public function methodB(): string;
}

abstract class BaseService implements FullService {
    public function methodA(): void {
        // implemented
    }
}

class ConcreteService extends BaseService {
}
"#;

    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 13, 0);
    let action = find_implement_action(&actions);
    assert!(action.is_some(), "Should offer action for methodB only");

    let action = action.unwrap();
    assert_eq!(
        action.title, "Implement `methodB`",
        "Should only need to implement methodB"
    );

    let text = extract_edit_text(action);
    assert!(
        !text.contains("methodA"),
        "Should not re-implement methodA. Got:\n{}",
        text
    );
    assert!(
        text.contains("function methodB()"),
        "Should implement methodB. Got:\n{}",
        text
    );
}
