use crate::common::collect_diagnostics_with;
use crate::common::message_text;
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

fn collect(php: &str) -> Vec<Diagnostic> {
    collect_diagnostics_with(
        &Backend::new_test(),
        php,
        Backend::collect_trait_property_conflict_diagnostics,
    )
}

fn messages(php: &str) -> Vec<String> {
    collect(php)
        .into_iter()
        .map(|d| message_text(&d.message).to_string())
        .collect()
}

#[test]
fn different_types_from_two_traits() {
    let php = r#"<?php
trait Left  { public string $prop; }
trait Right { public int $prop; }
final class Composed { use Left; use Right; }
"#;
    let diags = collect(php);
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].severity, Some(DiagnosticSeverity::Error));
    assert_eq!(
        diags[0].code,
        Some(Code::String("incompatible_trait_property".to_string()))
    );
    assert_eq!(
        message_text(&diags[0].message),
        "Left and Right define the same property ($prop) in the composition of Composed, but the definitions are incompatible"
    );
    assert_eq!(diags[0].range.start.line, 3);
}

#[test]
fn identical_declarations_are_allowed() {
    let php = r#"<?php
trait Left  { public ?int $prop = null; protected static $count = 0; }
trait Right { public int|null $prop = NULL; protected static $count = 0x0; }
class Composed { use Left, Right; }
"#;
    assert!(messages(php).is_empty());
}

#[test]
fn class_body_conflicts_with_trait() {
    let php = r#"<?php
trait HasName { public string $name = 'a'; }
class Person {
    use HasName;
    public string $name = 'b';
}
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(msgs[0].starts_with("Person and HasName"), "{msgs:?}");
}

#[test]
fn visibility_static_and_readonly_must_match() {
    let php = r#"<?php
trait A { public int $v; protected int $w; public static int $x = 1; public readonly int $y; }
trait B { protected int $v; protected int $w; public int $x = 1; public int $y; }
class C { use A, B; }
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 3, "{msgs:?}");
    assert!(msgs.iter().any(|m| m.contains("($v)")));
    assert!(msgs.iter().any(|m| m.contains("($x)")));
    assert!(msgs.iter().any(|m| m.contains("($y)")));
}

#[test]
fn default_presence_and_value_must_match() {
    let php = r#"<?php
trait A { public int $typed; public $untyped; public int $n = 1; }
trait B { public int $typed = 0; public $untyped = null; public int $n = 2; }
class C { use A, B; }
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 2, "{msgs:?}");
    assert!(msgs.iter().any(|m| m.contains("($typed)")));
    assert!(msgs.iter().any(|m| m.contains("($n)")));
}

#[test]
fn constant_expression_defaults_are_not_compared() {
    let php = r#"<?php
trait A { public int $n = self::ONE; }
trait B { public int $n = 1; }
class C { const ONE = 1; use A, B; }
"#;
    assert!(messages(php).is_empty());
}

#[test]
fn nested_trait_properties_are_seen() {
    let php = r#"<?php
trait Inner { public string $prop; }
trait Outer { use Inner; }
trait Other { public int $prop; }
class C { use Outer, Other; }
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(msgs[0].starts_with("Outer and Other"), "{msgs:?}");
}

#[test]
fn trait_composing_conflicting_traits_is_reported() {
    let php = r#"<?php
trait Left  { public string $prop; }
trait Right { public int $prop; }
trait Both { use Left, Right; }
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(msgs[0].contains("composition of Both"), "{msgs:?}");
}

#[test]
fn distinct_property_names_are_fine() {
    let php = r#"<?php
trait Left  { public string $a; }
trait Right { public int $b; }
class C { use Left, Right; public bool $c; }
"#;
    assert!(messages(php).is_empty());
}
