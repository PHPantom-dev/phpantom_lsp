//! Integration tests for `@param-out`: the type a by-reference parameter
//! leaves in the caller's variable, including a PHPStan conditional keyed
//! on what the caller passed.

use crate::common::{create_test_backend, hover_at, hover_text};

const OUT_PARAM_CLASSES: &str = r#"
class A {}

interface I { function method(): void; }

class Testing
{
    /**
     * @param A|null $arg
     * @param-out ($arg is null ? A&I : A) $arg
     */
    public static function testMethod(?A &$arg = null): void
    {
        if ($arg === null) {
            $arg = new A();
        }
    }

    /**
     * @param-out int $count
     */
    public static function count(mixed &$count): void
    {
    }
}
"#;

/// Hover on the last occurrence of `needle` in `content`.
fn hover_last(content: &str, needle: &str) -> String {
    let backend = create_test_backend();
    let offset = content.rfind(needle).expect("needle present");
    let before = &content[..offset];
    let line = before.matches('\n').count() as u32;
    let character = (offset - before.rfind('\n').map_or(0, |i| i + 1)) as u32;
    let hover =
        hover_at(&backend, "file:///test.php", content, line, character).expect("expected hover");
    hover_text(&hover).to_string()
}

/// A `null` handed to the out-parameter takes the conditional's `is null`
/// branch.
#[test]
fn conditional_param_out_takes_null_branch_for_null_argument() {
    let content = format!(
        r#"<?php
{OUT_PARAM_CLASSES}
function f(): void {{
    $b = null;
    Testing::testMethod($b);
    $b;
}}
"#
    );
    let text = hover_last(&content, "$b;");
    assert!(
        text.contains("A&I"),
        "expected $b to read as A&I after passing null, got: {text}"
    );
}

/// A non-null argument takes the conditional's other branch.
#[test]
fn conditional_param_out_takes_else_branch_for_object_argument() {
    let content = format!(
        r#"<?php
{OUT_PARAM_CLASSES}
function f(A $a): void {{
    Testing::testMethod($a);
    $a;
}}
"#
    );
    let text = hover_last(&content, "$a;");
    assert!(
        !text.contains("A&I") && text.contains('A'),
        "expected $a to read as plain A after passing an A, got: {text}"
    );
}

/// A plain `@param-out` type replaces the declared input type.
#[test]
fn plain_param_out_replaces_declared_input_type() {
    let content = format!(
        r#"<?php
{OUT_PARAM_CLASSES}
function f(): void {{
    $n = 'x';
    Testing::count($n);
    $n;
}}
"#
    );
    let text = hover_last(&content, "$n;");
    assert!(
        text.contains("int") && !text.contains("string"),
        "expected $n to read as int after count(), got: {text}"
    );
}
