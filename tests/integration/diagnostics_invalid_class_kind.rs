#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tower_lsp::lsp_types::*;

    use phpantom_lsp::Backend;

    fn collect(php: &str) -> Vec<Diagnostic> {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        backend.update_ast(uri, &Arc::new(php.to_string()));
        let mut out = Vec::new();
        backend.collect_invalid_class_kind_diagnostics(uri, php, &mut out);
        out
    }

    // ── new ─────────────────────────────────────────────────────────

    #[test]
    fn new_concrete_class_no_diagnostic() {
        let diags = collect(
            r#"<?php
class Foo {}
$x = new Foo();
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn new_abstract_class_error() {
        let diags = collect(
            r#"<?php
abstract class Foo {}
$x = new Foo();
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
        assert!(diags[0].message.contains("abstract"));
    }

    #[test]
    fn new_interface_error() {
        let diags = collect(
            r#"<?php
interface Foo {}
$x = new Foo();
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
        assert!(diags[0].message.contains("interface"));
    }

    #[test]
    fn new_trait_error() {
        let diags = collect(
            r#"<?php
trait Foo {}
$x = new Foo();
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
        assert!(diags[0].message.contains("trait"));
    }

    #[test]
    fn new_enum_error() {
        let diags = collect(
            r#"<?php
enum Color { case Red; }
$x = new Color();
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
        assert!(diags[0].message.contains("enum"));
    }

    // ── extends (class) ─────────────────────────────────────────────

    #[test]
    fn class_extends_class_no_diagnostic() {
        let diags = collect(
            r#"<?php
class Base {}
class Child extends Base {}
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn class_extends_final_class_error() {
        let diags = collect(
            r#"<?php
final class Base {}
class Child extends Base {}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
        assert!(diags[0].message.contains("final"));
    }

    #[test]
    fn class_extends_interface_error() {
        let diags = collect(
            r#"<?php
interface Iface {}
class Child extends Iface {}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
        assert!(diags[0].message.contains("interface"));
    }

    #[test]
    fn class_extends_trait_error() {
        let diags = collect(
            r#"<?php
trait MyTrait {}
class Child extends MyTrait {}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
        assert!(diags[0].message.contains("trait"));
    }

    #[test]
    fn class_extends_enum_error() {
        let diags = collect(
            r#"<?php
enum Color { case Red; }
class Child extends Color {}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
        assert!(diags[0].message.contains("enum"));
    }

    // ── extends (interface) ─────────────────────────────────────────

    #[test]
    fn interface_extends_interface_no_diagnostic() {
        let diags = collect(
            r#"<?php
interface Base {}
interface Child extends Base {}
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn interface_extends_class_error() {
        let diags = collect(
            r#"<?php
class Foo {}
interface Child extends Foo {}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
        assert!(diags[0].message.contains("class"));
    }

    #[test]
    fn interface_extends_trait_error() {
        let diags = collect(
            r#"<?php
trait MyTrait {}
interface Child extends MyTrait {}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
        assert!(diags[0].message.contains("trait"));
    }

    // ── implements ──────────────────────────────────────────────────

    #[test]
    fn class_implements_interface_no_diagnostic() {
        let diags = collect(
            r#"<?php
interface Iface {}
class Foo implements Iface {}
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn class_implements_class_error() {
        let diags = collect(
            r#"<?php
class Base {}
class Foo implements Base {}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
        assert!(diags[0].message.contains("class"));
    }

    #[test]
    fn class_implements_trait_error() {
        let diags = collect(
            r#"<?php
trait MyTrait {}
class Foo implements MyTrait {}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
        assert!(diags[0].message.contains("trait"));
    }

    #[test]
    fn enum_implements_interface_no_diagnostic() {
        let diags = collect(
            r#"<?php
interface Iface {}
enum Color implements Iface { case Red; }
"#,
        );
        assert!(diags.is_empty());
    }

    // ── trait use ───────────────────────────────────────────────────

    #[test]
    fn use_trait_no_diagnostic() {
        let diags = collect(
            r#"<?php
trait MyTrait {}
class Foo { use MyTrait; }
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn use_class_error() {
        let diags = collect(
            r#"<?php
class Base {}
class Foo { use Base; }
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
        assert!(diags[0].message.contains("class"));
    }

    #[test]
    fn use_interface_error() {
        let diags = collect(
            r#"<?php
interface Iface {}
class Foo { use Iface; }
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
        assert!(diags[0].message.contains("interface"));
    }

    #[test]
    fn use_enum_error() {
        let diags = collect(
            r#"<?php
enum Color { case Red; }
class Foo { use Color; }
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
        assert!(diags[0].message.contains("enum"));
    }

    // ── instanceof ──────────────────────────────────────────────────

    #[test]
    fn instanceof_class_no_diagnostic() {
        let diags = collect(
            r#"<?php
class Foo {}
function test($x) { return $x instanceof Foo; }
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn instanceof_interface_no_diagnostic() {
        let diags = collect(
            r#"<?php
interface Iface {}
function test($x) { return $x instanceof Iface; }
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn instanceof_trait_warning() {
        let diags = collect(
            r#"<?php
trait MyTrait {}
function test($x) { return $x instanceof MyTrait; }
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::WARNING));
        assert!(diags[0].message.contains("false"));
    }

    // ── catch ───────────────────────────────────────────────────────

    #[test]
    fn catch_exception_no_diagnostic() {
        let diags = collect(
            r#"<?php
class MyException extends \Exception {}
function test() {
    try {} catch (MyException $e) {}
}
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn catch_trait_warning() {
        let diags = collect(
            r#"<?php
trait MyTrait {}
function test() {
    try {} catch (MyTrait $e) {}
}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::WARNING));
        assert!(diags[0].message.contains("Trait"));
    }

    #[test]
    fn catch_enum_error() {
        let diags = collect(
            r#"<?php
enum Color { case Red; }
function test() {
    try {} catch (Color $e) {}
}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
        assert!(diags[0].message.contains("Enum"));
    }

    #[test]
    fn catch_non_throwable_class_error() {
        let diags = collect(
            r#"<?php
class NotAnException {}
function test() {
    try {} catch (NotAnException $e) {}
}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
        assert!(diags[0].message.contains("Throwable"));
    }

    // ── type hints ──────────────────────────────────────────────────

    #[test]
    fn type_hint_class_no_diagnostic() {
        let diags = collect(
            r#"<?php
class Foo {}
function test(Foo $x): Foo { return $x; }
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn type_hint_interface_no_diagnostic() {
        let diags = collect(
            r#"<?php
interface Iface {}
function test(Iface $x): Iface { return $x; }
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn type_hint_trait_warning() {
        let diags = collect(
            r#"<?php
trait MyTrait {}
function test(MyTrait $x): MyTrait { return $x; }
"#,
        );
        // One for parameter type, one for return type.
        assert_eq!(diags.len(), 2);
        for d in &diags {
            assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
            assert!(d.message.contains("trait") || d.message.contains("Trait"));
        }
    }

    #[test]
    fn property_type_trait_warning() {
        let diags = collect(
            r#"<?php
trait MyTrait {}
class Foo {
    public MyTrait $prop;
}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::WARNING));
    }

    // ── diagnostic metadata ─────────────────────────────────────────

    #[test]
    fn diagnostic_has_code_and_source() {
        let diags = collect(
            r#"<?php
interface Iface {}
class Foo extends Iface {}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].code,
            Some(NumberOrString::String("invalid_class_kind".to_string()))
        );
        assert_eq!(diags[0].source, Some("phpantom".to_string()));
    }

    #[test]
    fn diagnostic_range_covers_class_name() {
        let php = r#"<?php
abstract class Abs {}
$x = new Abs();
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1);
        // "Abs" on line 2 (0-indexed): `$x = new Abs();`
        // Column of "Abs" is 9.
        assert_eq!(diags[0].range.start.line, 2);
        assert_eq!(diags[0].range.start.character, 9);
        assert_eq!(diags[0].range.end.line, 2);
        assert_eq!(diags[0].range.end.character, 12);
    }

    // ── no false positives ──────────────────────────────────────────

    #[test]
    fn no_diagnostic_for_unknown_class() {
        // Unknown classes should not be flagged — that's the
        // unknown-class diagnostic's job.
        let diags = collect(
            r#"<?php
$x = new UnknownClass();
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn no_diagnostic_for_self_static_parent() {
        // self, static, parent are not ClassReference spans.
        let diags = collect(
            r#"<?php
class Foo {
    public function test(): static { return new self(); }
}
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn class_extends_abstract_class_no_diagnostic() {
        // Abstract classes CAN be extended — only instantiation is forbidden.
        let diags = collect(
            r#"<?php
abstract class Base {}
class Child extends Base {}
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn catch_throwable_interface_no_diagnostic() {
        // Direct use of Throwable interface in catch is valid.
        let diags = collect(
            r#"<?php
function test() {
    try {} catch (\Throwable $e) {}
}
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn catch_exception_subclass_no_diagnostic() {
        let diags = collect(
            r#"<?php
class AppError extends \RuntimeException {}
function test() {
    try {} catch (AppError $e) {}
}
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn nullable_trait_type_hint_warning() {
        let diags = collect(
            r#"<?php
trait MyTrait {}
function test(?MyTrait $x) {}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::WARNING));
    }

    #[test]
    fn union_type_hint_with_trait_warning() {
        let diags = collect(
            r#"<?php
trait MyTrait {}
class Foo {}
function test(Foo|MyTrait $x) {}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::WARNING));
        assert!(diags[0].message.contains("MyTrait"));
    }
}
