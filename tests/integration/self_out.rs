//! Integration tests for `@psalm-this-out` / `@phpstan-self-out`: a method
//! call that changes the type the walker tracks for its receiver, the way
//! an assignment changes a variable's type.

use crate::common::{create_test_backend, hover_at, hover_text};
use tower_lsp::lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

const MUTABLE_BOX: &str = r#"
/** @template T */
final class MutableBox {
    /** @param T $value */
    public function __construct(public mixed $value) {}

    /** @return T */
    public function get(): mixed { return $this->value; }

    /**
     * @template U
     * @param U $value
     * @psalm-this-out self<U>
     * @phpstan-self-out self<U>
     */
    public function replace(mixed $value): void {}
}

final class Pen { public function write(): string { return ''; } }
final class Pencil { public function sketch(): string { return ''; } }
"#;

// ─── Tests ──────────────────────────────────────────────────────────────────

/// After `$box->replace('x')`, the walker must read `$box` as
/// `MutableBox<string>` rather than the pre-call `MutableBox<int>` — the
/// same way it would after a reassignment.
#[test]
fn hover_shows_self_out_substituted_type_after_call() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = format!(
        r#"<?php
{MUTABLE_BOX}

/** @param MutableBox<int> $box */
function f(MutableBox $box): void {{
    $box->replace('x');
    $box;
}}
"#
    );

    // Hover on `$box;` on the last statement of `f`.
    let hover = hover_at(&backend, uri, &content, 26, 4).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("MutableBox<string>"),
        "expected $box to read as MutableBox<string> after replace('x'), got: {text}"
    );
    assert!(
        !text.contains("MutableBox<int>"),
        "did not expect $box to still read as MutableBox<int>, got: {text}"
    );
}

/// Before the `replace()` call, `$box` must still read as the declared
/// `MutableBox<int>` — the mutation must not retroactively apply.
#[test]
fn hover_shows_unmutated_type_before_self_out_call() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = format!(
        r#"<?php
{MUTABLE_BOX}

/** @param MutableBox<int> $box */
function f(MutableBox $box): void {{
    $box;
    $box->replace('x');
}}
"#
    );

    // Hover on `$box;`, the statement before the `replace()` call.
    let hover = hover_at(&backend, uri, &content, 25, 4).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("MutableBox<int>"),
        "expected $box to still read as MutableBox<int> before replace('x'), got: {text}"
    );
}

/// The mutation must reach *member* resolution too, not just the variable's
/// own hover: a member typed by the class's template parameter has to follow
/// the new binding.  Keeping the receiver's pre-call `class_info` would leave
/// `get()` and `$value` resolving to the old type argument, which then
/// reports a false "method not found" on the correct call.
#[test]
fn members_follow_the_receivers_new_template_binding() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = format!(
        r#"<?php
{MUTABLE_BOX}

/** @param MutableBox<Pen> $box */
function f(MutableBox $box): void {{
    $box->replace(new Pencil());
    $box->get();
    $box->get()->sketch();
    $box->value->sketch();
}}
"#
    );

    backend.update_ast(uri, &content);

    // `@return T` on get() must resolve through the post-call binding.
    let hover = hover_at(&backend, uri, &content, 26, 11).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("Pencil"),
        "expected get() to return Pencil after the self-out call, got: {text}"
    );

    // ...and calling a Pencil-only method on the result must not be reported.
    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri, &content, &mut diags);
    let unknown_members: Vec<&String> = diags
        .iter()
        .filter(|d| {
            d.code
                .as_ref()
                .is_some_and(|c| matches!(c, NumberOrString::String(s) if s == "unknown_member"))
        })
        .map(|d| &d.message)
        .collect();
    assert!(
        unknown_members.is_empty(),
        "expected no unknown-member reports after the self-out call, got: {unknown_members:?}"
    );
}

/// A method with template params but no `@psalm-this-out` /
/// `@phpstan-self-out` tag must leave the receiver's type untouched — the
/// walker should not treat every templated call as a mutation.
#[test]
fn no_self_out_mutation_without_the_tag() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/** @template T */
final class Box {
    /** @param T $value */
    public function __construct(public mixed $value) {}

    /**
     * @template U
     * @param U $value
     */
    public function peek(mixed $value): void {}
}

/** @param Box<int> $box */
function f(Box $box): void {
    $box->peek('x');
    $box;
}
"#;

    let hover = hover_at(&backend, uri, content, 16, 4).expect("expected hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("Box<int>"),
        "expected $box to remain Box<int> without a self-out tag, got: {text}"
    );
    assert!(
        !text.contains("Box<string>"),
        "did not expect $box to be mutated without a self-out tag, got: {text}"
    );
}
