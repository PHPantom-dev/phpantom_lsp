use crate::common::{create_test_backend, message_text};
use tower_lsp_server::gen_lsp_types::*;

fn diagnostics(text: &str, enabled: bool) -> Vec<Diagnostic> {
    let backend = create_test_backend();
    if enabled {
        let mut cfg = backend.config();
        cfg.diagnostics.partial_union_members = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    backend.update_ast(uri, text);
    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, text, &mut out);
    out
}

fn partial(diags: &[Diagnostic]) -> Vec<String> {
    diags
        .iter()
        .filter(|d| matches!(&d.code, Some(Code::String(c)) if c == "partial_union_member"))
        .map(|d| message_text(&d.message).to_string())
        .collect()
}

const USER_OR_GUEST: &str = r#"<?php
final class User { public function name(): string { return 'u'; } }
final class Guest {}

function f(User|Guest $v): void {
    echo $v->name();
}
"#;

#[test]
fn off_by_default() {
    let diags = diagnostics(USER_OR_GUEST, false);
    assert!(diags.is_empty(), "got: {diags:?}");
}

#[test]
fn reports_method_missing_from_one_branch() {
    let diags = diagnostics(USER_OR_GUEST, true);
    assert_eq!(
        partial(&diags),
        vec!["Method 'name' exists on User but not on Guest".to_string()],
        "got: {diags:?}"
    );
    assert_eq!(diags.len(), 1, "got: {diags:?}");
    assert_eq!(diags[0].range.start.line, 5);
}

#[test]
fn reports_property_missing_from_one_branch() {
    let text = r#"<?php
class Cat { public string $name = ''; }
class Rock {}

function f(Cat|Rock $v): void {
    echo $v->name;
}
"#;
    assert_eq!(
        partial(&diagnostics(text, true)),
        vec!["Property 'name' exists on Cat but not on Rock".to_string()]
    );
}

#[test]
fn lists_every_lacking_branch() {
    let text = r#"<?php
class A { public function go(): void {} }
class B {}
class C {}

function f(A|B|C $v): void {
    $v->go();
}
"#;
    assert_eq!(
        partial(&diagnostics(text, true)),
        vec!["Method 'go' exists on A but not on B, C".to_string()]
    );
}

#[test]
fn silent_when_every_branch_declares_the_member() {
    let text = r#"<?php
class Base { public function go(): void {} }
class A extends Base {}
interface Goes { public function go(): void; }
abstract class B implements Goes {}

function f(A|B $v): void {
    $v->go();
}
"#;
    let diags = diagnostics(text, true);
    assert!(diags.is_empty(), "got: {diags:?}");
}

#[test]
fn silent_when_a_lacking_branch_has_a_magic_handler() {
    let text = r#"<?php
class A { public function go(): void {} public string $p = ''; }
class B {
    public function __call(string $n, array $a): mixed { return null; }
    public function __get(string $n): mixed { return null; }
}

function f(A|B $v): void {
    $v->go();
    echo $v->p;
}
"#;
    let diags = diagnostics(text, true);
    assert!(diags.is_empty(), "got: {diags:?}");
}

/// A class that could not be loaded may declare the member, but the
/// loaded class that does not still fails whenever it arrives.
#[test]
fn reports_a_loaded_branch_beside_one_that_could_not_be_loaded() {
    let text = r#"<?php
class A { public function go(): void {} }
class B {}

function g(): A|B|NotLoadable { return new A(); }

function f(A|B|NotLoadable $v): void {
    $v->go();
    g()->go();
}
"#;
    assert_eq!(
        partial(&diagnostics(text, true)),
        vec![
            "Method 'go' exists on A but not on B".to_string(),
            "Method 'go' exists on A but not on B".to_string(),
        ]
    );
}

#[test]
fn an_intersection_is_one_alternative() {
    let text = r#"<?php
class A { public function a(): void {} }
interface I { public function i(): void; }

function g(): A&I { throw new Exception(); }

function f(A&I $v, A $w): void {
    $v->i();
    $v->a();
    g()->i();
    if ($w instanceof I) {
        $w->i();
    }
}

final class Holder {
    private A&I $held;
    /** @var A&I */
    private $documented;

    public function run(): void {
        $this->held->i();
        $this->held->a();
        $this->documented->i();
    }
}
"#;
    let diags = diagnostics(text, true);
    assert!(diags.is_empty(), "got: {diags:?}");
}

#[test]
fn reports_the_alternative_beside_an_intersection() {
    let text = r#"<?php
class A { public function a(): void {} }
interface I { public function i(): void; }
class C { public function c(): void {} }

/** @return (A&I)|C */
function h(): mixed { throw new Exception(); }

/** @param (A&I)|C $x */
function f($x): void {
    $x->i();
    h()->a();
    $x->c();
}
"#;
    assert_eq!(
        partial(&diagnostics(text, true)),
        vec![
            "Method 'i' exists on A&I but not on C".to_string(),
            "Method 'a' exists on A&I but not on C".to_string(),
            "Method 'c' exists on C but not on A&I".to_string(),
        ]
    );
}

#[test]
fn an_intersection_with_an_object_shape_is_one_alternative() {
    let text = r#"<?php
class Model { public int $id = 0; }

/** @return (Model&object{pivot: int})|null */
function first(): ?object { return null; }

function f(): void {
    echo first()?->id;
    echo first()?->pivot;
}
"#;
    let diags = diagnostics(text, true);
    assert!(partial(&diags).is_empty(), "got: {diags:?}");
}

#[test]
fn silent_when_an_intersection_could_not_be_loaded_in_full() {
    let text = r#"<?php
class A {}
class C { public function c(): void {} }

/** @param (A&NotLoadable)|C $x */
function f($x): void {
    $x->c();
}
"#;
    assert!(partial(&diagnostics(text, true)).is_empty());
}

#[test]
fn silent_when_a_branch_is_not_a_class() {
    let text = r#"<?php
class A { public function go(): void {} }

function f(A|int $v): void {
    $v->go();
}
"#;
    assert!(partial(&diagnostics(text, true)).is_empty());
}

#[test]
fn null_branch_is_not_a_lacking_class() {
    let text = r#"<?php
class A { public function go(): void {} }

function f(?A $v): void {
    $v?->go();
}
"#;
    let diags = diagnostics(text, true);
    assert!(diags.is_empty(), "got: {diags:?}");
}

#[test]
fn member_missing_from_every_branch_is_only_an_unknown_member() {
    let text = r#"<?php
class A {}
class B {}

function f(A|B $v): void {
    $v->go();
}
"#;
    let diags = diagnostics(text, true);
    assert!(partial(&diags).is_empty(), "got: {diags:?}");
    assert_eq!(diags.len(), 1, "got: {diags:?}");
}

#[test]
fn silent_after_instanceof_narrowing() {
    let text = r#"<?php
class A { public function go(): void {} }
class B {}

function f(A|B $v): void {
    if ($v instanceof A) {
        $v->go();
    }
    if (!$v instanceof B) {
        $v->go();
    }
}
"#;
    let diags = diagnostics(text, true);
    assert!(diags.is_empty(), "got: {diags:?}");
}

#[test]
fn silent_after_expression_level_narrowing() {
    let text = r#"<?php
class A {
    public function go(): int { return 1; }
    public function both(): void {}
}
class B { public function both(): void {} }

function f(A|B $v): void {
    $v->both();
    $x = $v instanceof A ? $v->go() : 0;
    $y = $v instanceof A && $v->go();
    $v->go();
}
"#;
    let diags = diagnostics(text, true);
    let lines: Vec<u32> = diags
        .iter()
        .filter(|d| !partial(std::slice::from_ref(d)).is_empty())
        .map(|d| d.range.start.line)
        .collect();
    assert_eq!(lines, vec![11], "got: {diags:?}");
}

#[test]
fn reports_a_static_call() {
    let text = r#"<?php
class A { public static function make(): void {} }
class B {}

function f(A|B $v): void {
    $v::make();
}
"#;
    assert_eq!(
        partial(&diagnostics(text, true)),
        vec!["Method 'make' exists on A but not on B".to_string()]
    );
}

/// A check on a call narrows the same call written out again, arguments
/// included, and only that call.
#[test]
fn a_call_with_arguments_is_narrowed_by_a_check_on_it() {
    let text = r#"<?php
class Rock { public function crush(): string { return ''; } }
class Banana {}
class Holder {
    public function maybe(): Rock|Banana|null { return null; }
    public function lookUp(string $name): Rock|Banana|null { return null; }
}

function f(Holder $holder): string {
    if ($holder->maybe() instanceof Rock) {
        return $holder->maybe()->crush();
    }
    if ($holder->lookUp('rock') instanceof Rock) {
        return $holder->lookUp('rock')->crush();
    }
    if ($holder->lookUp('rock') instanceof Rock) {
        return $holder->lookUp('banana')->crush();
    }
    return '';
}
"#;
    let diags = diagnostics(text, true);
    let lines: Vec<u32> = diags.iter().map(|d| d.range.start.line).collect();
    assert_eq!(lines, vec![16], "got: {diags:?}");
}
