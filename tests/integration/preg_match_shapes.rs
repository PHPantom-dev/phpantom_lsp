//! Integration tests for the shape `preg_match` leaves in `$matches`.
//!
//! A literal pattern says which keys the out-parameter has, so a group read
//! off it inside the guard resolves to `string` instead of the `mixed` a bare
//! `array` yields. Which keys are there depends on the match having succeeded,
//! so the condition that tests the result is what tells the two branches
//! apart, and a read that neither branch covers carries the `null` a missing
//! key yields.

use crate::common::assert_assigned_types;

/// Assert the type of each assignment in `content`, keyed by the variable it
/// assigns to.
/// A named group contributes its name as a key, so reading it needs no cast
/// and no `??` — and the numbered key it also gets resolves the same.
#[test]
fn named_groups_are_readable_by_name_and_by_number() {
    let content = r#"<?php
function probe(string $size): void {
    if (preg_match('/(?<amount>\d+)(?<unit>\w*)/', $size, $match)) {
        $unit = $match['unit'];
        $amount = $match['amount'];
        $byNumber = $match[2];
        $whole = $match[0];
    }
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$unit", "string"),
            ("$amount", "string"),
            ("$byNumber", "string"),
            ("$whole", "string"),
        ],
    );
}

/// The shape itself, as hover reports it for the out-parameter. Nothing has
/// tested the outcome of the call, so the empty array a failed match leaves
/// behind is still an alternative.
#[test]
fn the_out_parameter_carries_the_group_shape() {
    let content = r#"<?php
function probe(string $s): void {
    preg_match('/(\d+)-(?<name>\w+)/', $s, $literal);
    $shape = $literal;
}
"#;
    assert_assigned_types(
        content,
        &[(
            "$shape",
            "array{0: string, 1: string, name: string, 2: string}|array{}",
        )],
    );
}

/// A guard on the call rules the failed match out, leaving the shape alone.
#[test]
fn a_guard_on_the_call_rules_out_the_empty_array() {
    let content = r#"<?php
function probe(string $s): void {
    if (preg_match('/(\d+)/', $s, $matches)) {
        $inside = $matches;
        $group = $matches[1];
    }
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$inside", "array{0: string, 1: string}"),
            ("$group", "string"),
        ],
    );
}

/// Without such a guard the keys may not be there, so a group read carries
/// the `null` an offset read of a missing key yields. After the guarded block
/// the two paths rejoin, and the shape says of every key what only one of them
/// established.
#[test]
fn a_group_read_outside_a_guard_may_miss() {
    let content = r#"<?php
function probe(string $s): void {
    preg_match('/(\d+)/', $s, $matches);
    $unguarded = $matches[1];
    if (preg_match('/(\d+)/', $s, $checked)) {
        echo 'matched';
    }
    $whole = $checked;
    $after = $checked[1];
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$unguarded", "string|null"),
            ("$whole", "array{0?: string, 1?: string}"),
            ("$after", "?string"),
        ],
    );
}

/// Storing the call's result first must not cost the out-parameter its shape:
/// the call still ran, so the groups it may have written are still there.
#[test]
fn storing_the_result_keeps_the_group_shape() {
    let content = r#"<?php
function probe(string $s): void {
    $ok = preg_match('/(\d+)/', $s, $matches);
    $whole = $matches;
    $group = $matches[1];
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$whole", "array{0: string, 1: string}|array{}"),
            ("$group", "string|null"),
        ],
    );
}

/// The assignment happens once the call has returned, so a variable that is
/// both the statement's target and an out-parameter of its call keeps what it
/// was assigned rather than what the parameter is declared as.
#[test]
fn assigning_a_calls_result_to_its_own_out_parameter_keeps_the_result() {
    let content = r#"<?php
function probe(string $s): void {
    $parts = explode('/', $s);
    $last = end($parts);
    $file = explode('/', $s);
    $file = end($file);
    $tail = $file;
}
"#;
    assert_assigned_types(content, &[("$last", "string"), ("$tail", "string")]);
}

/// The variable holding the result stands for the outcome of the match, so
/// testing it narrows the out-parameter the way testing the call does.
#[test]
fn a_guard_on_the_stored_result_narrows_the_out_parameter() {
    let content = r#"<?php
function probe(string $s): void {
    $ok = preg_match('/(\d+)/', $s, $matches);
    if ($ok) {
        $matched = $matches;
        $group = $matches[1];
    } else {
        $failed = $matches;
    }
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$matched", "array{0: string, 1: string}"),
            ("$group", "string"),
            ("$failed", "array{}"),
        ],
    );
}

/// The branch that runs when the match failed knows the array is empty.
#[test]
fn a_failed_match_leaves_the_empty_array() {
    let content = r#"<?php
function probe(string $s): void {
    if (preg_match('/(\d+)/', $s, $matches)) {
        echo 'matched';
    } else {
        $failed = $matches;
    }
}
"#;
    assert_assigned_types(content, &[("$failed", "array{}")]);
}

/// A guard clause that returns on a failed match leaves the shape behind for
/// the rest of the function.
#[test]
fn a_guard_clause_narrows_the_fall_through() {
    let content = r#"<?php
function probe(string $s): string {
    if (!preg_match('/(\d+)/', $s, $matches)) {
        return '';
    }
    $group = $matches[1];
    return $group;
}
"#;
    assert_assigned_types(content, &[("$group", "string")]);
}

/// The result compared against a literal guards the same way the bare call
/// does, in either direction and with the operands either way round.
#[test]
fn comparing_the_result_guards_the_same_way() {
    let content = r#"<?php
function probe(string $s): void {
    if (preg_match('/(\d+)/', $s, $identical) === 1) {
        $fromIdentical = $identical[1];
    }
    if (preg_match('/(\d+)/', $s, $positive) > 0) {
        $fromPositive = $positive[1];
    }
    if (0 < preg_match('/(\d+)/', $s, $mirrored)) {
        $fromMirrored = $mirrored[1];
    }
    if (preg_match('/(\d+)/', $s, $atLeast) >= 1) {
        $fromAtLeast = $atLeast[1];
    }
    if (preg_match('/(\d+)/', $s, $zero) === 0) {
        $fromZero = $zero;
    }
    if (preg_match('/(\d+)/', $s, $below) < 1) {
        $fromBelow = $below;
    }
    if (preg_match('/(\d+)/', $s, $notZero) !== 0) {
        $fromNotZero = $notZero[1];
    }
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$fromIdentical", "string"),
            ("$fromPositive", "string"),
            ("$fromMirrored", "string"),
            ("$fromAtLeast", "string"),
            ("$fromZero", "array{}"),
            ("$fromBelow", "array{}"),
            ("$fromNotZero", "string"),
        ],
    );
}

/// A comparison that separates nothing narrows nothing: every result is `>= 0`,
/// so the branch it guards knows no more than the call site did.
#[test]
fn a_comparison_that_holds_either_way_narrows_nothing() {
    let content = r#"<?php
function probe(string $s): void {
    if (preg_match('/(\d+)/', $s, $matches) >= 0) {
        $either = $matches;
    }
}
"#;
    assert_assigned_types(
        content,
        &[("$either", "array{0: string, 1: string}|array{}")],
    );
}

/// An `elseif` guards its body the same way the leading `if` does: the call
/// has to be seeded before the check narrows what it wrote.
#[test]
fn an_elseif_guards_its_body_too() {
    let content = r#"<?php
function probe(string $s): void {
    if ($s === '') {
        echo 'empty';
    } elseif (preg_match('/\[(\d+)\]$/', $s, $matches)) {
        $index = $matches[1];
    }
}
"#;
    assert_assigned_types(content, &[("$index", "string")]);
}

/// A `while` loop body runs on a successful match, and the code after it on a
/// failed one.
#[test]
fn a_while_loop_narrows_its_body_and_its_exit() {
    let content = r#"<?php
function probe(string $s): void {
    while (preg_match('/(\d+)/', $s, $matches)) {
        $inLoop = $matches[1];
        $s = substr($s, 1);
    }
    $afterLoop = $matches;
}
"#;
    assert_assigned_types(content, &[("$inLoop", "string"), ("$afterLoop", "array{}")]);
}

/// `preg_match_all` collects every match of a group, so a group read is a
/// list of strings rather than one.
#[test]
fn match_all_group_reads_are_lists() {
    let content = r#"<?php
function probe(string $s): void {
    preg_match_all('/(\d+)/', $s, $matches);
    $group = $matches[1];
    $first = $matches[1][0];
}
"#;
    assert_assigned_types(content, &[("$group", "list<string>"), ("$first", "string")]);
}

/// `PREG_SET_ORDER` inverts the nesting: one entry per match, each holding
/// that match's groups.
#[test]
fn match_all_in_set_order_yields_one_shape_per_match() {
    let content = r#"<?php
function probe(string $s): void {
    preg_match_all('/(\d+)/', $s, $matches, PREG_SET_ORDER);
    $set = $matches[0];
    $group = $matches[0][1];
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$set", "array{0: string, 1: string}"),
            ("$group", "string"),
        ],
    );
}

/// `PREG_OFFSET_CAPTURE` pairs every entry with the position it matched at.
#[test]
fn offset_capture_pairs_each_group_with_its_position() {
    let content = r#"<?php
function probe(string $s): void {
    if (preg_match('/(\d+)/', $s, $matches, PREG_OFFSET_CAPTURE)) {
        $group = $matches[1];
        $text = $matches[1][0];
        $offset = $matches[1][1];
    }
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$group", "array{string, int<-1, max>}"),
            ("$text", "string"),
            ("$offset", "int<-1, max>"),
        ],
    );
}

/// A pattern that is not a literal, and one whose groups cannot be counted,
/// still type their entries: every entry of a `preg_match` result is a
/// string, whatever the keys turn out to be.
#[test]
fn an_unreadable_pattern_still_types_the_entries() {
    let content = r#"<?php
function probe(string $s, string $pattern): void {
    preg_match($pattern, $s, $dynamic);
    $fromDynamic = $dynamic['whatever'];
    preg_match('/(?|(a)|(b))/', $s, $branchReset);
    $fromBranchReset = $branchReset[1];
}
"#;
    assert_assigned_types(
        content,
        &[("$fromDynamic", "string"), ("$fromBranchReset", "string")],
    );
}

/// A `$flags` argument that cannot be read falls back to the parameter's own
/// declared type rather than assuming the default flags:
/// `PREG_OFFSET_CAPTURE` would change every entry from a string to a pair.
#[test]
fn unreadable_flags_fall_back_to_the_declared_parameter_type() {
    let content = r#"<?php
function probe(string $s, int $flags): void {
    preg_match('/(\d+)/', $s, $matches, $flags);
    $shape = $matches;
}
"#;
    assert_assigned_types(content, &[("$shape", "array<string>")]);
}

/// A flag the analysis does not model is the same case:
/// `PREG_SPLIT_OFFSET_CAPTURE` belongs to `preg_split`, and a call that
/// passes it is not one whose result this analysis knows the shape of.
#[test]
fn unmodelled_flags_fall_back_to_the_declared_parameter_type() {
    let content = r#"<?php
function probe(string $s): void {
    preg_match('/(\d+)/', $s, $matches, PREG_SPLIT_OFFSET_CAPTURE);
    $shape = $matches;
}
"#;
    assert_assigned_types(content, &[("$shape", "array<string>")]);
}

/// A `(bool)` cast on the call is true exactly when the pattern matched, so
/// it narrows the matches the way the bare call does.
#[test]
fn a_bool_cast_on_the_call_narrows_like_the_call() {
    let content = r#"<?php
function probe(string $s): void {
    if ((bool) preg_match('~(\d+)~', $s, $m)) {
        $matched = $m;
    }
}
"#;
    assert_assigned_types(content, &[("$matched", "array{0: string, 1: string}")]);
}
