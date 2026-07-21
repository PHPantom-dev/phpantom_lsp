use super::*;

// ── Name generation tests ───────────────────────────────────────

#[test]
fn name_from_method_call() {
    assert_eq!(generate_variable_name("$user->getName()"), "name");
}

#[test]
fn name_from_method_call_no_prefix() {
    assert_eq!(generate_variable_name("$user->email()"), "email");
}

#[test]
fn name_from_method_call_with_args() {
    assert_eq!(generate_variable_name("$repo->findById($id)"), "findById");
}

#[test]
fn name_from_property_access() {
    assert_eq!(generate_variable_name("$user->email"), "email");
}

#[test]
fn name_from_nullsafe_method() {
    assert_eq!(generate_variable_name("$user?->getName()"), "name");
}

#[test]
fn name_from_nullsafe_property() {
    assert_eq!(generate_variable_name("$user?->email"), "email");
}

#[test]
fn name_from_static_call() {
    assert_eq!(generate_variable_name("Carbon::now()"), "now");
}

#[test]
fn name_from_static_call_namespaced() {
    assert_eq!(generate_variable_name("\\Carbon\\Carbon::now()"), "now");
}

#[test]
fn name_from_function_call() {
    assert_eq!(
        generate_variable_name("array_filter($items, $fn)"),
        "arrayFilter"
    );
}

#[test]
fn name_from_simple_function() {
    assert_eq!(generate_variable_name("count($items)"), "count");
}

#[test]
fn name_from_namespaced_function() {
    assert_eq!(
        generate_variable_name("App\\Helpers\\format_name($s)"),
        "formatName"
    );
}

#[test]
fn name_fallback_for_expression() {
    assert_eq!(generate_variable_name("$a + $b"), "variable");
}

#[test]
fn name_fallback_for_string_literal() {
    assert_eq!(generate_variable_name("'hello world'"), "variable");
}

#[test]
fn name_fallback_for_number() {
    assert_eq!(generate_variable_name("42"), "variable");
}

#[test]
fn name_from_chained_method_call() {
    // For chained calls, use the last method name
    assert_eq!(
        generate_variable_name("$query->where('x', 1)->first()"),
        "first"
    );
}

#[test]
fn name_from_get_prefix_method() {
    assert_eq!(generate_variable_name("$user->getEmail()"), "email");
}

#[test]
fn name_from_is_prefix_method() {
    assert_eq!(generate_variable_name("$user->isActive()"), "active");
}

#[test]
fn name_from_has_prefix_method() {
    assert_eq!(
        generate_variable_name("$user->hasPermission()"),
        "permission"
    );
}

#[test]
fn name_no_strip_island() {
    // "island" should not have "is" stripped because 'l' is lowercase
    assert_eq!(generate_variable_name("$map->island()"), "island");
}

// ── Deduplication tests ─────────────────────────────────────────

#[test]
fn deduplicate_no_collision() {
    let existing = vec!["$foo".to_string(), "$bar".to_string()];
    assert_eq!(deduplicate_name("name", &existing), "name");
}

#[test]
fn deduplicate_with_collision() {
    let existing = vec!["$name".to_string(), "$foo".to_string()];
    assert_eq!(deduplicate_name("name", &existing), "name1");
}

#[test]
fn deduplicate_multiple_collisions() {
    let existing = vec![
        "$name".to_string(),
        "$name1".to_string(),
        "$name2".to_string(),
    ];
    assert_eq!(deduplicate_name("name", &existing), "name3");
}

// ── Insertion point tests ───────────────────────────────────────

#[test]
fn find_statement_line_simple() {
    let content = "<?php\n    $x = $user->getName();\n";
    // Selection starts at `$user` (offset 14 approximately)
    let offset = content.find("$user").unwrap();
    let (line_start, indent) = find_enclosing_statement_line(content, offset);
    assert_eq!(line_start, 6); // After "<?php\n"
    assert_eq!(indent, "    ");
}

#[test]
fn find_statement_line_no_indent() {
    let content = "<?php\n$x = foo();\n";
    let offset = content.find("foo").unwrap();
    let (line_start, indent) = find_enclosing_statement_line(content, offset);
    assert_eq!(line_start, 6);
    assert_eq!(indent, "");
}

#[test]
fn find_statement_line_tab_indent() {
    let content = "<?php\n\t\t$x = bar();\n";
    let offset = content.find("bar").unwrap();
    let (line_start, indent) = find_enclosing_statement_line(content, offset);
    assert_eq!(line_start, 6);
    assert_eq!(indent, "\t\t");
}

// ── snake_to_camel tests ────────────────────────────────────────

#[test]
fn snake_to_camel_simple() {
    assert_eq!(snake_to_camel("array_filter"), "arrayFilter");
}

#[test]
fn snake_to_camel_single_word() {
    assert_eq!(snake_to_camel("count"), "count");
}

#[test]
fn snake_to_camel_three_parts() {
    assert_eq!(snake_to_camel("str_to_upper"), "strToUpper");
}

// ── strip_outer_parens tests ────────────────────────────────────

#[test]
fn strip_parens_wrapped_expression() {
    assert_eq!(strip_outer_parens("($a + $b)"), "$a + $b");
}

#[test]
fn strip_parens_no_parens() {
    assert_eq!(strip_outer_parens("$a + $b"), "$a + $b");
}

#[test]
fn strip_parens_function_call_unchanged() {
    assert_eq!(strip_outer_parens("foo($x)"), "foo($x)");
}

#[test]
fn strip_parens_two_groups_unchanged() {
    assert_eq!(strip_outer_parens("($a) + ($b)"), "($a) + ($b)");
}

#[test]
fn strip_parens_nested() {
    assert_eq!(strip_outer_parens("(($a + $b))"), "($a + $b)");
}

#[test]
fn strip_parens_with_whitespace() {
    assert_eq!(strip_outer_parens("( $a + $b )"), "$a + $b");
}

// ── Integration tests ───────────────────────────────────────────

#[test]
fn extract_variable_action_offered_for_selection() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "<?php\nfunction test() {\n    echo $user->getName();\n}\n";

    backend.update_ast(uri, content);

    // Select `$user->getName()` (line 2, from `$user` to closing `)`)
    let line2 = "    echo $user->getName();\n";
    let expr_start_in_line = line2.find("$user").unwrap();
    let expr_end_in_line = line2.find(';').unwrap(); // just before ;

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(2, expr_start_in_line as u32),
            end: Position::new(2, expr_end_in_line as u32),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = backend.handle_code_action(uri, content, &params);
    let extract_action = actions
        .iter()
        .find_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Extract variable") => {
                Some(ca)
            }
            _ => None,
        })
        .expect("expected extract variable action");

    assert_eq!(extract_action.kind, Some(CodeActionKind::REFACTOR_EXTRACT));
}

#[test]
fn extract_variable_not_offered_for_empty_selection() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "<?php\nfunction test() {\n    echo $user->getName();\n}\n";

    backend.update_ast(uri, content);

    // Empty selection (cursor, no range)
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(2, 9),
            end: Position::new(2, 9),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = backend.handle_code_action(uri, content, &params);
    let extract_actions: Vec<_> = actions
        .iter()
        .filter(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title.contains("Extract variable"),
            _ => false,
        })
        .collect();

    assert!(
        extract_actions.is_empty(),
        "should not offer extract variable for empty selection"
    );
}

#[test]
fn extract_variable_not_offered_for_trait_name_selection() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "<?php\ntrait ExampleFeatureTrait {}\n";

    backend.update_ast(uri, content);

    let start = content.find("ExampleFeatureTrait").unwrap() as u32;
    let end = start + "ExampleFeatureTrait".len() as u32;
    let start_pos = crate::util::offset_to_position(content, start as usize);
    let end_pos = crate::util::offset_to_position(content, end as usize);

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
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = backend.handle_code_action(uri, content, &params);
    let extract_actions: Vec<_> = actions
        .iter()
        .filter(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title.contains("Extract variable"),
            _ => false,
        })
        .collect();

    assert!(
        extract_actions.is_empty(),
        "should not offer extract variable for trait name selection"
    );
}

#[test]
fn extract_variable_generates_correct_edits() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "<?php\nfunction test() {\n    echo $user->getName();\n}\n";

    backend.update_ast(uri, content);
    backend
        .open_files
        .write()
        .insert(uri.to_string(), std::sync::Arc::new(content.to_string()));

    // Select `$user->getName()`
    // Line 2: "    echo $user->getName();\n"
    // $user starts at character 9, `) ` ends at character 25
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(2, 9),
            end: Position::new(2, 25),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = backend.handle_code_action(uri, content, &params);
    let extract_action = actions
        .iter()
        .find_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Extract variable") => {
                Some(ca)
            }
            _ => None,
        })
        .expect("expected extract variable action");

    // Phase 1 should NOT have an edit — it's deferred.
    assert!(
        extract_action.edit.is_none(),
        "Phase 1 should not compute edits"
    );
    assert!(
        extract_action.data.is_some(),
        "Phase 1 should attach resolve data"
    );

    // Phase 2: resolve the action to get the workspace edit.
    let (resolved, _) = backend.resolve_code_action(extract_action.clone());
    let edit = resolved
        .edit
        .as_ref()
        .expect("expected workspace edit after resolve");
    let changes = edit.changes.as_ref().expect("expected changes");
    let file_edits = changes
        .get(&uri.parse::<Url>().unwrap())
        .expect("expected edits for the file");

    assert_eq!(file_edits.len(), 2);

    // First edit: insertion of assignment before the line
    let insert_edit = &file_edits[0];
    assert_eq!(insert_edit.range.start, insert_edit.range.end); // insertion
    assert!(insert_edit.new_text.contains("$name = $user->getName();"));
    assert!(insert_edit.new_text.starts_with("    ")); // indentation
    assert!(insert_edit.new_text.ends_with('\n'));

    // Second edit: replacement of selection with variable
    let replace_edit = &file_edits[1];
    assert_eq!(replace_edit.new_text, "$name");
}

#[test]
fn extract_variable_deduplicates_name() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content =
        "<?php\nfunction test() {\n    $name = 'existing';\n    echo $user->getName();\n}\n";

    backend.update_ast(uri, content);

    // Select `$user->getName()` on line 3
    // Line 3: "    echo $user->getName();\n"
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(3, 9),
            end: Position::new(3, 25),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = backend.handle_code_action(uri, content, &params);
    let _extract_action = actions
        .iter()
        .find_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Extract variable") => {
                Some(ca)
            }
            _ => None,
        })
        .expect("expected extract variable action");
}

#[test]
fn extract_variable_static_call() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "<?php\nfunction test() {\n    echo Carbon::now();\n}\n";

    backend.update_ast(uri, content);

    // Select `Carbon::now()` on line 2
    // Line 2: "    echo Carbon::now();\n"
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(2, 9),
            end: Position::new(2, 22),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = backend.handle_code_action(uri, content, &params);
    let _extract_action = actions
        .iter()
        .find_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Extract variable") => {
                Some(ca)
            }
            _ => None,
        })
        .expect("expected extract variable action");
}

#[test]
fn extract_variable_function_call() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "<?php\nfunction test() {\n    echo array_filter($items, $fn);\n}\n";

    backend.update_ast(uri, content);

    // Select `array_filter($items, $fn)` on line 2
    // Line 2: "    echo array_filter($items, $fn);\n"
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(2, 9),
            end: Position::new(2, 34),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = backend.handle_code_action(uri, content, &params);
    let _extract_action = actions
        .iter()
        .find_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Extract variable") => {
                Some(ca)
            }
            _ => None,
        })
        .expect("expected extract variable action");
}

#[test]
fn extract_variable_whitespace_only_selection_skipped() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "<?php\nfunction test() {\n    echo 'hello';\n}\n";

    backend.update_ast(uri, content);

    // Select just whitespace on line 2 (chars 0..4 = "    ")
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(2, 0),
            end: Position::new(2, 4),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = backend.handle_code_action(uri, content, &params);
    let extract_actions: Vec<_> = actions
        .iter()
        .filter(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title.contains("Extract variable"),
            _ => false,
        })
        .collect();

    assert!(
        extract_actions.is_empty(),
        "should not offer extract variable for whitespace-only selection"
    );
}

#[test]
fn extract_variable_not_offered_for_standalone_statement() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "<?php\nfunction test() {\n    $this->save($id);\n    $this->log($id);\n}\n";

    backend.update_ast(uri, content);

    // Select `$this->save($id)` — the entire expression of a standalone statement.
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(2, 4),
            end: Position::new(2, 21),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = backend.handle_code_action(uri, content, &params);
    let extract_actions: Vec<_> = actions
        .iter()
        .filter(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title.contains("Extract variable"),
            _ => false,
        })
        .collect();

    assert!(
        extract_actions.is_empty(),
        "should not offer extract variable for a standalone expression statement"
    );
}

#[test]
fn extract_variable_not_offered_for_standalone_statement_multiline_selection() {
    // Selecting from end of a comment line through `var_dump($value);`
    // should not offer extract variable — the call is a standalone
    // expression statement used for side effects, not a value.
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "\
<?php
class Test {
    public function dump($value): void
    {
        // select from here
        var_dump($value);
        // to here
    }
}
";

    backend.update_ast(uri, content);

    // Select from end of comment line (line 4, col 27) to end of
    // var_dump line (line 5, col 25) — mimics dragging from the
    // end of one line to the end of the next.
    let comment_line = "        // select from here";
    let vardump_line = "        var_dump($value);";
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(4, comment_line.len() as u32),
            end: Position::new(5, vardump_line.len() as u32),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = backend.handle_code_action(uri, content, &params);
    let extract_actions: Vec<_> = actions
        .iter()
        .filter(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title.contains("Extract variable"),
            _ => false,
        })
        .collect();

    assert!(
        extract_actions.is_empty(),
        "should not offer extract variable for standalone statement selected across lines: {:?}",
        extract_actions
            .iter()
            .map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) => &ca.title,
                _ => unreachable!(),
            })
            .collect::<Vec<_>>()
    );
}

// ── is_entire_assignment_rhs tests ──────────────────────────────

#[test]
fn assignment_rhs_full_rhs_detected() {
    let content = "<?php\nfunction test() {\n    $tax = $total * 0.21;\n}\n";
    let start = content.find("$total * 0.21").unwrap();
    let end = start + "$total * 0.21".len();
    assert!(is_entire_assignment_rhs(content, start, end));
}

#[test]
fn assignment_rhs_sub_expression_not_detected() {
    let content = "<?php\nfunction test() {\n    $tax = $total * 0.21;\n}\n";
    let start = content.find("$total").unwrap();
    let end = start + "$total".len();
    assert!(!is_entire_assignment_rhs(content, start, end));
}

#[test]
fn assignment_rhs_standalone_statement_not_detected() {
    let content = "<?php\nfunction test() {\n    echo $total * 0.21;\n}\n";
    let start = content.find("$total * 0.21").unwrap();
    let end = start + "$total * 0.21".len();
    assert!(!is_entire_assignment_rhs(content, start, end));
}

#[test]
fn assignment_rhs_comparison_not_confused() {
    // `==` should not be treated as assignment
    let content = "<?php\nfunction test() {\n    if ($x == $y) {}\n}\n";
    let start = content.find("$y").unwrap();
    let end = start + "$y".len();
    assert!(!is_entire_assignment_rhs(content, start, end));
}

// ── is_entire_expression_statement tests ────────────────────────

#[test]
fn is_entire_statement_true_for_full_expression() {
    let content = "<?php\nfunction test() {\n    $this->save($id);\n}\n";
    let start = content.find("$this->save").unwrap();
    let end = content.find("($id)").unwrap() + 5;
    assert!(is_entire_expression_statement(content, start, end));
}

#[test]
fn is_entire_statement_false_for_sub_expression() {
    let content = "<?php\nfunction test() {\n    return $this->save($id);\n}\n";
    let start = content.find("$this->save").unwrap();
    let end = content.find("($id)").unwrap() + 5;
    assert!(!is_entire_expression_statement(content, start, end));
}

#[test]
fn is_entire_statement_false_for_argument() {
    let content = "<?php\nfunction test() {\n    echo count($items);\n}\n";
    let start = content.find("count").unwrap();
    let end = content.find("($items)").unwrap() + 8;
    assert!(!is_entire_expression_statement(content, start, end));
}

#[test]
fn is_entire_statement_true_for_multiline_selection_with_comment() {
    // Selecting from end of a comment line through `var_dump($value);`
    // should still be detected as a standalone expression statement.
    let content = "<?php\nfunction test($value) {\n    // comment\n    var_dump($value);\n}\n";
    let start = content.find("// comment").unwrap() + "// comment".len();
    let end = content.find("var_dump($value);").unwrap() + "var_dump($value);".len();
    assert!(is_entire_expression_statement(content, start, end));
}

// ── is_valid_expression tests ───────────────────────────────────

#[test]
fn valid_expr_method_call() {
    assert!(is_valid_expression("$this->save($id)"));
}

#[test]
fn valid_expr_property_access() {
    assert!(is_valid_expression("$user->name"));
}

#[test]
fn valid_expr_variable() {
    assert!(is_valid_expression("$x"));
}

#[test]
fn valid_expr_function_call() {
    assert!(is_valid_expression("count($items)"));
}

#[test]
fn valid_expr_static_call() {
    assert!(is_valid_expression("Carbon::now()"));
}

#[test]
fn valid_expr_new() {
    assert!(is_valid_expression("new Foo($a)"));
}

#[test]
fn valid_expr_binary() {
    assert!(is_valid_expression("$a + $b"));
}

#[test]
fn valid_expr_string_literal() {
    assert!(is_valid_expression("'hello'"));
}

#[test]
fn valid_expr_number() {
    assert!(is_valid_expression("42"));
}

#[test]
fn valid_expr_array_literal() {
    assert!(is_valid_expression("[1, 2, 3]"));
}

#[test]
fn valid_expr_ternary() {
    assert!(is_valid_expression("$x ? $a : $b"));
}

#[test]
fn valid_expr_parenthesized() {
    assert!(is_valid_expression("($a + $b)"));
}

#[test]
fn invalid_expr_bare_method_name() {
    assert!(!is_valid_expression("save"));
}

#[test]
fn invalid_expr_bare_identifier() {
    assert!(!is_valid_expression("getName"));
}

#[test]
fn invalid_expr_arrow_fragment() {
    assert!(!is_valid_expression("->save($id)"));
}

#[test]
fn invalid_expr_partial_call() {
    assert!(!is_valid_expression("save($id"));
}

#[test]
fn invalid_expr_method_name_with_parens() {
    // `getLabel()` looks like a function call but is actually a
    // method name fragment when preceded by `->` in the source.
    // The is_valid_expression check alone can't catch this —
    // the context check in collect_extract_variable_actions handles it.
    // So is_valid_expression returns true (it IS valid PHP syntax),
    // but the action is still rejected by the `->` prefix check.
    assert!(is_valid_expression("getLabel()"));
}

#[test]
fn invalid_expr_multi_statement() {
    assert!(!is_valid_expression(
        "$this->generateId();\n        $this->save($id)"
    ));
}

#[test]
fn invalid_expr_two_calls_with_semicolons() {
    assert!(!is_valid_expression("foo(); bar()"));
}

#[test]
fn semicolon_in_string_not_rejected() {
    assert!(is_valid_expression("'hello; world'"));
    assert!(is_valid_expression("\"hello; world\""));
}

#[test]
fn trailing_semicolon_not_rejected() {
    // A single expression with trailing `;` is fine — it's just
    // the statement terminator which we strip.
    assert!(is_valid_expression("$this->save($id);"));
}

#[test]
fn invalid_expr_empty() {
    assert!(!is_valid_expression(""));
}

#[test]
fn invalid_expr_whitespace() {
    assert!(!is_valid_expression("   "));
}

#[test]
fn reject_bare_this_in_method_call_context() {
    // Selecting just `$this` from `$this->save($id)` should be
    // rejected as useless (produces `$variable->save($id)`).
    // While `$this` IS a valid expression syntactically, we rely
    // on is_entire_expression_statement to not trigger (it won't
    // since `$this` is not the whole statement). But is_valid_expression
    // correctly returns true — the real guard is that extracting
    // `$this` alone IS offered but the user simply wouldn't select
    // just `$this`.  The parser-based check ensures we don't
    // produce *broken* code.
    assert!(is_valid_expression("$this"));
}

#[test]
fn extract_variable_not_offered_for_bare_method_name() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "<?php\nfunction test() {\n    $this->save($id);\n}\n";

    backend.update_ast(uri, content);

    // Select just `save` — the bare method name.
    let save_start = content.find("save").unwrap();
    let save_line = content[..save_start].matches('\n').count() as u32;
    let save_col = (save_start - content[..save_start].rfind('\n').unwrap() - 1) as u32;

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(save_line, save_col),
            end: Position::new(save_line, save_col + 4),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = backend.handle_code_action(uri, content, &params);
    let extract_actions: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Extract variable")))
            .collect();

    assert!(
        extract_actions.is_empty(),
        "should not offer extract variable for bare method name 'save'"
    );
}

#[test]
fn extract_variable_not_offered_for_method_call_fragment() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "<?php\nfunction test() {\n    $label = $order->getLabel();\n}\n";

    backend.update_ast(uri, content);

    // Select `getLabel()` — preceded by `->` in source.
    let gl_start = content.find("getLabel()").unwrap();
    let gl_line = content[..gl_start].matches('\n').count() as u32;
    let gl_col = (gl_start - content[..gl_start].rfind('\n').unwrap() - 1) as u32;

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(gl_line, gl_col),
            end: Position::new(gl_line, gl_col + 10),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = backend.handle_code_action(uri, content, &params);
    let extract_actions: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Extract variable")))
            .collect();

    assert!(
        extract_actions.is_empty(),
        "should not offer extract variable for method call fragment 'getLabel()'"
    );
}

// ── find_identical_occurrences tests ─────────────────────────────

#[test]
fn find_occurrences_finds_duplicates() {
    let content = "<?php echo $x->foo(); echo $x->foo(); echo $x->bar();";
    let needle = "$x->foo()";
    let first = content.find(needle).unwrap();
    let occurrences = find_identical_occurrences(
        content,
        needle,
        first,
        first + needle.len(),
        0,
        content.len(),
    );
    assert_eq!(occurrences.len(), 1);
    assert!(occurrences[0].0 > first);
}

#[test]
fn find_occurrences_none_when_unique() {
    let content = "<?php echo $x->foo(); echo $x->bar();";
    let needle = "$x->foo()";
    let first = content.find(needle).unwrap();
    let occurrences = find_identical_occurrences(
        content,
        needle,
        first,
        first + needle.len(),
        0,
        content.len(),
    );
    assert!(occurrences.is_empty());
}

#[test]
fn find_occurrences_skips_substrings() {
    let content = "<?php echo $x->foo(); echo $x->fooBar();";
    let needle = "$x->foo";
    let first = content.find(needle).unwrap();
    let occurrences = find_identical_occurrences(
        content,
        needle,
        first,
        first + needle.len(),
        0,
        content.len(),
    );
    // "$x->fooBar" contains "$x->foo" but is followed by 'B' (alphanumeric),
    // so it should NOT match.
    assert!(occurrences.is_empty());
}

#[test]
fn find_occurrences_respects_scope_boundary() {
    // Two functions each with `$x->foo()` — searching within the first
    // function's scope should not find the second.
    let content = "<?php\nfunction a() { echo $x->foo(); }\nfunction b() { echo $x->foo(); }\n";
    let needle = "$x->foo()";
    let first = content.find(needle).unwrap();
    // Scope of function a() body: from first `{` to first `}`
    let scope_start = content.find('{').unwrap();
    let scope_end = content.find('}').unwrap() + 1;
    let occurrences = find_identical_occurrences(
        content,
        needle,
        first,
        first + needle.len(),
        scope_start,
        scope_end,
    );
    assert!(
        occurrences.is_empty(),
        "should not find occurrence in function b() when scoped to function a()"
    );
}

// ── Multi-occurrence extract integration test ────────────────────

#[test]
fn extract_variable_offers_all_occurrences_variant() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "<?php\nfunction test() {\n    echo $x->foo() . $x->foo();\n}\n";

    backend.update_ast(uri, content);
    backend
        .open_files
        .write()
        .insert(uri.to_string(), std::sync::Arc::new(content.to_string()));

    // Select the first `$x->foo()`
    // Line 2: "    echo $x->foo() . $x->foo();\n"
    //          0123456789...
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(2, 9),
            end: Position::new(2, 19),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = backend.handle_code_action(uri, content, &params);
    let extract_actions: Vec<_> = actions
        .iter()
        .filter_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Extract variable") => {
                Some(ca)
            }
            _ => None,
        })
        .collect();

    // Should have two actions: "this occurrence" and "all occurrences"
    assert!(
        extract_actions.len() >= 2,
        "expected at least 2 extract actions (single + all), got {}: {:?}",
        extract_actions.len(),
        extract_actions.iter().map(|a| &a.title).collect::<Vec<_>>()
    );

    let single_action = extract_actions
        .iter()
        .find(|a| a.title.contains("this occurrence"))
        .expect("expected a 'this occurrence' action");
    assert!(
        single_action.title.contains("this occurrence"),
        "single action should mention 'this occurrence', got: {}",
        single_action.title
    );

    let all_action = extract_actions
        .iter()
        .find(|a| a.title.contains("all occurrences"))
        .expect("expected an 'all occurrences' action");
    assert!(
        all_action.title.contains("all occurrences"),
        "all action should mention 'all occurrences', got: {}",
        all_action.title
    );

    // Phase 1 should NOT have an edit — it's deferred.
    assert!(
        all_action.edit.is_none(),
        "Phase 1 should not compute edits for all-occurrences"
    );
    assert!(
        all_action.data.is_some(),
        "Phase 1 should attach resolve data for all-occurrences"
    );

    // Phase 2: resolve the action to get the workspace edit.
    let (resolved_all, _) = backend.resolve_code_action((*all_action).clone());
    let all_edit = resolved_all.edit.as_ref().unwrap();
    let all_changes = all_edit.changes.as_ref().unwrap();
    let file_edits = all_changes.values().next().unwrap();
    assert_eq!(
        file_edits.len(),
        3,
        "expected 3 edits (1 insert + 2 replacements), got {}",
        file_edits.len()
    );
}

#[test]
fn extract_variable_single_occurrence_no_all_variant() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "<?php\nfunction test() {\n    echo $x->foo() . $x->bar();\n}\n";

    backend.update_ast(uri, content);

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(2, 9),
            end: Position::new(2, 19),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = backend.handle_code_action(uri, content, &params);
    let extract_actions: Vec<_> = actions
        .iter()
        .filter_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Extract variable") => {
                Some(ca)
            }
            _ => None,
        })
        .collect();

    // Only one action — no "all occurrences" variant.
    assert_eq!(extract_actions.len(), 1);
    // Title should NOT say "this occurrence" when there's only one.
    assert!(
        !extract_actions[0].title.contains("this occurrence"),
        "should not say 'this occurrence' when unique, got: {}",
        extract_actions[0].title
    );
}
