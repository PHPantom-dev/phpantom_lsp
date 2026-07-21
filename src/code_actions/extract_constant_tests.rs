use super::*;

// ── is_extractable_literal ──────────────────────────────────────

// ── literal_type_name ───────────────────────────────────────────

#[test]
fn type_string_literal() {
    assert_eq!(literal_type_name("'hello'"), Some(PhpType::string()));
}

#[test]
fn type_double_quoted_string() {
    assert_eq!(literal_type_name("\"hello\""), Some(PhpType::string()));
}

#[test]
fn type_integer() {
    assert_eq!(literal_type_name("42"), Some(PhpType::int()));
}

#[test]
fn type_hex() {
    assert_eq!(literal_type_name("0xFF"), Some(PhpType::int()));
}

#[test]
fn type_float() {
    assert_eq!(literal_type_name("3.14"), Some(PhpType::float()));
}

#[test]
fn type_float_exponent() {
    assert_eq!(literal_type_name("1e10"), Some(PhpType::float()));
}

#[test]
fn type_negative_int() {
    assert_eq!(literal_type_name("-42"), Some(PhpType::int()));
}

#[test]
fn type_negative_float() {
    assert_eq!(literal_type_name("-3.14"), Some(PhpType::float()));
}

#[test]
fn type_true() {
    assert_eq!(literal_type_name("true"), Some(PhpType::bool()));
}

#[test]
fn type_false() {
    assert_eq!(literal_type_name("false"), Some(PhpType::bool()));
}

#[test]
fn type_null_returns_none() {
    assert_eq!(literal_type_name("null"), None);
}

#[test]
fn type_concat_is_string() {
    assert_eq!(literal_type_name("'a' . 'b'"), Some(PhpType::string()));
}

// ── is_extractable_literal ──────────────────────────────────────

#[test]
fn literal_single_quoted_string() {
    assert!(is_extractable_literal("'pending'"));
}

#[test]
fn literal_double_quoted_string() {
    assert!(is_extractable_literal("\"active\""));
}

#[test]
fn literal_integer() {
    assert!(is_extractable_literal("200"));
}

#[test]
fn literal_hex() {
    assert!(is_extractable_literal("0xFF"));
}

#[test]
fn literal_binary() {
    assert!(is_extractable_literal("0b1010"));
}

#[test]
fn literal_octal() {
    assert!(is_extractable_literal("0o77"));
}

#[test]
fn literal_float() {
    assert!(is_extractable_literal("3.14"));
}

#[test]
fn literal_float_exponent() {
    assert!(is_extractable_literal("1e10"));
}

#[test]
fn literal_negative_integer() {
    assert!(is_extractable_literal("-42"));
}

#[test]
fn literal_negative_float() {
    assert!(is_extractable_literal("-3.14"));
}

#[test]
fn literal_true() {
    assert!(is_extractable_literal("true"));
}

#[test]
fn literal_false() {
    assert!(is_extractable_literal("false"));
}

#[test]
fn literal_null() {
    assert!(is_extractable_literal("null"));
}

#[test]
fn literal_concat() {
    assert!(is_extractable_literal("'prefix_' . 'suffix'"));
}

#[test]
fn literal_with_whitespace() {
    assert!(is_extractable_literal("  'pending'  "));
}

#[test]
fn not_literal_variable() {
    assert!(!is_extractable_literal("$var"));
}

#[test]
fn not_literal_function_call() {
    assert!(!is_extractable_literal("strlen($x)"));
}

#[test]
fn not_literal_array() {
    assert!(!is_extractable_literal("[1, 2, 3]"));
}

#[test]
fn not_literal_empty() {
    assert!(!is_extractable_literal(""));
}

#[test]
fn not_literal_identifier() {
    assert!(!is_extractable_literal("SOME_CONST"));
}

#[test]
fn not_literal_method_call() {
    assert!(!is_extractable_literal("$this->method()"));
}

#[test]
fn literal_underscored_integer() {
    assert!(is_extractable_literal("1_000_000"));
}

// ── generate_constant_name ──────────────────────────────────────

#[test]
fn name_from_string_simple() {
    assert_eq!(generate_constant_name("'pending'"), "PENDING");
}

#[test]
fn name_from_string_snake_case() {
    assert_eq!(generate_constant_name("'order_status'"), "ORDER_STATUS");
}

#[test]
fn name_from_string_with_hyphens() {
    assert_eq!(generate_constant_name("'my-key'"), "MY_KEY");
}

#[test]
fn name_from_integer() {
    assert_eq!(generate_constant_name("200"), "VALUE_200");
}

#[test]
fn name_from_float() {
    assert_eq!(generate_constant_name("3.14"), "VALUE");
}

#[test]
fn name_from_true() {
    assert_eq!(generate_constant_name("true"), "IS_ENABLED");
}

#[test]
fn name_from_false() {
    assert_eq!(generate_constant_name("false"), "IS_DISABLED");
}

#[test]
fn name_from_null() {
    assert_eq!(generate_constant_name("null"), "DEFAULT_VALUE");
}

#[test]
fn name_from_negative_integer() {
    assert_eq!(generate_constant_name("-42"), "VALUE_42");
}

#[test]
fn name_from_double_quoted_string() {
    assert_eq!(generate_constant_name("\"active\""), "ACTIVE");
}

// ── deduplicate_constant_name ───────────────────────────────────

#[test]
fn deduplicate_no_collision() {
    let name = deduplicate_constant_name("PENDING", &[]);
    assert_eq!(name, "PENDING");
}

#[test]
fn deduplicate_with_collision() {
    let name = deduplicate_constant_name("PENDING", &["PENDING".to_string()]);
    assert_eq!(name, "PENDING_1");
}

#[test]
fn deduplicate_multiple_collisions() {
    let existing = vec!["PENDING".to_string(), "PENDING_1".to_string()];
    let name = deduplicate_constant_name("PENDING", &existing);
    assert_eq!(name, "PENDING_2");
}

// ── string_to_screaming_snake ───────────────────────────────────

#[test]
fn screaming_snake_simple() {
    assert_eq!(string_to_screaming_snake("pending"), "PENDING");
}

#[test]
fn screaming_snake_with_underscores() {
    assert_eq!(string_to_screaming_snake("order_status"), "ORDER_STATUS");
}

#[test]
fn screaming_snake_with_hyphens() {
    assert_eq!(string_to_screaming_snake("my-key"), "MY_KEY");
}

#[test]
fn screaming_snake_with_spaces() {
    assert_eq!(string_to_screaming_snake("hello world"), "HELLO_WORLD");
}

#[test]
fn screaming_snake_mixed_case() {
    assert_eq!(string_to_screaming_snake("orderStatus"), "ORDERSTATUS");
}

#[test]
fn screaming_snake_consecutive_separators() {
    assert_eq!(string_to_screaming_snake("a--b"), "A_B");
}

#[test]
fn screaming_snake_only_special_chars() {
    assert_eq!(string_to_screaming_snake("@#$"), "");
}

// ── detect_member_indent ────────────────────────────────────────

#[test]
fn detect_indent_four_spaces() {
    let content = "class Foo {\n    public function bar() {}\n}";
    let brace = content.find('{').unwrap();
    assert_eq!(detect_member_indent(content, brace), "    ");
}

#[test]
fn detect_indent_tab() {
    let content = "class Foo {\n\tpublic function bar() {}\n}";
    let brace = content.find('{').unwrap();
    assert_eq!(detect_member_indent(content, brace), "\t");
}

#[test]
fn detect_indent_fallback() {
    let content = "class Foo {}";
    let brace = content.find('{').unwrap();
    assert_eq!(detect_member_indent(content, brace), "    ");
}

// ── is_numeric_literal ──────────────────────────────────────────

#[test]
fn numeric_simple_int() {
    assert!(is_numeric_literal("42"));
}

#[test]
fn numeric_float() {
    assert!(is_numeric_literal("3.14"));
}

#[test]
fn numeric_hex() {
    assert!(is_numeric_literal("0xFF"));
}

#[test]
fn numeric_binary() {
    assert!(is_numeric_literal("0b101"));
}

#[test]
fn numeric_octal() {
    assert!(is_numeric_literal("0o77"));
}

#[test]
fn numeric_with_underscores() {
    assert!(is_numeric_literal("1_000_000"));
}

#[test]
fn numeric_exponent() {
    assert!(is_numeric_literal("1e10"));
}

#[test]
fn numeric_exponent_negative() {
    assert!(is_numeric_literal("1e-5"));
}

#[test]
fn not_numeric_empty() {
    assert!(!is_numeric_literal(""));
}

#[test]
fn not_numeric_alpha() {
    assert!(!is_numeric_literal("abc"));
}

// ── concat expression detection ─────────────────────────────────

#[test]
fn concat_two_strings() {
    assert!(is_concat_expression("'a' . 'b'"));
}

#[test]
fn concat_three_strings() {
    assert!(is_concat_expression("'a' . 'b' . 'c'"));
}

#[test]
fn not_concat_single_string() {
    assert!(!is_concat_expression("'hello'"));
}

#[test]
fn concat_string_and_number() {
    assert!(is_concat_expression("'item_' . 42"));
}

// ── Integration: code action on Backend (Phase 1) ───────────────

#[test]
fn extract_constant_offered_for_string_in_method() {
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar() {
        $status = 'pending';
    }
}"#;

    let backend = crate::Backend::new_test();

    // Select 'pending'
    let pending_start = content.find("'pending'").unwrap();
    let pending_end = pending_start + "'pending'".len();
    let start_pos = offset_to_position(content, pending_start);
    let end_pos = offset_to_position(content, pending_end);

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
    };

    let mut out = Vec::new();
    backend.collect_extract_constant_actions(uri, content, &params, &mut out);

    let extract = out
            .iter()
            .find(|a| matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Extract constant")));
    assert!(
        extract.is_some(),
        "should offer extract constant for string literal in method body"
    );
}

#[test]
fn extract_constant_not_offered_for_empty_selection() {
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar() {
        $status = 'pending';
    }
}"#;

    let backend = crate::Backend::new_test();

    let pos = offset_to_position(content, content.find("'pending'").unwrap());
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: pos,
            end: pos,
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
    };

    let mut out = Vec::new();
    backend.collect_extract_constant_actions(uri, content, &params, &mut out);

    let extract = out
            .iter()
            .find(|a| matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Extract constant")));
    assert!(
        extract.is_none(),
        "should not offer extract constant for empty selection"
    );
}

#[test]
fn extract_constant_not_offered_outside_class() {
    let uri = "file:///test.php";
    let content = r#"<?php
function foo() {
    $status = 'pending';
}"#;

    let backend = crate::Backend::new_test();

    let pending_start = content.find("'pending'").unwrap();
    let pending_end = pending_start + "'pending'".len();
    let start_pos = offset_to_position(content, pending_start);
    let end_pos = offset_to_position(content, pending_end);

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
    };

    let mut out = Vec::new();
    backend.collect_extract_constant_actions(uri, content, &params, &mut out);

    let extract = out
            .iter()
            .find(|a| matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Extract constant")));
    assert!(
        extract.is_none(),
        "should not offer extract constant outside a class"
    );
}

#[test]
fn extract_constant_not_offered_for_existing_constant_value() {
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    private const STATUS = 'pending';
}"#;

    let backend = crate::Backend::new_test();

    let pending_start = content.find("'pending'").unwrap();
    let pending_end = pending_start + "'pending'".len();
    let start_pos = offset_to_position(content, pending_start);
    let end_pos = offset_to_position(content, pending_end);

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
    };

    let mut out = Vec::new();
    backend.collect_extract_constant_actions(uri, content, &params, &mut out);

    let extract = out
            .iter()
            .find(|a| matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Extract constant")));
    assert!(
        extract.is_none(),
        "should not offer extract constant for a value already in a constant declaration"
    );
}

#[test]
fn extract_constant_not_offered_for_non_literal() {
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar() {
        $status = $this->getStatus();
    }
}"#;

    let backend = crate::Backend::new_test();

    let call_start = content.find("$this->getStatus()").unwrap();
    let call_end = call_start + "$this->getStatus()".len();
    let start_pos = offset_to_position(content, call_start);
    let end_pos = offset_to_position(content, call_end);

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
    };

    let mut out = Vec::new();
    backend.collect_extract_constant_actions(uri, content, &params, &mut out);

    let extract = out
            .iter()
            .find(|a| matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Extract constant")));
    assert!(
        extract.is_none(),
        "should not offer extract constant for a method call expression"
    );
}

#[test]
fn extract_constant_offers_all_occurrences_when_duplicates_exist() {
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar() {
        $a = 'pending';
        $b = 'pending';
    }
}"#;

    let backend = crate::Backend::new_test();

    let pending_start = content.find("'pending'").unwrap();
    let pending_end = pending_start + "'pending'".len();
    let start_pos = offset_to_position(content, pending_start);
    let end_pos = offset_to_position(content, pending_end);

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
    };

    let mut out = Vec::new();
    backend.collect_extract_constant_actions(uri, content, &params, &mut out);

    let titles: Vec<String> = out
        .iter()
        .filter_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Extract constant") => {
                Some(ca.title.clone())
            }
            _ => None,
        })
        .collect();

    assert!(
        titles.contains(&"Extract constant (this occurrence)".to_string()),
        "should offer single-occurrence variant"
    );
    assert!(
        titles.contains(&"Extract constant (all occurrences)".to_string()),
        "should offer all-occurrences variant"
    );
}

// ── Integration: resolve (Phase 2) ──────────────────────────────

#[test]
fn resolve_extract_constant_single_occurrence() {
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar() {
        $status = 'pending';
    }
}"#;

    let backend = crate::Backend::new_test();

    let pending_start = content.find("'pending'").unwrap();
    let pending_end = pending_start + "'pending'".len();
    let start_pos = offset_to_position(content, pending_start);
    let end_pos = offset_to_position(content, pending_end);

    let data = CodeActionData {
        action_kind: "refactor.extractConstant".to_string(),
        uri: uri.to_string(),
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        extra: serde_json::json!({ "all_occurrences": false }),
    };

    let edit = backend.resolve_extract_constant(&data, content);
    assert!(edit.is_some(), "should produce a workspace edit");

    let ws_edit = edit.unwrap();
    let changes = ws_edit.changes.unwrap();
    let edits = changes.values().next().unwrap();

    // Should have exactly 2 edits: insert constant + replace literal.
    assert_eq!(edits.len(), 2, "expected 2 edits (insert + replace)");

    // The insert should contain the typed constant declaration
    // (default test backend is PHP 8.5 which supports typed consts).
    let insert = &edits[0];
    assert!(
        insert
            .new_text
            .contains("const string PENDING = 'pending';"),
        "insert should contain typed constant declaration, got: {}",
        insert.new_text
    );

    // Should have a trailing blank line to separate from the method.
    assert!(
        insert.new_text.ends_with("\n\n"),
        "insert should have trailing blank line before method, got: {:?}",
        insert.new_text
    );

    // The replace should use self::PENDING.
    let replace = &edits[1];
    assert_eq!(replace.new_text, "self::PENDING");
}

#[test]
fn resolve_extract_constant_with_existing_constants() {
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    private const PENDING = 'other';

    public function bar() {
        $status = 'pending';
    }
}"#;

    let backend = crate::Backend::new_test();

    let pending_start = content.rfind("'pending'").unwrap();
    let pending_end = pending_start + "'pending'".len();
    let start_pos = offset_to_position(content, pending_start);
    let end_pos = offset_to_position(content, pending_end);

    let data = CodeActionData {
        action_kind: "refactor.extractConstant".to_string(),
        uri: uri.to_string(),
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        extra: serde_json::json!({ "all_occurrences": false }),
    };

    let edit = backend.resolve_extract_constant(&data, content);
    assert!(edit.is_some(), "should produce a workspace edit");

    let ws_edit = edit.unwrap();
    let changes = ws_edit.changes.unwrap();
    let edits = changes.values().next().unwrap();

    // The constant name should be deduplicated.
    let insert = &edits[0];
    assert!(
        insert.new_text.contains("PENDING_1"),
        "should deduplicate constant name, got: {}",
        insert.new_text
    );

    // There's already a blank line between the last constant and the
    // method, so the new constant should NOT add another blank line.
    assert!(
        !insert.new_text.ends_with("\n\n"),
        "should not add extra blank line when one already exists, got: {:?}",
        insert.new_text
    );
}

#[test]
fn resolve_extract_constant_integer() {
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar() {
        return 200;
    }
}"#;

    let backend = crate::Backend::new_test();

    let num_start = content.find("200").unwrap();
    let num_end = num_start + "200".len();
    let start_pos = offset_to_position(content, num_start);
    let end_pos = offset_to_position(content, num_end);

    let data = CodeActionData {
        action_kind: "refactor.extractConstant".to_string(),
        uri: uri.to_string(),
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        extra: serde_json::json!({ "all_occurrences": false }),
    };

    let edit = backend.resolve_extract_constant(&data, content);
    assert!(edit.is_some(), "should produce a workspace edit");

    let ws_edit = edit.unwrap();
    let changes = ws_edit.changes.unwrap();
    let edits = changes.values().next().unwrap();

    let insert = &edits[0];
    assert!(
        insert.new_text.contains("const int VALUE_200 = 200;"),
        "should use typed int const for integer literals, got: {}",
        insert.new_text
    );
}

#[test]
fn resolve_extract_constant_uses_context_visibility() {
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    protected function bar() {
        $status = 'active';
    }
}"#;

    let backend = crate::Backend::new_test();

    let active_start = content.find("'active'").unwrap();
    let active_end = active_start + "'active'".len();
    let start_pos = offset_to_position(content, active_start);
    let end_pos = offset_to_position(content, active_end);

    let data = CodeActionData {
        action_kind: "refactor.extractConstant".to_string(),
        uri: uri.to_string(),
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        extra: serde_json::json!({ "all_occurrences": false }),
    };

    let edit = backend.resolve_extract_constant(&data, content);
    assert!(edit.is_some(), "should produce a workspace edit");

    let ws_edit = edit.unwrap();
    let changes = ws_edit.changes.unwrap();
    let edits = changes.values().next().unwrap();

    let insert = &edits[0];
    assert!(
        insert.new_text.contains("protected const string ACTIVE"),
        "should use context visibility, got: {}",
        insert.new_text
    );
}

#[test]
fn resolve_extract_constant_php82_uses_docblock() {
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar() {
        $status = 'pending';
    }
}"#;

    let backend = crate::Backend::new_test();
    backend.set_php_version(PhpVersion::new(8, 2));

    let pending_start = content.find("'pending'").unwrap();
    let pending_end = pending_start + "'pending'".len();
    let start_pos = offset_to_position(content, pending_start);
    let end_pos = offset_to_position(content, pending_end);

    let data = CodeActionData {
        action_kind: "refactor.extractConstant".to_string(),
        uri: uri.to_string(),
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        extra: serde_json::json!({ "all_occurrences": false }),
    };

    let edit = backend.resolve_extract_constant(&data, content);
    assert!(edit.is_some(), "should produce a workspace edit");

    let ws_edit = edit.unwrap();
    let changes = ws_edit.changes.unwrap();
    let edits = changes.values().next().unwrap();

    let insert = &edits[0];
    assert!(
        insert.new_text.contains("/** @var string */"),
        "PHP 8.2 should use docblock annotation, got: {}",
        insert.new_text
    );
    assert!(
        insert
            .new_text
            .contains("public const PENDING = 'pending';"),
        "PHP 8.2 should NOT have type in const syntax, got: {}",
        insert.new_text
    );
}

#[test]
fn resolve_extract_constant_php83_uses_typed_const() {
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar() {
        return 42;
    }
}"#;

    let backend = crate::Backend::new_test();
    backend.set_php_version(PhpVersion::new(8, 3));

    let num_start = content.find("42").unwrap();
    let num_end = num_start + "42".len();
    let start_pos = offset_to_position(content, num_start);
    let end_pos = offset_to_position(content, num_end);

    let data = CodeActionData {
        action_kind: "refactor.extractConstant".to_string(),
        uri: uri.to_string(),
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        extra: serde_json::json!({ "all_occurrences": false }),
    };

    let edit = backend.resolve_extract_constant(&data, content);
    assert!(edit.is_some(), "should produce a workspace edit");

    let ws_edit = edit.unwrap();
    let changes = ws_edit.changes.unwrap();
    let edits = changes.values().next().unwrap();

    let insert = &edits[0];
    assert!(
        insert.new_text.contains("public const int VALUE_42 = 42;"),
        "PHP 8.3 should use typed const syntax, got: {}",
        insert.new_text
    );
}

#[test]
fn resolve_extract_constant_null_has_no_type() {
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar() {
        $x = null;
    }
}"#;

    let backend = crate::Backend::new_test();

    let null_start = content.find("null").unwrap();
    let null_end = null_start + "null".len();
    let start_pos = offset_to_position(content, null_start);
    let end_pos = offset_to_position(content, null_end);

    let data = CodeActionData {
        action_kind: "refactor.extractConstant".to_string(),
        uri: uri.to_string(),
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        extra: serde_json::json!({ "all_occurrences": false }),
    };

    let edit = backend.resolve_extract_constant(&data, content);
    assert!(edit.is_some(), "should produce a workspace edit");

    let ws_edit = edit.unwrap();
    let changes = ws_edit.changes.unwrap();
    let edits = changes.values().next().unwrap();

    let insert = &edits[0];
    // null has no valid PHP const type — should omit type entirely
    assert!(
        !insert.new_text.contains("/** @var"),
        "null should not get a docblock type, got: {}",
        insert.new_text
    );
    assert!(
        insert
            .new_text
            .contains("public const DEFAULT_VALUE = null;"),
        "null const should have no type annotation, got: {}",
        insert.new_text
    );
}

#[test]
fn resolve_extract_constant_in_trait() {
    let uri = "file:///test.php";
    let content = r#"<?php
trait Foo {
    public function bar() {
        return 'default';
    }
}"#;

    let backend = crate::Backend::new_test();

    let val_start = content.find("'default'").unwrap();
    let val_end = val_start + "'default'".len();
    let start_pos = offset_to_position(content, val_start);
    let end_pos = offset_to_position(content, val_end);

    let data = CodeActionData {
        action_kind: "refactor.extractConstant".to_string(),
        uri: uri.to_string(),
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        extra: serde_json::json!({ "all_occurrences": false }),
    };

    let edit = backend.resolve_extract_constant(&data, content);
    assert!(edit.is_some(), "should produce a workspace edit for trait");
}

#[test]
fn resolve_extract_constant_in_enum() {
    let uri = "file:///test.php";
    let content = r#"<?php
enum Status {
    case Active;

    public function label(): string {
        return 'active_label';
    }
}"#;

    let backend = crate::Backend::new_test();

    let val_start = content.find("'active_label'").unwrap();
    let val_end = val_start + "'active_label'".len();
    let start_pos = offset_to_position(content, val_start);
    let end_pos = offset_to_position(content, val_end);

    let data = CodeActionData {
        action_kind: "refactor.extractConstant".to_string(),
        uri: uri.to_string(),
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        extra: serde_json::json!({ "all_occurrences": false }),
    };

    let edit = backend.resolve_extract_constant(&data, content);
    assert!(edit.is_some(), "should produce a workspace edit for enum");
}

#[test]
fn resolve_extract_constant_trailing_blank_line_when_no_gap() {
    let uri = "file:///test.php";
    // No blank line between last constant and method.
    let content = r#"<?php
class Foo {
    public const A = 1;
    public function bar() {
        return 'test';
    }
}"#;

    let backend = crate::Backend::new_test();

    let val_start = content.find("'test'").unwrap();
    let val_end = val_start + "'test'".len();
    let start_pos = offset_to_position(content, val_start);
    let end_pos = offset_to_position(content, val_end);

    let data = CodeActionData {
        action_kind: "refactor.extractConstant".to_string(),
        uri: uri.to_string(),
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        extra: serde_json::json!({ "all_occurrences": false }),
    };

    let edit = backend.resolve_extract_constant(&data, content);
    assert!(edit.is_some(), "should produce a workspace edit");

    let ws_edit = edit.unwrap();
    let changes = ws_edit.changes.unwrap();
    let edits = changes.values().next().unwrap();

    let insert = &edits[0];
    assert!(
        insert.new_text.ends_with("\n\n"),
        "should add trailing blank line when no gap exists, got: {:?}",
        insert.new_text
    );
}

#[test]
fn resolve_extract_constant_no_extra_blank_line_when_gap_exists() {
    let uri = "file:///test.php";
    // Already a blank line between constants and method.
    let content = r#"<?php
class Foo {
    public const A = 1;

    public function bar() {
        return 'test';
    }
}"#;

    let backend = crate::Backend::new_test();

    let val_start = content.find("'test'").unwrap();
    let val_end = val_start + "'test'".len();
    let start_pos = offset_to_position(content, val_start);
    let end_pos = offset_to_position(content, val_end);

    let data = CodeActionData {
        action_kind: "refactor.extractConstant".to_string(),
        uri: uri.to_string(),
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        extra: serde_json::json!({ "all_occurrences": false }),
    };

    let edit = backend.resolve_extract_constant(&data, content);
    assert!(edit.is_some(), "should produce a workspace edit");

    let ws_edit = edit.unwrap();
    let changes = ws_edit.changes.unwrap();
    let edits = changes.values().next().unwrap();

    let insert = &edits[0];
    assert!(
        !insert.new_text.ends_with("\n\n"),
        "should not add extra blank line when gap already exists, got: {:?}",
        insert.new_text
    );
}

#[test]
fn resolve_extract_constant_inserts_after_existing_constants() {
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public const A = 1;
    public const B = 2;

    public function bar() {
        return 'test';
    }
}"#;

    let backend = crate::Backend::new_test();

    let val_start = content.find("'test'").unwrap();
    let val_end = val_start + "'test'".len();
    let start_pos = offset_to_position(content, val_start);
    let end_pos = offset_to_position(content, val_end);

    let data = CodeActionData {
        action_kind: "refactor.extractConstant".to_string(),
        uri: uri.to_string(),
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        extra: serde_json::json!({ "all_occurrences": false }),
    };

    let edit = backend.resolve_extract_constant(&data, content);
    assert!(edit.is_some(), "should produce a workspace edit");

    let ws_edit = edit.unwrap();
    let changes = ws_edit.changes.unwrap();
    let edits = changes.values().next().unwrap();

    // The insert should be after the last existing constant.
    let insert = &edits[0];
    let insert_line = insert.range.start.line;
    let const_b_line = content[..content.find("public const B").unwrap()]
        .chars()
        .filter(|c| *c == '\n')
        .count() as u32;
    assert!(
        insert_line > const_b_line,
        "new constant should be inserted after existing constants (insert at line {}, const B at line {})",
        insert_line,
        const_b_line
    );
}

#[test]
fn extract_constant_offered_for_property_default() {
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    private string $status = 'pending';
}"#;

    let backend = crate::Backend::new_test();

    let pending_start = content.find("'pending'").unwrap();
    let pending_end = pending_start + "'pending'".len();
    let start_pos = offset_to_position(content, pending_start);
    let end_pos = offset_to_position(content, pending_end);

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
    };

    let mut out = Vec::new();
    backend.collect_extract_constant_actions(uri, content, &params, &mut out);

    let extract = out
            .iter()
            .find(|a| matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Extract constant")));
    assert!(
        extract.is_some(),
        "should offer extract constant for property default value"
    );
}

#[test]
fn extract_constant_offered_for_integer_literal() {
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar() {
        return 200;
    }
}"#;

    let backend = crate::Backend::new_test();

    let num_start = content.find("200").unwrap();
    let num_end = num_start + 3;
    let start_pos = offset_to_position(content, num_start);
    let end_pos = offset_to_position(content, num_end);

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
    };

    let mut out = Vec::new();
    backend.collect_extract_constant_actions(uri, content, &params, &mut out);

    let extract = out
            .iter()
            .find(|a| matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Extract constant")));
    assert!(
        extract.is_some(),
        "should offer extract constant for integer literal"
    );
}

#[test]
fn extract_constant_in_namespace() {
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App\Models;

class Foo {
    public function bar() {
        return 'value';
    }
}"#;

    let backend = crate::Backend::new_test();

    let val_start = content.find("'value'").unwrap();
    let val_end = val_start + "'value'".len();
    let start_pos = offset_to_position(content, val_start);
    let end_pos = offset_to_position(content, val_end);

    let data = CodeActionData {
        action_kind: "refactor.extractConstant".to_string(),
        uri: uri.to_string(),
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        extra: serde_json::json!({ "all_occurrences": false }),
    };

    let edit = backend.resolve_extract_constant(&data, content);
    assert!(
        edit.is_some(),
        "should produce a workspace edit for class in namespace"
    );

    let ws_edit = edit.unwrap();
    let changes = ws_edit.changes.unwrap();
    let edits = changes.values().next().unwrap();

    let insert = &edits[0];
    assert!(
        insert.new_text.contains("const string VALUE = 'value';"),
        "constant declaration should be correct, got: {}",
        insert.new_text
    );
}

// ── has_blank_line ──────────────────────────────────────────────

#[test]
fn blank_line_two_newlines() {
    assert!(has_blank_line("\n\n"));
}

#[test]
fn blank_line_with_whitespace() {
    assert!(has_blank_line("\n  \n"));
}

#[test]
fn no_blank_line_single_newline() {
    assert!(!has_blank_line("\n"));
}

#[test]
fn no_blank_line_content_between() {
    assert!(!has_blank_line("\n    foo\n"));
}

#[test]
fn blank_line_empty_string() {
    assert!(!has_blank_line(""));
}
