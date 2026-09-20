//! Narrowing carried by a boolean variable.
//!
//! `$ok = $x instanceof Foo;` stores the assertion in `$ok`, so a later
//! truthy check on `$ok` must narrow `$x` exactly as the original
//! `instanceof` expression would.

use crate::common::{create_test_backend, unknown_member_diagnostics_with_scope_cache};
use tower_lsp::lsp_types::*;

fn argument_diagnostics(backend: &phpantom_lsp::Backend, uri: &str, text: &str) -> Vec<Diagnostic> {
    backend.update_ast(uri, text);
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri, text, &mut out);
    out.retain(|d| {
        d.code.as_ref().is_some_and(
            |c| matches!(c, NumberOrString::String(s) if s == "type_mismatch_argument"),
        )
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
function takesHtml(HtmlString $h): void {}
function takesEither(HtmlString|PlainString $r): void {}
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
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, &text);
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
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, &text);
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
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, &text);
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
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, &text);
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
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, &text);
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
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, &text);
    assert!(
        diags.is_empty(),
        "Rebinding the boolean must not keep narrowing, got: {diags:?}"
    );
}

/// A ternary arm reads the boolean back, so the arm has to see the check
/// the boolean stands for — not just the boolean's own truthiness.
#[test]
fn assertion_variable_narrows_the_value_a_ternary_arm_yields() {
    let backend = create_test_backend();
    let uri = "file:///assertion_ternary_value.php";
    let text = format!(
        "{SCAFFOLD}
class C {{
    public function m(Renderable $raw, HtmlString $fallback): void {{
        $isHtml = $raw instanceof HtmlString;
        $picked = $isHtml ? $raw : $fallback;
        takesHtml($picked);
    }}
}}
"
    );
    let diags = argument_diagnostics(&backend, uri, &text);
    assert!(
        diags.is_empty(),
        "The then-arm of a ternary on a boolean assertion should yield the \
         narrowed subject, got: {diags:?}"
    );
}

/// The subject the boolean narrows can be an array element rather than a
/// plain variable, and the ternary has to read it back the same way.
#[test]
fn assertion_variable_narrows_an_array_element_in_a_ternary() {
    let backend = create_test_backend();
    let uri = "file:///assertion_ternary_dim.php";
    let text = format!(
        "{SCAFFOLD}
class C {{
    /** @param array<int, Renderable> $items */
    public function m(array $items, int $i, HtmlString $fallback): void {{
        $isHtml = $items[$i] instanceof HtmlString;
        $picked = $isHtml ? $items[$i] : $fallback;
        takesHtml($picked);
    }}
}}
"
    );
    let diags = argument_diagnostics(&backend, uri, &text);
    assert!(
        diags.is_empty(),
        "A boolean recording a check on an array element should narrow it \
         in the ternary that reads the boolean back, got: {diags:?}"
    );
}

/// A boolean assigned an `||` chain stands for the whole disjunction: the
/// subject is one of the classes it lists.
#[test]
fn assertion_variable_carries_an_or_chain() {
    let backend = create_test_backend();
    let uri = "file:///assertion_or_chain.php";
    let text = format!(
        "{SCAFFOLD}
class C {{
    public function m(Renderable $raw): void {{
        $isKnown = $raw instanceof HtmlString || $raw instanceof PlainString;
        if ($isKnown) {{
            takesEither($raw);
        }}
    }}
}}
"
    );
    let diags = argument_diagnostics(&backend, uri, &text);
    assert!(
        diags.is_empty(),
        "A boolean holding an `||` chain of instanceof checks should narrow \
         its subject to the union, got: {diags:?}"
    );
}

/// The negation of that chain rules out every class it lists.
#[test]
fn a_negated_or_chain_boolean_excludes_every_class() {
    let backend = create_test_backend();
    let uri = "file:///assertion_or_chain_negated.php";
    let text = format!(
        "{SCAFFOLD}
class C {{
    public function m(Renderable $raw): void {{
        $isKnown = $raw instanceof HtmlString || $raw instanceof PlainString;
        if (!$isKnown) {{
            return;
        }}
        takesEither($raw);
    }}
}}
"
    );
    let diags = argument_diagnostics(&backend, uri, &text);
    assert!(
        diags.is_empty(),
        "A guard clause on the negated chain should leave the subject \
         narrowed to the union afterwards, got: {diags:?}"
    );
}

/// A leg about a different value makes the disjunction prove nothing
/// about either subject, so nothing may be recorded.
#[test]
fn an_or_chain_over_two_subjects_records_no_assertion() {
    let backend = create_test_backend();
    let uri = "file:///assertion_or_chain_mixed.php";
    let text = format!(
        "{SCAFFOLD}
class C {{
    public function m(Renderable $raw, Renderable $other): void {{
        $isEither = $raw instanceof HtmlString || $other instanceof HtmlString;
        if ($isEither) {{
            takesEither($raw);
        }}
    }}
}}
"
    );
    let diags = argument_diagnostics(&backend, uri, &text);
    assert!(
        diags.len() == 1 && diags[0].message.contains("got Repro\\Renderable"),
        "An `||` chain naming two subjects proves nothing about either, so \
         `$raw` stays `Renderable`, got: {diags:?}"
    );
}
