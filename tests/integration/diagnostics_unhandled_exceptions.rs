use crate::common::{
    apply_edits, create_test_backend_with_full_stubs, extract_edits, find_action,
    messages_with_code, resolve_action,
};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

const CODE: &str = "unhandled_exception";

/// A backend with the built-in classes loaded, so `RuntimeException` and
/// `Error` can be told apart from `Exception`.
fn backend(enabled: bool) -> Backend {
    let backend = create_test_backend_with_full_stubs();
    if enabled {
        let mut cfg = backend.config();
        cfg.diagnostics.unhandled_exceptions = Some(true);
        backend.set_config(cfg);
    }
    backend
}

fn diagnostics_on(backend: &Backend, text: &str) -> Vec<Diagnostic> {
    let uri = "file:///test.php";
    backend.update_ast(uri, text);
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri, text, &mut out);
    out.retain(|d| matches!(&d.code, Some(Code::String(c)) if c == CODE));
    out
}

fn messages(text: &str) -> Vec<String> {
    messages_with_code(&diagnostics_on(&backend(true), text), CODE)
}

fn unhandled(exception: &str) -> String {
    format!("Unhandled exception {exception}: neither caught nor declared with @throws")
}

const THROWS_CHECKED: &str = r#"<?php
namespace App;

class Failed extends \Exception {}

class Loader {
    public function load(): void {
        throw new Failed();
    }
}
"#;

#[test]
fn off_by_default() {
    let diags = diagnostics_on(&backend(false), THROWS_CHECKED);
    assert!(diags.is_empty(), "got: {diags:?}");
}

#[test]
fn reports_an_undocumented_throw() {
    let diags = diagnostics_on(&backend(true), THROWS_CHECKED);
    assert_eq!(
        messages_with_code(&diags, CODE),
        vec![unhandled("App\\Failed")]
    );
    assert_eq!(diags[0].severity, Some(DiagnosticSeverity::Hint));
    assert_eq!(diags[0].range.start, Position::new(7, 8));
    assert_eq!(diags[0].range.end, Position::new(7, 26));
}

#[test]
fn a_throws_tag_covers_its_class_and_subclasses() {
    let text = r#"<?php
namespace App;

class Base extends \Exception {}
class Child extends Base {}

class Loader {
    /**
     * @throws Base
     */
    public function load(): void {
        throw new Child();
    }

    /** @throws \Exception */
    public function save(): void {
        throw new Child();
    }
}
"#;
    assert!(messages(text).is_empty(), "got: {:?}", messages(text));
}

#[test]
fn a_catch_handles_the_exception() {
    let text = r#"<?php
class Failed extends \Exception {}

function load(): void {
    try {
        throw new Failed();
    } catch (\Exception $e) {
    }
}
"#;
    assert!(messages(text).is_empty(), "got: {:?}", messages(text));
}

#[test]
fn a_rethrow_from_a_catch_is_reported() {
    let text = r#"<?php
class Failed extends \Exception {}

function load(): void {
    try {
        throw new Failed();
    } catch (Failed $e) {
        throw $e;
    }
}
"#;
    assert_eq!(messages(text), vec![unhandled("Failed")]);
}

#[test]
fn reports_what_a_callee_declares_at_the_call() {
    let text = r#"<?php
namespace App;

class Failed extends \Exception {}

class Repo {
    /** @throws Failed */
    public function find(int $id): string { return ''; }

    /** @throws Failed */
    public static function make(): self { return new self(); }
}

/** @throws Failed */
function fetch(): void {}

class Service {
    public function __construct(private Repo $repo) {}

    public function run(): void {
        $this->repo->find(1);
        Repo::make();
        fetch();
    }
}
"#;
    let diags = diagnostics_on(&backend(true), text);
    assert_eq!(
        messages_with_code(&diags, CODE),
        vec![unhandled("App\\Failed"); 3],
        "got: {diags:?}"
    );
    let lines: Vec<u32> = diags.iter().map(|d| d.range.start.line).collect();
    assert_eq!(lines, vec![20, 21, 22]);
}

#[test]
fn unchecked_exceptions_are_not_reported() {
    let text = r#"<?php
function load(int $n): void {
    if ($n === 1) {
        throw new \InvalidArgumentException();
    }
    if ($n === 2) {
        throw new \UnexpectedValueException();
    }
    if ($n === 3) {
        throw new \TypeError();
    }
    throw new \DOMException();
}
"#;
    assert_eq!(messages(text), vec![unhandled("DOMException")]);
}

#[test]
fn the_unchecked_list_is_configurable() {
    let text = r#"<?php
namespace App;

class Failed extends \Exception {}
class Ignored extends Failed {}

function load(int $n): void {
    if ($n === 1) {
        throw new Ignored();
    }
    throw new \RuntimeException();
}
"#;
    let backend = backend(true);
    let mut cfg = backend.config();
    cfg.diagnostics.unchecked_exceptions = Some(vec!["\\App\\Failed".to_string()]);
    backend.set_config(cfg);
    assert_eq!(
        messages_with_code(&diagnostics_on(&backend, text), CODE),
        vec![unhandled("RuntimeException")]
    );
}

#[test]
fn closures_are_not_judged_against_the_enclosing_docblock() {
    let text = r#"<?php
class Failed extends \Exception {}

function later(): \Closure {
    return function () {
        throw new Failed();
    };
}
"#;
    assert!(messages(text).is_empty(), "got: {:?}", messages(text));
}

#[test]
fn quick_fix_adds_the_throws_tag() {
    let backend = backend(true);
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App\Services;

use App\Errors\Failed;

class Loader {
    public function load(): void {
        throw new \App\Errors\Missing();
    }
}
"#;
    let errors = r#"<?php
namespace App\Errors;

class Failed extends \Exception {}
class Missing extends \Exception {}
"#;
    backend.update_ast("file:///errors.php", errors);
    let diags = diagnostics_on(&backend, content);
    assert_eq!(diags.len(), 1, "got: {diags:?}");

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: diags[0].range,
        context: CodeActionContext {
            diagnostics: diags.clone(),
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    let actions = backend.handle_code_action(uri, content, &params);
    let action = find_action(&actions, "Add @throws Missing").expect("should offer Add @throws");
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edits(content, &extract_edits(&resolved));
    assert!(
        result.contains("     * @throws Missing\n"),
        "got:\n{result}"
    );
    assert!(
        result.contains("use App\\Errors\\Missing;"),
        "got:\n{result}"
    );

    backend.update_ast(uri, &result);
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri, &result, &mut out);
    assert!(
        messages_with_code(&out, CODE).is_empty(),
        "the fix should clear the diagnostic:\n{result}"
    );
}
