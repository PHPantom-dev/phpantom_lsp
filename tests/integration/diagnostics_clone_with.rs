//! The properties a PHP 8.5 `clone($object, [...])` call writes on the copy
//! go through the checks an `$copy->prop = …` assignment gets.

use crate::common::{collect_diagnostics_with, create_test_backend, messages_with_code};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

/// Every diagnostic but the test backend's missing `clone()` stub.
fn collect(php: &str) -> Vec<Diagnostic> {
    let mut out = collect_diagnostics_with(
        &create_test_backend(),
        php,
        Backend::collect_slow_diagnostics,
    );
    out.retain(|d| !matches!(&d.code, Some(Code::String(code)) if code == "unknown_function"));
    out
}

const POINT: &str = r#"<?php
final class Point {
    public function __construct(
        public int $x,
        private int $secret = 0,
        public readonly int $id = 0,
    ) {}

    public function moved(): static
    {
        return clone($this, ['id' => 2, 'secret' => 1]);
    }
}

$p = new Point(1);
"#;

fn with_point(code: &str) -> Vec<Diagnostic> {
    collect(&format!("{POINT}{code}\n"))
}

#[test]
fn valid_writes_are_not_flagged() {
    let diags = with_point(
        "$copy = clone($p, ['x' => 2]);\n$named = clone(object: $p, withProperties: ['x' => 3]);",
    );
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

#[test]
fn an_undeclared_property_is_flagged() {
    let diags = with_point("$copy = clone($p, ['nope' => 1]);");
    let msgs = messages_with_code(&diags, "unknown_member");
    assert_eq!(msgs.len(), 1, "got {diags:?}");
    assert!(msgs[0].contains("nope"), "message: {}", msgs[0]);
}

#[test]
fn a_value_of_the_wrong_type_is_flagged() {
    let diags = with_point("$copy = clone($p, ['x' => 'str']);");
    let msgs = messages_with_code(&diags, "type_mismatch_property");
    assert_eq!(msgs.len(), 1, "got {diags:?}");
    assert!(msgs[0].contains("$x"), "message: {}", msgs[0]);
}

#[test]
fn a_nested_call_is_checked() {
    let diags = with_point("function f(Point $p): array { return [clone($p, ['x' => 'str'])]; }");
    let msgs = messages_with_code(&diags, "type_mismatch_property");
    assert_eq!(msgs.len(), 1, "got {diags:?}");
}

#[test]
fn a_private_property_from_outside_is_flagged() {
    let diags = with_point("$copy = clone($p, ['secret' => 1]);");
    let msgs = messages_with_code(&diags, "invalid_member_access");
    assert_eq!(msgs.len(), 1, "got {diags:?}");
    assert!(msgs[0].contains("secret"), "message: {}", msgs[0]);
}

#[test]
fn a_readonly_property_from_outside_is_flagged() {
    let diags = with_point("$copy = clone($p, ['id' => 1]);");
    let msgs = messages_with_code(&diags, "invalid_readonly_write");
    assert_eq!(msgs.len(), 1, "got {diags:?}");
    assert!(msgs[0].contains("Point::$id"), "message: {}", msgs[0]);
}

#[test]
fn computed_keys_are_not_checked() {
    let diags = with_point("$k = 'nope';\n$copy = clone($p, [$k => 1, \"n\\x6fpe\" => 1]);");
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

#[test]
fn single_argument_clone_writes_nothing() {
    let diags = with_point("$copy = clone($p);\n$other = clone $p;");
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}
