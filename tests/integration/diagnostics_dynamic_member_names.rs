use std::collections::HashMap;

use crate::common::{collect_diagnostics_with, message_text};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

fn collect_full(php: &str) -> Vec<Diagnostic> {
    let stubs = HashMap::from([
        ("stdClass", "<?php\nclass stdClass {}\n"),
        (
            "Stringable",
            "<?php\ninterface Stringable { public function __toString(): string; }\n",
        ),
    ]);
    collect_diagnostics_with(
        &Backend::new_test_with_stubs(stubs),
        php,
        Backend::collect_dynamic_member_name_diagnostics,
    )
}

fn collect(php: &str) -> Vec<String> {
    collect_full(php)
        .iter()
        .map(|d| {
            assert_eq!(
                d.code,
                Some(Code::String("invalid_member_name".to_string()))
            );
            assert_eq!(d.severity, Some(DiagnosticSeverity::Error));
            message_text(&d.message).to_string()
        })
        .collect()
}

#[test]
fn stringable_names_a_property_but_not_a_method() {
    let php = r#"<?php
class Target {}

function f(\Stringable $s, \stdClass $o, Target $t): void {
    $t->{$s};
    $t->{$o};
    $t->{$s}();
    $t->{$o}();
}
"#;
    assert_eq!(
        collect(php),
        vec![
            "Property name must be a string, but stdClass was given",
            "Method name must be a string, but Stringable was given",
            "Method name must be a string, but stdClass was given",
        ]
    );
}

#[test]
fn scalars_name_a_property() {
    let php = r#"<?php
class Target {}

function f(Target $t, int $i, float $f, bool $b, ?string $n, string $s): void {
    $t->{$i};
    $t->{$f};
    $t->{$b};
    $t->{$n};
    $t->{$s};
    $t->{'literal'};
    $t->{$s}();
    $t->{'literal'}();
}
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn only_a_string_names_a_method() {
    let php = r#"<?php
class Target {}

function f(Target $t, int $i, ?int $n, array $a, \Closure $c): void {
    $t->{$i}();
    $t->{$n}();
    $t->{$a}();
    $t->{$c}();
}
"#;
    assert_eq!(
        collect(php),
        vec![
            "Method name must be a string, but int was given",
            "Method name must be a string, but ?int was given",
            "Method name must be a string, but array was given",
            "Method name must be a string, but Closure was given",
        ]
    );
}

#[test]
fn arrays_closures_and_enums_do_not_name_a_property() {
    let php = r#"<?php
class Target {}
enum Suit: string { case Hearts = 'h'; }

function f(Target $t, array $a, \Closure $c, Suit $e): void {
    $t->{$a};
    $t->{$c};
    $t->{$e};
}
"#;
    assert_eq!(
        collect(php),
        vec![
            "Property name must be a string, but array was given",
            "Property name must be a string, but Closure was given",
            "Property name must be a string, but Suit was given",
        ]
    );
}

#[test]
fn a_union_with_one_valid_branch_is_not_reported() {
    let php = r#"<?php
class Target {}

/** @param string|\stdClass $either */
function f(Target $t, $either, int|bool $neither): void {
    $t->{$either};
    $t->{$either}();
    $t->{$neither}();
}
"#;
    assert_eq!(
        collect(php),
        vec!["Method name must be a string, but int|bool was given"]
    );
}

#[test]
fn unknown_selector_types_are_not_reported() {
    let php = r#"<?php
class Target {}

/**
 * @template T
 * @param T $templated
 */
function f(Target $t, mixed $m, $untyped, object $obj, $templated, \Missing $missing): void {
    $t->{$m};
    $t->{$m}();
    $t->{$untyped};
    $t->{$untyped}();
    $t->{$obj};
    $t->{$templated};
    $t->{$templated}();
    $t->{$missing};
}
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn a_class_converts_through_its_own_or_inherited_to_string() {
    let php = r#"<?php
namespace App\Names;

class Target {}
class Label { public function __toString(): string { return 'a'; } }
class SubLabel extends Label {}
trait Spells { public function __toString(): string { return 'b'; } }
class TraitLabel { use Spells; }
interface Named extends \Stringable {}
class Plain {}

function f(Target $t, Label $l, SubLabel $s, TraitLabel $tl, Named $n, Plain $p): void {
    $t->{$l};
    $t->{$s};
    $t->{$tl};
    $t->{$n};
    $t->{$p};
}
"#;
    assert_eq!(
        collect(php),
        vec!["Property name must be a string, but App\\Names\\Plain was given"]
    );
}

#[test]
fn every_dynamic_selector_form_is_checked() {
    let php = r#"<?php
class Target {}

function f(?Target $t, \stdClass $o, \Stringable $s): void {
    $t->$o;
    $t?->{$o};
    $t?->{$s}();
    Target::{$s}();
    Target::$s();
    $t->{$s}(...);
    Target::{$s}(...);
}
"#;
    assert_eq!(collect(php).len(), 7);
}

#[test]
fn the_selector_inside_a_method_and_closure_is_checked() {
    let php = r#"<?php
class Target
{
    public function run(\stdClass $o): void
    {
        $this->{$o};
        $fn = function (int $i) { return $this->{$i}(); };
    }
}
"#;
    assert_eq!(
        collect(php),
        vec![
            "Property name must be a string, but stdClass was given",
            "Method name must be a string, but int was given",
        ]
    );
}

#[test]
fn a_narrowed_selector_is_read_at_the_access() {
    let php = r#"<?php
class Target {}

function f(Target $t, int|string $name): void {
    if (is_string($name)) {
        $t->{$name}();
    }
}
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn diagnostic_covers_the_selector() {
    let php = "<?php\nfunction f(\\stdClass $t, \\stdClass $o): void {\n    $t->{$o};\n}\n";
    let diagnostics = collect_full(php);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].range.start, Position::new(2, 8));
    assert_eq!(diagnostics[0].range.end, Position::new(2, 12));
}
