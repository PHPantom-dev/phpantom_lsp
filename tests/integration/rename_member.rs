//! Rename of class members: methods, properties, constants declared on a
//! class, and enum cases.

use crate::common::{
    apply_edits, create_test_backend, edits_for_uri, line_char_of, open_php, prepare_rename,
    rename, rename_result,
};
use tower_lsp::lsp_types::*;

// ─── Method Rename ──────────────────────────────────────────────────────────

#[tokio::test]
async fn rename_method() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Service {\n",
        "    public function process(): void {}\n",
        "}\n",
        "function demo(): void {\n",
        "    $s = new Service();\n",
        "    $s->process();\n",
        "    $s->process();\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Rename from call site (line 6).
    let edit = rename(&backend, &uri, 6, 9, "execute").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for method rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    // Should find: declaration (L2) + 2 call sites (L6, L7) = at least 3.
    assert!(
        file_edits.len() >= 3,
        "Expected at least 3 edits for process, got {}",
        file_edits.len()
    );

    for te in &file_edits {
        assert_eq!(te.new_text, "execute");
    }
}

#[tokio::test]
async fn rename_static_method() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Factory {\n",
        "    public static function create(): self { return new self(); }\n",
        "}\n",
        "function demo(): void {\n",
        "    Factory::create();\n",
        "    Factory::create();\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 5, 14, "build").await;
    assert!(edit.is_some());

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    assert!(
        file_edits.len() >= 3,
        "Expected at least 3 edits for create, got {}",
        file_edits.len()
    );

    for te in &file_edits {
        assert_eq!(te.new_text, "build");
    }
}

// ─── Property Rename ────────────────────────────────────────────────────────

#[tokio::test]
async fn rename_property_from_access() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name = '';\n",
        "    public function greet(): string {\n",
        "        return $this->name;\n",
        "    }\n",
        "}\n",
        "function demo(): void {\n",
        "    $u = new User();\n",
        "    $u->name = 'Alice';\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Rename from access site (line 9, `$u->name`).
    let edit = rename(&backend, &uri, 9, 9, "displayName").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for property rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    // Should have edits for: declaration ($name), $this->name, $u->name.
    assert!(
        file_edits.len() >= 3,
        "Expected at least 3 edits for name property, got {}",
        file_edits.len()
    );

    // The declaration site includes `$`, access sites don't.
    for te in &file_edits {
        assert!(
            te.new_text == "displayName" || te.new_text == "$displayName",
            "Unexpected edit text: {}",
            te.new_text
        );
    }
}

#[tokio::test]
async fn rename_promoted_property_from_parameter() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class SomeService {\n",
        "    public function __construct(\n",
        "        private int $someField,\n",
        "    ) {}\n",
        "\n",
        "    public function handle(): int {\n",
        "        return $this->someField;\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Rename from the constructor-promoted parameter declaration itself.
    let (line, character) = line_char_of(text, "$someField,");
    let edit = rename(&backend, &uri, line, character, "otherField").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for promoted property rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    // Should have edits for: the promoted parameter and $this->someField.
    assert!(
        file_edits.len() >= 2,
        "Expected at least 2 edits for someField, got {}",
        file_edits.len()
    );

    // The declaration site includes `$`, the `$this->` access site doesn't.
    for te in &file_edits {
        assert!(
            te.new_text == "otherField" || te.new_text == "$otherField",
            "Unexpected edit text: {}",
            te.new_text
        );
    }

    let updated = apply_edits(text, &file_edits);
    assert!(
        updated.contains("$this->otherField"),
        "Expected $this->someField usage to cascade to otherField:\n{updated}"
    );
}

// ─── Class-Aware Member Rename ──────────────────────────────────────────────

#[tokio::test]
async fn rename_method_does_not_leak_to_unrelated_class() {
    // Two unrelated classes with the same method name.  Renaming the
    // method on one class must not touch the other.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                 // L0
        "class Dog {\n",                           // L1
        "    public function speak(): void {}\n",  // L2
        "}\n",                                     // L3
        "class Cat {\n",                           // L4
        "    public function speak(): void {}\n",  // L5
        "}\n",                                     // L6
        "function demo(Dog $d, Cat $c): void {\n", // L7
        "    $d->speak();\n",                      // L8
        "    $c->speak();\n",                      // L9
        "}\n",                                     // L10
    );

    open_php(&backend, &uri, text).await;

    // Rename speak() from the Dog::speak declaration (line 2, col 21).
    // "    public function speak(): void {}"
    //                     ^ col 20
    let edit = rename(&backend, &uri, 2, 21, "bark").await;
    assert!(edit.is_some(), "Rename should produce edits");

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    // Dog::speak and $d->speak should be renamed to bark.
    assert!(
        result.contains("function bark()"),
        "Dog's method should be renamed to bark; got:\n{}",
        result
    );
    assert!(
        result.contains("$d->bark()"),
        "$d->speak() should become $d->bark(); got:\n{}",
        result
    );

    // Cat::speak and $c->speak must NOT be renamed.
    assert!(
        result.contains("class Cat {\n    public function speak(): void {}"),
        "Cat's method should remain speak; got:\n{}",
        result
    );
    assert!(
        result.contains("$c->speak()"),
        "$c->speak() should remain unchanged; got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_private_method_excludes_unresolved_same_name_calls() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Listener {\n",
        "    private function recalculate(): void {}\n",
        "    public function handle(): void {\n",
        "        $this->recalculate();\n",
        "    }\n",
        "}\n",
        "class Other {\n",
        "    public function recalculate(): void {}\n",
        "}\n",
        "function demo($unknown, Other $other): void {\n",
        "    $unknown->recalculate();\n",
        "    $other->recalculate();\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 2, 25, "calculate").await;
    assert!(edit.is_some(), "Rename should produce edits");

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert!(
        result.contains("private function calculate(): void {}"),
        "Private declaration should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("$this->calculate()"),
        "$this call should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("$unknown->recalculate()"),
        "Unresolved same-name call should remain unchanged; got:\n{}",
        result
    );
    assert!(
        result.contains("$other->recalculate()"),
        "Resolved unrelated class call should remain unchanged; got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_listener_private_method_cross_file_stays_scoped() {
    let backend = create_test_backend();
    let uri_a = Url::parse("file:///Listener.php").unwrap();
    let uri_b = Url::parse("file:///Other.php").unwrap();
    let uri_c = Url::parse("file:///Use.php").unwrap();

    let text_a = concat!(
        "<?php\n",
        "final class InvoiceListener {\n",
        "    private function recalculate(): void {}\n",
        "    public function handle(): void {\n",
        "        $this->recalculate();\n",
        "    }\n",
        "}\n",
    );

    let text_b = concat!(
        "<?php\n",
        "class PricingService {\n",
        "    public function recalculate(): void {}\n",
        "}\n",
    );

    let text_c = concat!(
        "<?php\n",
        "function demo($unknown, PricingService $service): void {\n",
        "    $unknown->recalculate();\n",
        "    $service->recalculate();\n",
        "}\n",
    );

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;
    open_php(&backend, &uri_c, text_c).await;

    let edit = rename(&backend, &uri_a, 2, 25, "calculate").await;
    assert!(edit.is_some(), "Rename should produce edits");

    let edit = edit.unwrap();
    let edits_a = edits_for_uri(&edit, &uri_a);
    let edits_b = edits_for_uri(&edit, &uri_b);
    let edits_c = edits_for_uri(&edit, &uri_c);

    let result_a = apply_edits(text_a, &edits_a);
    let result_b = apply_edits(text_b, &edits_b);
    let result_c = apply_edits(text_c, &edits_c);

    assert!(
        result_a.contains("private function calculate(): void {}"),
        "Listener private method should be renamed; got:\n{}",
        result_a
    );
    assert!(
        result_a.contains("$this->calculate()"),
        "Listener self-call should be renamed; got:\n{}",
        result_a
    );
    assert!(
        result_b.contains("public function recalculate(): void {}"),
        "Unrelated class declaration should stay unchanged; got:\n{}",
        result_b
    );
    assert!(
        result_c.contains("$unknown->recalculate()"),
        "Unknown receiver call should stay unchanged; got:\n{}",
        result_c
    );
    assert!(
        result_c.contains("$service->recalculate()"),
        "Unrelated typed receiver call should stay unchanged; got:\n{}",
        result_c
    );
}

#[tokio::test]
async fn rename_method_on_implementation_does_not_leak_to_sibling_implementors() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface Recalculates {\n",
        "    public function recalculate(): void;\n",
        "}\n",
        "class Listener implements Recalculates {\n",
        "    public function recalculate(): void {}\n",
        "}\n",
        "class Worker implements Recalculates {\n",
        "    public function recalculate(): void {}\n",
        "}\n",
        "function demo(Listener $listener, Worker $worker): void {\n",
        "    $listener->recalculate();\n",
        "    $worker->recalculate();\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 5, 20, "calculate").await;
    assert!(edit.is_some(), "Rename should produce edits");

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert!(
        result.contains(
            "class Listener implements Recalculates {\n    public function calculate(): void {}"
        ),
        "Implementation declaration should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("$listener->calculate()"),
        "Listener call should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("interface Recalculates {\n    public function recalculate(): void;"),
        "Interface declaration should remain unchanged; got:\n{}",
        result
    );
    assert!(
        result.contains(
            "class Worker implements Recalculates {\n    public function recalculate(): void {}"
        ),
        "Sibling implementation should remain unchanged; got:\n{}",
        result
    );
    assert!(
        result.contains("$worker->recalculate()"),
        "Sibling call should remain unchanged; got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_interface_method_updates_implementations_and_calls() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface Recalculates {\n",
        "    public function recalculate(): void;\n",
        "}\n",
        "class Listener implements Recalculates {\n",
        "    public function recalculate(): void {}\n",
        "}\n",
        "class Worker implements Recalculates {\n",
        "    public function recalculate(): void {}\n",
        "}\n",
        "function demo(Recalculates $item, Listener $listener, Worker $worker): void {\n",
        "    $item->recalculate();\n",
        "    $listener->recalculate();\n",
        "    $worker->recalculate();\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 2, 20, "calculate").await;
    assert!(edit.is_some(), "Rename should produce edits");

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert!(
        result.contains("interface Recalculates {\n    public function calculate(): void;"),
        "Interface declaration should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains(
            "class Listener implements Recalculates {\n    public function calculate(): void {}"
        ),
        "First implementation should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains(
            "class Worker implements Recalculates {\n    public function calculate(): void {}"
        ),
        "Second implementation should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("$item->calculate()"),
        "Interface-typed call should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("$listener->calculate()"),
        "Listener call should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("$worker->calculate()"),
        "Worker call should be renamed; got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_child_override_stays_on_child_branch() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class BaseListener {\n",
        "    protected function recalculate(): void {}\n",
        "}\n",
        "class InvoiceListener extends BaseListener {\n",
        "    protected function recalculate(): void {\n",
        "        $this->recalculate();\n",
        "    }\n",
        "}\n",
        "class OrderListener extends BaseListener {\n",
        "    protected function recalculate(): void {\n",
        "        $this->recalculate();\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 5, 24, "calculate").await;
    assert!(edit.is_some(), "Rename should produce edits");

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert!(
        result.contains("class InvoiceListener extends BaseListener {\n    protected function calculate(): void {\n        $this->calculate();"),
        "Child override should be renamed with its self-call; got:\n{}",
        result
    );
    assert!(
        result.contains("class BaseListener {\n    protected function recalculate(): void {}"),
        "Parent declaration should remain unchanged; got:\n{}",
        result
    );
    assert!(
        result.contains("class OrderListener extends BaseListener {\n    protected function recalculate(): void {\n        $this->recalculate();"),
        "Sibling override branch should remain unchanged; got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_parent_method_updates_overrides_and_subclass_calls() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class BaseJob {\n",
        "    protected function recalculate(): void {}\n",
        "}\n",
        "class ChildJob extends BaseJob {\n",
        "    protected function recalculate(): void {\n",
        "        $this->recalculate();\n",
        "    }\n",
        "    public function run(): void {\n",
        "        $this->recalculate();\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 2, 24, "calculate").await;
    assert!(edit.is_some(), "Rename should produce edits");

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert!(
        result.contains("class BaseJob {\n    protected function calculate(): void {}"),
        "Parent declaration should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("class ChildJob extends BaseJob {\n    protected function calculate(): void {\n        $this->calculate();\n    }\n    public function run(): void {\n        $this->calculate();"),
        "Override and subclass calls should be renamed; got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_method_includes_inherited_class() {
    // Renaming a method on a parent class should also rename it on
    // accesses through a child class.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                    // L0
        "class Base {\n",                             // L1
        "    public function run(): void {}\n",       // L2
        "}\n",                                        // L3
        "class Child extends Base {}\n",              // L4
        "function demo(Base $b, Child $c): void {\n", // L5
        "    $b->run();\n",                           // L6
        "    $c->run();\n",                           // L7
        "}\n",                                        // L8
    );

    open_php(&backend, &uri, text).await;

    // Rename run() from $b->run() (line 6, col 10).
    let edit = rename(&backend, &uri, 6, 10, "execute").await;
    assert!(edit.is_some(), "Rename should produce edits");

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    // Both $b->run() and $c->run() should be renamed (Child extends Base).
    assert!(
        result.contains("$b->execute()"),
        "$b->run() should become $b->execute(); got:\n{}",
        result
    );
    assert!(
        result.contains("$c->execute()"),
        "$c->run() should become $c->execute() (inherited); got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_static_method_does_not_leak_to_unrelated_class() {
    let backend = create_test_backend();
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

    open_php(&backend, &uri, text).await;

    // Rename create() from Alpha::create() call (line 8, col 12).
    // "    Alpha::create();"
    //             ^ col 11
    let edit = rename(&backend, &uri, 8, 12, "make").await;
    assert!(edit.is_some(), "Rename should produce edits");

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    // Alpha::create should be renamed.
    assert!(
        result.contains("Alpha::make()"),
        "Alpha::create() should become Alpha::make(); got:\n{}",
        result
    );

    // Beta::create must NOT be renamed.
    assert!(
        result.contains("Beta::create()"),
        "Beta::create() should remain unchanged; got:\n{}",
        result
    );
}

// ─── Enum Case Rename ───────────────────────────────────────────────────────

#[tokio::test]
async fn prepare_rename_enum_case_at_declaration() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                 // 0
        "enum TaskType: int {\n",                  // 1
        "    case Task  = 1;\n",                   // 2
        "    case Issue = 2;\n",                   // 3
        "    public function isIssue(): bool {\n", // 4
        "        return $this === self::Issue;\n", // 5
        "    }\n",                                 // 6
        "}\n",                                     // 7
    );

    open_php(&backend, &uri, text).await;

    // Cursor on `Issue` in `case Issue = 2;` (line 3, col 9)
    let result = prepare_rename(&backend, &uri, 3, 9).await;
    assert!(
        result.is_some(),
        "prepare_rename should succeed on enum case declaration"
    );

    if let Some(PrepareRenameResponse::RangeWithPlaceholder { placeholder, .. }) = result {
        assert_eq!(
            placeholder, "Issue",
            "Placeholder should be the enum case name"
        );
    } else {
        panic!("Expected RangeWithPlaceholder, got {:?}", result);
    }
}

#[tokio::test]
async fn prepare_rename_enum_case_at_reference() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                 // 0
        "enum TaskType: int {\n",                  // 1
        "    case Task  = 1;\n",                   // 2
        "    case Issue = 2;\n",                   // 3
        "    public function isIssue(): bool {\n", // 4
        "        return $this === self::Issue;\n", // 5
        "    }\n",                                 // 6
        "}\n",                                     // 7
    );

    open_php(&backend, &uri, text).await;

    // Cursor on `Issue` in `self::Issue` (line 5, col 36)
    let result = prepare_rename(&backend, &uri, 5, 36).await;
    assert!(
        result.is_some(),
        "prepare_rename should succeed on enum case reference"
    );

    if let Some(PrepareRenameResponse::RangeWithPlaceholder { placeholder, .. }) = result {
        assert_eq!(
            placeholder, "Issue",
            "Placeholder should be the enum case name"
        );
    } else {
        panic!("Expected RangeWithPlaceholder, got {:?}", result);
    }
}

#[tokio::test]
async fn rename_enum_case_from_declaration() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                 // 0
        "enum TaskType: int {\n",                  // 1
        "    case Task  = 1;\n",                   // 2
        "    case Issue = 2;\n",                   // 3
        "    public function isIssue(): bool {\n", // 4
        "        return $this === self::Issue;\n", // 5
        "    }\n",                                 // 6
        "}\n",                                     // 7
    );

    open_php(&backend, &uri, text).await;

    // Rename `Issue` from its declaration site (line 3, col 9)
    let edit = rename(&backend, &uri, 3, 9, "Ticket").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for enum case rename from declaration"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    // Should have at least 2 edits: the declaration + the self::Issue reference
    assert!(
        file_edits.len() >= 2,
        "Expected at least 2 edits for Issue → Ticket, got {}",
        file_edits.len()
    );

    for te in &file_edits {
        assert_eq!(te.new_text, "Ticket");
    }

    let result = apply_edits(text, &file_edits);
    assert!(
        result.contains("case Ticket"),
        "Declaration should be renamed: {}",
        result
    );
    assert!(
        result.contains("self::Ticket"),
        "Reference should be renamed: {}",
        result
    );
    assert!(
        !result.contains("case Issue"),
        "Old declaration should not remain: {}",
        result
    );
    assert!(
        !result.contains("self::Issue"),
        "Old reference should not remain: {}",
        result
    );
}

#[tokio::test]
async fn rename_enum_case_from_reference() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                 // 0
        "enum TaskType: int {\n",                  // 1
        "    case Task  = 1;\n",                   // 2
        "    case Issue = 2;\n",                   // 3
        "    public function isIssue(): bool {\n", // 4
        "        return $this === self::Issue;\n", // 5
        "    }\n",                                 // 6
        "}\n",                                     // 7
    );

    open_php(&backend, &uri, text).await;

    // Rename `Issue` from a reference site: `self::Issue` (line 5, col 36)
    let edit = rename(&backend, &uri, 5, 36, "Ticket").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for enum case rename from reference"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    assert!(
        file_edits.len() >= 2,
        "Expected at least 2 edits for Issue → Ticket, got {}",
        file_edits.len()
    );

    for te in &file_edits {
        assert_eq!(te.new_text, "Ticket");
    }

    let result = apply_edits(text, &file_edits);
    assert!(
        result.contains("case Ticket"),
        "Declaration should be renamed: {}",
        result
    );
    assert!(
        result.contains("self::Ticket"),
        "Reference should be renamed: {}",
        result
    );
}

#[tokio::test]
async fn rename_enum_case_does_not_affect_other_cases() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                 // 0
        "enum TaskType: int {\n",                  // 1
        "    case Task  = 1;\n",                   // 2
        "    case Issue = 2;\n",                   // 3
        "    public function isIssue(): bool {\n", // 4
        "        return $this === self::Issue;\n", // 5
        "    }\n",                                 // 6
        "    public function isTask(): bool {\n",  // 7
        "        return $this === self::Task;\n",  // 8
        "    }\n",                                 // 9
        "}\n",                                     // 10
    );

    open_php(&backend, &uri, text).await;

    // Rename `Issue` from declaration (line 3, col 9)
    let edit = rename(&backend, &uri, 3, 9, "Ticket").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for enum case rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    // `Task` case should remain untouched
    assert!(
        result.contains("case Task"),
        "Other enum case 'Task' should not be affected: {}",
        result
    );
    assert!(
        result.contains("self::Task"),
        "Other enum case reference 'self::Task' should not be affected: {}",
        result
    );
}

#[tokio::test]
async fn rename_unit_enum_case() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                   // 0
        "enum Color {\n",            // 1
        "    case Red;\n",           // 2
        "    case Blue;\n",          // 3
        "}\n",                       // 4
        "function demo(): void {\n", // 5
        "    $c = Color::Red;\n",    // 6
        "}\n",                       // 7
    );

    open_php(&backend, &uri, text).await;

    // Rename `Red` from declaration (line 2, col 9)
    let edit = rename(&backend, &uri, 2, 9, "Crimson").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for unit enum case rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    assert!(
        file_edits.len() >= 2,
        "Expected at least 2 edits for Red → Crimson, got {}",
        file_edits.len()
    );

    for te in &file_edits {
        assert_eq!(te.new_text, "Crimson");
    }

    let result = apply_edits(text, &file_edits);
    assert!(
        result.contains("case Crimson"),
        "Declaration should be renamed: {}",
        result
    );
    assert!(
        result.contains("Color::Crimson"),
        "Reference should be renamed: {}",
        result
    );
}

// ─── Eloquent magic members ─────────────────────────────────────────────────

const ELOQUENT_MODEL: &str = r#"<?php
namespace Illuminate\Database\Eloquent {
    abstract class Model {}
}
namespace App {
    use Illuminate\Database\Eloquent\Model;
    class Author extends Model {
        public function scopeActive($query): void {}
        public function getDisplayNameAttribute(): string { return ''; }
    }
}
"#;

const ELOQUENT_USAGE: &str = r#"<?php
namespace App;
function show(Author $author): void {
    Author::active();
    echo $author->display_name;
}
"#;

/// Open the model and a file that uses it, returning both URIs.
async fn open_eloquent_files(backend: &phpantom_lsp::Backend) -> (Url, Url) {
    let model_uri = Url::parse("file:///Author.php").unwrap();
    let usage_uri = Url::parse("file:///usage.php").unwrap();
    open_php(backend, &model_uri, ELOQUENT_MODEL).await;
    open_php(backend, &usage_uri, ELOQUENT_USAGE).await;
    (model_uri, usage_uri)
}

#[tokio::test]
async fn renaming_a_scope_renames_its_calls_under_the_scope_name() {
    let backend = create_test_backend();
    let (uri, usage_uri) = open_eloquent_files(&backend).await;

    let (line, character) = line_char_of(ELOQUENT_MODEL, "scopeActive");
    let edit = rename(&backend, &uri, line, character + 1, "scopeRecent")
        .await
        .expect("expected a scope rename");
    let result = apply_edits(ELOQUENT_MODEL, &edits_for_uri(&edit, &uri));
    assert!(result.contains("function scopeRecent($query)"), "{result}");
    let usage = apply_edits(ELOQUENT_USAGE, &edits_for_uri(&edit, &usage_uri));
    assert!(usage.contains("Author::recent();"), "{usage}");
}

#[tokio::test]
async fn renaming_an_accessor_renames_its_property_reads() {
    let backend = create_test_backend();
    let (uri, usage_uri) = open_eloquent_files(&backend).await;

    let (line, character) = line_char_of(ELOQUENT_MODEL, "getDisplayNameAttribute");
    let edit = rename(&backend, &uri, line, character + 1, "getFullNameAttribute")
        .await
        .expect("expected an accessor rename");
    let result = apply_edits(ELOQUENT_MODEL, &edits_for_uri(&edit, &uri));
    assert!(
        result.contains("function getFullNameAttribute()"),
        "{result}"
    );
    let usage = apply_edits(ELOQUENT_USAGE, &edits_for_uri(&edit, &usage_uri));
    assert!(usage.contains("$author->full_name;"), "{usage}");
}

#[tokio::test]
async fn renaming_a_scope_out_of_its_convention_is_refused() {
    let backend = create_test_backend();
    let (uri, _) = open_eloquent_files(&backend).await;

    let (line, character) = line_char_of(ELOQUENT_MODEL, "scopeActive");
    let refusal = rename_result(&backend, &uri, line, character + 1, "recent")
        .await
        .expect_err("a scope renamed without its prefix strands its calls");
    assert!(refusal.contains("`active`"), "{refusal}");
}
