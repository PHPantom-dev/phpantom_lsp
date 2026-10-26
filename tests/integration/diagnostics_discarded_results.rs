use crate::common::message_text;
use crate::common::{collect_diagnostics_with, create_test_backend};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

fn collect(php: &str) -> Vec<Diagnostic> {
    let mut out = collect_diagnostics_with(
        &create_test_backend(),
        php,
        Backend::collect_slow_diagnostics,
    );
    out.retain(|d| {
        d.code
            .as_ref()
            .is_some_and(|c| matches!(c, Code::String(s) if s == "discarded_return_value"))
    });
    out
}

/// The lines (0-based) of every discarded-result diagnostic.
fn lines(php: &str) -> Vec<u32> {
    collect(php).iter().map(|d| d.range.start.line).collect()
}

// ─── Functions ──────────────────────────────────────────────────────────────

#[test]
fn a_no_discard_function_called_as_a_statement_is_flagged() {
    let php = r#"<?php
#[\NoDiscard]
function mintId(): int { return 1; }

mintId();
"#;
    let diags = collect(php);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.severity, Some(DiagnosticSeverity::Warning));
    assert_eq!(d.range.start, Position::new(4, 0));
    assert_eq!(d.range.end, Position::new(4, 8));
    assert_eq!(
        message_text(&d.message),
        "The return value of function mintId() should either be used or intentionally ignored by casting it as (void)"
    );
}

#[test]
fn a_used_or_void_cast_result_is_not_flagged() {
    let php = r#"<?php
#[\NoDiscard]
function mintId(): int { return 1; }

echo mintId();
$id = mintId();
(void) mintId();
var_dump(mintId());
if (mintId()) {}
$ids = [mintId()];
"#;
    assert_eq!(lines(php), Vec::<u32>::new());
}

#[test]
fn a_function_without_the_attribute_is_not_flagged() {
    let php = r#"<?php
function mintId(): int { return 1; }

mintId();
"#;
    assert_eq!(lines(php), Vec::<u32>::new());
}

#[test]
fn a_parenthesized_call_statement_is_flagged() {
    let php = r#"<?php
#[\NoDiscard]
function mintId(): int { return 1; }

(mintId());
"#;
    assert_eq!(lines(php), vec![4]);
}

#[test]
fn a_call_statement_inside_a_closure_is_flagged() {
    let php = r#"<?php
#[\NoDiscard]
function mintId(): int { return 1; }

$f = function (): void {
    mintId();
};
"#;
    assert_eq!(lines(php), vec![5]);
}

// ─── Attribute name resolution ──────────────────────────────────────────────

#[test]
fn the_attribute_name_is_case_insensitive() {
    let php = r#"<?php
#[\nOdISCArD]
function mintId(): int { return 1; }

mintId();
"#;
    assert_eq!(lines(php), vec![4]);
}

#[test]
fn an_unimported_attribute_in_a_namespace_is_a_different_class() {
    let php = r#"<?php
namespace App;

#[NoDiscard]
function mintId(): int { return 1; }

mintId();
"#;
    assert_eq!(lines(php), Vec::<u32>::new());
}

#[test]
fn an_imported_attribute_in_a_namespace_is_the_native_one() {
    let php = r#"<?php
namespace App;

use NoDiscard;

#[NoDiscard]
function mintId(): int { return 1; }

mintId();
\App\mintId();
"#;
    assert_eq!(lines(php), vec![8, 9]);
}

// ─── Methods ────────────────────────────────────────────────────────────────

#[test]
fn instance_nullsafe_and_static_method_statements_are_flagged() {
    let php = r#"<?php
class Ids {
    #[\NoDiscard]
    public function next(): int { return 1; }

    #[\NoDiscard]
    public static function make(): self { return new self(); }

    public function reset(): int { return 0; }

    public function run(): void {
        $this->next();
        self::make();
        $this->reset();
    }
}

$ids = new Ids();
$ids->next();
$ids?->next();
Ids::make();
(void) $ids->next();
$ids->reset();
$n = $ids->next();
"#;
    let diags = collect(php);
    let got: Vec<u32> = diags.iter().map(|d| d.range.start.line).collect();
    assert_eq!(got, vec![11, 12, 18, 19, 20], "got {diags:?}");
    assert!(
        message_text(&diags[0].message).starts_with("The return value of method Ids::next()"),
        "unexpected message: {}",
        message_text(&diags[0].message)
    );
}

#[test]
fn only_the_outermost_call_of_a_chain_is_discarded() {
    let php = r#"<?php
class Ids {
    #[\NoDiscard]
    public function fork(): self { return $this; }

    public function reset(): void {}
}

$ids = new Ids();
$ids->fork()->reset();
$ids->reset();
$ids->fork()->fork();
"#;
    assert_eq!(lines(php), vec![11]);
}

#[test]
fn an_override_without_the_attribute_is_not_flagged() {
    let php = r#"<?php
class Base {
    #[\NoDiscard]
    public function next(): int { return 1; }
}

class Child extends Base {
    public function next(): int { return 2; }
}

class Heir extends Base {}

(new Child())->next();
(new Heir())->next();
"#;
    assert_eq!(lines(php), vec![13]);
}
