//! Integration tests for the "Make parameter explicitly nullable" code
//! action, which rewrites PHP 8.4's deprecated implicit-nullable form
//! `Type $x = null` into `?Type $x = null` or `A|B|null $x = null`.

use crate::common::{
    apply_workspace_edit, create_test_backend, find_actions, get_code_actions_at,
    get_code_actions_on_line, inject_phpstan_diag,
};
use phpantom_lsp::types::PhpVersion;
use tower_lsp_server::gen_lsp_types::*;

const TITLE: &str = "Make $";

/// Apply the single explicit-nullable action offered at `line:character`.
fn fix_at(content: &str, line: u32, character: u32) -> Option<String> {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    backend.update_ast(uri, content);
    let actions = get_code_actions_at(&backend, uri, content, line, character);
    let found = find_actions(&actions, TITLE);
    assert!(found.len() <= 1, "actions: {found:?}");
    let action = found.first()?;
    assert_eq!(action.kind, Some(CodeActionKind::QuickFix));
    Some(apply_workspace_edit(content, action.edit.as_ref()?))
}

#[test]
fn prefixes_a_single_type() {
    let content = "<?php\nfunction foo(string $p = null) {}\n";
    assert_eq!(
        fix_at(content, 1, 22).as_deref(),
        Some("<?php\nfunction foo(?string $p = null) {}\n")
    );
}

#[test]
fn prefixes_a_class_type() {
    let content = "<?php\nfunction foo(\\App\\User $user = NULL) {}\n";
    assert_eq!(
        fix_at(content, 1, 15).as_deref(),
        Some("<?php\nfunction foo(?\\App\\User $user = NULL) {}\n")
    );
}

#[test]
fn appends_null_to_a_union() {
    let content = "<?php\nfunction foo(int|string $p = null) {}\n";
    assert_eq!(
        fix_at(content, 1, 26).as_deref(),
        Some("<?php\nfunction foo(int|string|null $p = null) {}\n")
    );
}

#[test]
fn appends_null_to_a_dnf_union() {
    let content = "<?php\nfunction foo((A&B)|C $p = null) {}\n";
    assert_eq!(
        fix_at(content, 1, 22).as_deref(),
        Some("<?php\nfunction foo((A&B)|C|null $p = null) {}\n")
    );
}

#[test]
fn fixes_a_method_closure_and_promoted_parameter() {
    let method = "<?php\nclass A {\n    public function f(array $a = null) {}\n}\n";
    assert_eq!(
        fix_at(method, 2, 30).as_deref(),
        Some("<?php\nclass A {\n    public function f(?array $a = null) {}\n}\n")
    );

    let closure = "<?php\n$f = function (int $a = null) {};\n";
    assert_eq!(
        fix_at(closure, 1, 20).as_deref(),
        Some("<?php\n$f = function (?int $a = null) {};\n")
    );

    let promoted =
        "<?php\nclass A {\n    public function __construct(private Foo $foo = null) {}\n}\n";
    assert_eq!(
        fix_at(promoted, 2, 45).as_deref(),
        Some("<?php\nclass A {\n    public function __construct(private ?Foo $foo = null) {}\n}\n")
    );
}

#[test]
fn fixes_only_the_parameter_under_the_cursor() {
    let content = "<?php\nfunction foo(int $a = null, string $b = null) {}\n";
    assert_eq!(
        fix_at(content, 1, 30).as_deref(),
        Some("<?php\nfunction foo(int $a = null, ?string $b = null) {}\n")
    );
}

#[test]
fn not_offered_when_null_is_already_explicit_or_impossible() {
    for content in [
        "<?php\nfunction foo(?string $p = null) {}\n",
        "<?php\nfunction foo(string|null $p = null) {}\n",
        "<?php\nfunction foo(null|int|string $p = null) {}\n",
        "<?php\nfunction foo(mixed $p = null) {}\n",
        "<?php\nfunction foo(A&B $p = null) {}\n",
        "<?php\nfunction foo($p = null) {}\n",
        "<?php\nfunction foo(string $p = 'x') {}\n",
        "<?php\nfunction foo(string $p) {}\n",
    ] {
        let at = content.find("$p").unwrap() as u32 - 6;
        assert_eq!(fix_at(content, 1, at), None, "offered for {content}");
    }
}

#[test]
fn needs_php_7_1_for_the_nullable_prefix() {
    let backend = create_test_backend();
    backend.set_php_version(PhpVersion::new(7, 0));
    let uri = "file:///test.php";
    let content = "<?php\nfunction foo(string $p = null) {}\n";
    backend.update_ast(uri, content);
    let actions = get_code_actions_at(&backend, uri, content, 1, 22);
    assert!(find_actions(&actions, TITLE).is_empty());
}

#[test]
fn attaches_the_phpstan_diagnostic_from_anywhere_on_its_line() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nfunction foo(int $a = null, string $b = null) {}\n";
    backend.update_ast(uri, content);
    let diag = inject_phpstan_diag(
        &backend,
        uri,
        1,
        "Deprecated in PHP 8.4: Parameter #2 $b (string) is implicitly nullable via default value null.",
        "parameter.implicitlyNullable",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 1);
    let found = find_actions(&actions, TITLE);
    assert_eq!(found.len(), 1, "actions: {found:?}");
    assert_eq!(found[0].title, "Make $b explicitly nullable");
    assert_eq!(found[0].diagnostics.as_deref(), Some(&[diag][..]));
    assert_eq!(
        apply_workspace_edit(content, found[0].edit.as_ref().unwrap()),
        "<?php\nfunction foo(int $a = null, ?string $b = null) {}\n"
    );
}
