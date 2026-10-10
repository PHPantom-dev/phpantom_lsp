//! An expression is evaluated in order, and each part of it sees what the
//! parts before it wrote: the right operand of `($a = g()) && $a->bar()`
//! reads the `$a` the left one assigned, the second item of `[$d++, $d]`
//! reads the incremented `$d`, and the first item of `[$b, $b = 1]` reads
//! the `$b` the second one has not replaced yet.

use crate::common::{create_test_backend, hover_at, hover_text, slow_diagnostic_messages};

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

fn unknown_members(php: &str) -> Vec<String> {
    slow_diagnostic_messages(
        &create_test_backend(),
        "file:///evaluation_order.php",
        php,
        "unknown_member",
    )
}

const FOO: &str = r#"<?php
class Foo { public function bar(): bool { return true; } }
function g(): ?Foo { return null; }
"#;

// ─── A write on the left of `&&` / `||` ─────────────────────────────────────

#[test]
fn the_right_operand_reads_what_the_left_one_assigned() {
    let cases = [
        ("if condition", "if (($a = g()) && $a->nope()) {}"),
        (
            "compared against null",
            "if (($a = g()) !== null && $a->nope()) {}",
        ),
        ("assignment", "$ok = ($a = g()) && $a->nope(); echo $ok;"),
        ("expression statement", "($a = g()) && $a->nope();"),
        ("while condition", "while (($a = g()) && $a->nope()) {}"),
        ("`||`", "if (!($a = g()) || $a->nope()) {}"),
        ("later operand", "if (true && ($a = g()) && $a->nope()) {}"),
        (
            "elseif condition",
            "if (rand()) {} elseif (($a = g()) && $a->nope()) {}",
        ),
    ];
    for (position, statement) in cases {
        let php = format!("{FOO}function t(): void {{ {statement} }}\n");
        let diagnostics = unknown_members(&php);
        assert!(
            diagnostics.iter().any(|d| d.contains("'nope'")),
            "{position}: expected `nope` to be reported on `Foo`, got {diagnostics:?}"
        );
    }
}

#[test]
fn the_right_operand_does_not_read_the_value_the_left_one_replaced() {
    let php = format!("{FOO}function t(Foo $a): void {{ if (($a = 1) && $a->bar()) {{}} }}\n");
    let diagnostics = slow_diagnostic_messages(
        &create_test_backend(),
        "file:///evaluation_order.php",
        &php,
        "scalar_member_access",
    );
    assert!(
        diagnostics.iter().any(|d| d.contains("on type '1'")),
        "`$a` is `1` on the right, not the `Foo` it replaced: {diagnostics:?}"
    );
}

#[test]
fn an_item_reads_what_an_earlier_item_assigned() {
    let php =
        format!("{FOO}function t(): void {{ $x = [$b = new Foo(), $b->nope()]; echo $x; }}\n");
    let diagnostics = unknown_members(&php);
    assert!(
        diagnostics.iter().any(|d| d.contains("'nope'")),
        "{diagnostics:?}"
    );
}

// ─── What an operand proved, and what came after it ─────────────────────────

#[test]
fn a_later_operand_s_write_replaces_an_earlier_operand_s_narrowing() {
    let php = r#"<?php
class Foo {}
class Bar {}
function make(): Bar { return new Bar(); }
function t(mixed $x): void {
    if ($x instanceof Foo && ($x = make())) {
        $x; // here
    }
}
"#;
    assert_eq!(type_at_marker(php), "Bar");
}

#[test]
fn a_call_that_changes_its_receiver_drops_what_an_earlier_operand_proved() {
    let php = r#"<?php
class Foo {}
function needFoo(Foo $f): bool { return true; }
class Holder {
    public ?Foo $a = null;
    /** @phpstan-impure */
    public function reset(): bool { $this->a = null; return true; }
    public function kept(): void {
        if ($this->a !== null && needFoo($this->a)) {}
    }
    public function dropped(): void {
        if ($this->a !== null && $this->reset() && needFoo($this->a)) {}
    }
}
"#;
    let diagnostics = slow_diagnostic_messages(
        &create_test_backend(),
        "file:///evaluation_order.php",
        php,
        "type_mismatch_argument",
    );
    assert_eq!(
        diagnostics.len(),
        1,
        "only the operand after `reset()` reads the nullable property: {diagnostics:?}"
    );
    assert!(diagnostics[0].contains("?Foo"), "{diagnostics:?}");
}

#[test]
fn a_ternary_arm_reads_what_an_earlier_item_assigned() {
    let php = r#"<?php
function needInt(int $i): void {}
function t(): void {
    $x = 'a';
    $r = [$x = 1, $x > 0 ? needInt($x) : 0];
    echo count($r);
}
"#;
    let diagnostics = slow_diagnostic_messages(
        &create_test_backend(),
        "file:///evaluation_order.php",
        php,
        "type_mismatch_argument",
    );
    assert!(
        diagnostics.is_empty(),
        "the arm reads the `1` the first item wrote, not the `'a'` before it: {diagnostics:?}"
    );
}

#[test]
fn a_call_nested_in_an_expression_writes_through_its_reference_parameters() {
    let statement = r#"<?php
function t(string $s): void {
    preg_match('/^(\d+)$/', $s, $m);
    $m; // here
}
"#;
    let nested = r#"<?php
function t(string $s): void {
    $found = [preg_match('/^(\d+)$/', $s, $m), 1];
    $m; // here
}
"#;
    assert_eq!(type_at_marker(nested), type_at_marker(statement));
}

#[test]
fn what_an_operand_proved_stays_inside_its_condition() {
    let php = r#"<?php
class Foo {}
function needFoo(Foo $f): bool { return true; }
function both(bool $a, bool $b): void {}
function t(?Foo $a, bool $x): void {
    both($a instanceof Foo && $x, needFoo($a));
}
"#;
    let diagnostics = slow_diagnostic_messages(
        &create_test_backend(),
        "file:///evaluation_order.php",
        php,
        "type_mismatch_argument",
    );
    assert_eq!(
        diagnostics.len(),
        1,
        "the second argument reads `$a` unnarrowed: {diagnostics:?}"
    );
}

#[test]
fn the_value_a_var_docblock_retypes_is_read_as_it_was() {
    for docblock in ["/** @var Inner */", "/** @var Inner $node */"] {
        let php = format!(
            r#"<?php
class Inner {{}}
class Outer {{ public Inner $inner; }}
function t(Outer $node): void {{
    {docblock}
    $node = $node->inner;
}}
"#
        );
        let backend = create_test_backend();
        let diagnostics = slow_diagnostic_messages(
            &backend,
            "file:///evaluation_order.php",
            &php,
            "unknown_member",
        );
        assert!(
            diagnostics.is_empty(),
            "{docblock}: `$node->inner` reads the `Outer` the write replaces: {diagnostics:?}"
        );
    }
}

// ─── `++` / `--` inside an expression ───────────────────────────────────────

#[test]
fn a_postfix_increment_yields_the_old_value_and_leaves_the_new_one() {
    let php = r#"<?php
function t(): void {
    $d = 1;
    $e = [$d++, $d];
    $e; // here
}
"#;
    assert_eq!(type_at_marker(php), "array{1, int}");
}

#[test]
fn every_step_reads_the_one_before_it() {
    let php = r#"<?php
function t(): void {
    $f = 5;
    $g = [$f--, $f, --$f, $f];
    $g; // here
}
"#;
    assert_eq!(type_at_marker(php), "array{5, int, int, int}");
}

#[test]
fn an_increment_in_an_argument_is_in_force_after_the_call() {
    let php = r#"<?php
function takes(int $n): void {}
function t(): void {
    $i = 1;
    takes($i++);
    $i; // here
}
"#;
    assert_eq!(type_at_marker(php), "int");
}

// ─── A read before a later write ────────────────────────────────────────────

#[test]
fn an_item_reads_the_value_a_later_item_replaces() {
    let php = r#"<?php
function t(): void {
    $b = 'x';
    $h = [$b, $b = 1, $b + 1];
    $h; // here
}
"#;
    assert_eq!(type_at_marker(php), "array{'x', 1, 2}");
}

// ─── Conditional writes ─────────────────────────────────────────────────────

#[test]
fn the_right_operand_of_a_coalesce_may_not_run() {
    let php = r#"<?php
function t(?int $x): void {
    $y = 'a';
    $z = $x ?? ($y = 1);
    $y; // here
}
"#;
    assert_eq!(type_at_marker(php), "'a'|1");
}

#[test]
fn a_do_while_condition_writes_before_the_loop_exits() {
    let php = r#"<?php
class Foo {}
function g(): ?Foo { return null; }
function t(): void {
    do {
    } while (($x = g()) !== null);
    $x; // here
}
"#;
    assert_eq!(type_at_marker(php), "null");
}
