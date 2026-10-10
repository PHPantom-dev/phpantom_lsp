//! Integration tests for the "Convert to Instance Variable" code action
//! (`refactor.extract`).
//!
//! These tests exercise the full two-phase pipeline: a cursor on a local
//! variable assignment inside a method body offers an action with no edit
//! but with resolve data, and resolving it produces a new `private`
//! property declaration plus `$this->…` (or `self::$…` in a static
//! method) replacements for every occurrence of the variable in the
//! method scope.

use crate::common::{
    apply_edits, create_test_backend, extract_edits, find_action, get_code_actions_in_range,
    position_of, resolve_action,
};
use tower_lsp::lsp_types::*;

/// Helper: given PHP source with a cursor marker `/*|*/`, run the
/// convert-to-instance-variable action and return the resulting edits.
fn run_convert(php: &str) -> Option<Vec<TextEdit>> {
    let marker = "/*|*/";
    // Everything before the marker is identical in `php` and `content`,
    // so the marker's own position is the cursor position once it is
    // stripped out.
    let position = position_of(php, marker);
    let content = php.replace(marker, "");

    let uri = "file:///test.php";
    let backend = create_test_backend();
    backend.update_ast(uri, &content);

    let actions =
        get_code_actions_in_range(&backend, uri, &content, Range::new(position, position));
    let action = find_action(&actions, "Convert $")?;

    assert!(action.edit.is_none(), "Phase 1 should not compute edits");
    assert!(action.data.is_some(), "Phase 1 should attach resolve data");

    // Phase 2: resolve.
    let resolved = resolve_action(&backend, uri, &content, action);
    Some(extract_edits(&resolved))
}

// ── Basic conversion ────────────────────────────────────────────────

#[test]
fn converts_simple_variable() {
    let php =
        "<?php\nclass Foo {\n    public function bar() {\n        /*|*/$result = 42;\n    }\n}";
    let content = php.replace("/*|*/", "");
    let edits = run_convert(php).expect("action should be offered");
    let result = apply_edits(&content, &edits);
    assert!(
        result.contains("private $result;"),
        "should declare property: {}",
        result
    );
    assert!(
        result.contains("$this->result = 42;"),
        "should replace assignment: {}",
        result
    );
}

#[test]
fn replaces_all_occurrences_in_method() {
    let php = "<?php\nclass Foo {\n    public function bar() {\n        /*|*/$x = 1;\n        echo $x;\n        return $x;\n    }\n}";
    let content = php.replace("/*|*/", "");
    let edits = run_convert(php).expect("action should be offered");
    let result = apply_edits(&content, &edits);
    assert!(
        result.contains("private $x;"),
        "should declare property: {}",
        result
    );
    // All three occurrences should be replaced.
    let count = result.matches("$this->x").count();
    assert_eq!(
        count, 3,
        "should replace all 3 occurrences, got: {}",
        result
    );
    // No bare $x should remain in the method body.
    // The property declaration `private $x;` still contains `$x`, so
    // check that the method body lines don't have bare `$x`.
    assert!(
        !result.contains("echo $x;") && !result.contains("return $x;"),
        "no bare $x should remain in method body: {}",
        result
    );
}

#[test]
fn rejects_when_property_exists() {
    let php = "<?php\nclass Foo {\n    private $result;\n    public function bar() {\n        /*|*/$result = 42;\n    }\n}";
    assert!(
        run_convert(php).is_none(),
        "should not offer action when property exists"
    );
}

#[test]
fn rejects_when_property_is_inherited() {
    let php = "<?php\nclass Base { protected $x; }\nclass Foo extends Base {\n    public function bar() {\n        /*|*/$x = 1;\n    }\n}";
    assert!(
        run_convert(php).is_none(),
        "should not offer action when a parent declares the property"
    );
}

#[test]
fn offers_when_parent_property_is_private() {
    let php = "<?php\nclass Base { private $x; }\nclass Foo extends Base {\n    public function bar() {\n        /*|*/$x = 1;\n    }\n}";
    assert!(run_convert(php).is_some());
}

#[test]
fn rejects_when_promoted_property_exists() {
    let php = "<?php\nclass Foo {\n    public function __construct(private $result) {}\n    public function bar() {\n        /*|*/$result = 42;\n    }\n}";
    assert!(
        run_convert(php).is_none(),
        "should not offer action when promoted property exists"
    );
}

#[test]
fn converts_in_static_method() {
    let php = "<?php\nclass Foo {\n    public static function bar() {\n        /*|*/$result = 42;\n    }\n}";
    let content = php.replace("/*|*/", "");
    let edits = run_convert(php).expect("action should be offered for static method");
    let result = apply_edits(&content, &edits);
    assert!(
        result.contains("private static $result;"),
        "should declare static property: {}",
        result
    );
    assert!(
        result.contains("self::$result = 42;"),
        "should use self:: access: {}",
        result
    );
}

#[test]
fn rejects_outside_method_body() {
    let php = "<?php\n/*|*/$result = 42;\n";
    assert!(
        run_convert(php).is_none(),
        "should not offer action outside a method"
    );
}

#[test]
fn rejects_this_variable() {
    // $this can never be converted — it's special.
    let php = "<?php\nclass Foo {\n    public function bar() {\n        /*|*/$this = new self();\n    }\n}";
    assert!(
        run_convert(php).is_none(),
        "should not offer action for $this"
    );
}

// ── Closures, arrow functions, and declaring positions ─────────────

/// Run the conversion on a method `bar` whose body is `body` (with the
/// cursor on its first statement) and return the rewritten method body,
/// or `None` when the action is not offered.
fn convert_body(body: &str, is_static: bool) -> Option<String> {
    let modifier = if is_static { "static " } else { "" };
    let php = format!(
        "<?php\nclass Foo {{\n    public {modifier}function bar() {{\n        /*|*/$x = 1;\n{body}\n    }}\n}}"
    );
    let content = php.replace("/*|*/", "");
    let edits = run_convert(&php)?;
    let result = apply_edits(&content, &edits);
    assert!(result.contains("x = 1;\n") && !result.contains(" $x = 1;"));
    let start = result.find("x = 1;\n").map(|i| i + "x = 1;\n".len())?;
    let end = result.rfind("\n    }\n}")?;
    Some(result[start..end].to_string())
}

#[test]
fn closure_use_capture_is_dropped_and_body_converted() {
    let body = convert_body("        $f = function () use ($x) { return $x; };", false)
        .expect("action should be offered");
    assert_eq!(body, "        $f = function () { return $this->x; };");
}

#[test]
fn closure_use_capture_is_dropped_from_a_longer_list() {
    let body = convert_body(
        "        $a = 2;\n        $f = function () use ($a, &$x) { $x++; return $a; };\n        $g = function () use ($x, $a): int { return $x + $a; };",
        false,
    )
    .expect("action should be offered");
    assert_eq!(
        body,
        "        $a = 2;\n        $f = function () use ($a) { $this->x++; return $a; };\n        $g = function () use ($a): int { return $this->x + $a; };"
    );
}

#[test]
fn arrow_function_body_is_converted() {
    let body =
        convert_body("        $h = fn() => $x + 1;", false).expect("action should be offered");
    assert_eq!(body, "        $h = fn() => $this->x + 1;");
}

#[test]
fn arrow_function_parameter_shadows_the_variable() {
    let body = convert_body("        $h = fn($x) => $x + 1;\n        return $x;", false)
        .expect("action should be offered");
    assert_eq!(
        body,
        "        $h = fn($x) => $x + 1;\n        return $this->x;"
    );
}

#[test]
fn closure_without_capture_keeps_its_own_variable() {
    let body = convert_body(
        "        $f = function () { $x = 2; return $x; };\n        return $x;",
        false,
    )
    .expect("action should be offered");
    assert_eq!(
        body,
        "        $f = function () { $x = 2; return $x; };\n        return $this->x;"
    );
}

#[test]
fn nested_captures_are_converted() {
    let body = convert_body(
        "        $f = function () use ($x) { return fn() => function () use ($x) { return $x; }; };",
        false,
    )
    .expect("action should be offered");
    assert_eq!(
        body,
        "        $f = function () { return fn() => function () { return $this->x; }; };"
    );
}

#[test]
fn rejects_static_closure_capture_in_instance_method() {
    assert!(convert_body("        $f = static fn() => $x;", false).is_none());
    assert!(
        convert_body(
            "        $f = static function () use ($x) { return $x; };",
            false
        )
        .is_none()
    );
}

#[test]
fn static_closure_capture_converts_in_static_method() {
    let body = convert_body(
        "        $f = static function () use ($x) { return $x; };",
        true,
    )
    .expect("action should be offered");
    assert_eq!(
        body,
        "        $f = static function () { return self::$x; };"
    );
}

#[test]
fn rejects_declaring_positions() {
    for body in [
        "        try {} catch (\\Exception $x) {}",
        "        static $x;",
        "        global $x;",
    ] {
        assert!(
            convert_body(body, false).is_none(),
            "should not offer action with `{body}`"
        );
    }
}

#[test]
fn rejects_access_by_name() {
    for body in [
        "        return compact('x');",
        "        return compact(['y', 'x']);",
        "        extract([]);",
        "        $name = 'x';\n        return $$name;",
    ] {
        assert!(
            convert_body(body, false).is_none(),
            "should not offer action with `{body}`"
        );
    }
}

#[test]
fn compact_of_other_variables_is_allowed() {
    let body = convert_body(
        "        $y = 2;\n        return compact('y') + [$x];",
        false,
    )
    .expect("action should be offered");
    assert_eq!(
        body,
        "        $y = 2;\n        return compact('y') + [$this->x];"
    );
}

#[test]
fn access_by_name_in_unrelated_closure_is_allowed() {
    let body = convert_body(
        "        $f = function () { return compact('x'); };\n        return $x;",
        false,
    )
    .expect("action should be offered");
    assert_eq!(
        body,
        "        $f = function () { return compact('x'); };\n        return $this->x;"
    );
}

#[test]
fn rejects_variable_that_is_a_method_parameter() {
    let php = "<?php\nclass Foo {\n    public function bar($x) {\n        echo $x;\n        /*|*/$x = 1;\n    }\n}";
    assert!(run_convert(php).is_none());
}
