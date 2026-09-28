//! Rename of variables, parameters, and closure/arrow-function bindings.

use crate::common::{
    apply_edits, create_test_backend, edits_for_uri, open_php, prepare_rename, rename, split_cursor,
};
use tower_lsp::lsp_types::*;

// ─── Variable Rename ────────────────────────────────────────────────────────

#[tokio::test]
async fn rename_variable_in_function() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $user = new User();\n",
        "    $user->name = 'Alice';\n",
        "    echo $user->name;\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Rename $user on line 2 (the assignment)
    let edit = rename(&backend, &uri, 2, 5, "$person").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for variable rename"
    );

    let edit = edit.unwrap();
    let file_edits = edits_for_uri(&edit, &uri);
    assert!(
        file_edits.len() >= 3,
        "Expected at least 3 edits for $user (decl + 2 usages), got {}",
        file_edits.len()
    );

    // All edits should use the new name with `$`.
    for te in &file_edits {
        assert_eq!(te.new_text, "$person");
    }
}

#[tokio::test]
async fn rename_variable_without_dollar_prefix() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $x = 1;\n",
        "    echo $x;\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // User provides new name without `$` — the handler should add it.
    let edit = rename(&backend, &uri, 2, 5, "y").await;
    assert!(edit.is_some());

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    for te in &file_edits {
        assert_eq!(te.new_text, "$y");
    }
}

#[tokio::test]
async fn rename_variable_updates_compact_string() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo(): array {\n",
        "    $user = 'alice';\n",
        "    return compact('user');\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 2, 6, "$person").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for variable rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let updated = apply_edits(text, &file_edits);
    assert!(updated.contains("$person = 'alice';"));
    assert!(updated.contains("compact('person')"));
}

#[tokio::test]
async fn rename_variable_updates_dynamic_property_selector() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo(object $message, string $type): void {\n",
        "    $attribute = strtolower($type);\n",
        "    if (empty($message->{$attribute})) {\n",
        "        return;\n",
        "    }\n",
        "    echo $attribute;\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 2, 6, "$field").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for variable rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let updated = apply_edits(text, &file_edits);
    assert!(updated.contains("$field = strtolower($type);"));
    assert!(updated.contains("$message->{$field}"));
    assert!(updated.contains("echo $field;"));
}

#[tokio::test]
async fn rename_from_compact_string_updates_variable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo(): array {\n",
        "    $user = 'alice';\n",
        "    return compact('user');\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 3, 21, "person").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for compact rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let updated = apply_edits(text, &file_edits);
    assert!(updated.contains("$person = 'alice';"));
    assert!(updated.contains("compact('person')"));
}

#[tokio::test]
async fn prepare_rename_variable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $count = 0;\n",
        "    $count++;\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let response = prepare_rename(&backend, &uri, 2, 6).await;
    assert!(
        response.is_some(),
        "Expected prepare rename to succeed for $count"
    );

    if let Some(PrepareRenameResponse::RangeWithPlaceholder { placeholder, .. }) = response {
        assert_eq!(placeholder, "$count");
    } else {
        panic!("Expected RangeWithPlaceholder response");
    }
}

#[tokio::test]
async fn prepare_rename_compact_string_uses_bare_name() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo(): array {\n",
        "    $user = 'alice';\n",
        "    return compact('user');\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let response = prepare_rename(&backend, &uri, 3, 21).await;
    assert!(
        response.is_some(),
        "Expected prepare rename on compact string"
    );

    if let Some(PrepareRenameResponse::RangeWithPlaceholder { placeholder, .. }) = response {
        assert_eq!(placeholder, "user");
    } else {
        panic!("Expected RangeWithPlaceholder response");
    }
}

// ─── Variable Scoping ───────────────────────────────────────────────────────

#[tokio::test]
async fn rename_variable_does_not_leak_across_functions() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function alpha(): void {\n",
        "    $x = 1;\n",
        "    echo $x;\n",
        "}\n",
        "function beta(): void {\n",
        "    $x = 2;\n",
        "    echo $x;\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Rename $x in alpha (line 2).
    let edit = rename(&backend, &uri, 2, 5, "$y").await;
    assert!(edit.is_some());

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    // alpha should have $y, beta should still have $x.
    assert!(result.contains("function alpha(): void {\n    $y = 1;\n    echo $y;\n}"));
    assert!(result.contains("function beta(): void {\n    $x = 2;\n    echo $x;\n}"));
}

// ─── Parameter Rename (closure / function / method) ─────────────────────────

#[tokio::test]
async fn rename_closure_parameter_from_param() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Factory {\n",
        "    public function build(): void {\n",
        "        $this->afterMaking(function (Box $item): void {\n",
        "            $item->item_id ??= $item->segment_id\n",
        "                ? $item->segment->run->item_id\n",
        "                : Item::randomOrFactoryCreate()->getKey();\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor on `$item` in the closure parameter list (line 3, col 44).
    let edit = rename(&backend, &uri, 3, 44, "$box").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for closure parameter rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    // The parameter and all usages in the closure body should be renamed.
    assert!(
        result.contains("function (Box $box)"),
        "Parameter declaration not renamed: {}",
        result
    );
    assert!(
        result.contains("$box->item_id"),
        "Body usage not renamed: {}",
        result
    );
    assert!(
        result.contains("$box->segment_id"),
        "Body usage not renamed: {}",
        result
    );
    assert!(
        result.contains("$box->segment->run"),
        "Chained body usage not renamed: {}",
        result
    );
    // Old name should be gone.
    assert!(
        !result.contains("$item"),
        "Old variable name still present: {}",
        result
    );
}

#[tokio::test]
async fn rename_closure_parameter_from_body_usage() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Factory {\n",
        "    public function build(): void {\n",
        "        $this->afterMaking(function (Box $item): void {\n",
        "            $item->name = 'test';\n",
        "            echo $item->name;\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor on `$item` in the closure body (line 4, col 13).
    let edit = rename(&backend, &uri, 4, 13, "$box").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit when renaming from body usage"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    // Both the parameter and body usages should be renamed.
    assert!(
        result.contains("function (Box $box)"),
        "Parameter declaration not renamed: {}",
        result
    );
    assert!(
        result.contains("$box->name = 'test'"),
        "Assignment usage not renamed: {}",
        result
    );
    assert!(
        result.contains("echo $box->name"),
        "Echo usage not renamed: {}",
        result
    );
    assert!(
        !result.contains("$item"),
        "Old variable name still present: {}",
        result
    );
}

#[tokio::test]
async fn rename_function_parameter_from_param() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function greet(string $name): string {\n",
        "    return 'Hello, ' . $name . '!';\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor on `$name` in the parameter list (line 1, col 23).
    let edit = rename(&backend, &uri, 1, 23, "$who").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for function parameter rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert!(
        result.contains("string $who)"),
        "Parameter not renamed: {}",
        result
    );
    assert!(
        result.contains("$who . '!'"),
        "Body usage not renamed: {}",
        result
    );
    assert!(
        !result.contains("$name"),
        "Old variable name still present: {}",
        result
    );
}

#[tokio::test]
async fn rename_method_parameter_from_param() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Greeter {\n",
        "    public function greet(string $name): string {\n",
        "        return 'Hello, ' . $name . '!';\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor on `$name` in the parameter list (line 2, col 35).
    let edit = rename(&backend, &uri, 2, 35, "$who").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for method parameter rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert!(
        result.contains("string $who)"),
        "Parameter not renamed: {}",
        result
    );
    assert!(
        result.contains("$who . '!'"),
        "Body usage not renamed: {}",
        result
    );
    assert!(
        !result.contains("$name"),
        "Old variable name still present: {}",
        result
    );
}

#[tokio::test]
async fn rename_parameter_includes_docblock_param_tag() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Greeter {\n",
        "    /**\n",
        "     * @param string $name The person's name.\n",
        "     */\n",
        "    public function greet(string $name): string {\n",
        "        return 'Hello, ' . $name . '!';\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor on `$name` in the parameter list (line 5, col 35).
    let edit = rename(&backend, &uri, 5, 35, "$who").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for parameter rename with docblock"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert!(
        result.contains("@param string $who"),
        "Docblock @param not renamed: {}",
        result
    );
    assert!(
        result.contains("string $who)"),
        "Parameter not renamed: {}",
        result
    );
    assert!(
        result.contains("$who . '!'"),
        "Body usage not renamed: {}",
        result
    );
    assert!(
        !result.contains("$name"),
        "Old variable name still present: {}",
        result
    );
}

#[tokio::test]
async fn rename_parameter_includes_docblock_from_body_usage() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @param string $name The person's name.\n",
        " */\n",
        "function greet(string $name): string {\n",
        "    return 'Hello, ' . $name . '!';\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor on `$name` in the function body (line 5, col 24).
    let edit = rename(&backend, &uri, 5, 24, "$who").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for parameter rename from body"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert!(
        result.contains("@param string $who"),
        "Docblock @param not renamed: {}",
        result
    );
    assert!(
        result.contains("string $who)"),
        "Parameter not renamed: {}",
        result
    );
    assert!(
        result.contains("$who . '!'"),
        "Body usage not renamed: {}",
        result
    );
    assert!(
        !result.contains("$name"),
        "Old variable name still present: {}",
        result
    );
}

#[tokio::test]
async fn rename_parameter_multiple_docblock_params() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Math {\n",
        "    /**\n",
        "     * @param int $a First operand.\n",
        "     * @param int $b Second operand.\n",
        "     */\n",
        "    public function add(int $a, int $b): int {\n",
        "        return $a + $b;\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Rename $a (line 6, col 29).
    let edit = rename(&backend, &uri, 6, 29, "$x").await;
    assert!(edit.is_some());

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    // Only $a should be renamed, not $b.
    assert!(
        result.contains("@param int $x First"),
        "Docblock @param for $a not renamed: {}",
        result
    );
    assert!(
        result.contains("@param int $b Second"),
        "Docblock @param for $b was wrongly renamed: {}",
        result
    );
    assert!(
        result.contains("int $x, int $b)"),
        "Parameter $a not renamed: {}",
        result
    );
    assert!(
        result.contains("return $x + $b"),
        "Body usage not renamed correctly: {}",
        result
    );
}

#[tokio::test]
async fn rename_parameter_includes_conditional_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @param bool $strict\n",
        " * @return ($strict is true ? Result : ?Result)\n",
        " */\n",
        "function findUser(bool $strict = true): ?Result {\n",
        "    if ($strict) {\n",
        "        throw new \\Exception('not found');\n",
        "    }\n",
        "    return null;\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor on `$strict` in the parameter list (line 5, col 23).
    let edit = rename(&backend, &uri, 5, 23, "$mustExist").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for parameter rename with conditional return"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert!(
        result.contains("@param bool $mustExist"),
        "Docblock @param not renamed: {}",
        result
    );
    assert!(
        result.contains("($mustExist is true"),
        "Conditional return type param not renamed: {}",
        result
    );
    assert!(
        result.contains("bool $mustExist ="),
        "Parameter not renamed: {}",
        result
    );
    assert!(
        result.contains("if ($mustExist)"),
        "Body usage not renamed: {}",
        result
    );
    assert!(
        !result.contains("$strict"),
        "Old variable name still present: {}",
        result
    );
}

#[tokio::test]
async fn rename_parameter_includes_nested_conditional_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @return ($strict is true ? Result : ($fallback is true ? Result : ?Result))\n",
        " */\n",
        "function findUser(bool $strict = true, bool $fallback = false): ?Result {\n",
        "    return null;\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Rename $fallback (line 4, col 45).
    let edit = rename(&backend, &uri, 4, 45, "$useFallback").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for nested conditional return rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert!(
        result.contains("($useFallback is true"),
        "Nested conditional return type param not renamed: {}",
        result
    );
    assert!(
        result.contains("bool $useFallback ="),
        "Parameter not renamed: {}",
        result
    );
    // $strict should remain untouched.
    assert!(
        result.contains("($strict is true"),
        "$strict was wrongly renamed: {}",
        result
    );
}

#[tokio::test]
async fn rename_parameter_conditional_return_from_body_usage() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @param bool $strict\n",
        " * @return ($strict is true ? Result : ?Result)\n",
        " */\n",
        "function findUser(bool $strict = true): ?Result {\n",
        "    if ($strict) {\n",
        "        throw new \\Exception('not found');\n",
        "    }\n",
        "    return null;\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor on `$strict` in the function body (line 6, col 8).
    let edit = rename(&backend, &uri, 6, 8, "$mustExist").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for rename from body with conditional return"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert!(
        result.contains("@param bool $mustExist"),
        "Docblock @param not renamed: {}",
        result
    );
    assert!(
        result.contains("($mustExist is true"),
        "Conditional return type param not renamed from body: {}",
        result
    );
    assert!(
        !result.contains("$strict"),
        "Old variable name still present: {}",
        result
    );
}

#[tokio::test]
async fn rename_arrow_function_parameter() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $fn = fn(int $x) => $x * 2;\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor on `$x` in the arrow function parameter (line 2, col 18).
    let edit = rename(&backend, &uri, 2, 18, "$n").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for arrow function parameter rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert!(
        result.contains("fn(int $n)"),
        "Arrow function parameter not renamed: {}",
        result
    );
    assert!(
        result.contains("$n * 2"),
        "Arrow function body not renamed: {}",
        result
    );
    assert!(
        !result.contains("$x"),
        "Old variable name still present: {}",
        result
    );
}

#[tokio::test]
async fn rename_closure_parameter_does_not_leak_to_outer_scope() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $item = 'outer';\n",
        "    $fn = function (string $item): string {\n",
        "        return $item . '!';\n",
        "    };\n",
        "    echo $item;\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Rename `$item` in the closure parameter (line 3, col 28).
    let edit = rename(&backend, &uri, 3, 28, "$inner").await;
    assert!(edit.is_some());

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    // Closure parameter and body should be renamed.
    assert!(
        result.contains("function (string $inner)"),
        "Closure parameter not renamed: {}",
        result
    );
    assert!(
        result.contains("return $inner . '!'"),
        "Closure body not renamed: {}",
        result
    );
    // Outer scope $item should NOT be renamed.
    assert!(
        result.contains("$item = 'outer'"),
        "Outer scope was wrongly renamed: {}",
        result
    );
    assert!(
        result.contains("echo $item"),
        "Outer scope echo was wrongly renamed: {}",
        result
    );
}

#[tokio::test]
async fn rename_function_param_propagates_into_closure_use_and_arrow() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function test(string $param): void\n",
        "{\n",
        "    function () use ($param): void {\n",
        "        echo $param;\n",
        "    };\n",
        "\n",
        "    fn () => $param;\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor on `$param` in the function parameter list (line 1, col 22).
    let edit = rename(&backend, &uri, 1, 22, "$renamed").await;
    assert!(edit.is_some(), "Expected a workspace edit");
    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    // Function parameter should be renamed.
    assert!(
        result.contains("function test(string $renamed)"),
        "Function parameter not renamed: {}",
        result
    );
    // Closure use should be renamed.
    assert!(
        result.contains("use ($renamed)"),
        "Closure use not renamed: {}",
        result
    );
    // Variable inside closure body should be renamed.
    assert!(
        result.contains("echo $renamed;"),
        "Closure body variable not renamed: {}",
        result
    );
    // Arrow function body should be renamed.
    assert!(
        result.contains("fn () => $renamed;"),
        "Arrow function body not renamed: {}",
        result
    );
}

// ─── Scope boundaries ───────────────────────────────────────────────────────
//
// Cases adapted from laravel-lsp's MIT-licensed test suite. Each renames the
// variable at the `§` cursor and compares the whole rewritten file, so an
// edit that strays into a neighbouring scope fails the test.

/// Renames the variable at the `§` cursor in `source` to `new_name` and
/// returns the rewritten file.
async fn rename_at_cursor(source: &str, new_name: &str) -> String {
    let (text, position) = split_cursor(source);
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    open_php(&backend, &uri, &text).await;
    let edit = rename(&backend, &uri, position.line, position.character, new_name)
        .await
        .expect("expected a workspace edit");
    apply_edits(&text, &edits_for_uri(&edit, &uri))
}

const CLOSURE_WITH_ITS_OWN_LOCAL: &str = "<?php
function outer() {
    $user = 1;
    $fn = function () {
        $user = 2;
        return $user;
    };
    return $user + $fn();
}
";

#[tokio::test]
async fn rename_outer_local_leaves_a_closures_same_named_local_alone() {
    let source = CLOSURE_WITH_ITS_OWN_LOCAL.replacen("$user = 1", "$§user = 1", 1);
    assert_eq!(
        rename_at_cursor(&source, "$person").await,
        CLOSURE_WITH_ITS_OWN_LOCAL
            .replace("$user = 1", "$person = 1")
            .replace("$user + $fn", "$person + $fn"),
    );
}

#[tokio::test]
async fn rename_closure_local_leaves_the_outer_same_named_local_alone() {
    let source = CLOSURE_WITH_ITS_OWN_LOCAL.replacen("$user = 2", "$§user = 2", 1);
    assert_eq!(
        rename_at_cursor(&source, "$person").await,
        CLOSURE_WITH_ITS_OWN_LOCAL
            .replace("$user = 2", "$person = 2")
            .replace("return $user;", "return $person;"),
    );
}

#[tokio::test]
async fn rename_variable_captured_by_reference_keeps_the_ampersand() {
    let result = rename_at_cursor(
        "<?php
function make() {
    $§count = 0;
    $inc = function () use (&$count) {
        $count++;
    };
    $inc();
    return $count;
}
",
        "$total",
    )
    .await;
    assert_eq!(
        result,
        "<?php
function make() {
    $total = 0;
    $inc = function () use (&$total) {
        $total++;
    };
    $inc();
    return $total;
}
"
    );
}

#[tokio::test]
async fn rename_by_reference_parameter_keeps_the_ampersand() {
    let result = rename_at_cursor(
        "<?php
function bump(&$§c) {
    $c = $c + 1;
    return $c;
}
",
        "$counter",
    )
    .await;
    assert_eq!(
        result,
        "<?php
function bump(&$counter) {
    $counter = $counter + 1;
    return $counter;
}
"
    );
}

const ARROW_PARAM_SHADOWING_A_LOCAL: &str = "<?php
function f() {
    $x = 1;
    $g = fn($x) => $x * 2;
    return $g($x);
}
";

#[tokio::test]
async fn rename_outer_local_leaves_a_shadowing_arrow_parameter_alone() {
    let source = ARROW_PARAM_SHADOWING_A_LOCAL.replacen("$x = 1", "$§x = 1", 1);
    assert_eq!(
        rename_at_cursor(&source, "$y").await,
        ARROW_PARAM_SHADOWING_A_LOCAL
            .replace("$x = 1", "$y = 1")
            .replace("$g($x)", "$g($y)"),
    );
}

#[tokio::test]
async fn rename_arrow_parameter_leaves_the_shadowed_outer_local_alone() {
    let source = ARROW_PARAM_SHADOWING_A_LOCAL.replacen("fn($x)", "fn($§x)", 1);
    assert_eq!(
        rename_at_cursor(&source, "$y").await,
        ARROW_PARAM_SHADOWING_A_LOCAL.replace("fn($x) => $x * 2", "fn($y) => $y * 2"),
    );
}

#[tokio::test]
async fn rename_local_reaches_an_arrow_function_that_captures_it() {
    let result = rename_at_cursor(
        "<?php
function calc() {
    $§base = 10;
    $add = fn($x) => $x + $base;
    return $add(1) + $base;
}
",
        "$offset",
    )
    .await;
    assert_eq!(
        result,
        "<?php
function calc() {
    $offset = 10;
    $add = fn($x) => $x + $offset;
    return $add(1) + $offset;
}
"
    );
}

#[tokio::test]
async fn rename_parameter_leaves_same_named_properties_alone() {
    let source = "<?php
class Account {
    public static $user = 's';
    public $user2;
    public function show($§user) {
        $this->user = $user;
        return self::$user . $user;
    }
}
";
    assert_eq!(
        rename_at_cursor(source, "$member").await,
        "<?php
class Account {
    public static $user = 's';
    public $user2;
    public function show($member) {
        $this->user = $member;
        return self::$user . $member;
    }
}
"
    );
}

#[tokio::test]
async fn rename_top_level_script_variable() {
    let result = rename_at_cursor(
        "<?php
$§user = 'a';
echo $user;
function f() {
    $user = 'b';
    return $user;
}
",
        "$name",
    )
    .await;
    assert_eq!(
        result,
        "<?php
$name = 'a';
echo $name;
function f() {
    $user = 'b';
    return $user;
}
"
    );
}

#[tokio::test]
async fn rename_outer_local_leaves_a_nested_closures_compact_string_alone() {
    let source = "<?php
function outer() {
    $§user = cu();
    $cb = function () {
        $user = g();
        return compact('user');
    };
    return [$user, $cb];
}
";
    assert_eq!(
        rename_at_cursor(source, "$account").await,
        "<?php
function outer() {
    $account = cu();
    $cb = function () {
        $user = g();
        return compact('user');
    };
    return [$account, $cb];
}
"
    );
}

#[tokio::test]
async fn rename_closure_local_rewrites_its_own_compact_string() {
    let source = "<?php
function outer() {
    $user = cu();
    $cb = function () {
        $§user = g();
        return compact('user');
    };
    return [$user, $cb];
}
";
    assert_eq!(
        rename_at_cursor(source, "$account").await,
        "<?php
function outer() {
    $user = cu();
    $cb = function () {
        $account = g();
        return compact('account');
    };
    return [$user, $cb];
}
"
    );
}

#[tokio::test]
async fn rename_variable_used_as_a_variable_variable_name() {
    let result = rename_at_cursor(
        "<?php
function f() {
    $§name = 'a';
    $$name = 1;
    return $name;
}
",
        "$key",
    )
    .await;
    assert_eq!(
        result,
        "<?php
function f() {
    $key = 'a';
    $$key = 1;
    return $key;
}
"
    );
}

#[tokio::test]
async fn rename_variable_inside_interpolated_strings_and_heredoc_but_not_nowdoc() {
    let result = rename_at_cursor(
        "<?php
function f() {
    $§x = 1;
    $a = \"Hello $x\";
    $b = \"V {$x}\";
    $h = <<<EOT
val $x
EOT;
    $n = <<<'EOT'
raw $x
EOT;
    return $a . $b . $h . $n;
}
",
        "$y",
    )
    .await;
    assert_eq!(
        result,
        "<?php
function f() {
    $y = 1;
    $a = \"Hello $y\";
    $b = \"V {$y}\";
    $h = <<<EOT
val $y
EOT;
    $n = <<<'EOT'
raw $x
EOT;
    return $a . $b . $h . $n;
}
"
    );
}

#[tokio::test]
async fn rename_variable_used_as_an_unbraced_dynamic_property_name() {
    let result = rename_at_cursor(
        "<?php
function f($obj, $§key) {
    return $obj->$key;
}
",
        "$field",
    )
    .await;
    assert_eq!(
        result,
        "<?php
function f($obj, $field) {
    return $obj->$field;
}
"
    );
}

const GLOBAL_COUNT: &str = "<?php
$count = 10;
function bump() {
    global $count;
    $count = 20;
    return $count;
}
function other() {
    $count = 1;
    return $count;
}
echo $count;
";

/// [`GLOBAL_COUNT`] with every `$count` bound to the global renamed to `$total`.
fn global_count_renamed() -> String {
    GLOBAL_COUNT
        .replace("$count = 10", "$total = 10")
        .replace(
            "global $count;\n    $count = 20;\n    return $count;",
            "global $total;\n    $total = 20;\n    return $total;",
        )
        .replace("echo $count", "echo $total")
}

#[tokio::test]
async fn rename_top_level_variable_reaches_functions_that_declare_it_global() {
    let source = GLOBAL_COUNT.replacen("$count = 10", "$§count = 10", 1);
    assert_eq!(
        rename_at_cursor(&source, "$total").await,
        global_count_renamed()
    );
}

#[tokio::test]
async fn rename_from_a_global_declaration_reaches_the_top_level_variable() {
    let source = GLOBAL_COUNT.replacen("global $count", "global $§count", 1);
    assert_eq!(
        rename_at_cursor(&source, "$total").await,
        global_count_renamed()
    );
}

#[tokio::test]
async fn rename_from_a_use_of_a_global_reaches_the_top_level_variable() {
    let source = GLOBAL_COUNT.replacen("$count = 20", "$§count = 20", 1);
    assert_eq!(
        rename_at_cursor(&source, "$total").await,
        global_count_renamed()
    );
}

#[tokio::test]
async fn rename_outer_local_leaves_a_closure_that_declares_the_name_global_alone() {
    let source = "<?php
function outer() {
    $§x = 1;
    $fn = function () {
        global $x;
        return $x;
    };
    return $x;
}
";
    assert_eq!(
        rename_at_cursor(source, "$y").await,
        "<?php
function outer() {
    $y = 1;
    $fn = function () {
        global $x;
        return $x;
    };
    return $y;
}
"
    );
}
