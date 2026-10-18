use crate::common::collect_diagnostics_with;
use phpantom_lsp::Backend;
use tower_lsp::lsp_types::*;

fn collect(php: &str) -> Vec<Diagnostic> {
    collect_diagnostics_with(
        &Backend::new_test(),
        php,
        Backend::collect_destructuring_offset_diagnostics,
    )
}

fn messages(php: &str) -> Vec<String> {
    collect(php).into_iter().map(|d| d.message).collect()
}

#[test]
fn positional_destructure_of_string_keyed_array() {
    let php = r#"<?php
/** @return array<string, int> */
function stringKeyed(): array { return ['a' => 1]; }

function demo(): void {
    [$a, $b] = stringKeyed();
}
"#;
    let diags = collect(php);
    assert_eq!(diags.len(), 2, "{diags:?}");
    assert_eq!(
        diags[0].message,
        "Offset 0 does not exist on array<string, int>"
    );
    assert_eq!(
        diags[1].message,
        "Offset 1 does not exist on array<string, int>"
    );
    assert_eq!(diags[0].severity, Some(DiagnosticSeverity::WARNING));
    assert_eq!(
        diags[0].code,
        Some(NumberOrString::String("unknown_offset".to_string()))
    );
    assert_eq!(diags[0].range.start, Position::new(5, 5));
    assert_eq!(diags[0].range.end, Position::new(5, 7));
}

#[test]
fn keyed_destructure_of_shape_without_the_key() {
    let php = r#"<?php
/** @param array{id: int, name: string} $row */
function demo(array $row): void {
    ['id' => $id, 'email' => $email] = $row;
}
"#;
    assert_eq!(
        messages(php),
        vec!["Offset 'email' does not exist on array{id: int, name: string}"]
    );
}

#[test]
fn list_destructure_past_the_end_of_a_list_shape() {
    let php = r#"<?php
/** @param array{int, string} $pair */
function demo(array $pair): void {
    list($a, $b, $c) = $pair;
}
"#;
    assert_eq!(
        messages(php),
        vec!["Offset 2 does not exist on array{int, string}"]
    );
}

#[test]
fn string_key_of_a_list() {
    let php = r#"<?php
/** @param list<int> $items */
function demo(array $items): void {
    ['first' => $first] = $items;
}
"#;
    assert_eq!(
        messages(php),
        vec!["Offset 'first' does not exist on list<int>"]
    );
}

#[test]
fn nested_pattern_is_checked() {
    let php = r#"<?php
/** @param list<array{x: int}> $points */
function demo(array $points): void {
    [['x' => $x, 'y' => $y]] = $points;
}
"#;
    assert_eq!(
        messages(php),
        vec!["Offset 'y' does not exist on array{x: int}"]
    );
}

#[test]
fn keys_the_array_may_hold_are_not_reported() {
    let php = r#"<?php
/**
 * @param array<string, int> $map
 * @param list<int> $list
 * @param int[] $ints
 * @param array{a: int, b?: int} $shape
 * @param array{int, string} $pair
 * @param array<numeric-string, int> $numeric
 */
function demo(array $map, array $list, array $ints, array $shape, array $pair, array $numeric, array $plain, $untyped): void {
    ['a' => $a] = $map;
    [$first, $second] = $list;
    [$i] = $ints;
    ['a' => $sa, 'b' => $sb] = $shape;
    [$p, $q] = $pair;
    [0 => $p0, '1' => $p1] = $pair;
    [$n] = $numeric;
    [$x, 'k' => $y] = $plain;
    [$u] = $untyped;
    [, $skipped] = $pair;
}
"#;
    let diags = collect(php);
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn union_reports_only_when_no_member_has_the_key() {
    let php = r#"<?php
/**
 * @param array{a: int}|array{b: int} $either
 * @param array{a: int}|null $maybe
 */
function demo(array $either, ?array $maybe): void {
    ['b' => $b] = $either;
    ['c' => $c] = $either;
    ['z' => $z] = $maybe;
}
"#;
    assert_eq!(
        messages(php),
        vec![
            "Offset 'c' does not exist on array{a: int}|array{b: int}",
            "Offset 'z' does not exist on array{a: int}|null",
        ]
    );
}

#[test]
fn inline_var_docblock_types_the_destructure() {
    let php = r#"<?php
function demo(): void {
    /** @var array<string, int> */
    [$a] = load();
}
"#;
    assert_eq!(
        messages(php),
        vec!["Offset 0 does not exist on array<string, int>"]
    );
}

#[test]
fn inline_var_docblock_naming_the_rhs_types_the_destructure() {
    let php = r#"<?php
function demo($data): void {
    /** @var array<string, int> $data */
    [$a] = $data;
}
"#;
    assert_eq!(
        messages(php),
        vec!["Offset 0 does not exist on array<string, int>"]
    );
}

#[test]
fn inline_var_docblock_naming_destructured_variables_leaves_the_rhs_alone() {
    let php = r#"<?php
class Scope {}

/** @return array{array<string, Scope>, array<string, string>} */
function cached(): array { return [[], []]; }

function demo(): void {
    $cached = cached();
    /**
     * @var array<string, Scope> $map
     * @var array<string, string> $hashes
     */
    [$map, $hashes] = $cached;
}
"#;
    assert!(messages(php).is_empty(), "{:?}", messages(php));
}

#[test]
fn literal_array_rhs() {
    let php = r#"<?php
function demo(): void {
    ['a' => $a, 'b' => $b] = ['a' => 1];
}
"#;
    assert_eq!(messages(php).len(), 1, "{:?}", messages(php));
}
