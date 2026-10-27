//! Each case of an enum is a type of its own: a subtype of the enum that no
//! other case is. Comparing against cases narrows to the ones left, a value
//! that holds one case reads that case's `name` and `value`, and the cases
//! join back into the enum once every one of them is there.
//!
//! Each type case hovers the variable at the `$x; // here` line and
//! compares the type the hover reports.
use crate::common::message_text;

use crate::common::{
    collect_diagnostics_with, create_test_backend, create_test_backend_with_full_stubs, hover_at,
    hover_text,
};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

/// The type hover reports for the variable on the line marked `// here`.
///
/// A union of classes hovers as one section per class, which are joined
/// back into the union.
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

    let types: Vec<&str> = text
        .lines()
        .filter_map(|l| l.split_once(" = ").map(|(_, ty)| ty.trim()))
        .collect();
    assert!(!types.is_empty(), "no type in hover: {text}");
    types.join("|")
}

fn argument_type_errors(php: &str) -> Vec<String> {
    collect_diagnostics_with(
        &create_test_backend_with_full_stubs(),
        php,
        Backend::collect_argument_type_diagnostics,
    )
    .into_iter()
    .filter(|d| {
        d.code
            .as_ref()
            .is_some_and(|c| matches!(c, Code::String(s) if s == "type_mismatch_argument"))
    })
    .map(|d| message_text(&d.message).to_string())
    .collect()
}

const SUIT: &str = r#"<?php
enum Suit: string {
    case Hearts = 'hearts';
    case Spades = 'spades';
    case Clubs = 'clubs';
}
"#;

fn with_suit(body: &str) -> String {
    format!("{SUIT}{body}")
}

// ─── The case itself ────────────────────────────────────────────────────────

#[test]
fn a_case_read_into_a_variable_is_that_case() {
    let php = with_suit(
        r#"
function f(): void {
    $s = Suit::Hearts;
    $s; // here
}
"#,
    );
    assert_eq!(type_at_marker(&php), "Suit::Hearts");
}

#[test]
fn a_case_reads_its_own_name_and_value() {
    let php = with_suit(
        r#"
function f(): void {
    $s = Suit::Spades;
    $value = $s->value;
    $value; // here
}
"#,
    );
    assert_eq!(type_at_marker(&php), "'spades'");

    let php = with_suit(
        r#"
function f(): void {
    $s = Suit::Spades;
    $name = $s->name;
    $name; // here
}
"#,
    );
    assert_eq!(type_at_marker(&php), "'Spades'");
}

#[test]
fn the_whole_enum_reads_every_value() {
    let php = with_suit(
        r#"
function f(Suit $s): void {
    $value = $s->value;
    $value; // here
}
"#,
    );
    assert_eq!(type_at_marker(&php), "'hearts'|'spades'|'clubs'");
}

// ─── Identity comparisons ───────────────────────────────────────────────────

#[test]
fn an_identical_case_narrows_to_it() {
    let php = with_suit(
        r#"
function f(Suit $s): void {
    if ($s === Suit::Clubs) {
        $s; // here
    }
}
"#,
    );
    assert_eq!(type_at_marker(&php), "Suit::Clubs");
}

#[test]
fn an_elseif_chain_leaves_the_cases_not_yet_compared() {
    let php = with_suit(
        r#"
function f(Suit $s): void {
    if ($s === Suit::Hearts) {
    } elseif ($s === Suit::Spades) {
    } else {
        $s; // here
    }
}
"#,
    );
    assert_eq!(type_at_marker(&php), "Suit::Clubs");
}

#[test]
fn a_guard_clause_rules_its_case_out() {
    let php = with_suit(
        r#"
function f(Suit $s): void {
    if ($s === Suit::Hearts) {
        return;
    }
    $s; // here
}
"#,
    );
    assert_eq!(type_at_marker(&php), "Suit::Spades|Suit::Clubs");
}

#[test]
fn a_not_identical_case_leaves_the_others() {
    let php = with_suit(
        r#"
function f(?Suit $s): void {
    if ($s !== Suit::Hearts) {
        $s; // here
    }
}
"#,
    );
    assert_eq!(type_at_marker(&php), "Suit::Spades|Suit::Clubs|null");
}

#[test]
fn the_cases_join_back_into_the_enum() {
    let php = with_suit(
        r#"
function f(Suit $s): void {
    if ($s === Suit::Hearts) {
        $x = 1;
    } else {
        $x = 2;
    }
    $s; // here
}
"#,
    );
    assert_eq!(type_at_marker(&php), "Suit");
}

#[test]
fn comparing_away_every_case_leaves_nothing() {
    let php = with_suit(
        r#"
function f(Suit $s): void {
    if ($s === Suit::Hearts) {
    } elseif ($s === Suit::Spades) {
    } elseif ($s === Suit::Clubs) {
    } else {
        $s; // here
    }
}
"#,
    );
    assert_eq!(type_at_marker(&php), "never");
}

#[test]
fn another_enum_s_case_is_not_ruled_out() {
    let php = with_suit(
        r#"
enum Other { case Hearts; }
function f(Suit|Other $s): void {
    if ($s !== Other::Hearts) {
        $s; // here
    }
}
"#,
    );
    assert_eq!(type_at_marker(&php), "Suit");
}

// ─── Backing value comparisons ──────────────────────────────────────────────

#[test]
fn an_identical_backing_value_narrows_to_its_case() {
    let php = with_suit(
        r#"
function f(Suit $s): void {
    if ($s->value === 'spades') {
        $s; // here
    }
}
"#,
    );
    assert_eq!(type_at_marker(&php), "Suit::Spades");
}

#[test]
fn a_backing_value_written_first_narrows_the_same() {
    let php = with_suit(
        r#"
function f(Suit $s): void {
    if ("spades" === $s->value) {
        $s; // here
    }
}
"#,
    );
    assert_eq!(type_at_marker(&php), "Suit::Spades");
}

#[test]
fn a_not_identical_backing_value_leaves_the_other_cases() {
    let php = with_suit(
        r#"
function f(?Suit $s): void {
    if ($s?->value !== 'hearts') {
        $s; // here
    }
}
"#,
    );
    assert_eq!(type_at_marker(&php), "Suit::Spades|Suit::Clubs|null");
}

#[test]
fn a_backing_value_guard_rules_its_case_out() {
    let php = with_suit(
        r#"
function f(Suit $s): void {
    if ($s->value === 'hearts') {
        return;
    }
    $s; // here
}
"#,
    );
    assert_eq!(type_at_marker(&php), "Suit::Spades|Suit::Clubs");
}

#[test]
fn an_int_backing_value_narrows_by_its_number() {
    let php = r#"<?php
enum Level: int {
    case Low = 1;
    case High = 2;
}
function f(Level $l): void {
    if ($l->value === 2) {
        $l; // here
    }
}
"#;
    assert_eq!(type_at_marker(php), "Level::High");
}

#[test]
fn a_value_of_the_other_backing_type_narrows_nothing() {
    let php = r#"<?php
enum Level: int {
    case Low = 1;
    case High = 2;
}
function f(Level $l): void {
    if ($l->value !== '2') {
        $l; // here
    }
}
"#;
    assert_eq!(type_at_marker(php), "Level");
}

#[test]
fn another_object_with_a_value_is_not_ruled_out() {
    let php = with_suit(
        r#"
class Crate { public string $value = ''; }
function f(Suit|Crate $s): void {
    if ($s->value === 'hearts') {
        $s; // here
    }
}
"#,
    );
    assert_eq!(type_at_marker(&php), "Suit|Crate");
}

#[test]
fn a_strict_in_array_of_backing_values_narrows_to_their_cases() {
    let php = with_suit(
        r#"
function f(Suit $s): void {
    if (in_array($s->value, ['hearts', 'clubs'], true)) {
        $s; // here
    }
}
"#,
    );
    assert_eq!(type_at_marker(&php), "Suit::Hearts|Suit::Clubs");
}

#[test]
fn a_failed_in_array_of_backing_values_rules_their_cases_out() {
    let php = with_suit(
        r#"
function f(Suit $s): void {
    if (!in_array($s->value, ['hearts', 'clubs'], true)) {
        $s; // here
    }
}
"#,
    );
    assert_eq!(type_at_marker(&php), "Suit::Spades");
}

// ─── match ──────────────────────────────────────────────────────────────────

#[test]
fn a_match_arm_sees_the_cases_it_names() {
    let php = with_suit(
        r#"
function f(Suit $s): void {
    match ($s) {
        Suit::Hearts, Suit::Spades => $s, // here
        default => null,
    };
}
"#,
    );
    assert_eq!(type_at_marker(&php), "Suit::Hearts|Suit::Spades");
}

#[test]
fn a_match_default_sees_the_cases_no_arm_names() {
    let php = with_suit(
        r#"
function f(Suit $s): void {
    match ($s) {
        default => $s, // here
        Suit::Hearts => 1,
        Suit::Spades => 2,
    };
}
"#,
    );
    assert_eq!(type_at_marker(&php), "Suit::Clubs");
}

// ─── Backing values through a template ──────────────────────────────────────

#[test]
fn a_case_bound_to_a_template_keeps_its_own_backing_value() {
    let php = with_suit(
        r#"
/**
 * @template T of Suit
 * @param T $case
 * @return value-of<T>
 */
function backingValue(Suit $case) { return $case->value; }

/** @param 'hearts' $value */
function acceptsHearts(string $value): void {}

function f(): void {
    acceptsHearts(backingValue(Suit::Hearts));
}
"#,
    );
    assert_eq!(argument_type_errors(&php), Vec::<String>::new());
}

#[test]
fn a_case_held_in_a_variable_keeps_its_backing_value() {
    let php = with_suit(
        r#"
/** @param 'hearts' $value */
function acceptsHearts(string $value): void {}

function f(): void {
    $case = Suit::Hearts;
    acceptsHearts($case->value);
}
"#,
    );
    assert_eq!(argument_type_errors(&php), Vec::<String>::new());
}

#[test]
fn a_different_case_s_backing_value_is_still_reported() {
    let php = with_suit(
        r#"
/** @param 'hearts' $value */
function acceptsHearts(string $value): void {}

function f(): void {
    $case = Suit::Spades;
    acceptsHearts($case->value);
}
"#,
    );
    assert_eq!(argument_type_errors(&php).len(), 1);
}

#[test]
fn a_case_satisfies_a_parameter_typed_as_its_enum() {
    let php = with_suit(
        r#"
function takesSuit(Suit $s): void {}

function f(): void {
    takesSuit(Suit::Hearts);
}
"#,
    );
    assert_eq!(argument_type_errors(&php), Vec::<String>::new());
}
