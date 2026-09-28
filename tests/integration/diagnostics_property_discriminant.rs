//! Narrowing a union of object types by a check on a discriminating
//! property.
//!
//! `is_string($b->v)` on a `StrBox|IntBox` subject proves the value is a
//! `StrBox` when `IntBox::$v` is declared `int`, and an identity check
//! against a literal (`$r->tag === 'ok'`) discriminates a tagged union
//! the same way.  A member whose property could also have passed the
//! check must survive it.

use crate::common::{collect_diagnostics_with, create_test_backend, messages_with_code};
use phpantom_lsp::Backend;
use tower_lsp::lsp_types::*;

fn collect(php: &str) -> Vec<Diagnostic> {
    collect_diagnostics_with(
        &create_test_backend(),
        php,
        Backend::collect_return_type_diagnostics,
    )
}

fn has_return_error(diags: &[Diagnostic]) -> bool {
    !messages_with_code(diags, "type_mismatch_return").is_empty()
}

/// Two boxes whose `$v` differs only in its declared scalar type.
const BOXES: &str = r#"<?php
final class StrBox { public string $v = ''; }
final class IntBox { public int $v = 0; }
final class WideBox { /** @var string|int */ public $v = 0; }
"#;

/// A tagged union: the discriminant is a literal-typed property.
const TAGGED: &str = r#"<?php
final class Success { /** @var 'ok' */ public string $tag = 'ok'; }
final class Failure { /** @var 'err' */ public string $tag = 'err'; }
"#;

// ─── Type guards ────────────────────────────────────────────────────────────

#[test]
fn type_guard_narrows_union_to_the_member_that_could_pass() {
    let php = format!(
        "{BOXES}
function pick(StrBox|IntBox $b): StrBox {{
    if (is_string($b->v)) {{
        return $b;
    }}
    throw new \\LogicException('not a StrBox');
}}
"
    );
    let diags = collect(&php);
    assert!(
        !has_return_error(&diags),
        "is_string(\\$b->v) should narrow StrBox|IntBox to StrBox, got: {diags:?}"
    );
}

#[test]
fn else_branch_keeps_the_member_that_could_fail() {
    let php = format!(
        "{BOXES}
function pick(StrBox|IntBox $b): IntBox {{
    if (is_string($b->v)) {{
        throw new \\LogicException('not an IntBox');
    }}
    return $b;
}}
"
    );
    let diags = collect(&php);
    assert!(
        !has_return_error(&diags),
        "The else branch should narrow StrBox|IntBox to IntBox, got: {diags:?}"
    );
}

#[test]
fn guard_clause_narrows_after_the_if() {
    let php = format!(
        "{BOXES}
function pick(StrBox|IntBox $b): StrBox {{
    if (!is_string($b->v)) {{
        throw new \\LogicException('not a StrBox');
    }}
    return $b;
}}
"
    );
    let diags = collect(&php);
    assert!(
        !has_return_error(&diags),
        "A negated guard clause should leave StrBox after the if, got: {diags:?}"
    );
}

#[test]
fn and_chain_narrows_through_its_other_conjuncts() {
    let php = format!(
        "{BOXES}
function pick(StrBox|IntBox $b, bool $flag): StrBox {{
    if ($flag && is_string($b->v)) {{
        return $b;
    }}
    throw new \\LogicException('not a StrBox');
}}
"
    );
    let diags = collect(&php);
    assert!(
        !has_return_error(&diags),
        "A guard inside an && chain should still narrow, got: {diags:?}"
    );
}

#[test]
fn member_whose_property_could_also_pass_is_kept() {
    let php = format!(
        "{BOXES}
function pick(StrBox|WideBox $b): StrBox {{
    if (is_string($b->v)) {{
        return $b;
    }}
    throw new \\LogicException('not a StrBox');
}}
"
    );
    let diags = collect(&php);
    assert!(
        has_return_error(&diags),
        "WideBox declares $v as string|int, so is_string() cannot rule it out: {diags:?}"
    );
}

// ─── Identity checks ────────────────────────────────────────────────────────

#[test]
fn identity_check_against_a_literal_discriminates_a_tagged_union() {
    let php = format!(
        "{TAGGED}
function pick(Success|Failure $r): Success {{
    if ($r->tag === 'ok') {{
        return $r;
    }}
    throw new \\LogicException('not a Success');
}}
"
    );
    let diags = collect(&php);
    assert!(
        !has_return_error(&diags),
        "$r->tag === 'ok' should narrow Success|Failure to Success, got: {diags:?}"
    );
}

#[test]
fn inverse_identity_check_keeps_the_other_tag() {
    let php = format!(
        "{TAGGED}
function pick(Success|Failure $r): Failure {{
    if ($r->tag !== 'err') {{
        throw new \\LogicException('not a Failure');
    }}
    return $r;
}}
"
    );
    let diags = collect(&php);
    assert!(
        !has_return_error(&diags),
        "$r->tag !== 'err' should leave Failure in the else branch, got: {diags:?}"
    );
}

#[test]
fn identity_check_discriminates_by_scalar_family() {
    let php = format!(
        "{BOXES}
function pick(StrBox|IntBox $b): StrBox {{
    if ($b->v === 'x') {{
        return $b;
    }}
    throw new \\LogicException('not a StrBox');
}}
"
    );
    let diags = collect(&php);
    assert!(
        !has_return_error(&diags),
        "An int property can never be identical to a string, got: {diags:?}"
    );
}

#[test]
fn not_identical_only_rules_out_a_property_pinned_to_that_value() {
    let php = format!(
        "{BOXES}
function pick(StrBox|IntBox $b): IntBox {{
    if ($b->v !== 'x') {{
        return $b;
    }}
    throw new \\LogicException('not an IntBox');
}}
"
    );
    let diags = collect(&php);
    assert!(
        has_return_error(&diags),
        "A string property other than 'x' also satisfies !== 'x': {diags:?}"
    );
}

// ─── Cases the check says nothing about ─────────────────────────────────────

#[test]
fn union_is_left_alone_when_every_member_could_pass() {
    let php = format!(
        "{BOXES}
function pick(StrBox|WideBox $b): StrBox|WideBox {{
    if (is_string($b->v)) {{
        return $b;
    }}
    throw new \\LogicException('unreachable');
}}
"
    );
    let diags = collect(&php);
    assert!(
        !has_return_error(&diags),
        "Both members can pass, so the union stands: {diags:?}"
    );
}

#[test]
fn union_is_left_alone_when_the_property_is_undeclared() {
    let php = r#"<?php
class Loose { public function __get(string $name): mixed { return null; } }
final class StrBox { public string $v = ''; }

function pick(StrBox|Loose $b): StrBox {
    if (is_string($b->v)) {
        return $b;
    }
    throw new \LogicException('not a StrBox');
}
"#;
    let diags = collect(php);
    assert!(
        has_return_error(&diags),
        "A member that declares no $v cannot be ruled out: {diags:?}"
    );
}
