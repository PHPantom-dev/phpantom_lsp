//! What a branch knows about a value from the test that led into it: the
//! `case` labels of a `switch`, the falsy side of a truthiness check, an
//! `instanceof` check a union can pass through a subclass, the tag of a
//! union of array shapes, and `is_callable()` on a class alternative.
//!
//! Each case hovers the variable at the `$x; // here` line and compares
//! the type the hover reports.

use crate::common::{create_test_backend, hover_at, hover_text};

/// The type hover reports for the variable on the line marked `// here`.
fn type_at_marker(php: &str) -> String {
    let backend = create_test_backend();
    let (line, text) = php
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("// here"))
        .expect("the source marks a line with `// here`");
    let column = text.find('$').expect("the marked line reads a variable") as u32 + 1;
    let hover = hover_at(&backend, "file:///test.php", php, line as u32, column)
        .expect("the variable hovers");
    let text = hover_text(&hover);

    text.lines()
        .find_map(|l| l.split_once(" = ").map(|(_, ty)| ty.trim().to_string()))
        .unwrap_or_else(|| panic!("no type in hover: {text}"))
}

// ─── switch ─────────────────────────────────────────────────────────────────

#[test]
fn a_group_of_case_labels_keeps_each_value_they_name() {
    let php = r#"<?php
/** @param 'a'|'b'|'c' $s */
function f(string $s): void {
    switch ($s) {
        case 'a':
        case 'b':
            $s; // here
            break;
    }
}
"#;
    assert_eq!(type_at_marker(php), "'a'|'b'");
}

#[test]
fn the_default_arm_drops_every_value_a_label_names() {
    let php = r#"<?php
/** @param 'a'|'b'|'c' $s */
function f(string $s): void {
    switch ($s) {
        case 'a':
            break;
        default:
            $s; // here
            break;
        case 'b':
            break;
    }
}
"#;
    assert_eq!(type_at_marker(php), "'c'");
}

#[test]
fn a_case_label_compares_loosely() {
    let php = r#"<?php
/** @param 1|2|'x' $s */
function f($s): void {
    switch ($s) {
        case '1':
            $s; // here
            break;
    }
}
"#;
    assert_eq!(type_at_marker(php), "1");
}

#[test]
fn an_arm_the_previous_one_falls_into_is_not_narrowed() {
    let php = r#"<?php
/** @param 'a'|'b' $s */
function f(string $s): void {
    switch ($s) {
        case 'a':
            echo 'a';
        case 'b':
            $s; // here
            break;
    }
}
"#;
    assert_eq!(type_at_marker(php), "'a'|'b'");
}

#[test]
fn a_case_label_that_is_not_a_literal_leaves_the_subject_alone() {
    let php = r#"<?php
/** @param 'a'|'b' $s */
function f(string $s, string $other): void {
    switch ($s) {
        case $other:
            $s; // here
            break;
    }
}
"#;
    assert_eq!(type_at_marker(php), "'a'|'b'");
}

// ─── Falsy branch ───────────────────────────────────────────────────────────

#[test]
fn a_negated_truthiness_check_leaves_only_the_falsy_part() {
    let php = r#"<?php
class Foo {}
/** @param Foo|null $a */
function f($a): void {
    if (!$a) {
        $a; // here
    }
}
"#;
    assert_eq!(type_at_marker(php), "null");
}

#[test]
fn empty_leaves_only_the_falsy_part() {
    let php = r#"<?php
class Foo {}
/** @param Foo|false $a */
function f($a): void {
    if (empty($a)) {
        $a; // here
    }
}
"#;
    assert_eq!(type_at_marker(php), "false");
}

#[test]
fn a_class_string_is_never_falsy() {
    let php = r#"<?php
class Foo {}
/** @param class-string<Foo>|false $c */
function f($c): void {
    if ($c) {
    } else {
        $c; // here
    }
}
"#;
    assert_eq!(type_at_marker(php), "false");
}

// ─── instanceof through a subclass ──────────────────────────────────────────

#[test]
fn a_final_class_does_not_gain_an_interface_it_lacks() {
    let php = r#"<?php
final class Sealed {}
interface Tagged {}
/** @param Sealed|Tagged $x */
function f($x): void {
    if ($x instanceof Tagged) {
        $x; // here
    }
}
"#;
    assert_eq!(type_at_marker(php), "Tagged");
}

#[test]
fn a_subclass_of_the_checked_interface_narrows_its_half_to_the_class() {
    let php = r#"<?php
interface Animal {}
class Dog implements Animal {}
class Cat {}
/** @param Cat|Animal $x */
function f($x): void {
    if ($x instanceof Dog) {
        $x; // here
    }
}
"#;
    assert_eq!(type_at_marker(php), "Dog");
}

// ─── tagged unions of shapes ────────────────────────────────────────────────

#[test]
fn a_loose_tag_comparison_keeps_the_matching_shape() {
    let php = r#"<?php
/** @param array{type: 'a', a: string}|array{type: 'b', b: string} $in */
function f(array $in): void {
    if ($in['type'] == 'a') {
        $in; // here
    }
}
"#;
    assert_eq!(type_at_marker(php), "array{type: 'a', a: string}");
}

#[test]
fn the_default_arm_of_a_tag_switch_drops_every_named_shape() {
    let php = r#"<?php
/** @param array{type: 'a', a: string}|array{type: 'b', b: int}|array{type: 'c', c: bool} $in */
function f(array $in): void {
    switch ($in['type']) {
        case 'a':
        case 'b':
            break;
        default:
            $in; // here
    }
}
"#;
    assert_eq!(type_at_marker(php), "array{type: 'c', c: bool}");
}

#[test]
fn a_tag_whose_entry_is_not_a_literal_keeps_the_shape() {
    let php = r#"<?php
/** @param array{type: 'a', a: string}|array{type: string, other: int} $in */
function f(array $in): void {
    if ($in['type'] === 'a') {
        $in; // here
    }
}
"#;
    assert_eq!(
        type_at_marker(php),
        "array{type: 'a', a: string}|array{type: 'a', other: int}"
    );
}

// ─── is_callable() ──────────────────────────────────────────────────────────

#[test]
fn is_callable_keeps_a_class_that_declares_invoke() {
    let php = r#"<?php
class Handler { public function __invoke(): int { return 1; } }
final class Plain {}
/** @param Handler|Plain|string $h */
function f($h): void {
    if (is_callable($h)) {
        $h; // here
    }
}
"#;
    assert_eq!(type_at_marker(php), "Handler");
}

#[test]
fn is_callable_finds_invoke_on_a_parent() {
    let php = r#"<?php
abstract class Base { public function __invoke(): int { return 1; } }
final class Child extends Base {}
/** @param Child|int $h */
function f($h): void {
    if (is_callable($h)) {
        $h; // here
    }
}
"#;
    assert_eq!(type_at_marker(php), "Child");
}
