use crate::common::create_test_backend;
use tower_lsp::lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

fn collect(php: &str) -> Vec<Diagnostic> {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    backend.update_ast(uri, php);
    let mut out = Vec::new();
    backend.collect_unreachable_code_diagnostics(uri, php, &mut out);
    out.retain(|d| {
        d.code
            .as_ref()
            .is_some_and(|c| matches!(c, NumberOrString::String(s) if s == "unreachable_code"))
    });
    out
}

/// The dead text each diagnostic covers, so a test can assert on what was
/// dimmed rather than on line numbers.
fn dimmed(php: &str) -> Vec<String> {
    let lines: Vec<&str> = php.lines().collect();
    collect(php)
        .into_iter()
        .map(|d| {
            let first = lines[d.range.start.line as usize].trim();
            let last = lines[d.range.end.line as usize].trim();
            if d.range.start.line == d.range.end.line {
                first.to_string()
            } else {
                format!("{first} … {last}")
            }
        })
        .collect()
}

// ─── Each way of leaving a block ────────────────────────────────────────────

#[test]
fn code_after_return_is_dead() {
    let php = r#"<?php
function f(): void {
    return;
    echo 'never';
}
"#;
    assert_eq!(dimmed(php), vec!["echo 'never';"]);
}

#[test]
fn code_after_throw_is_dead() {
    let php = r#"<?php
function f(): void {
    throw new \RuntimeException('x');
    echo 'never';
}
"#;
    assert_eq!(dimmed(php), vec!["echo 'never';"]);
}

#[test]
fn code_after_exit_and_die_is_dead() {
    let php = r#"<?php
function f(): void {
    exit(1);
    echo 'never';
}

function g(): void {
    die();
    echo 'never either';
}
"#;
    assert_eq!(dimmed(php), vec!["echo 'never';", "echo 'never either';"]);
}

#[test]
fn code_after_continue_and_break_is_dead() {
    let php = r#"<?php
function f(array $items): void {
    foreach ($items as $item) {
        continue;
        echo 'never';
    }

    foreach ($items as $item) {
        break;
        echo 'never either';
    }
}
"#;
    assert_eq!(dimmed(php), vec!["echo 'never';", "echo 'never either';"]);
}

// ─── Branches ───────────────────────────────────────────────────────────────

#[test]
fn code_after_an_if_where_every_branch_leaves_is_dead() {
    let php = r#"<?php
function f(bool $flag): void {
    if ($flag) {
        return;
    } elseif ($flag) {
        throw new \RuntimeException('x');
    } else {
        return;
    }
    echo 'never';
}
"#;
    assert_eq!(dimmed(php), vec!["echo 'never';"]);
}

#[test]
fn code_after_an_if_without_an_else_is_reachable() {
    let php = r#"<?php
function f(bool $flag): void {
    if ($flag) {
        return;
    }
    echo 'reached when the flag is false';
}
"#;
    assert!(
        dimmed(php).is_empty(),
        "an if with no else always has a path that falls through: {:?}",
        dimmed(php)
    );
}

#[test]
fn code_after_an_if_whose_else_falls_through_is_reachable() {
    let php = r#"<?php
function f(bool $flag): void {
    if ($flag) {
        return;
    } else {
        echo 'here';
    }
    echo 'reached';
}
"#;
    assert!(
        dimmed(php).is_empty(),
        "one branch that falls through is enough: {:?}",
        dimmed(php)
    );
}

#[test]
fn a_return_inside_a_loop_does_not_kill_what_follows_the_loop() {
    let php = r#"<?php
function f(array $items): void {
    foreach ($items as $item) {
        return;
    }
    echo 'reached when the array is empty';
}
"#;
    assert!(
        dimmed(php).is_empty(),
        "the loop body may never run at all: {:?}",
        dimmed(php)
    );
}

// ─── Declarations ───────────────────────────────────────────────────────────

#[test]
fn a_declaration_after_a_top_level_return_is_reachable() {
    let php = r#"<?php
return;

function stillDeclared(): void {}

class StillDeclared {}
"#;
    assert!(
        dimmed(php).is_empty(),
        "top-level declarations are hoisted before the script runs: {:?}",
        dimmed(php)
    );
}

#[test]
fn a_declaration_inside_a_function_after_a_return_is_dead() {
    let php = r#"<?php
function outer(): void {
    return;
    function inner(): void {}
}
"#;
    assert_eq!(
        dimmed(php),
        vec!["function inner(): void {}"],
        "a nested declaration is created by executing the statement, so it \
         never happens after a return"
    );
}

#[test]
fn a_use_statement_is_never_dimmed() {
    let php = r#"<?php
namespace App;

return;

use RuntimeException;

echo 'x';
"#;
    let dimmed = dimmed(php);
    assert!(
        !dimmed.iter().any(|d| d.contains("use ")),
        "an import binds a name for the whole file: {dimmed:?}"
    );
}

// ─── goto labels ────────────────────────────────────────────────────────────

#[test]
fn a_label_ends_the_dead_run() {
    let php = r#"<?php
function f(): void {
    goto tail;
    echo 'never';
    tail:
    echo 'reached by the jump';
}
"#;
    assert_eq!(
        dimmed(php),
        vec!["echo 'never';"],
        "a label is an entry point, so only what precedes it is dead"
    );
}

#[test]
fn a_dead_run_resumes_after_a_label() {
    let php = r#"<?php
function f(): void {
    goto tail;
    echo 'first dead';
    tail:
    return;
    echo 'second dead';
}
"#;
    assert_eq!(
        dimmed(php),
        vec!["echo 'first dead';", "echo 'second dead';"]
    );
}

#[test]
fn a_goto_whose_label_is_in_the_same_block_does_not_kill_what_follows_the_block() {
    let php = r#"<?php
function f(): void {
    {
        goto resume;
        resume:
        echo 'inside';
    }

    echo 'after the block';
}
"#;
    assert!(
        dimmed(php).is_empty(),
        "the jump lands inside the block, so the block still falls through: {:?}",
        dimmed(php)
    );
}

#[test]
fn a_return_jumped_over_by_a_goto_does_not_kill_what_follows_the_block() {
    let php = r#"<?php
function f(): void {
    {
        goto resume;
        return;
        resume:
        echo 'inside';
    }

    echo 'after the block';
}
"#;
    assert_eq!(
        dimmed(php),
        vec!["return;"],
        "the jump skips the return and lands on the label, so the block runs \
         to its end and what follows it is reachable"
    );
}

#[test]
fn a_block_with_no_label_after_its_exit_still_kills_what_follows() {
    let php = r#"<?php
function f(): void {
    {
        echo 'first';
        return;
        echo 'never';
    }

    echo 'never either';
}
"#;
    assert_eq!(
        dimmed(php),
        vec!["echo 'never either';", "echo 'never';"],
        "nothing brings control back, so the block does leave — the outer \
         list is scanned before the block's own body, hence the order"
    );
}

// ─── Bodies that hang off expressions ───────────────────────────────────────

#[test]
fn a_closure_body_is_scanned() {
    let php = r#"<?php
$callback = function (): void {
    return;
    echo 'never';
};
"#;
    assert_eq!(dimmed(php), vec!["echo 'never';"]);
}

#[test]
fn a_closure_passed_as_an_argument_is_scanned() {
    let php = r#"<?php
array_map(function (int $n): int {
    return $n;
    echo 'never';
}, [1, 2]);
"#;
    assert_eq!(dimmed(php), vec!["echo 'never';"]);
}

#[test]
fn an_anonymous_class_method_is_scanned() {
    let php = r#"<?php
$service = new class {
    public function run(): void
    {
        return;
        echo 'never';
    }
};
"#;
    assert_eq!(dimmed(php), vec!["echo 'never';"]);
}

// ─── Inline output ──────────────────────────────────────────────────────────

#[test]
fn inline_html_after_a_return_is_dead() {
    let php = "<?php\nreturn;\n?>\nThis text is never emitted.\n";
    assert!(
        !collect(php).is_empty(),
        "inline output is a runtime statement, not a declaration"
    );
}

// ─── Spans ──────────────────────────────────────────────────────────────────

#[test]
fn consecutive_dead_statements_are_one_diagnostic() {
    let php = r#"<?php
function f(): void {
    return;
    echo 'one';
    echo 'two';
    echo 'three';
}
"#;
    let diagnostics = collect(php);
    assert_eq!(
        diagnostics.len(),
        1,
        "one run of dead code is one diagnostic, got {diagnostics:?}"
    );
    assert_eq!(dimmed(php), vec!["echo 'one'; … echo 'three';"]);
}

#[test]
fn a_hoisted_declaration_splits_the_run_around_itself() {
    let php = r#"<?php
namespace App;

return;

echo 'first dead';

use RuntimeException;

echo 'second dead';
"#;
    let diagnostics = collect(php);
    assert_eq!(
        diagnostics.len(),
        2,
        "the import is not dead, so it cannot sit inside a dimmed span: {diagnostics:?}"
    );
    assert_eq!(
        dimmed(php),
        vec!["echo 'first dead';", "echo 'second dead';"]
    );
}

// ─── Nesting ────────────────────────────────────────────────────────────────

#[test]
fn method_bodies_are_scanned() {
    let php = r#"<?php
class Service {
    public function run(): void
    {
        return;
        echo 'never';
    }
}
"#;
    assert_eq!(dimmed(php), vec!["echo 'never';"]);
}

#[test]
fn dead_code_inside_a_branch_is_found() {
    let php = r#"<?php
function f(bool $flag): void {
    if ($flag) {
        return;
        echo 'never';
    }
    echo 'reached';
}
"#;
    assert_eq!(dimmed(php), vec!["echo 'never';"]);
}

#[test]
fn try_catch_and_finally_are_each_scanned() {
    let php = r#"<?php
function f(): void {
    try {
        return;
        echo 'dead in try';
    } catch (\RuntimeException $e) {
        return;
        echo 'dead in catch';
    } finally {
        return;
        echo 'dead in finally';
    }
}
"#;
    assert_eq!(
        dimmed(php),
        vec![
            "echo 'dead in try';",
            "echo 'dead in catch';",
            "echo 'dead in finally';"
        ]
    );
}

#[test]
fn a_switch_case_is_scanned() {
    let php = r#"<?php
function f(int $n): void {
    switch ($n) {
        case 1:
            break;
            echo 'never';
        default:
            return;
            echo 'never either';
    }
}
"#;
    assert_eq!(dimmed(php), vec!["echo 'never';", "echo 'never either';"]);
}

// ─── Every kind of loop ─────────────────────────────────────────────────────

#[test]
fn while_and_for_and_do_while_bodies_are_scanned() {
    let php = r#"<?php
function f(int $n): void {
    while ($n > 0) {
        break;
        echo 'never in while';
    }

    for ($i = 0; $i < $n; $i++) {
        continue;
        echo 'never in for';
    }

    do {
        break;
        echo 'never in do';
    } while ($n > 0);
}
"#;
    assert_eq!(
        dimmed(php),
        vec![
            "echo 'never in while';",
            "echo 'never in for';",
            "echo 'never in do';"
        ]
    );
}

// ─── Alternative syntax ─────────────────────────────────────────────────────

#[test]
fn a_colon_delimited_if_is_scanned() {
    let php = r#"<?php
function f(bool $flag): void {
    if ($flag):
        return;
        echo 'never';
    endif;
}
"#;
    assert_eq!(dimmed(php), vec!["echo 'never';"]);
}

#[test]
fn code_after_a_colon_delimited_if_where_every_branch_leaves_is_dead() {
    let php = r#"<?php
function f(bool $flag): void {
    if ($flag):
        return;
    else:
        throw new \RuntimeException('x');
    endif;

    echo 'never';
}
"#;
    assert_eq!(dimmed(php), vec!["echo 'never';"]);
}

#[test]
fn a_colon_delimited_if_without_an_else_leaves_what_follows_reachable() {
    let php = r#"<?php
function f(bool $flag): void {
    if ($flag):
        return;
    endif;

    echo 'reached when the flag is false';
}
"#;
    assert!(dimmed(php).is_empty(), "{:?}", dimmed(php));
}

#[test]
fn colon_delimited_loop_bodies_are_scanned() {
    let php = r#"<?php
function f(array $items): void {
    foreach ($items as $item):
        continue;
        echo 'never in foreach';
    endforeach;

    while (true):
        break;
        echo 'never in while';
    endwhile;

    for ($i = 0; $i < 3; $i++):
        continue;
        echo 'never in for';
    endfor;
}
"#;
    assert_eq!(
        dimmed(php),
        vec![
            "echo 'never in foreach';",
            "echo 'never in while';",
            "echo 'never in for';"
        ]
    );
}

// ─── Declarations that hold statement lists ─────────────────────────────────

#[test]
fn a_trait_and_an_enum_method_are_scanned() {
    let php = r#"<?php
trait Greets {
    public function greet(): void
    {
        return;
        echo 'never in the trait';
    }
}

enum Suit: string {
    case Hearts = 'H';

    public function label(): string
    {
        return 'hearts';
        echo 'never in the enum';
    }
}
"#;
    assert_eq!(
        dimmed(php),
        vec!["echo 'never in the trait';", "echo 'never in the enum';"]
    );
}

#[test]
fn a_declare_body_is_scanned() {
    let php = r#"<?php
function f(): void {
    declare(ticks=1) {
        return;
        echo 'never';
    }
}
"#;
    assert_eq!(dimmed(php), vec!["echo 'never';"]);
}

// ─── Nothing to report ──────────────────────────────────────────────────────

#[test]
fn a_trailing_return_reports_nothing() {
    let php = r#"<?php
function f(): int {
    $value = 1;
    return $value;
}
"#;
    assert!(dimmed(php).is_empty(), "{:?}", dimmed(php));
}

#[test]
fn a_never_returning_call_is_not_recognised() {
    let php = r#"<?php
function stop(): never {
    exit(1);
}

function f(): void {
    stop();
    echo 'unreachable in truth, but this check does not resolve types';
}
"#;
    assert!(
        dimmed(php).is_empty(),
        "recognising this needs the type engine, which this phase does not run: {:?}",
        dimmed(php)
    );
}

// ─── Diagnostic shape ───────────────────────────────────────────────────────

#[test]
fn the_diagnostic_is_a_hint_tagged_unnecessary() {
    let php = r#"<?php
function f(): void {
    return;
    echo 'never';
}
"#;
    let diagnostics = collect(php);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].severity,
        Some(DiagnosticSeverity::HINT),
        "dead code is tidying, not a defect"
    );
    assert_eq!(
        diagnostics[0].tags,
        Some(vec![DiagnosticTag::UNNECESSARY]),
        "the tag is what makes editors grey the text out"
    );
    assert_eq!(diagnostics[0].source.as_deref(), Some("phpantom"));
    assert!(matches!(
        diagnostics[0].code,
        Some(NumberOrString::String(ref s)) if s == "unreachable_code"
    ));
}
