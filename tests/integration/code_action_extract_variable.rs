//! Integration tests for the "Extract variable" code action.
//!
//! These tests exercise the full pipeline: a non-empty selection over an
//! expression offers a `refactor.extract` action whose edit is deferred to
//! the resolve phase, where it becomes an assignment inserted before the
//! enclosing statement plus a replacement of the selection (or of every
//! identical occurrence in scope) with the new variable.

use crate::common::{
    create_test_backend, extract_edits, find_action, find_action_containing, find_actions,
    get_code_actions_at, get_code_actions_in_range, position_after, position_of, resolve_action,
};
use tower_lsp::lsp_types::*;

// ── Offering the action and the edits it generates ──────────────────────────

#[test]
fn extract_variable_action_offered_for_selection() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nfunction test() {\n    echo $user->getName();\n}\n";

    backend.update_ast(uri, content);

    // Select `$user->getName()` (line 2, from `$user` to closing `)`)
    let actions = get_code_actions_in_range(
        &backend,
        uri,
        content,
        Range::new(
            position_of(content, "$user->getName()"),
            position_after(content, "$user->getName()"),
        ),
    );
    let extract_action =
        find_action(&actions, "Extract variable").expect("expected extract variable action");

    assert_eq!(extract_action.kind, Some(CodeActionKind::REFACTOR_EXTRACT));
}

#[test]
fn extract_variable_not_offered_for_empty_selection() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nfunction test() {\n    echo $user->getName();\n}\n";

    backend.update_ast(uri, content);

    // Empty selection (cursor, no range)
    let actions = get_code_actions_at(&backend, uri, content, 2, 9);
    let extract_actions = find_actions(&actions, "Extract variable");

    assert!(
        extract_actions.is_empty(),
        "should not offer extract variable for empty selection"
    );
}

#[test]
fn extract_variable_not_offered_for_trait_name_selection() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\ntrait ExampleFeatureTrait {}\n";

    backend.update_ast(uri, content);

    let actions = get_code_actions_in_range(
        &backend,
        uri,
        content,
        Range::new(
            position_of(content, "ExampleFeatureTrait"),
            position_after(content, "ExampleFeatureTrait"),
        ),
    );
    let extract_actions = find_actions(&actions, "Extract variable");

    assert!(
        extract_actions.is_empty(),
        "should not offer extract variable for trait name selection"
    );
}

#[test]
fn extract_variable_generates_correct_edits() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nfunction test() {\n    echo $user->getName();\n}\n";

    backend.update_ast(uri, content);

    // Select `$user->getName()`
    // Line 2: "    echo $user->getName();\n"
    // $user starts at character 9, `) ` ends at character 25
    let actions = get_code_actions_in_range(
        &backend,
        uri,
        content,
        Range::new(Position::new(2, 9), Position::new(2, 25)),
    );
    let extract_action =
        find_action(&actions, "Extract variable").expect("expected extract variable action");

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
    let resolved = resolve_action(&backend, uri, content, extract_action);
    let file_edits = extract_edits(&resolved);

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
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content =
        "<?php\nfunction test() {\n    $name = 'existing';\n    echo $user->getName();\n}\n";

    backend.update_ast(uri, content);

    // Select `$user->getName()` on line 3
    // Line 3: "    echo $user->getName();\n"
    let actions = get_code_actions_in_range(
        &backend,
        uri,
        content,
        Range::new(Position::new(3, 9), Position::new(3, 25)),
    );
    let _extract_action =
        find_action(&actions, "Extract variable").expect("expected extract variable action");
}

#[test]
fn extract_variable_static_call() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nfunction test() {\n    echo Carbon::now();\n}\n";

    backend.update_ast(uri, content);

    // Select `Carbon::now()` on line 2
    // Line 2: "    echo Carbon::now();\n"
    let actions = get_code_actions_in_range(
        &backend,
        uri,
        content,
        Range::new(Position::new(2, 9), Position::new(2, 22)),
    );
    let _extract_action =
        find_action(&actions, "Extract variable").expect("expected extract variable action");
}

#[test]
fn extract_variable_function_call() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nfunction test() {\n    echo array_filter($items, $fn);\n}\n";

    backend.update_ast(uri, content);

    // Select `array_filter($items, $fn)` on line 2
    // Line 2: "    echo array_filter($items, $fn);\n"
    let actions = get_code_actions_in_range(
        &backend,
        uri,
        content,
        Range::new(Position::new(2, 9), Position::new(2, 34)),
    );
    let _extract_action =
        find_action(&actions, "Extract variable").expect("expected extract variable action");
}

#[test]
fn extract_variable_whitespace_only_selection_skipped() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nfunction test() {\n    echo 'hello';\n}\n";

    backend.update_ast(uri, content);

    // Select just whitespace on line 2 (chars 0..4 = "    ")
    let actions = get_code_actions_in_range(
        &backend,
        uri,
        content,
        Range::new(Position::new(2, 0), Position::new(2, 4)),
    );
    let extract_actions = find_actions(&actions, "Extract variable");

    assert!(
        extract_actions.is_empty(),
        "should not offer extract variable for whitespace-only selection"
    );
}

#[test]
fn extract_variable_not_offered_for_standalone_statement() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nfunction test() {\n    $this->save($id);\n    $this->log($id);\n}\n";

    backend.update_ast(uri, content);

    // Select `$this->save($id)` — the entire expression of a standalone statement.
    let actions = get_code_actions_in_range(
        &backend,
        uri,
        content,
        Range::new(Position::new(2, 4), Position::new(2, 21)),
    );
    let extract_actions = find_actions(&actions, "Extract variable");

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
    let backend = create_test_backend();
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
    let actions = get_code_actions_in_range(
        &backend,
        uri,
        content,
        Range::new(
            Position::new(4, comment_line.len() as u32),
            Position::new(5, vardump_line.len() as u32),
        ),
    );
    let extract_actions = find_actions(&actions, "Extract variable");

    assert!(
        extract_actions.is_empty(),
        "should not offer extract variable for standalone statement selected across lines: {:?}",
        extract_actions.iter().map(|a| &a.title).collect::<Vec<_>>()
    );
}

// ── Selection context rejections ────────────────────────────────────────────

#[test]
fn extract_variable_not_offered_for_bare_method_name() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nfunction test() {\n    $this->save($id);\n}\n";

    backend.update_ast(uri, content);

    // Select just `save` — the bare method name.
    let actions = get_code_actions_in_range(
        &backend,
        uri,
        content,
        Range::new(
            position_of(content, "save"),
            position_after(content, "save"),
        ),
    );
    let extract_actions = find_actions(&actions, "Extract variable");

    assert!(
        extract_actions.is_empty(),
        "should not offer extract variable for bare method name 'save'"
    );
}

#[test]
fn extract_variable_not_offered_for_method_call_fragment() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nfunction test() {\n    $label = $order->getLabel();\n}\n";

    backend.update_ast(uri, content);

    // Select `getLabel()` — preceded by `->` in source.
    let actions = get_code_actions_in_range(
        &backend,
        uri,
        content,
        Range::new(
            position_of(content, "getLabel()"),
            position_after(content, "getLabel()"),
        ),
    );
    let extract_actions = find_actions(&actions, "Extract variable");

    assert!(
        extract_actions.is_empty(),
        "should not offer extract variable for method call fragment 'getLabel()'"
    );
}

// ── Multi-occurrence extract integration test ───────────────────────────────

#[test]
fn extract_variable_offers_all_occurrences_variant() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nfunction test() {\n    echo $x->foo() . $x->foo();\n}\n";

    backend.update_ast(uri, content);

    // Select the first `$x->foo()`
    // Line 2: "    echo $x->foo() . $x->foo();\n"
    //          0123456789...
    let actions = get_code_actions_in_range(
        &backend,
        uri,
        content,
        Range::new(Position::new(2, 9), Position::new(2, 19)),
    );
    let extract_actions = find_actions(&actions, "Extract variable");

    // Should have two actions: "this occurrence" and "all occurrences"
    assert!(
        extract_actions.len() >= 2,
        "expected at least 2 extract actions (single + all), got {}: {:?}",
        extract_actions.len(),
        extract_actions.iter().map(|a| &a.title).collect::<Vec<_>>()
    );

    let single_action = find_action_containing(&actions, "this occurrence")
        .expect("expected a 'this occurrence' action");
    assert!(
        single_action.title.contains("this occurrence"),
        "single action should mention 'this occurrence', got: {}",
        single_action.title
    );

    let all_action = find_action_containing(&actions, "all occurrences")
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
    let resolved_all = resolve_action(&backend, uri, content, all_action);
    let file_edits = extract_edits(&resolved_all);
    assert_eq!(
        file_edits.len(),
        3,
        "expected 3 edits (1 insert + 2 replacements), got {}",
        file_edits.len()
    );
}

#[test]
fn extract_variable_single_occurrence_no_all_variant() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nfunction test() {\n    echo $x->foo() . $x->bar();\n}\n";

    backend.update_ast(uri, content);

    let actions = get_code_actions_in_range(
        &backend,
        uri,
        content,
        Range::new(Position::new(2, 9), Position::new(2, 19)),
    );
    let extract_actions = find_actions(&actions, "Extract variable");

    // Only one action — no "all occurrences" variant.
    assert_eq!(extract_actions.len(), 1);
    // Title should NOT say "this occurrence" when there's only one.
    assert!(
        !extract_actions[0].title.contains("this occurrence"),
        "should not say 'this occurrence' when unique, got: {}",
        extract_actions[0].title
    );
}
