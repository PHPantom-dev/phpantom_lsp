use crate::common::{create_test_backend_with_full_stubs, messages_with_code};
use tower_lsp_server::gen_lsp_types::*;

const CODE: &str = "dead_catch";

fn diagnostics(text: &str) -> Vec<Diagnostic> {
    let backend = create_test_backend_with_full_stubs();
    let uri = "file:///test.php";
    backend.update_ast(uri, text);
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri, text, &mut out);
    out.retain(|d| matches!(&d.code, Some(Code::String(c)) if c == CODE));
    out
}

fn messages(text: &str) -> Vec<String> {
    messages_with_code(&diagnostics(text), CODE)
}

fn dead(class: &str) -> String {
    format!("Dead catch: {class} is never thrown in the try block")
}

const REPO: &str = r#"<?php
namespace App;

class OrmFailed extends \Exception {}
class OrmLocked extends OrmFailed {}
class Unrelated extends \Exception {}

class Repo {
    /** @throws OrmFailed */
    public function save(): void {}

    /** @throws OrmLocked */
    public function lock(): void {}

    public function undeclared(): void {}
}
"#;

fn with_repo(body: &str) -> String {
    format!("{REPO}\nfunction run(Repo $repo): void {{\n{body}\n}}\n")
}

#[test]
fn reports_a_catch_nothing_declared_can_reach() {
    let text = with_repo(
        r#"    try {
        $repo->save();
    } catch (\JsonException $e) {
    }"#,
    );
    let diags = diagnostics(&text);
    assert_eq!(messages(&text), vec![dead("JsonException")]);
    let line = text
        .lines()
        .nth(diags[0].range.start.line as usize)
        .unwrap();
    let start = diags[0].range.start.character as usize;
    let end = diags[0].range.end.character as usize;
    assert_eq!(&line[start..end], "\\JsonException");
}

#[test]
fn keeps_a_catch_of_the_declared_class_its_parent_or_its_child() {
    let text = with_repo(
        r#"    try {
        $repo->save();
    } catch (OrmLocked $e) {
    } catch (OrmFailed $e) {
    } catch (\Exception $e) {
    } catch (\Throwable $e) {
    }"#,
    );
    assert!(messages(&text).is_empty(), "got: {:?}", messages(&text));
}

#[test]
fn reports_a_sibling_class() {
    let text = with_repo(
        r#"    try {
        $repo->lock();
    } catch (Unrelated $e) {
    }"#,
    );
    assert_eq!(messages(&text), vec![dead("App\\Unrelated")]);
}

#[test]
fn judges_each_class_of_a_multi_catch() {
    let text = with_repo(
        r#"    try {
        $repo->save();
    } catch (OrmFailed | Unrelated $e) {
    }"#,
    );
    let diags = diagnostics(&text);
    assert_eq!(messages(&text), vec![dead("App\\Unrelated")]);
    let line = text
        .lines()
        .nth(diags[0].range.start.line as usize)
        .unwrap();
    let start = diags[0].range.start.character as usize;
    let end = diags[0].range.end.character as usize;
    assert_eq!(&line[start..end], "Unrelated");
}

#[test]
fn reports_a_catch_a_throw_cannot_reach() {
    let text = with_repo(
        r#"    try {
        throw new OrmLocked();
    } catch (Unrelated $e) {
    } catch (OrmFailed $e) {
    }"#,
    );
    assert_eq!(messages(&text), vec![dead("App\\Unrelated")]);
}

#[test]
fn stays_quiet_when_a_call_declares_nothing() {
    let text = with_repo(
        r#"    try {
        $repo->save();
        $repo->undeclared();
    } catch (Unrelated $e) {
    }"#,
    );
    assert!(messages(&text).is_empty(), "got: {:?}", messages(&text));
}

#[test]
fn stays_quiet_when_the_body_throws_nothing() {
    let text = with_repo(
        r#"    try {
        $ok = true;
    } catch (Unrelated $e) {
    }"#,
    );
    assert!(messages(&text).is_empty(), "got: {:?}", messages(&text));
}

#[test]
fn stays_quiet_for_an_interface() {
    let text = with_repo(
        r#"    try {
        $repo->save();
    } catch (\JsonSerializable $e) {
    }"#,
    );
    assert!(messages(&text).is_empty(), "got: {:?}", messages(&text));
}

#[test]
fn stays_quiet_for_an_unknown_class() {
    let text = with_repo(
        r#"    try {
        $repo->save();
    } catch (NoSuchException $e) {
    }"#,
    );
    assert!(messages(&text).is_empty(), "got: {:?}", messages(&text));
}

#[test]
fn reports_inside_a_loop_and_a_method() {
    let text = format!(
        "{REPO}\nclass Service {{\n    public function __construct(private Repo $repo) {{}}\n    public function run(array $items): void {{\n        foreach ($items as $item) {{\n            try {{\n                $this->repo->save();\n            }} catch (Unrelated $e) {{\n            }}\n        }}\n    }}\n}}\n"
    );
    assert_eq!(messages(&text), vec![dead("App\\Unrelated")]);
}

#[test]
fn keeps_a_catch_a_later_loop_pass_can_reach() {
    // The first pass through the loop sees `$x` as `Repo`, whose `save()`
    // throws only `OrmFailed`.  Later passes see it as an `ArrayObject` too,
    // which has no `save()` declaring what it throws.
    let text = with_repo(
        r#"    $x = $repo;
    foreach ([1, 2] as $_) {
        try {
            $x->save();
        } catch (Unrelated $e) {
        }
        $x = new \ArrayObject();
    }"#,
    );
    assert!(messages(&text).is_empty(), "got: {:?}", messages(&text));
}
