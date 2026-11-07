use crate::common::{collect_diagnostics_with, create_test_backend_with_full_stubs};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

fn collect(php: &str) -> Vec<Diagnostic> {
    collect_diagnostics_with(
        &create_test_backend_with_full_stubs(),
        php,
        Backend::collect_argument_type_diagnostics,
    )
}

/// The `unknown_enum_value` diagnostics, as (severity, message) pairs.
fn unknown_values(php: &str) -> Vec<(DiagnosticSeverity, String)> {
    collect(php)
        .into_iter()
        .filter(|d| matches!(&d.code, Some(Code::String(code)) if code == "unknown_enum_value"))
        .map(|d| {
            (
                d.severity.unwrap(),
                crate::common::message_text(&d.message).to_string(),
            )
        })
        .collect()
}

const STATUS: &str = r#"<?php
enum Status: string {
    case Active = 'active';
    case Inactive = "inactive";
}
enum Priority: int {
    case Low = 1;
    case High = 0x10;
}
"#;

fn with_enums(body: &str) -> String {
    format!("{STATUS}{body}\n")
}

#[test]
fn from_with_unknown_string_is_an_error() {
    let diags = unknown_values(&with_enums("Status::from('Active');"));
    assert_eq!(
        diags,
        vec![(
            DiagnosticSeverity::Error,
            "No case of enum 'Status' is backed by 'Active', so Status::from() throws a ValueError"
                .to_string()
        )]
    );
}

#[test]
fn try_from_with_unknown_string_is_a_warning() {
    let diags = unknown_values(&with_enums("Status::tryFrom('Active');"));
    assert_eq!(
        diags,
        vec![(
            DiagnosticSeverity::Warning,
            "No case of enum 'Status' is backed by 'Active', so Status::tryFrom() always returns null"
                .to_string()
        )]
    );
}

#[test]
fn known_values_are_not_reported() {
    let php = with_enums(
        "Status::from('active');\n\
         Status::tryFrom(\"inactive\");\n\
         Priority::from(1);\n\
         Priority::from(16);\n\
         Priority::tryFrom(value: 1);",
    );
    assert!(
        unknown_values(&php).is_empty(),
        "{:?}",
        unknown_values(&php)
    );
}

#[test]
fn unknown_int_is_reported() {
    let diags = unknown_values(&with_enums("Priority::from(2);"));
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].0, DiagnosticSeverity::Error);
}

#[test]
fn named_argument_is_checked() {
    let diags = unknown_values(&with_enums("Status::from(value: 'nope');"));
    assert_eq!(diags.len(), 1, "{diags:?}");
}

#[test]
fn literal_held_by_a_variable_is_checked() {
    let diags = unknown_values(&with_enums(
        "function f(): void { $s = 'Active'; Status::from($s); }",
    ));
    assert_eq!(diags.len(), 1, "{diags:?}");
}

#[test]
fn union_with_one_known_value_is_not_reported() {
    let php = with_enums(
        "/** @param 'active'|'nope' $s */\n\
         function f(string $s): void { Status::from($s); }",
    );
    assert!(unknown_values(&php).is_empty());
}

#[test]
fn union_of_unknown_values_is_reported() {
    let php = with_enums(
        "/** @param 'nope'|'nada' $s */\n\
         function f(string $s): void { Status::from($s); }",
    );
    assert_eq!(unknown_values(&php).len(), 1);
}

#[test]
fn non_literal_argument_is_not_reported() {
    let php = with_enums("function f(string $s): void { Status::from($s); }");
    assert!(unknown_values(&php).is_empty());
}

#[test]
fn coercive_mode_converts_between_int_and_string() {
    let php = with_enums(
        "Priority::from('1');\n\
         Priority::from('16');",
    );
    assert!(unknown_values(&php).is_empty());
    let diags = unknown_values(&with_enums("Priority::from('2');"));
    assert_eq!(diags.len(), 1, "{diags:?}");
}

#[test]
fn numeric_string_backing_matches_an_int_in_coercive_mode() {
    let php = r#"<?php
enum Code: string {
    case One = '1';
}
Code::from(1);
"#;
    assert!(unknown_values(php).is_empty());
    let php = r#"<?php
enum Code: string {
    case One = '1';
}
Code::from(2);
"#;
    assert_eq!(unknown_values(php).len(), 1);
}

#[test]
fn strict_types_leaves_the_other_scalar_type_alone() {
    // Under strict_types the call is a TypeError, not a missing case.
    let php = r#"<?php
declare(strict_types=1);
enum Priority: int {
    case Low = 1;
}
Priority::from('2');
"#;
    assert!(unknown_values(php).is_empty());
}

#[test]
fn case_backed_by_a_constant_expression_is_undecidable() {
    let php = r#"<?php
enum Status: string {
    const PREFIX = 'x';
    case Active = self::PREFIX . 'active';
}
Status::from('xactive');
"#;
    assert!(unknown_values(php).is_empty());
}

#[test]
fn static_and_self_calls_inside_the_enum_are_checked() {
    let php = r#"<?php
enum Status: string {
    case Active = 'active';

    public static function make(): self {
        return static::from('Active');
    }

    public static function maybe(): ?self {
        return self::tryFrom('active');
    }
}
"#;
    assert_eq!(unknown_values(php).len(), 1);
}

#[test]
fn namespaced_enum_is_checked() {
    let php = r#"<?php
namespace App;
enum Status: string {
    case Active = 'active';
}
function f(): void {
    \App\Status::from('Active');
    Status::from('active');
}
"#;
    let diags = unknown_values(php);
    assert_eq!(diags.len(), 1, "{diags:?}");
}

#[test]
fn diagnostic_spans_the_argument() {
    let php = with_enums("Status::from('Active');");
    let diag = collect(&php)
        .into_iter()
        .find(|d| matches!(&d.code, Some(Code::String(c)) if c == "unknown_enum_value"))
        .expect("a diagnostic");
    let last_line = php.lines().count() as u32 - 1;
    assert_eq!(diag.range.start, Position::new(last_line, 13));
    assert_eq!(diag.range.end, Position::new(last_line, 21));
}
