//! Diagnostics must not judge a branch the source rules out.
//!
//! A guard whose value the source decides (`false`, or a
//! `method_exists()` on a class and method both spelled out) decides its
//! branches too. Nothing inside a branch that cannot run is a finding,
//! however wrong it would be in live code.

use crate::common::create_test_backend;
use tower_lsp::lsp_types::*;

/// Every slow diagnostic reported for `text`, with the reachability
/// filter applied: the same set the editor and the `analyse` CLI see.
fn slow_diagnostics(backend: &phpantom_lsp::Backend, uri: &str, text: &str) -> Vec<Diagnostic> {
    backend.update_ast(uri, text);
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri, text, &mut out);
    out
}

/// Diagnostic codes reported for `text`, for the assertions that care
/// about which kind fired rather than where.
fn codes(backend: &phpantom_lsp::Backend, uri: &str, text: &str) -> Vec<String> {
    slow_diagnostics(backend, uri, text)
        .iter()
        .filter_map(|d| match d.code.as_ref() {
            Some(NumberOrString::String(s)) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════
// Literal guards
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn if_false_branch_is_not_judged() {
    let backend = create_test_backend();
    let uri = "file:///if_false.php";
    let text = r#"<?php
class Base {
    public function present(): void {}
}
function run(Base $base): void {
    if (false) {
        $base->gone();
    }
}
"#;
    assert!(
        codes(&backend, uri, text).is_empty(),
        "a branch guarded by `false` cannot run: {:?}",
        codes(&backend, uri, text)
    );
}

#[test]
fn else_of_if_true_is_not_judged() {
    let backend = create_test_backend();
    let uri = "file:///else_of_true.php";
    let text = r#"<?php
class Base {
    public function present(): void {}
}
function run(Base $base): void {
    if (true) {
        $base->present();
    } else {
        $base->gone();
    }
}
"#;
    assert!(
        codes(&backend, uri, text).is_empty(),
        "the else of `if (true)` cannot run: {:?}",
        codes(&backend, uri, text)
    );
}

#[test]
fn negated_literal_and_conjunction_fold() {
    let backend = create_test_backend();
    let uri = "file:///folded.php";
    let text = r#"<?php
class Base {
    public function present(): void {}
}
function run(Base $base, bool $flag): void {
    if (!true) {
        $base->goneA();
    }
    if ($flag && false) {
        $base->goneB();
    }
    if (false || false) {
        $base->goneC();
    }
}
"#;
    assert!(
        codes(&backend, uri, text).is_empty(),
        "`!true`, `$flag && false` and `false || false` all rule their branch out: {:?}",
        codes(&backend, uri, text)
    );
}

#[test]
fn a_live_branch_is_still_judged() {
    let backend = create_test_backend();
    let uri = "file:///live.php";
    let text = r#"<?php
class Base {
    public function present(): void {}
}
function run(Base $base, bool $flag): void {
    if (true) {
        $base->goneA();
    }
    if ($flag) {
        $base->goneB();
    }
    if ($flag || false) {
        $base->goneC();
    }
}
"#;
    let reported = codes(&backend, uri, text);
    assert_eq!(
        reported.len(),
        3,
        "the taken branch, an undecidable one, and `$flag || false` all run: {reported:?}"
    );
    assert!(reported.iter().all(|code| code == "unknown_member"));
}

// ═══════════════════════════════════════════════════════════════════════
// method_exists() over decidable arguments
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn negated_method_exists_on_a_known_method_rules_its_branch_out() {
    let backend = create_test_backend();
    let uri = "file:///method_exists.php";
    let text = r#"<?php
class Base {
    public function newName(string $path): void {}
}
class Child extends Base {
    public function run(string $path): void {
        if (!method_exists(parent::class, 'newName')) {
            parent::oldName($path);
        }
    }
}
"#;
    assert!(
        codes(&backend, uri, text).is_empty(),
        "`newName` is declared on the parent, so the negated branch cannot run: {:?}",
        codes(&backend, uri, text)
    );
}

#[test]
fn method_exists_finds_an_inherited_method() {
    let backend = create_test_backend();
    let uri = "file:///method_exists_inherited.php";
    let text = r#"<?php
class Grandparent {
    public function newName(string $path): void {}
}
class Base extends Grandparent {}
class Child extends Base {
    public function run(string $path): void {
        if (!method_exists(Base::class, 'newName')) {
            (new Base())->oldName($path);
        }
    }
}
"#;
    assert!(
        codes(&backend, uri, text).is_empty(),
        "the method is inherited, which `method_exists` reports as present: {:?}",
        codes(&backend, uri, text)
    );
}

#[test]
fn an_absent_method_is_not_proof_of_absence() {
    let backend = create_test_backend();
    let uri = "file:///method_exists_absent.php";
    let text = r#"<?php
class Base {
    public function present(): void {}
}
class Child extends Base {
    public function run(): void {
        if (method_exists(parent::class, 'neverDeclared')) {
            parent::gone();
        }
    }
}
"#;
    // Nothing loaded declares `neverDeclared`, but a class the index
    // has not seen in full could, so the branch is still judged.
    let reported = codes(&backend, uri, text);
    assert_eq!(reported, vec!["unknown_member".to_string()], "{reported:?}");
}

// ═══════════════════════════════════════════════════════════════════════
// Scope of the filter
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn the_guard_itself_is_still_judged() {
    let backend = create_test_backend();
    let uri = "file:///guard_itself.php";
    let text = r#"<?php
class Base {
    public function present(): void {}
}
function run(Base $base): void {
    if (false && $base->gone()) {
        $base->alsoGone();
    }
}
"#;
    let reported = slow_diagnostics(&backend, uri, text);
    assert_eq!(
        reported.len(),
        1,
        "the condition runs even when its branch does not: {reported:?}"
    );
    assert_eq!(reported[0].range.start.line, 5);
}

#[tokio::test]
async fn a_dead_branch_in_another_file_does_not_hide_this_files_diagnostics() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let backend = create_test_backend();

    // `make()` has no declared return type, so resolving `$h->make()`
    // walks this body to infer it — including the `if (false)` branch,
    // whose byte range belongs to this file only.  `return $result;`
    // (rather than `return $this;`, which shortcuts to a marker without
    // walking) forces that walk to resolve `$result`'s type.  The body
    // must be readable back from disk (as a real callee's would be) for
    // that inference to run at all.
    let helper_php = r#"<?php
class Helper {
    public function make() {
        $result = $this;
        if (false) {
            $this->neverA();
            $this->neverB();
            $this->neverC();
            $this->neverD();
            $this->neverE();
            $this->neverF();
        }
        return $result;
    }
}
"#;
    crate::common::open_php_at(&backend, &dir, "Helper.php", helper_php).await;

    // Spread unknown-member diagnostics across a range of byte offsets
    // that overlaps the dead branch above, whichever offsets it happens
    // to land on: this file starts at offset 0 just like the one above.
    let mut controller_php = String::from(
        r#"<?php
class Controller {
    public function run(Helper $h): void {
        $made = $h->make();
        $made->chained();
"#,
    );
    for i in 0..30 {
        controller_php.push_str(&format!("        $h->bogus{i}();\n"));
    }
    controller_php.push_str("    }\n}\n");

    backend.update_ast("file:///controller.php", &controller_php);

    let mut out = Vec::new();
    backend.collect_slow_diagnostics("file:///controller.php", &controller_php, &mut out);

    let reported: std::collections::HashSet<u32> = out.iter().map(|d| d.range.start.line).collect();
    assert!(
        reported.contains(&4),
        "chained() on the inferred `Helper` return was dropped by another file's dead branch: {out:?}"
    );
    // 30 `bogusN()` calls: every one of them is live code in this file
    // and must be judged, regardless of what another file's forward
    // walk recorded while inferring `make()`'s return type.
    for i in 0..30 {
        let line = 5 + i as u32;
        assert!(
            reported.contains(&line),
            "bogus{i}() at line {line} was dropped by another file's dead branch: {out:?}"
        );
    }
}

#[test]
fn a_dead_branch_still_counts_as_a_use_of_its_import() {
    let backend = create_test_backend();
    let uri = "file:///dead_import.php";
    let text = r#"<?php
namespace Demo;

use Demo\Helper;

class Runner {
    public function run(): void {
        if (false) {
            Helper::help();
        }
    }
}
"#;
    let mut out = Vec::new();
    backend.update_ast(uri, text);
    backend.collect_unused_import_diagnostics(uri, text, &mut out);
    assert!(
        !out.iter().any(|d| matches!(
            d.code.as_ref(),
            Some(NumberOrString::String(s)) if s == "unused_import"
        )),
        "an import the dead branch names is still imported: {out:?}"
    );
}
