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
            .is_some_and(|c| matches!(c, Code::String(s) if s == "void_result_used"))
    });
    out
}

/// The lines (0-based) of every used-void-result diagnostic.
fn lines(php: &str) -> Vec<u32> {
    collect(php).iter().map(|d| d.range.start.line).collect()
}

// ─── Functions ──────────────────────────────────────────────────────────────

#[test]
fn an_assigned_void_result_is_flagged() {
    let php = r#"<?php
function flush(): void {}

$flushed = flush();
"#;
    let diags = collect(php);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.severity, Some(DiagnosticSeverity::Warning));
    assert_eq!(d.range.start, Position::new(3, 11));
    assert_eq!(d.range.end, Position::new(3, 18));
    assert_eq!(
        message_text(&d.message),
        "Result of function flush() (void) is used"
    );
}

#[test]
fn every_way_of_reading_the_value_is_flagged() {
    let php = r#"<?php
function flush(): void {}
function take(mixed $v): int { return 1; }

echo flush();
take(flush());
if (flush()) {}
$n = 1 + flush();
$list = [flush()];
$x = $y ?? flush();
$z = $y ? flush() : null;
$w = match (true) { default => flush() };
while (flush()) {}
function relay(): mixed { return flush(); }
"#;
    assert_eq!(lines(php), vec![4, 5, 6, 7, 8, 9, 10, 11, 12, 13]);
}

#[test]
fn a_value_that_goes_nowhere_is_not_flagged() {
    let php = r#"<?php
function flush(): void {}

flush();
(flush());
@flush();
(void) flush();
$ready && flush();
$ready || flush();
$ready and flush();
$ready or flush();
$cache ?? flush();
$ready ? flush() : flush();
$ready ?: flush();
match ($ready) { true => flush(), default => flush() };
for (flush(), $i = 0; flush(), $i < 3; $i++, flush()) {}
$f = fn() => flush();
$g = fn() => $ready && flush();
"#;
    assert_eq!(lines(php), Vec::<u32>::new());
}

#[test]
fn the_left_hand_side_of_a_short_circuit_is_flagged() {
    let php = r#"<?php
function flush(): void {}

flush() && $ready;
flush() ?? $ready;
flush() ? 1 : 2;
"#;
    assert_eq!(lines(php), vec![3, 4, 5]);
}

#[test]
fn the_last_for_condition_is_flagged() {
    let php = r#"<?php
function flush(): void {}

for (; flush(); ) {}
"#;
    assert_eq!(lines(php), vec![3]);
}

#[test]
fn a_docblock_void_over_an_untyped_signature_is_not_flagged() {
    let php = r#"<?php
/** @return void */
function flush() {}

$flushed = flush();
"#;
    assert_eq!(lines(php), Vec::<u32>::new());
}

#[test]
fn never_and_value_returning_functions_are_not_flagged() {
    let php = r#"<?php
function fail(): never { throw new \Exception(); }
function count_up(): int { return 1; }
function maybe(): ?int { return null; }

$a = $b ?? fail();
$c = count_up();
$d = maybe();
"#;
    assert_eq!(lines(php), Vec::<u32>::new());
}

#[test]
fn a_first_class_callable_is_not_a_call() {
    let php = r#"<?php
function flush(): void {}

$f = flush(...);
"#;
    assert_eq!(lines(php), Vec::<u32>::new());
}

#[test]
fn a_namespaced_function_is_flagged() {
    let php = r#"<?php
namespace App;

function flush(): void {}

$a = flush();
$b = \App\flush();
"#;
    assert_eq!(lines(php), vec![5, 6]);
}

// ─── Methods ────────────────────────────────────────────────────────────────

#[test]
fn instance_nullsafe_and_static_methods_are_flagged() {
    let php = r#"<?php
class Mailer {
    public function send(): void {}

    public static function reset(): void {}

    public function id(): int { return 1; }

    public function run(): int {
        $this->send();
        self::reset();
        return $this->send();
    }
}

$mailer = new Mailer();
$a = $mailer->send();
$b = $mailer?->send();
$c = Mailer::reset();
$d = $mailer->id();
"#;
    let diags = collect(php);
    let got: Vec<u32> = diags.iter().map(|d| d.range.start.line).collect();
    assert_eq!(got, vec![11, 16, 17, 18], "got {diags:?}");
    assert_eq!(
        message_text(&diags[0].message),
        "Result of method Mailer::send() (void) is used"
    );
}

#[test]
fn a_chain_through_a_void_method_is_flagged() {
    let php = r#"<?php
class Mailer {
    public function send(): void {}

    public function fluent(): static { return $this; }
}

$mailer = new Mailer();
$mailer->fluent()->send();
$mailer->send()->fluent();
"#;
    assert_eq!(lines(php), vec![9]);
}

#[test]
fn an_inherited_and_an_interface_method_are_flagged() {
    let php = r#"<?php
interface Logger {
    public function log(string $message): void;
}

abstract class BaseLogger implements Logger {}

function relay(Logger $logger, BaseLogger $base): void {
    $a = $logger->log('a');
    $b = $base->log('b');
}
"#;
    assert_eq!(lines(php), vec![8, 9]);
}

#[test]
fn a_call_inside_a_closure_is_flagged() {
    let php = r#"<?php
function flush(): void {}

$f = function (): void {
    $flushed = flush();
    flush();
};
"#;
    assert_eq!(lines(php), vec![4]);
}

#[test]
fn a_union_receiver_is_flagged_only_when_every_member_returns_nothing() {
    let php = r#"<?php
interface Logger {
    public function warning(string $message): void;
}

class Mock {
    public function warning(string $message): Expectation { return new Expectation(); }
}

class Prophecy {
    public function __call(string $name, array $args): Expectation { return new Expectation(); }
}

class Quiet {
    public function warning(string $message): void {}
}

class Expectation {
    public function shouldBeCalled(): static { return $this; }
}

function relay(
    Logger|Mock $a,
    Mock|Logger $b,
    Logger|Prophecy $c,
    Prophecy|Logger $d,
    Logger|Quiet $e,
): void {
    $a->warning('x')->shouldBeCalled();
    $b->warning('x')->shouldBeCalled();
    $c->warning('x')->shouldBeCalled();
    $d->warning('x')->shouldBeCalled();
    $f = $e->warning('x');
}
"#;
    assert_eq!(lines(php), vec![32]);
}
