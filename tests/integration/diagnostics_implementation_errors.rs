#[cfg(test)]
mod tests {
    use crate::common::collect_diagnostics_with;
    use crate::common::message_text;
    use phpantom_lsp::Backend;
    use tower_lsp_server::gen_lsp_types::*;

    fn collect(php: &str) -> Vec<Diagnostic> {
        collect_diagnostics_with(
            &Backend::new_test(),
            php,
            Backend::collect_implementation_error_diagnostics,
        )
    }

    #[test]
    fn no_diagnostic_for_abstract_class() {
        let php = r#"<?php
interface Foo { public function bar(): void; }
abstract class Baz implements Foo {}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Abstract classes should not get diagnostics"
        );
    }

    #[test]
    fn no_diagnostic_for_interface() {
        let php = r#"<?php
interface Foo { public function bar(): void; }
interface Baz extends Foo { public function qux(): void; }
"#;
        let diags = collect(php);
        assert!(diags.is_empty(), "Interfaces should not get diagnostics");
    }

    #[test]
    fn no_diagnostic_when_all_methods_implemented() {
        let php = r#"<?php
interface Foo { public function bar(): void; }
class Baz implements Foo {
    public function bar(): void {}
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Fully implemented class should have no diagnostics"
        );
    }

    #[test]
    fn diagnostic_for_missing_interface_method() {
        let php = r#"<?php
interface Foo {
    public function bar(): void;
}
class Baz implements Foo {
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1);
        assert!(message_text(&diags[0].message).contains("Baz"));
        assert!(message_text(&diags[0].message).contains("bar()"));
        assert!(message_text(&diags[0].message).contains("interface"));
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::Error));
    }

    #[test]
    fn diagnostic_for_missing_abstract_method() {
        let php = r#"<?php
abstract class Base {
    abstract public function doSomething(): void;
}
class Child extends Base {
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1);
        assert!(message_text(&diags[0].message).contains("Child"));
        assert!(message_text(&diags[0].message).contains("doSomething()"));
        assert!(message_text(&diags[0].message).contains("class"));
    }

    #[test]
    fn diagnostic_lists_multiple_missing_methods() {
        let php = r#"<?php
interface Foo {
    public function bar(): void;
    public function baz(): void;
    public function qux(): void;
}
class Impl implements Foo {
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1);
        assert!(message_text(&diags[0].message).contains("3 methods"));
        assert!(message_text(&diags[0].message).contains("bar()"));
        assert!(message_text(&diags[0].message).contains("baz()"));
        assert!(message_text(&diags[0].message).contains("qux()"));
    }

    #[test]
    fn no_diagnostic_for_plain_class_without_interfaces() {
        let php = r#"<?php
class Simple {
    public function foo(): void {}
}
"#;
        let diags = collect(php);
        assert!(diags.is_empty());
    }

    #[test]
    fn diagnostic_has_correct_code_and_source() {
        let php = r#"<?php
interface Foo { public function bar(): void; }
class Baz implements Foo {}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].code,
            Some(Code::String("missing_implementation".to_string()))
        );
        assert_eq!(diags[0].source, Some("phpantom".to_string()));
    }

    #[test]
    fn no_diagnostic_for_trait() {
        let php = r#"<?php
trait MyTrait {
    abstract public function doIt(): void;
}
"#;
        let diags = collect(php);
        assert!(diags.is_empty(), "Traits should not get diagnostics");
    }

    #[test]
    fn no_diagnostic_for_enum_with_all_methods_implemented() {
        let php = r#"<?php
interface HasLabel { public function label(): string; }
enum Color implements HasLabel {
    case Red;
    case Blue;

    public function label(): string {
        return $this->name;
    }
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Enum with implemented methods should have no diagnostics"
        );
    }

    #[test]
    fn diagnostic_for_enum_missing_interface_method() {
        let php = r#"<?php
interface HasLabel { public function label(): string; }
enum Color implements HasLabel {
    case Red;
    case Blue;
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1);
        assert!(message_text(&diags[0].message).contains("Enum"));
        assert!(message_text(&diags[0].message).contains("Color"));
        assert!(message_text(&diags[0].message).contains("label()"));
        assert!(message_text(&diags[0].message).contains("interface"));
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::Error));
    }

    #[test]
    fn no_diagnostic_for_enum_without_interfaces() {
        let php = r#"<?php
enum Suit {
    case Hearts;
    case Diamonds;
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Enum without interfaces should have no diagnostics"
        );
    }

    #[test]
    fn enum_multiple_missing_methods() {
        let php = r#"<?php
interface HasLabel {
    public function label(): string;
    public function description(): string;
}
enum Color implements HasLabel {
    case Red;
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1);
        assert!(message_text(&diags[0].message).contains("Enum"));
        assert!(message_text(&diags[0].message).contains("2 methods"));
        assert!(message_text(&diags[0].message).contains("label()"));
        assert!(message_text(&diags[0].message).contains("description()"));
    }

    #[test]
    fn case_insensitive_method_matching() {
        let php = r#"<?php
interface Foo { public function doSomething(): void; }
class Bar implements Foo {
    public function DOSOMETHING(): void {}
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Method matching should be case-insensitive"
        );
    }

    #[test]
    fn parent_implements_interface_method() {
        let php = r#"<?php
interface Foo { public function bar(): void; }
class Base implements Foo {
    public function bar(): void {}
}
class Child extends Base {}
"#;
        let diags = collect(php);
        // Child doesn't declare implements Foo, so no check needed.
        // But even if it did, bar() is inherited from Base.
        assert!(diags.is_empty());
    }

    #[test]
    fn trait_satisfies_interface_method() {
        let php = r#"<?php
interface Wireable {
    public function toLivewire(): array;
    public function fromLivewire($value): static;
}

trait WireableData {
    public function toLivewire(): array { return []; }
    public static function fromLivewire($value): static { return new static(); }
}

class MyData implements Wireable {
    use WireableData;
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Trait methods should satisfy interface requirements, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn trait_satisfies_abstract_parent_method() {
        let php = r#"<?php
abstract class Base {
    abstract public function doSomething(): void;
}

trait DoesIt {
    public function doSomething(): void {}
}

class Child extends Base {
    use DoesIt;
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Trait methods should satisfy abstract parent requirements, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn nested_trait_satisfies_interface() {
        let php = r#"<?php
interface HasLabel {
    public function label(): string;
}

trait InnerTrait {
    public function label(): string { return 'hi'; }
}

trait OuterTrait {
    use InnerTrait;
}

class Widget implements HasLabel {
    use OuterTrait;
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Nested trait methods should satisfy interface requirements, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn parent_trait_satisfies_interface() {
        let php = r#"<?php
interface Serializable {
    public function toArray(): array;
    public function toJson(): string;
}

trait SerializableTrait {
    public function toArray(): array { return []; }
    public function toJson(): string { return '{}'; }
}

class Base {
    use SerializableTrait;
}

class Child extends Base implements Serializable {
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Parent class trait methods should satisfy child interface requirements, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn trait_with_abstract_method_does_not_satisfy() {
        let php = r#"<?php
interface Foo {
    public function bar(): void;
}

trait HalfImpl {
    abstract public function bar(): void;
}

class Baz implements Foo {
    use HalfImpl;
}
"#;
        let diags = collect(php);
        assert_eq!(
            diags.len(),
            1,
            "Abstract trait methods should not satisfy interface requirements"
        );
        assert!(message_text(&diags[0].message).contains("bar()"));
    }

    #[test]
    fn cyclic_interface_hierarchy_does_not_stack_overflow() {
        // interface A extends B, interface B extends A — user error, but
        // should not crash the LSP server.
        let php = r#"<?php
interface A extends B { public function foo(): void; }
interface B extends A { public function bar(): void; }
class C implements A {
    public function foo(): void {}
    public function bar(): void {}
}
"#;
        let diags = collect(php);
        // We only care that it doesn't hang or crash.  Whether a
        // diagnostic is emitted is secondary.
        let _ = diags;
    }

    #[test]
    fn cyclic_parent_class_does_not_stack_overflow() {
        // class A extends B, class B extends A — user error.
        let php = r#"<?php
interface I { public function work(): void; }
class A extends B implements I {}
class B extends A {}
"#;
        let diags = collect(php);
        let _ = diags;
    }

    #[test]
    fn diagnostic_range_covers_class_name() {
        let php = r#"<?php
interface Foo { public function bar(): void; }
class MyClass implements Foo {}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1);
        let range = diags[0].range;
        // The range should cover "MyClass" — verify it is on the correct line.
        let class_line = php[..php.find("MyClass").unwrap()]
            .chars()
            .filter(|&c| c == '\n')
            .count() as u32;
        assert_eq!(range.start.line, class_line);
    }

    #[test]
    fn no_diagnostic_for_backed_enum_implicit_interface() {
        // PHP backed enums automatically implement BackedEnum (which
        // extends UnitEnum).  The parser adds these as implicit
        // interfaces, but the implementation checker must skip them
        // because PHP provides from(), tryFrom(), and cases()
        // automatically at runtime.
        let php = r#"<?php
interface UnitEnum {
    public static function cases(): array;
}
interface BackedEnum extends UnitEnum {
    public static function from(int|string $value): static;
    public static function tryFrom(int|string $value): ?static;
}

enum Status: string {
    case Active = 'active';
    case Inactive = 'inactive';
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Backed enum should not require explicit BackedEnum/UnitEnum implementations, got: {diags:?}"
        );
    }

    #[test]
    fn no_diagnostic_for_unit_enum_implicit_interface() {
        // PHP enums without a backing type automatically implement
        // UnitEnum.  Same as above — cases() is provided by PHP.
        let php = r#"<?php
interface UnitEnum {
    public static function cases(): array;
}

enum Suit {
    case Hearts;
    case Diamonds;
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Unit enum should not require explicit UnitEnum implementation, got: {diags:?}"
        );
    }

    #[test]
    fn diagnostic_for_enum_missing_own_interface_method() {
        // Enums that explicitly implement a user-defined interface
        // must still satisfy those methods — only the implicit
        // BackedEnum/UnitEnum interfaces are exempt.
        let php = r#"<?php
interface Labelable {
    public function label(): string;
}

enum Color: string implements Labelable {
    case Red = 'red';
}
"#;
        let diags = collect(php);
        assert_eq!(
            diags.len(),
            1,
            "Enum should still flag missing methods from user-defined interfaces, got: {diags:?}"
        );
        assert!(
            message_text(&diags[0].message).contains("label"),
            "Diagnostic should mention the missing 'label' method"
        );
    }

    #[test]
    fn diagnostic_for_unimplemented_trait_abstract_method() {
        let php = r#"<?php
trait Foo {
    abstract protected function foo(): void;
}

class Bar {
    use Foo;
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1, "Expected 1 diagnostic, got: {diags:?}");
        assert!(message_text(&diags[0].message).contains("foo()"));
        assert!(message_text(&diags[0].message).contains("trait"));
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::Error));
    }

    #[test]
    fn no_diagnostic_when_trait_abstract_method_implemented() {
        let php = r#"<?php
trait Foo {
    abstract protected function foo(): void;
}

class Bar {
    use Foo;
    protected function foo(): void {}
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Implemented trait abstract method should not be flagged, got: {diags:?}"
        );
    }

    #[test]
    fn no_diagnostic_for_abstract_class_with_trait_abstract() {
        let php = r#"<?php
trait Foo {
    abstract protected function foo(): void;
}

abstract class Bar {
    use Foo;
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Abstract class should not flag trait abstract methods, got: {diags:?}"
        );
    }

    #[test]
    fn diagnostic_for_multiple_trait_abstract_methods() {
        let php = r#"<?php
trait HasName {
    abstract public function name(): string;
}

trait HasAge {
    abstract public function age(): int;
}

class Person {
    use HasName;
    use HasAge;
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1, "Expected 1 diagnostic, got: {diags:?}");
        assert!(message_text(&diags[0].message).contains("name()"));
        assert!(message_text(&diags[0].message).contains("age()"));
    }

    #[test]
    fn concrete_trait_method_satisfies_own_abstract() {
        let php = r#"<?php
trait Foo {
    abstract protected function bar(): void;
    protected function baz(): void {}
}

class Qux {
    use Foo;
    protected function bar(): void {}
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "All methods are implemented, got: {diags:?}"
        );
    }

    #[test]
    fn nested_trait_abstract_method_detected() {
        let php = r#"<?php
trait Inner {
    abstract public function innerMethod(): void;
}

trait Outer {
    use Inner;
}

class Concrete {
    use Outer;
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1, "Expected 1 diagnostic, got: {diags:?}");
        assert!(message_text(&diags[0].message).contains("innerMethod()"));
    }

    /// An abstract method a parent picks up from its trait is still abstract
    /// in the parent, so the concrete child has to implement it.
    #[test]
    fn abstract_method_of_a_parents_trait_detected() {
        let php = r#"<?php
trait Greets {
    abstract public function name(): string;
}

abstract class Base {
    use Greets;
}

class Child extends Base {
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1, "Expected 1 diagnostic, got: {diags:?}");
        assert!(message_text(&diags[0].message).contains("name()"));
        assert!(
            message_text(&diags[0].message).contains("from class 'Base'"),
            "the requirement should be traced to the parent: {}",
            message_text(&diags[0].message)
        );
    }

    /// The interface a requirement comes from is named even when it extends
    /// one with the same short name in another namespace.
    #[test]
    fn requirement_through_same_named_interface_names_the_interface() {
        let php = r#"<?php
namespace Lib {
    interface Repository { public function find(): void; }
}
namespace App {
    interface Repository extends \Lib\Repository {}
    class Users implements Repository {}
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1, "Expected 1 diagnostic, got: {diags:?}");
        assert!(
            message_text(&diags[0].message).contains("from interface 'App\\Repository'"),
            "got: {}",
            message_text(&diags[0].message)
        );
    }

    /// A stored parent is the class it names, not whatever the checking
    /// file imports under the same short name.
    #[test]
    fn global_parent_shadowed_by_import() {
        let php = r#"<?php
namespace {
    abstract class Base { abstract public function run(): void; }
}
namespace Lib {
    abstract class Base { abstract public function other(): void; }
}
namespace App {
    use Lib\Base;
    class Impl extends \Base {
        public function run(): void {}
    }
}
"#;
        let diags = collect(php);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }
}
