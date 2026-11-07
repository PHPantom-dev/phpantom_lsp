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
            .is_some_and(|c| matches!(c, Code::String(s) if s == "missing_return"))
    });
    out
}

/// The lines (0-based) of every missing-return diagnostic.
fn lines(php: &str) -> Vec<u32> {
    collect(php).iter().map(|d| d.range.start.line).collect()
}

// ─── What is reported ───────────────────────────────────────────────────────

#[test]
fn a_function_that_can_fall_off_its_end_is_flagged() {
    let php = r#"<?php
function score(int $x): int {
    if ($x > 0) {
        return 1;
    }
}
"#;
    let diags = collect(php);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.severity, Some(DiagnosticSeverity::Error));
    assert_eq!(d.range.start, Position::new(5, 0));
    assert_eq!(d.range.end, Position::new(5, 1));
    assert_eq!(
        message_text(&d.message),
        "Function score() should return int but can reach its end without returning"
    );
}

#[test]
fn methods_closures_and_empty_bodies_are_flagged() {
    let php = r#"<?php
class Repo {
    public function find(int $id): Repo {
        if ($id) { return $this; }
    }
    public function count(): int {}
}
$cb = function (int $x): string {
    foreach ([1] as $y) { return 'a'; }
};
$anon = new class {
    public function name(): string {}
};
"#;
    assert_eq!(lines(php), vec![4, 5, 9, 11]);
    let messages: Vec<String> = collect(php)
        .iter()
        .map(|d| message_text(&d.message).to_string())
        .collect();
    assert!(messages[0].starts_with("Method find() should return Repo"));
    assert!(messages[2].starts_with("Closure should return string"));
}

#[test]
fn a_catch_that_carries_on_is_flagged() {
    let php = r#"<?php
function load(): array {
    try {
        return [1];
    } catch (\Exception $e) {
        error_log($e->getMessage());
    }
}
"#;
    assert_eq!(lines(php), vec![7]);
}

#[test]
fn a_switch_without_default_is_flagged() {
    let php = r#"<?php
function label(int $x): string {
    switch ($x) {
        case 1:
            return 'one';
        case 2:
            return 'two';
    }
}
"#;
    assert_eq!(lines(php), vec![8]);
}

// ─── What is not reported ───────────────────────────────────────────────────

#[test]
fn types_that_accept_the_implicit_null_are_not_flagged() {
    let php = r#"<?php
function a(): void {}
function b(): ?int {}
function c(): int|null {}
function d(): mixed {}
function e(): null {}
function f() {}
/** @return int */
function g() {}
"#;
    assert_eq!(lines(php), Vec::<u32>::new());
}

#[test]
fn bodies_that_always_leave_are_not_flagged() {
    let php = r#"<?php
function a(bool $x): int {
    if ($x) { return 1; } else { return 2; }
}
function b(): int {
    throw new \RuntimeException('no');
}
function c(): int {
    exit(1);
}
function d(): int {
    while (true) {
        if (rand()) { return 1; }
    }
}
function e(): int {
    try {
        return 1;
    } catch (\Exception $e) {
        throw $e;
    } finally {
        echo 'done';
    }
}
function f(int $x): string {
    switch ($x) {
        case 1: return 'one';
        default: return 'many';
    }
}
function g(): int {
    for (;;) {}
}
"#;
    assert_eq!(lines(php), Vec::<u32>::new());
}

#[test]
fn a_generator_is_not_flagged() {
    let php = r#"<?php
function gen(): iterable {
    yield 1;
}
"#;
    assert_eq!(lines(php), Vec::<u32>::new());
}

#[test]
fn abstract_and_interface_methods_are_not_flagged() {
    let php = r#"<?php
interface Shape {
    public function area(): float;
}
abstract class Base {
    abstract public function name(): string;
}
"#;
    assert_eq!(lines(php), Vec::<u32>::new());
}

// ─── Calls that never return ────────────────────────────────────────────────

#[test]
fn a_body_ending_in_a_never_returning_call_is_not_flagged() {
    let php = r#"<?php
function fail(string $m): never { throw new \RuntimeException($m); }

/** @return never */
function abort_now() { exit(1); }

class Aborter {
    public function stop(): never { exit(1); }
    public static function now(): never { exit(1); }
}

class Controller {
    public function show(int $id, Aborter $aborter): string {
        if ($id) { return 'x'; }
        $aborter->stop();
    }
    public function edit(int $id): string {
        if ($id) { return 'x'; }
        $this->deny();
    }
    public function update(int $id): string {
        if ($id) { return 'x'; }
        Aborter::now();
    }
    public function destroy(int $id): string {
        if ($id) { return 'x'; }
        fail('no');
    }
    public function store(int $id): string {
        if ($id) { return 'x'; }
        abort_now();
    }
    public function index(int $id): string {
        if ($id) { return 'x'; } else { fail('no'); }
    }
    private function deny(): never { throw new \LogicException(); }
}
"#;
    assert_eq!(lines(php), Vec::<u32>::new());
}

#[test]
fn a_body_ending_in_a_call_that_returns_is_flagged() {
    let php = r#"<?php
function warn(string $m): void {}

class Controller {
    public function show(int $id): string {
        if ($id) { return 'x'; }
        warn('no');
    }
}
"#;
    assert_eq!(lines(php), vec![7]);
}

#[test]
fn a_match_whose_every_arm_throws_ends_the_body() {
    let php = r#"<?php
function state(string $s): int {
    if ($s === 'ok') { return 1; }
    match ($s) {
        'failed' => throw new \RuntimeException('failed'),
        default => throw new \LogicException($s),
    };
}
"#;
    assert_eq!(lines(php), Vec::<u32>::new());
}

#[test]
fn a_match_with_an_arm_that_returns_a_value_is_flagged() {
    let php = r#"<?php
function state(string $s): int {
    if ($s === 'ok') { return 1; }
    match ($s) {
        'failed' => throw new \RuntimeException('failed'),
        default => error_log($s),
    };
}
"#;
    assert_eq!(lines(php), vec![7]);
}

// ─── Switches the subject's type makes exhaustive ───────────────────────────

#[test]
fn a_switch_with_a_case_for_every_literal_is_not_flagged() {
    let php = r#"<?php
/** @param 'draft'|'live' $state */
function label(string $state): string {
    switch ($state) {
        case 'draft':
            return 'Draft';
        case 'live':
            return 'Live';
    }
}
"#;
    assert_eq!(lines(php), Vec::<u32>::new());
}

#[test]
fn a_switch_missing_a_literal_is_flagged() {
    let php = r#"<?php
/** @param 'draft'|'live'|'archived' $state */
function label(string $state): string {
    switch ($state) {
        case 'draft':
            return 'Draft';
        case 'live':
            return 'Live';
    }
}
"#;
    assert_eq!(lines(php), vec![9]);
}
