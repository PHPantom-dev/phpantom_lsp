use crate::common::message_text;
use crate::common::{collect_diagnostics_with, create_psr4_workspace};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

fn retain_unhandled(diags: Vec<Diagnostic>) -> Vec<Diagnostic> {
    diags
        .into_iter()
        .filter(|d| d.code == Some(Code::String("unhandled_match".to_string())))
        .collect()
}

fn messages(php: &str) -> Vec<String> {
    messages_with(&Backend::new_test(), php)
}

fn messages_with(backend: &Backend, php: &str) -> Vec<String> {
    retain_unhandled(collect_diagnostics_with(
        backend,
        php,
        Backend::collect_match_type_diagnostics,
    ))
    .iter()
    .map(|d| message_text(&d.message).to_string())
    .collect()
}

const SUIT: &str = r#"<?php
enum Suit: string {
    case Hearts = 'H';
    case Spades = 'S';
    case Clubs = 'C';
}
"#;

fn with_suit(body: &str) -> String {
    format!("{SUIT}{body}")
}

#[test]
fn reports_the_enum_cases_no_arm_handles() {
    let php = with_suit(
        r#"
function name(Suit $s): string {
    return match ($s) {
        Suit::Spades => 'spades',
    };
}
"#,
    );
    let diags = retain_unhandled(collect_diagnostics_with(
        &Backend::new_test(),
        &php,
        Backend::collect_match_type_diagnostics,
    ));
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(
        message_text(&diags[0].message),
        "Match expression does not handle remaining values: Suit::Hearts|Suit::Clubs"
    );
    assert_eq!(diags[0].severity, Some(DiagnosticSeverity::Error));
    // The range covers `match ($s)`, not the arms.
    assert_eq!(diags[0].range.start, Position::new(8, 11));
    assert_eq!(diags[0].range.end, Position::new(8, 21));
}

#[test]
fn every_case_handled_is_not_reported() {
    let php = with_suit(
        r#"
function name(Suit $s): string {
    return match ($s) {
        Suit::Spades, Suit::Clubs => 'black',
        Suit::Hearts => 'red',
    };
}
"#,
    );
    assert!(messages(&php).is_empty());
}

#[test]
fn a_default_arm_handles_everything() {
    let php = with_suit(
        r#"
function name(Suit $s): string {
    return match ($s) {
        Suit::Spades => 'spades',
        default => 'other',
    };
}
"#,
    );
    assert!(messages(&php).is_empty());
}

#[test]
fn a_case_ruled_out_by_an_early_return_needs_no_arm() {
    let php = with_suit(
        r#"
function name(Suit $s): string {
    if ($s === Suit::Hearts) {
        return 'hearts';
    }
    return match ($s) {
        Suit::Spades, Suit::Clubs => 'black',
    };
}
"#,
    );
    assert!(messages(&php).is_empty());
}

#[test]
fn an_enclosing_arm_narrows_a_nested_match() {
    let php = with_suit(
        r#"
function name(Suit $s): int {
    return match ($s) {
        Suit::Hearts => match ($s) { Suit::Hearts => 1 },
        Suit::Spades, Suit::Clubs => 2,
    };
}
"#,
    );
    assert!(messages(&php).is_empty());
}

#[test]
fn comparing_the_backing_value_does_not_narrow_the_enum() {
    let php = with_suit(
        r#"
function name(Suit $s): string {
    if ($s->value === 'H') {
        return 'hearts';
    }
    return match ($s) {
        Suit::Spades, Suit::Clubs => 'black',
    };
}
"#,
    );
    assert_eq!(
        messages(&php),
        ["Match expression does not handle remaining value: Suit::Hearts"]
    );
}

#[test]
fn this_inside_an_enum_is_the_enum() {
    let php = r#"<?php
enum Light {
    case Red;
    case Green;

    public function next(): self {
        return match ($this) {
            self::Red => self::Green,
        };
    }
}
"#;
    assert_eq!(
        messages(php),
        ["Match expression does not handle remaining value: Light::Green"]
    );
}

#[test]
fn a_nullable_enum_needs_a_null_arm() {
    let php = with_suit(
        r#"
function name(?Suit $s): string {
    return match ($s) {
        Suit::Spades, Suit::Clubs, Suit::Hearts => 'card',
    };
}
"#,
    );
    assert_eq!(
        messages(&php),
        ["Match expression does not handle remaining value: null"]
    );
}

#[test]
fn a_bool_subject_needs_both_arms() {
    let php = r#"<?php
function label(bool $on): string {
    return match ($on) {
        true => 'on',
    };
}
function full(bool $on): string {
    return match ($on) {
        true => 'on',
        false => 'off',
    };
}
"#;
    assert_eq!(
        messages(php),
        ["Match expression does not handle remaining value: false"]
    );
}

#[test]
fn a_union_of_literals_needs_an_arm_per_literal() {
    let php = r#"<?php
/** @param 'asc'|'desc'|1 $order */
function sql($order): string {
    return match ($order) {
        'asc' => 'ASC',
        "desc" => 'DESC',
    };
}
"#;
    assert_eq!(
        messages(php),
        ["Match expression does not handle remaining value: 1"]
    );
}

#[test]
fn an_open_subject_is_not_judged() {
    let php = r#"<?php
function label(int $i, string $s, $unknown): string {
    $a = match ($i) { 1 => 'one' };
    $b = match ($s) { 'a' => 'a' };
    $c = match ($unknown) { 1 => 'one' };
    return $a . $b . $c;
}
"#;
    assert!(messages(php).is_empty());
}

#[test]
fn a_condition_that_could_be_any_case_is_not_judged() {
    let php = with_suit(
        r#"
function same(Suit $s, Suit $t): bool {
    return match ($s) {
        $t => true,
    };
}
"#,
    );
    assert!(messages(&php).is_empty());
}

#[test]
fn a_namespaced_enum_from_another_file_is_named_by_its_fqn() {
    let composer_json = r#"{
    "autoload": {
        "psr-4": {
            "App\\": "src/"
        }
    }
}"#;
    let (backend, _tmp) = create_psr4_workspace(
        composer_json,
        &[(
            "src/Suit.php",
            r#"<?php
namespace App;

enum Suit {
    case Hearts;
    case Spades;
}
"#,
        )],
    );

    // A global enum with the same short name must not stand in for the
    // imported one.
    let uri = "file:///table.php";
    let php = r#"<?php
namespace {
    enum Suit { case Joker; }
}

namespace Table {
    use App\Suit;

    function name(Suit $s): string {
        return match ($s) {
            Suit::Spades => 'spades',
        };
    }
}
"#;
    backend.update_ast(uri, php);
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri, php, &mut out);
    let msgs: Vec<String> = retain_unhandled(out)
        .iter()
        .map(|d| message_text(&d.message).to_string())
        .collect();
    assert_eq!(
        msgs,
        ["Match expression does not handle remaining value: App\\Suit::Hearts"]
    );
}

#[test]
fn a_strict_in_array_guard_rules_out_the_cases_it_lists() {
    let php = with_suit(
        r#"
function name(Suit $s): string {
    if (in_array($s, [Suit::Hearts, Suit::Clubs], true)) {
        return 'skipped';
    }
    return match ($s) {
        Suit::Spades => 'spades',
    };
}
"#,
    );
    assert!(messages(&php).is_empty());
}

#[test]
fn a_negated_in_array_guard_leaves_only_the_listed_cases() {
    let php = with_suit(
        r#"
function name(Suit $s): string {
    if (!in_array($s, [Suit::Hearts, Suit::Clubs], true)) {
        return 'skipped';
    }
    return match ($s) {
        Suit::Hearts => 'hearts',
    };
}
"#,
    );
    assert_eq!(
        messages(&php),
        ["Match expression does not handle remaining value: Suit::Clubs"]
    );
}

#[test]
fn a_try_that_catches_the_error_expects_it() {
    let php = with_suit(
        r#"
function caught(Suit $s): string {
    try {
        return match ($s) { Suit::Spades => 'spades' };
    } catch (\UnhandledMatchError $e) {
        return 'other';
    }
}
function caught_as_throwable(Suit $s): string {
    try {
        return match ($s) { Suit::Spades => 'spades' };
    } catch (\InvalidArgumentException | \Throwable $e) {
        return 'other';
    }
}
"#,
    );
    assert!(messages_with(&Backend::new_test_with_full_stubs(), &php).is_empty());
}

#[test]
fn a_try_that_catches_something_else_does_not() {
    let php = with_suit(
        r#"
function uncaught(Suit $s): string {
    try {
        return match ($s) { Suit::Spades => 'spades' };
    } catch (\Exception $e) {
        return 'other';
    }
}
"#,
    );
    assert_eq!(messages(&php).len(), 1);
}

#[test]
fn a_try_outside_a_closure_does_not_cover_it() {
    let php = with_suit(
        r#"
function later(Suit $s): \Closure {
    try {
        return fn () => match ($s) { Suit::Spades => 'spades' };
    } catch (\Throwable $e) {
        return fn () => 'other';
    }
}
"#,
    );
    assert_eq!(messages(&php).len(), 1);
}

#[test]
fn a_documented_throws_tag_expects_the_error() {
    let php = with_suit(
        r#"
/** @throws \UnhandledMatchError for anything but spades */
function strict(Suit $s): string {
    return match ($s) { Suit::Spades => 'spades' };
}
"#,
    );
    assert!(messages_with(&Backend::new_test_with_full_stubs(), &php).is_empty());
}

#[test]
fn a_catch_name_resolves_in_the_current_namespace() {
    // `Throwable` without an import is `App\Throwable`, which catches
    // nothing.
    let php = r#"<?php
namespace App;

enum Suit { case Hearts; case Spades; }

function uncaught(Suit $s): string {
    try {
        return match ($s) { Suit::Spades => 'spades' };
    } catch (Throwable $e) {
        return 'other';
    }
}
"#;
    assert_eq!(
        messages(php),
        ["Match expression does not handle remaining value: App\\Suit::Hearts"]
    );
}
