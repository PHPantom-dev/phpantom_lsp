//! Integration tests for `@param-out`: the type a by-reference parameter
//! leaves in the caller's variable, including a PHPStan conditional keyed
//! on what the caller passed.

use tower_lsp_server::gen_lsp_types::Uri;

use crate::common::{create_test_backend, hover_last, line_char_of, markup_hover_at, open_php};

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
    let text = hover_last(&create_test_backend(), &content, "$b;");
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
    let text = hover_last(&create_test_backend(), &content, "$a;");
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
    let text = hover_last(&create_test_backend(), &content, "$n;");
    assert!(
        text.contains("int") && !text.contains("string"),
        "expected $n to read as int after count(), got: {text}"
    );
}

/// A `@param-out` wider than the argument replaces the argument's type
/// rather than keeping its shape, since it says what the callee writes.
#[test]
fn wider_param_out_replaces_argument_shape() {
    let content = r#"<?php
/** @param-out array $x */
function fill(array &$x): void {}

function f(): void {
    $values = [1 => 'a'];
    fill($values);
    $values;
}
"#;
    let text = hover_last(&create_test_backend(), content, "$values;");
    assert!(
        text.contains("array") && !text.contains("array{"),
        "expected $values to read as array after fill(), got: {text}"
    );
}

/// A `@param-out` naming the class's template, declared on an interface,
/// reads back as the type the implementing class binds it to.
#[test]
fn inherited_param_out_substitutes_class_template() {
    let content = r#"<?php
class Apple {}

/** @template T */
interface Filler {
    /** @param-out T $x */
    public function fill(mixed &$x): void;
}

/** @implements Filler<Apple> */
class AppleFiller implements Filler {
    public function fill(mixed &$x): void {}
}

function f(AppleFiller $filler): void {
    $value = 'x';
    $filler->fill($value);
    $value;
}
"#;
    let text = hover_last(&create_test_backend(), content, "$value;");
    assert!(
        text.contains("Apple") && !text.contains("string"),
        "expected $value to read as Apple after fill(), got: {text}"
    );
}

/// Each `return` leaves the parameter holding what it held there, so a
/// body whose every path returns reads back as the join of those, and the
/// closing brace no path reaches adds nothing.
#[tokio::test]
async fn every_return_contributes_to_the_out_type() {
    let content = r#"<?php
/** @param-out int $s */
function fill(?string &$s): void {
    if ($s === null) {
        $s = 5;
        return;
    }
    if ($s === '') {
        $s = 6;
        return;
    } else {
        $s = 7;
        return;
    }
}
function f(): void {
    fill($a);
    $a;
}
"#;
    // The body is read back from the open document, so it has to be opened
    // rather than only parsed.
    let backend = create_test_backend();
    let uri = Uri::parse("file:///test.php").unwrap();
    open_php(&backend, &uri, content).await;
    let (line, character) = line_char_of(content, "$a;");
    let text = markup_hover_at(&backend, &uri, line, character).await;
    assert!(
        text.contains('5') && text.contains('6') && text.contains('7') && !text.contains("string"),
        "expected $a to read as 5|6|7 after fill(), got: {text}"
    );
}
