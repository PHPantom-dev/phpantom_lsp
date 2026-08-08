//! Narrowing carried by a boolean variable.
//!
//! `$ok = $x instanceof Foo;` stores the assertion in `$ok`, so a later
//! truthy check on `$ok` must narrow `$x` exactly as the original
//! `instanceof` expression would.

use crate::common::create_test_backend;
use tower_lsp::lsp_types::*;

fn unknown_member_diagnostics(
    backend: &phpantom_lsp::Backend,
    uri: &str,
    text: &str,
) -> Vec<Diagnostic> {
    backend.update_ast(uri, text);
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri, text, &mut out);
    out.retain(|d| {
        d.code
            .as_ref()
            .is_some_and(|c| matches!(c, NumberOrString::String(s) if s == "unknown_member"))
    });
    out
}

const SCAFFOLD: &str = r#"<?php
namespace Repro;

interface Renderable {}
class HtmlString implements Renderable {
    public function toHtml(): string { return ''; }
}
class PlainString implements Renderable {
    public function toPlain(): string { return ''; }
}
"#;

#[test]
fn assertion_variable_narrows_in_ternary() {
    let backend = create_test_backend();
    let uri = "file:///assertion_ternary.php";
    let text = format!(
        "{SCAFFOLD}
class C {{
    public function m(Renderable $raw): string {{
        $isHtml = $raw instanceof HtmlString;

        return $isHtml ? $raw->toHtml() : 'x';
    }}
}}
"
    );
    let diags = unknown_member_diagnostics(&backend, uri, &text);
    assert!(
        diags.is_empty(),
        "A boolean holding an instanceof result should narrow in the \
         ternary's then-branch, got: {diags:?}"
    );
}

#[test]
fn assertion_variable_narrows_in_if_body() {
    let backend = create_test_backend();
    let uri = "file:///assertion_if.php";
    let text = format!(
        "{SCAFFOLD}
class C {{
    public function m(Renderable $raw): string {{
        $isHtml = $raw instanceof HtmlString;
        if ($isHtml) {{
            return $raw->toHtml();
        }}
        return 'x';
    }}
}}
"
    );
    let diags = unknown_member_diagnostics(&backend, uri, &text);
    assert!(
        diags.is_empty(),
        "A boolean holding an instanceof result should narrow inside \
         `if ($ok)`, got: {diags:?}"
    );
}

#[test]
fn assertion_variable_narrows_after_negated_guard() {
    let backend = create_test_backend();
    let uri = "file:///assertion_guard.php";
    let text = format!(
        "{SCAFFOLD}
class C {{
    public function m(Renderable $raw): string {{
        $isHtml = $raw instanceof HtmlString;
        if (!$isHtml) {{
            return 'x';
        }}
        return $raw->toHtml();
    }}
}}
"
    );
    let diags = unknown_member_diagnostics(&backend, uri, &text);
    assert!(
        diags.is_empty(),
        "A guard clause on the negated boolean should leave the subject \
         narrowed afterwards, got: {diags:?}"
    );
}

#[test]
fn assertion_variable_narrows_in_and_chain() {
    let backend = create_test_backend();
    let uri = "file:///assertion_and.php";
    let text = format!(
        "{SCAFFOLD}
class C {{
    public function m(Renderable $raw, bool $flag): string {{
        $isHtml = $raw instanceof HtmlString;
        if ($flag && $isHtml) {{
            return $raw->toHtml();
        }}
        return 'x';
    }}
}}
"
    );
    let diags = unknown_member_diagnostics(&backend, uri, &text);
    assert!(
        diags.is_empty(),
        "A boolean assertion in an `&&` chain should narrow the subject, \
         got: {diags:?}"
    );
}

/// Reassigning the subject invalidates the stored assertion: after
/// `$raw` is replaced, `$isHtml` no longer says anything about it.
#[test]
fn reassigning_subject_drops_the_assertion() {
    let backend = create_test_backend();
    let uri = "file:///assertion_stale.php";
    let text = format!(
        "{SCAFFOLD}
class C {{
    public function m(Renderable $raw, PlainString $other): string {{
        $isHtml = $raw instanceof HtmlString;
        $raw = $other;
        if ($isHtml) {{
            return $raw->toPlain();
        }}
        return 'x';
    }}
}}
"
    );
    let diags = unknown_member_diagnostics(&backend, uri, &text);
    assert!(
        diags.is_empty(),
        "A stale assertion must not re-narrow a reassigned subject, \
         got: {diags:?}"
    );
}

/// Reassigning the boolean itself drops the assertion it carried.
#[test]
fn reassigning_the_boolean_drops_the_assertion() {
    let backend = create_test_backend();
    let uri = "file:///assertion_rebound.php";
    let text = format!(
        "{SCAFFOLD}
class C {{
    public function m(Renderable $raw, bool $flag): string {{
        $isHtml = $raw instanceof HtmlString;
        $isHtml = $flag;
        if ($isHtml) {{
            return 'y';
        }}
        return 'x';
    }}
}}
"
    );
    let diags = unknown_member_diagnostics(&backend, uri, &text);
    assert!(
        diags.is_empty(),
        "Rebinding the boolean must not keep narrowing, got: {diags:?}"
    );
}
