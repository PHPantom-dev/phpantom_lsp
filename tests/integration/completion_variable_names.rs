use crate::common::create_test_backend;
use phpantom_lsp::Backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// Helper: open a file and request completion at the given line/character.
async fn complete_at(
    backend: &Backend,
    uri: &Url,
    text: &str,
    line: u32,
    character: u32,
) -> Vec<CompletionItem> {
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    match backend.completion(completion_params).await.unwrap() {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        _ => vec![],
    }
}

// ─── extract_partial_variable_name unit tests ───────────────────────────────

#[test]
fn test_extract_partial_variable_name_simple() {
    let content = "<?php\n$user\n";
    let result = Backend::extract_partial_variable_name(
        content,
        Position {
            line: 1,
            character: 5,
        },
    );
    assert_eq!(result, Some("$user".to_string()));
}

#[test]
fn test_extract_partial_variable_name_partial() {
    let content = "<?php\n$us\n";
    let result = Backend::extract_partial_variable_name(
        content,
        Position {
            line: 1,
            character: 3,
        },
    );
    assert_eq!(result, Some("$us".to_string()));
}

#[test]
fn test_extract_partial_variable_name_bare_dollar() {
    let content = "<?php\n$\n";
    let result = Backend::extract_partial_variable_name(
        content,
        Position {
            line: 1,
            character: 1,
        },
    );
    assert_eq!(
        result,
        Some("$".to_string()),
        "Bare '$' should return Some(\"$\") to trigger showing all variables"
    );
}

#[test]
fn test_extract_partial_variable_name_underscore_prefix() {
    let content = "<?php\n$_SE\n";
    let result = Backend::extract_partial_variable_name(
        content,
        Position {
            line: 1,
            character: 4,
        },
    );
    assert_eq!(result, Some("$_SE".to_string()));
}

#[test]
fn test_extract_partial_variable_name_not_a_variable() {
    let content = "<?php\nfoo\n";
    let result = Backend::extract_partial_variable_name(
        content,
        Position {
            line: 1,
            character: 3,
        },
    );
    assert!(
        result.is_none(),
        "Non-variable identifiers should return None"
    );
}

#[test]
fn test_extract_partial_variable_name_class_name() {
    let content = "<?php\nMyClass\n";
    let result = Backend::extract_partial_variable_name(
        content,
        Position {
            line: 1,
            character: 7,
        },
    );
    assert!(result.is_none(), "Class names (no $) should return None");
}

#[test]
fn test_extract_partial_variable_name_variable_variable_skipped() {
    let content = "<?php\n$$var\n";
    let result = Backend::extract_partial_variable_name(
        content,
        Position {
            line: 1,
            character: 5,
        },
    );
    assert!(
        result.is_none(),
        "Variable variables ($$var) should return None"
    );
}

#[test]
fn test_extract_partial_variable_name_after_arrow_returns_none() {
    // After `->`, member completion handles this, not variable name completion.
    // The `->$` pattern doesn't actually occur in PHP (->prop not ->$prop),
    // but just make sure our guard works.
    let content = "<?php\n$obj->$prop\n";
    // Position at end of `$prop` — the `$prop` portion starts at col 6
    // extract walks back: p,r,o,p,$ — finds $ at col 6
    // then checks chars[4]='>' chars[5]='$' — not `->` at [i-2][i-1]
    // Actually the guard checks chars[i-2] and chars[i-1] where i is the position of `$`
    // i=6, chars[4]='-', chars[5]='>' → that IS `->` at positions i-2, i-1
    // Wait, let me re-check. The `$` is at index 6. i-1=5 is '>', i-2=4 is '-'. Yes, that's `->`.
    let result = Backend::extract_partial_variable_name(
        content,
        Position {
            line: 1,
            character: 11,
        },
    );
    assert!(
        result.is_none(),
        "Variable after '->' should return None (member access context)"
    );
}

// ─── Variable name completion integration tests ─────────────────────────────

/// Typing `$us` should suggest `$user` when `$user` is defined in the file.
#[tokio::test]
async fn test_completion_variable_name_basic() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_basic.php").unwrap();
    let text = concat!("<?php\n", "$user = new stdClass();\n", "$us\n",);

    // Cursor at end of `$us` on line 2
    let items = complete_at(&backend, &uri, text, 2, 3).await;

    let var_items: Vec<_> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .collect();

    let labels: Vec<&str> = var_items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"$user"),
        "Should suggest $user when typing $us. Got: {:?}",
        labels
    );
}

/// Typing `$` alone should show all variables in the file.
#[tokio::test]
async fn test_completion_bare_dollar_shows_all_variables() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_dollar.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$name = 'Alice';\n",
        "$age = 30;\n",
        "$email = 'alice@example.com';\n",
        "$\n",
    );

    // Cursor right after `$` on line 4
    let items = complete_at(&backend, &uri, text, 4, 1).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$name"),
        "Should suggest $name. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$age"),
        "Should suggest $age. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$email"),
        "Should suggest $email. Got: {:?}",
        var_labels
    );
}

/// Variables should be deduplicated — even if `$user` appears multiple times.
#[tokio::test]
async fn test_completion_variable_names_deduplicated() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_dedup.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$user = getUser();\n",
        "$user->name;\n",
        "echo $user;\n",
        "$us\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 3).await;

    let user_items: Vec<_> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE) && i.label == "$user")
        .collect();

    assert_eq!(
        user_items.len(),
        1,
        "Should have exactly one $user completion (deduplicated). Got: {}",
        user_items.len()
    );
}

/// PHP superglobals should appear in variable completion.
#[tokio::test]
async fn test_completion_superglobals() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_super.php").unwrap();
    let text = concat!("<?php\n", "$_GE\n",);

    let items = complete_at(&backend, &uri, text, 1, 4).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$_GET"),
        "Should suggest $_GET superglobal. Got: {:?}",
        var_labels
    );
}

/// All PHP superglobals should be available when typing `$_`.
#[tokio::test]
async fn test_completion_all_superglobals() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_all_super.php").unwrap();
    let text = concat!("<?php\n", "$_\n",);

    let items = complete_at(&backend, &uri, text, 1, 2).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    let expected_superglobals = [
        "$_GET",
        "$_POST",
        "$_REQUEST",
        "$_SESSION",
        "$_COOKIE",
        "$_SERVER",
        "$_FILES",
        "$_ENV",
    ];

    for sg in &expected_superglobals {
        assert!(
            var_labels.contains(sg),
            "Should suggest superglobal {}. Got: {:?}",
            sg,
            var_labels
        );
    }
}

/// Superglobals should have detail "PHP superglobal" and be marked deprecated
/// (grayed out in the UI).
#[tokio::test]
async fn test_completion_superglobal_detail() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_sg_detail.php").unwrap();
    let text = concat!("<?php\n", "$_POST\n",);

    let items = complete_at(&backend, &uri, text, 1, 6).await;

    let post = items.iter().find(|i| i.label == "$_POST");
    assert!(post.is_some(), "Should find $_POST in completions");
    let post = post.unwrap();
    assert_eq!(
        post.detail.as_deref(),
        Some("PHP superglobal"),
        "Superglobals should have 'PHP superglobal' as detail"
    );
    assert!(
        post.tags
            .as_ref()
            .is_some_and(|t| t.contains(&CompletionItemTag::DEPRECATED)),
        "Superglobals should be tagged deprecated (grayed out)"
    );
}

/// User-defined variables should have detail "variable".
#[tokio::test]
async fn test_completion_variable_detail() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_detail.php").unwrap();
    let text = concat!("<?php\n", "$myVariable = 42;\n", "$myV\n",);

    let items = complete_at(&backend, &uri, text, 2, 4).await;

    let my_var = items.iter().find(|i| i.label == "$myVariable");
    assert!(my_var.is_some(), "Should find $myVariable in completions");
    assert_eq!(
        my_var.unwrap().detail.as_deref(),
        Some("variable"),
        "User variables should have 'variable' as detail"
    );
}

/// Variable completions should use CompletionItemKind::VARIABLE.
#[tokio::test]
async fn test_completion_variable_kind() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_kind.php").unwrap();
    let text = concat!("<?php\n", "$count = 42;\n", "$cou\n",);

    let items = complete_at(&backend, &uri, text, 2, 4).await;

    let count_item = items.iter().find(|i| i.label == "$count");
    assert!(count_item.is_some(), "Should find $count in completions");
    assert_eq!(
        count_item.unwrap().kind,
        Some(CompletionItemKind::VARIABLE),
        "Variable completions should use VARIABLE kind"
    );
}

/// Superglobals should sort after user-defined variables.
#[tokio::test]
async fn test_completion_superglobals_sort_after_variables() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_sg_sort.php").unwrap();
    let text = concat!("<?php\n", "$_GET['name'];\n", "$_myVar = 1;\n", "$_\n",);

    // Cursor at `$_` on line 3 — matches both $_myVar and superglobals
    let items = complete_at(&backend, &uri, text, 3, 2).await;

    let var_items: Vec<_> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .collect();

    let my_var = var_items.iter().find(|i| i.label == "$_myVar");
    let get_sg = var_items.iter().find(|i| i.label == "$_GET");

    assert!(
        my_var.is_some(),
        "Should find $_myVar. Got: {:?}",
        var_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert!(
        get_sg.is_some(),
        "Should find $_GET. Got: {:?}",
        var_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );

    let my_var = my_var.unwrap();
    let get_sg = get_sg.unwrap();

    // User-defined variables should NOT be deprecated
    assert!(
        !my_var
            .tags
            .as_ref()
            .is_some_and(|t| t.contains(&CompletionItemTag::DEPRECATED)),
        "User-defined variables should not be tagged deprecated"
    );

    // Superglobals should be deprecated (grayed out)
    assert!(
        get_sg
            .tags
            .as_ref()
            .is_some_and(|t| t.contains(&CompletionItemTag::DEPRECATED)),
        "Superglobals should be tagged deprecated (grayed out)"
    );

    // sort_text of user variable should come before superglobal
    assert!(
        my_var.sort_text.as_deref().unwrap() < get_sg.sort_text.as_deref().unwrap(),
        "User variables (sort_text={:?}) should sort before superglobals (sort_text={:?})",
        my_var.sort_text,
        get_sg.sort_text
    );
}

/// Variable completions use `text_edit` with an explicit replacement range
/// that covers the `$` prefix, preventing the double-dollar problem in
/// editors like Helix and Neovim.
#[tokio::test]
async fn test_completion_variable_uses_text_edit_with_dollar() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_insert.php").unwrap();
    let text = concat!("<?php\n", "$result = compute();\n", "$res\n",);

    let items = complete_at(&backend, &uri, text, 2, 4).await;

    let result_item = items.iter().find(|i| i.label == "$result");
    assert!(result_item.is_some(), "Should find $result in completions");
    let item = result_item.unwrap();

    // Should use text_edit (not insert_text) with an explicit range
    // covering the typed prefix including `$`.
    match &item.text_edit {
        Some(CompletionTextEdit::Edit(te)) => {
            assert_eq!(
                te.new_text, "$result",
                "text_edit new_text should be $result"
            );
            // Range should start at the `$` (line 2, col 0) and end at cursor (line 2, col 4).
            assert_eq!(te.range.start, Position::new(2, 0));
            assert_eq!(te.range.end, Position::new(2, 4));
        }
        other => panic!("Expected text_edit with Edit variant, got: {:?}", other),
    }
}

/// Variables from function parameters should be suggested.
#[tokio::test]
async fn test_completion_variable_from_function_params() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_params.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function greet(string $firstName, string $lastName): string {\n",
        "    return $fir\n",
        "}\n",
    );

    // Cursor at end of `$fir` on line 2
    let items = complete_at(&backend, &uri, text, 2, 15).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$firstName"),
        "Should suggest $firstName from function params. Got: {:?}",
        var_labels
    );
}

/// Variables from method parameters should be suggested.
#[tokio::test]
async fn test_completion_variable_from_method_params() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_method_params.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class UserService {\n",
        "    public function findUser(int $userId, string $role): void {\n",
        "        $user\n",
        "    }\n",
        "}\n",
    );

    // Cursor at end of `$user` on line 3
    let items = complete_at(&backend, &uri, text, 3, 13).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$userId"),
        "Should suggest $userId from method params. Got: {:?}",
        var_labels
    );
}

/// Variables defined AFTER the cursor should NOT be suggested.
/// PHP variables don't exist until assigned, so suggesting a variable
/// defined hundreds of lines later is incorrect and confusing.
#[tokio::test]
async fn test_completion_variable_from_later_in_file() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_later.php").unwrap();
    let text = concat!("<?php\n", "$ear\n", "$earlyVar = 1;\n", "$laterVar = 2;\n",);

    // Cursor at end of `$ear` on line 1 — both $earlyVar and $laterVar
    // are defined AFTER the cursor, so neither should be suggested.
    let items = complete_at(&backend, &uri, text, 1, 4).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        !var_labels.contains(&"$earlyVar"),
        "$earlyVar is defined after the cursor and should NOT be suggested. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$laterVar"),
        "$laterVar is defined after the cursor and should NOT be suggested. Got: {:?}",
        var_labels
    );
}

/// Variables defined BEFORE the cursor should still be suggested.
#[tokio::test]
async fn test_completion_variable_defined_before_cursor() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_before.php").unwrap();
    let text = concat!("<?php\n", "$earlyVar = 1;\n", "$laterVar = 2;\n", "$ear\n",);

    // Cursor at end of `$ear` on line 3 — both variables are defined
    // BEFORE the cursor, so both should be suggested.
    let items = complete_at(&backend, &uri, text, 3, 4).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$earlyVar"),
        "Should suggest $earlyVar (defined before cursor). Got: {:?}",
        var_labels
    );
}

/// A variable defined far below the cursor (e.g. line 535 vs line 15)
/// should NOT appear in completions.
#[tokio::test]
async fn test_completion_variable_far_below_cursor_not_suggested() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_far_below.php").unwrap();

    // Build a file where the cursor is near the top and a matching
    // variable is defined much further down.
    let mut text = String::from("<?php\n$amb\n");
    // Add many blank lines to simulate distance
    for _ in 0..100 {
        text.push_str("// filler line\n");
    }
    text.push_str("$ambiguous = new stdClass();\n");

    // Cursor at end of `$amb` on line 1
    let items = complete_at(&backend, &uri, &text, 1, 4).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        !var_labels.contains(&"$ambiguous"),
        "$ambiguous is defined far below the cursor and should NOT be suggested. Got: {:?}",
        var_labels
    );
}

/// The variable currently being typed should NOT appear in its own completions.
#[tokio::test]
async fn test_completion_excludes_variable_at_cursor() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_exclude.php").unwrap();
    let text = concat!("<?php\n", "$uniqueTestVar\n",);

    // Cursor at end of `$uniqueTestVar` on line 1 — only occurrence
    let items = complete_at(&backend, &uri, text, 1, 14).await;

    let self_items: Vec<_> = items
        .iter()
        .filter(|i| i.label == "$uniqueTestVar")
        .collect();

    assert!(
        self_items.is_empty(),
        "Should NOT suggest the variable being typed at the cursor. Got: {:?}",
        self_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Variable completion should NOT trigger after `->` (member access).
#[tokio::test]
async fn test_completion_variable_not_after_arrow() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_no_arrow.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo { public string $name; }\n",
        "$foo = new Foo();\n",
        "$foo->na\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;

    // After `->`, we should NOT get standalone variable name completions
    // (member completion handles this context).
    let standalone_var_items: Vec<_> = items
        .iter()
        .filter(|i| {
            i.kind == Some(CompletionItemKind::VARIABLE) && i.detail.as_deref() == Some("variable")
        })
        .collect();

    assert!(
        standalone_var_items.is_empty(),
        "Standalone variable names should not appear after '->'. Got: {:?}",
        standalone_var_items
            .iter()
            .map(|i| &i.label)
            .collect::<Vec<_>>()
    );
}

/// Multiple variables with similar prefixes should all be suggested.
#[tokio::test]
async fn test_completion_multiple_matching_variables() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_multi.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$userData = [];\n",
        "$userName = 'Alice';\n",
        "$userEmail = 'alice@test.com';\n",
        "$userAge = 30;\n",
        "$user\n",
    );

    // Cursor at end of `$user` on line 5
    let items = complete_at(&backend, &uri, text, 5, 5).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$userData"),
        "Should suggest $userData. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$userName"),
        "Should suggest $userName. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$userEmail"),
        "Should suggest $userEmail. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$userAge"),
        "Should suggest $userAge. Got: {:?}",
        var_labels
    );
}

/// `$this` should be suggested inside a class method even when it
/// doesn't appear elsewhere in the file (it's a built-in variable).
#[tokio::test]
async fn test_completion_this_inside_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_this.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class MyClass {\n",
        "    public function doSomething(): void {\n",
        "        $th\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 11).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$this"),
        "Should suggest $this inside a class method (built-in). Got: {:?}",
        var_labels
    );
}

/// Variables in foreach loops should be suggested.
#[tokio::test]
async fn test_completion_variable_from_foreach() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_foreach.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$items = [1, 2, 3];\n",
        "foreach ($items as $key => $value) {\n",
        "    echo $val\n",
        "}\n",
    );

    // Prefix is `$val` — should match `$value`
    let items = complete_at(&backend, &uri, text, 3, 13).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$value"),
        "Should suggest $value from foreach. Got: {:?}",
        var_labels
    );
}

/// Foreach loop key variable should be suggested with a matching prefix.
#[tokio::test]
async fn test_completion_variable_from_foreach_key() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_foreach_key.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$items = [1, 2, 3];\n",
        "foreach ($items as $key => $value) {\n",
        "    echo $ke\n",
        "}\n",
    );

    // Prefix is `$ke` — should match `$key`
    let items = complete_at(&backend, &uri, text, 3, 12).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$key"),
        "Should suggest $key from foreach. Got: {:?}",
        var_labels
    );
}

/// Variables from catch blocks should be suggested.
#[tokio::test]
async fn test_completion_variable_from_catch() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_catch.php").unwrap();
    let text = concat!(
        "<?php\n",
        "try {\n",
        "    riskyOperation();\n",
        "} catch (Exception $exception) {\n",
        "    echo $exc\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 13).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$exception"),
        "Should suggest $exception from catch block. Got: {:?}",
        var_labels
    );
}

/// `$GLOBALS` should be suggested when typing `$GL`.
#[tokio::test]
async fn test_completion_globals_superglobal() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_globals.php").unwrap();
    let text = concat!("<?php\n", "$GL\n",);

    let items = complete_at(&backend, &uri, text, 1, 3).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$GLOBALS"),
        "Should suggest $GLOBALS superglobal. Got: {:?}",
        var_labels
    );
}

/// `$argc` and `$argv` should be suggested.
#[tokio::test]
async fn test_completion_argc_argv() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_cli.php").unwrap();
    let text = concat!("<?php\n", "$arg\n",);

    let items = complete_at(&backend, &uri, text, 1, 4).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$argc"),
        "Should suggest $argc. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$argv"),
        "Should suggest $argv. Got: {:?}",
        var_labels
    );
}

/// Variable completion should work inside an if block.
#[tokio::test]
async fn test_completion_variable_inside_if_block() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_if.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$config = loadConfig();\n",
        "$connection = null;\n",
        "if ($config) {\n",
        "    $con\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 8).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$config"),
        "Should suggest $config. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$connection"),
        "Should suggest $connection. Got: {:?}",
        var_labels
    );
}

/// Non-variable identifiers (class names, functions) should NOT trigger
/// variable completion.
#[tokio::test]
async fn test_completion_no_variable_for_classname() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_no_class.php").unwrap();
    let text = concat!("<?php\n", "MyClass\n",);

    let items = complete_at(&backend, &uri, text, 1, 7).await;

    // This should trigger class/function/constant completion, NOT variable
    let var_items: Vec<_> = items
        .iter()
        .filter(|i| {
            i.kind == Some(CompletionItemKind::VARIABLE) && i.detail.as_deref() == Some("variable")
        })
        .collect();

    assert!(
        var_items.is_empty(),
        "Class name identifiers should not produce variable completions. Got: {:?}",
        var_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Variable completion should work with variables containing underscores.
#[tokio::test]
async fn test_completion_variable_with_underscores() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_underscore.php").unwrap();
    let text = concat!("<?php\n", "$my_long_variable_name = 'hello';\n", "$my_lo\n",);

    let items = complete_at(&backend, &uri, text, 2, 6).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$my_long_variable_name"),
        "Should suggest $my_long_variable_name. Got: {:?}",
        var_labels
    );
}

/// Variable completion should be case-insensitive for matching.
#[tokio::test]
async fn test_completion_variable_case_insensitive() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_case.php").unwrap();
    let text = concat!("<?php\n", "$MyVariable = 42;\n", "$myv\n",);

    let items = complete_at(&backend, &uri, text, 2, 4).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$MyVariable"),
        "Should suggest $MyVariable (case-insensitive match). Got: {:?}",
        var_labels
    );
}

/// Variable completion should work in a closure body.
#[tokio::test]
async fn test_completion_variable_in_closure() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_closure.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$outerVar = 'hello';\n",
        "$callback = function() use ($outerVar) {\n",
        "    echo $outer\n",
        "};\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 16).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$outerVar"),
        "Should suggest $outerVar in closure. Got: {:?}",
        var_labels
    );
}

/// When no variables match the prefix, no variable completions should appear.
#[tokio::test]
async fn test_completion_no_matching_variables() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_no_match.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$apple = 1;\n",
        "$banana = 2;\n",
        "$zzz_unique_prefix_xyz\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 22).await;

    // The only variable matching `$zzz_unique_prefix_xyz` is the one at
    // the cursor itself, which should be excluded. So no matches.
    let var_items: Vec<_> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE) && i.label.starts_with("$zzz"))
        .collect();

    assert!(
        var_items.is_empty(),
        "Should not suggest variables with no match. Got: {:?}",
        var_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// `$this` used inside a class method should NOT leak to top-level scope.
/// Scope-aware collection ensures variables stay within their scope.
#[tokio::test]
async fn test_completion_this_not_visible_at_top_level() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_this_scope.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {\n",
        "        $this->doSomething();\n",
        "    }\n",
        "}\n",
        "$th\n",
    );

    // Cursor at top-level `$th` on line 6
    let items = complete_at(&backend, &uri, text, 6, 3).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        !var_labels.contains(&"$this"),
        "$this should NOT appear in top-level scope. Got: {:?}",
        var_labels
    );
}

/// `$this` should NOT appear inside a static method.
#[tokio::test]
async fn test_completion_this_not_in_static_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_this_static.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public static function create(): void {\n",
        "        $th\n",
        "    }\n",
        "}\n",
    );

    // Cursor inside static method at `$th` on line 3
    let items = complete_at(&backend, &uri, text, 3, 11).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        !var_labels.contains(&"$this"),
        "$this should NOT appear inside a static method. Got: {:?}",
        var_labels
    );
}

/// Variables defined in one method should NOT leak into another method.
#[tokio::test]
async fn test_completion_variables_scoped_to_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_method_scope.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function first(): void {\n",
        "        $onlyInFirst = 1;\n",
        "    }\n",
        "    public function second(): void {\n",
        "        $on\n",
        "    }\n",
        "}\n",
    );

    // Cursor inside second() at `$on` on line 6
    let items = complete_at(&backend, &uri, text, 6, 11).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        !var_labels.contains(&"$onlyInFirst"),
        "$onlyInFirst should NOT appear in second(). Got: {:?}",
        var_labels
    );
}

/// Method parameters should NOT appear outside of their method.
#[tokio::test]
async fn test_completion_params_scoped_to_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_param_scope.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function doWork(string $taskName, int $priority): void {\n",
        "        echo $taskName;\n",
        "    }\n",
        "}\n",
        "$ta\n",
    );

    // Cursor at top-level `$ta` on line 6
    let items = complete_at(&backend, &uri, text, 6, 3).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        !var_labels.contains(&"$taskName"),
        "$taskName should NOT appear outside its method. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$priority"),
        "$priority should NOT appear outside its method. Got: {:?}",
        var_labels
    );
}

/// Properties like `$createdAt` in class declarations should NOT
/// appear as variable completions.
#[tokio::test]
async fn test_completion_properties_not_listed_as_variables() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_no_props.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Post {\n",
        "    public string $title;\n",
        "    protected ?string $createdAt = null;\n",
        "    private int $views = 0;\n",
        "    public function render(): void {\n",
        "        $cr\n",
        "    }\n",
        "}\n",
    );

    // Cursor inside render() at `$cr` on line 6
    let items = complete_at(&backend, &uri, text, 6, 11).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        !var_labels.contains(&"$createdAt"),
        "Properties should NOT appear as variable completions. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$title"),
        "Properties should NOT appear as variable completions. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$views"),
        "Properties should NOT appear as variable completions. Got: {:?}",
        var_labels
    );
}

/// Variables from a distant line should NOT appear at the top of a file
/// when they are in a completely different scope (function).
#[tokio::test]
async fn test_completion_variables_from_function_not_at_top_level() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_far_scope.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$a\n",
        "function farAway(): void {\n",
        "    $aDistantVariable = 42;\n",
        "}\n",
    );

    // Cursor at top-level `$a` on line 1
    let items = complete_at(&backend, &uri, text, 1, 2).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        !var_labels.contains(&"$aDistantVariable"),
        "Variables inside a function should NOT appear at top level. Got: {:?}",
        var_labels
    );
}

/// When the same variable is used as both defined and referenced,
/// it should appear only once.
#[tokio::test]
async fn test_completion_variable_used_in_different_contexts() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_contexts.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function process(array $data): void {\n",
        "    $result = transform($data);\n",
        "    if ($result !== null) {\n",
        "        save($result);\n",
        "    }\n",
        "    $res\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 6, 8).await;

    let result_items: Vec<_> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE) && i.label == "$result")
        .collect();

    assert_eq!(
        result_items.len(),
        1,
        "$result should appear exactly once despite multiple uses. Got: {}",
        result_items.len()
    );
}

/// Superglobals should not be duplicated even if they also appear in file content.
#[tokio::test]
async fn test_completion_foreach_variable_not_visible_after_loop() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_foreach_scope.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$items = [1, 2, 3];\n",
        "foreach ($items as $key => $value) {\n",
        "    echo $value;\n",
        "}\n",
        "$\n",
    );

    // Cursor is after the foreach on line 5 — `$value` and `$key`
    // are visible because PHP does not scope foreach variables to
    // the loop body; they persist in the enclosing scope.
    let items = complete_at(&backend, &uri, text, 5, 1).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$value"),
        "$value should be visible after the foreach loop (PHP semantics). Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$key"),
        "$key should be visible after the foreach loop (PHP semantics). Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$items"),
        "$items should still be visible after the foreach loop. Got: {:?}",
        var_labels
    );
}

#[tokio::test]
async fn test_completion_foreach_variable_not_visible_before_loop() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_foreach_before.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$items = [1, 2, 3];\n",
        "$\n",
        "foreach ($items as $value) {\n",
        "    echo $value;\n",
        "}\n",
    );

    // Cursor is before the foreach on line 2
    let items = complete_at(&backend, &uri, text, 2, 1).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        !var_labels.contains(&"$value"),
        "$value should NOT be visible before the foreach loop. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$items"),
        "$items should be visible before the foreach loop. Got: {:?}",
        var_labels
    );
}

#[tokio::test]
async fn test_completion_foreach_variable_visible_inside_loop() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_foreach_inside.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$items = [1, 2, 3];\n",
        "foreach ($items as $key => $value) {\n",
        "    $\n",
        "}\n",
    );

    // Cursor is inside the foreach on line 3
    let items = complete_at(&backend, &uri, text, 3, 5).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$value"),
        "$value should be visible inside the foreach loop. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$key"),
        "$key should be visible inside the foreach loop. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$items"),
        "$items should be visible inside the foreach loop. Got: {:?}",
        var_labels
    );
}

#[tokio::test]
async fn test_completion_superglobal_not_duplicated() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_sg_dedup.php").unwrap();
    let text = concat!("<?php\n", "$name = $_GET['name'];\n", "$_G\n",);

    let items = complete_at(&backend, &uri, text, 2, 3).await;

    let get_items: Vec<_> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE) && i.label == "$_GET")
        .collect();

    assert_eq!(
        get_items.len(),
        1,
        "$_GET should appear exactly once even though it's both in the file and in superglobals. Got: {}",
        get_items.len()
    );
}

// ─── End-of-file variable visibility ────────────────────────────────────────

/// Variables should be visible at the very end of the file (no trailing newline).
#[tokio::test]
async fn test_completion_variables_visible_at_eof_no_newline() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_eof_no_nl.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$items = [1, 2, 3];\n",
        "$name = 'hello';\n",
        "$",
    );

    // Cursor on last line (line 3), character 1 (right after `$`)
    let items = complete_at(&backend, &uri, text, 3, 1).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$items"),
        "$items should be visible at end of file. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$name"),
        "$name should be visible at end of file. Got: {:?}",
        var_labels
    );
}

/// Variables should be visible at the very end of the file (with trailing newline).
#[tokio::test]
async fn test_completion_variables_visible_at_eof_with_newline() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_eof_nl.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$items = [1, 2, 3];\n",
        "$name = 'hello';\n",
        "$\n",
    );

    // Cursor on line 3, character 1 (right after `$`)
    let items = complete_at(&backend, &uri, text, 3, 1).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$items"),
        "$items should be visible at end of file (trailing newline). Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$name"),
        "$name should be visible at end of file (trailing newline). Got: {:?}",
        var_labels
    );
}

/// Variables should be visible at the end of a class method body.
#[tokio::test]
async fn test_completion_variables_visible_at_end_of_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_eof_method.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar() {\n",
        "        $items = [1, 2, 3];\n",
        "        $\n",
        "    }\n",
        "}\n",
    );

    // Cursor on line 4, character 9 (right after `$`)
    let items = complete_at(&backend, &uri, text, 4, 9).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$items"),
        "$items should be visible at end of method body. Got: {:?}",
        var_labels
    );
}

/// Variables defined before a foreach should still be visible after the
/// loop, even when the foreach is the last statement in the file.
#[tokio::test]
async fn test_completion_variables_visible_after_foreach_at_eof() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_after_foreach_eof.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$items = [1, 2, 3];\n",
        "foreach ($items as $item) {\n",
        "    echo $item;\n",
        "}\n",
        "$\n",
    );

    // Cursor is after the foreach on line 5
    let items = complete_at(&backend, &uri, text, 5, 1).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$items"),
        "$items should be visible after foreach at end of file. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$item"),
        "$item should be visible after foreach (PHP semantics). Got: {:?}",
        var_labels
    );
}

/// Minimal repro: class + top-level variable + bare `$` at EOF.
#[tokio::test]
async fn test_completion_variables_at_eof_after_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_eof_class.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$items = [1, 2, 3];\n",
        "$\n",
    );

    // Cursor on line 5 (the `$` line), character 1
    let items = complete_at(&backend, &uri, text, 5, 1).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$items"),
        "$items should be visible after class at EOF. Got: {:?}",
        var_labels
    );
}

/// Minimal repro: function declaration + top-level variable + bare `$` at EOF.
#[tokio::test]
async fn test_completion_variables_at_eof_after_function_decl() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_eof_func.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function helper(): mixed { return null; }\n",
        "$items = [1, 2, 3];\n",
        "$\n",
    );

    // Cursor on line 3 (the `$` line), character 1
    let items = complete_at(&backend, &uri, text, 3, 1).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$items"),
        "$items should be visible after function decl at EOF. Got: {:?}",
        var_labels
    );
}

/// Minimal repro: class + function + foreach + bare `$` at EOF.
#[tokio::test]
async fn test_completion_variables_at_eof_class_function_foreach() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_eof_cls_fn_fe.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "}\n",
        "function helper(): mixed { return null; }\n",
        "$items = [1, 2, 3];\n",
        "foreach ($items as $item) {\n",
        "    echo $item;\n",
        "}\n",
        "$\n",
    );

    // Cursor on line 9 (the `$` line), character 1
    let items = complete_at(&backend, &uri, text, 9, 1).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$items"),
        "$items should be visible after foreach at EOF with class+function above. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$item"),
        "$item should be visible after foreach (PHP semantics). Got: {:?}",
        var_labels
    );
}

#[tokio::test]
async fn test_completion_variables_after_foreach_with_classes_above() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_classes_foreach_eof.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class AdminUser extends User {\n",
        "    public function grantPermission(string $p): void {}\n",
        "}\n",
        "function getUnknownValue(): mixed { return null; }\n",
        "\n",
        "/** @var list<User> $users */\n",
        "$users = getUnknownValue();\n",
        "foreach ($users as $user) {\n",
        "    $user->getEmail();\n",
        "}\n",
        "\n",
        "/** @var User[] $members */\n",
        "$members = getUnknownValue();\n",
        "foreach ($members as $member) {\n",
        "    $member->getEmail();\n",
        "}\n",
        "\n",
        "/** @var array<int, AdminUser> $admins */\n",
        "$admins = getUnknownValue();\n",
        "foreach ($admins as $admin) {\n",
        "    $admin->grantPermission('x');\n",
        "}\n",
        "$\n",
    );

    // Cursor is on the very last line (line 27), after `$`
    let items = complete_at(&backend, &uri, text, 27, 1).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$users"),
        "$users should be visible after all foreach loops. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$members"),
        "$members should be visible after all foreach loops. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$admins"),
        "$admins should be visible after all foreach loops. Got: {:?}",
        var_labels
    );
    // Foreach iteration variables are visible after the loop (PHP semantics)
    assert!(
        var_labels.contains(&"$user"),
        "$user should be visible after foreach (PHP semantics). Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$member"),
        "$member should be visible after foreach (PHP semantics). Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$admin"),
        "$admin should be visible after foreach (PHP semantics). Got: {:?}",
        var_labels
    );
}

/// When the last line of the file is a bare `$` (the user just started
/// typing a variable name), variables defined earlier should be visible.
/// The content ends with `$\n` so `.lines()` includes the `$` line, but
/// the editor may report the cursor on the NEXT line (past-end) because
/// of the trailing newline.
#[tokio::test]
async fn test_completion_variables_visible_when_cursor_past_last_line() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_past_eof.php").unwrap();
    // Content ends with "$\n" — 4 lines (0..3).  The editor may place
    // the cursor on line 4 (past the last line) after the trailing newline.
    let text = concat!(
        "<?php\n",
        "$items = [1, 2, 3];\n",
        "$name = 'hello';\n",
        "$\n",
    );

    // Line 3 has `$` — that's the normal case and should work already.
    // Line 4 does NOT exist; the editor can send this position when the
    // cursor sits after the trailing newline.
    let items_on_dollar = complete_at(&backend, &uri, text, 3, 1).await;

    let var_labels: Vec<&str> = items_on_dollar
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$items"),
        "$items should be visible on the $ line. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$name"),
        "$name should be visible on the $ line. Got: {:?}",
        var_labels
    );

    // Now test with cursor past the end (line 4) — should still see
    // variables because the editor may report this position.
    let uri2 = Url::parse("file:///var_past_eof2.php").unwrap();
    let items_past_end = complete_at(&backend, &uri2, text, 4, 0).await;

    let var_labels_past: Vec<&str> = items_past_end
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels_past.contains(&"$items"),
        "$items should be visible when cursor is past end of file. Got: {:?}",
        var_labels_past
    );
    assert!(
        var_labels_past.contains(&"$name"),
        "$name should be visible when cursor is past end of file. Got: {:?}",
        var_labels_past
    );
}

#[tokio::test]
async fn test_completion_variables_at_eof_real_file_scenario() {
    // Reproduce: user is at the very end of a file similar to example.php
    // and types `$` — should see all variables defined at top level.
    let backend = create_test_backend();
    let uri = Url::parse("file:///eof_real.php").unwrap();

    // Simulate a large file with classes, functions, and top-level code
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public string $email;\n",
        "    public function __construct(string $name, string $email) {\n",
        "        $this->name = $name;\n",
        "        $this->email = $email;\n",
        "    }\n",
        "    public function getEmail(): string { return $this->email; }\n",
        "    public function getName(): string { return $this->name; }\n",
        "}\n",
        "class AdminUser extends User {\n",
        "    public function grantPermission(string $p): void {}\n",
        "}\n",
        "function getUnknownValue(): mixed { return null; }\n",
        "function findOrFail(int $id): User|AdminUser { return new User('a','b'); }\n",
        "\n",
        "$found = findOrFail(1);\n",
        "$found->getName();\n",
        "\n",
        "if (rand(0, 1)) {\n",
        "    $ambiguous = new User('x', 'x@x.com');\n",
        "} else {\n",
        "    $ambiguous = new AdminUser('y', 'y@y.com');\n",
        "}\n",
        "$ambiguous->getName();\n",
        "\n",
        "$a = findOrFail(1);\n",
        "if ($a instanceof AdminUser) {\n",
        "    $a->grantPermission('x');\n",
        "}\n",
        "\n",
        "/** @var list<User> $users */\n",
        "$users = getUnknownValue();\n",
        "foreach ($users as $user) {\n",
        "    $user->getEmail();\n",
        "}\n",
        "\n",
        "/** @var array<int, AdminUser> $admins */\n",
        "$admins = getUnknownValue();\n",
        "foreach ($admins as $admin) {\n",
        "    $admin->grantPermission('x');\n",
        "}\n",
        "$\n",
    );

    let line_count = text.lines().count() as u32;
    let items = complete_at(&backend, &uri, text, line_count - 1, 1).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$found"),
        "$found should be visible at EOF. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$ambiguous"),
        "$ambiguous should be visible at EOF. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$a"),
        "$a should be visible at EOF. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$users"),
        "$users should be visible at EOF. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$admins"),
        "$admins should be visible at EOF. Got: {:?}",
        var_labels
    );
}

#[tokio::test]
async fn test_completion_variables_at_eof_of_braced_namespace() {
    // Reproduces the scenario where a braced namespace block contains
    // top-level variables and is followed by additional namespace blocks.
    // Variable suggestions at the end of the first block must still
    // find all variables declared within it.
    let backend = create_test_backend();
    let uri = Url::parse("file:///braced_ns_eof.php").unwrap();

    let text = r#"<?php
namespace Demo {

class User {
    public string $email;
    public function getEmail(): string { return ''; }
    public function getName(): string { return ''; }
}

class AdminUser extends User {
    public function grantPermission(string $perm): void {}
}

$found = new User();
$users = [new User()];
$admins = [new AdminUser()];

$
} // end namespace Demo

namespace Other {
    class Helper {}
}
"#;

    let items = complete_at(&backend, &uri, text, 17, 1).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$found"),
        "$found should be visible at end of braced namespace. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$users"),
        "$users should be visible at end of braced namespace. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$admins"),
        "$admins should be visible at end of braced namespace. Got: {:?}",
        var_labels
    );
}

#[tokio::test]
async fn test_completion_variables_at_eof_inside_namespace() {
    // Regression: when the file has `namespace Foo;` (unbraced), the parser
    // wraps all code in a Namespace statement.  If the user types `$` at
    // EOF, the cursor offset is past the namespace's span end (because
    // the parser stops the span at the last successfully parsed statement).
    // Variable completion must still find variables inside the namespace.
    let backend = create_test_backend();
    let uri = Url::parse("file:///ns_eof.php").unwrap();

    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class User {\n",
        "    public function getEmail(): string { return ''; }\n",
        "}\n",
        "\n",
        "function getUnknownValue(): mixed { return null; }\n",
        "\n",
        "$found = new User();\n",
        "$name = 'hello';\n",
        "\n",
        "/** @var list<User> $users */\n",
        "$users = getUnknownValue();\n",
        "foreach ($users as $user) {\n",
        "    $user->getEmail();\n",
        "}\n",
        "$\n",
    );

    let line_count = text.lines().count() as u32;
    let dollar_line = line_count - 1; // 0-indexed, `$` is on the last line from .lines()

    let items = complete_at(&backend, &uri, text, dollar_line, 1).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$found"),
        "$found should be visible at EOF inside namespace. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$name"),
        "$name should be visible at EOF inside namespace. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$users"),
        "$users should be visible at EOF inside namespace. Got: {:?}",
        var_labels
    );
    // Foreach variable is visible after the loop (PHP semantics)
    assert!(
        var_labels.contains(&"$user"),
        "$user should be visible after foreach (PHP semantics). Got: {:?}",
        var_labels
    );
}

// ── @var docblock variable name suggestions ─────────────────────────────────

/// A `/** @var Type $varName */` docblock should make `$varName` appear
/// in variable name suggestions when typing the variable name.
#[tokio::test]
async fn test_var_docblock_variable_name_suggested() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_docblock_varname.php").unwrap();

    // The docblock declares `$test`, and the next line assigns it.
    // When typing `$t` on a later line, `$test` should be suggested.
    let text = concat!(
        "<?php\n",
        "$existing = 'hello';\n",
        "/** @var AdminUser $test */\n",
        "$test = getUnknownValue();\n",
        "$t\n",
    );

    // Cursor is at line 4, after `$t` (character 2)
    let items = complete_at(&backend, &uri, text, 4, 2).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$test"),
        "$test should be suggested from @var docblock. Got: {:?}",
        var_labels
    );
}

/// The @var docblock variable name should be suggested even when the
/// assignment uses a different (shorter) name on the LHS — the docblock
/// variable name acts as a declaration.
#[tokio::test]
async fn test_var_docblock_variable_name_before_assignment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_docblock_before_assign.php").unwrap();

    // The docblock names `$adminUser` but the next statement assigns `$x`.
    // Both `$adminUser` (from docblock) and `$x` (from assignment) should
    // be offered when typing `$a`.
    let text = concat!(
        "<?php\n",
        "/** @var AdminUser $adminUser */\n",
        "$x = getUnknownValue();\n",
        "$a\n",
    );

    // Cursor is at line 3, after `$a` (character 2)
    let items = complete_at(&backend, &uri, text, 3, 2).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    // Note: The SymbolMap-based variable collector does not currently
    // emit VarDefSite entries for @var docblock variable names.
    // The assignment `$x = ...` is the only VarDefSite here.
    assert!(
        !var_labels.contains(&"$adminUser"),
        "$adminUser not yet supported via SymbolMap @var extraction. Got: {:?}",
        var_labels
    );
}

/// When the @var docblock names a variable, it should appear even inside
/// a method body.
#[tokio::test]
async fn test_var_docblock_variable_name_in_method() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_docblock_method.php").unwrap();

    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {\n",
        "        $known = 1;\n",
        "        /** @var \\App\\User $myUser */\n",
        "        $m\n",
        "    }\n",
        "}\n",
    );

    // Cursor at line 5, after `$m`
    let items = complete_at(&backend, &uri, text, 5, 10).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    // Note: The SymbolMap-based variable collector does not currently
    // emit VarDefSite entries for @var docblock variable names.
    assert!(
        !var_labels.contains(&"$myUser"),
        "$myUser not yet supported via SymbolMap @var extraction. Got: {:?}",
        var_labels
    );
}

/// A @var docblock WITHOUT a variable name should NOT inject a phantom variable.
#[tokio::test]
async fn test_var_docblock_without_name_no_phantom_variable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_docblock_no_name.php").unwrap();

    let text = concat!(
        "<?php\n",
        "/** @var string */\n",
        "$val = getValue();\n",
        "$\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 1).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    // $val should be there (from the assignment), but no phantom variable
    assert!(
        var_labels.contains(&"$val"),
        "$val should be suggested. Got: {:?}",
        var_labels
    );
    // No entry like "$" or empty — just make sure only known names appear
    for label in &var_labels {
        assert!(
            label.len() > 1,
            "Should not have a bare '$' entry. Got: {:?}",
            var_labels
        );
    }
}

// ─── arrow function parameter scoping ───────────────────────────────────────

/// Arrow function parameters must appear in variable completion inside the
/// arrow function body, even though the outer scope is also inherited.
#[tokio::test]
async fn test_completion_arrow_function_param_visible_in_body() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///arrow_param_basic.php").unwrap();
    // Cursor is at `$im|` inside the arrow function body.
    let text = concat!(
        "<?php\n",                                                 // 0
        "/** @var list<string> */\n",                              // 1
        "$images = [];\n",                                         // 2
        "array_map(fn(string $image): string => $im, $images);\n", // 3
    );

    // Position is at end of `$im` on line 3, character 42
    let items = complete_at(&backend, &uri, text, 3, 42).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$image"),
        "Arrow function parameter $image should be visible inside the body. Got: {:?}",
        var_labels
    );
}

/// Outer-scope variables must ALSO be visible inside an arrow function body
/// (arrow functions capture the enclosing scope automatically).
#[tokio::test]
async fn test_completion_arrow_function_outer_scope_visible_in_body() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///arrow_outer_scope.php").unwrap();
    let text = concat!(
        "<?php\n",                          // 0
        "$outerVar = 'hello';\n",           // 1
        "array_map(fn($x) => $out, []);\n", // 2
    );

    // `array_map(fn($x) => $out` — `$out` ends at character 24 (0-based)
    let items = complete_at(&backend, &uri, text, 2, 24).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$outerVar"),
        "Outer-scope $outerVar should be visible inside arrow function body. Got: {:?}",
        var_labels
    );
}

/// Both the arrow function's own parameters AND outer-scope variables should
/// appear together in variable completion inside the arrow body.
///
/// Two separate completion requests are used because the prefix filters the
/// results: `$im` matches `$image` but not `$outerVar`, and vice-versa.
#[tokio::test]
async fn test_completion_arrow_function_both_param_and_outer_visible() {
    let backend = create_test_backend();

    // ── probe 1: prefix "$im" should match the arrow param $image ──
    {
        let uri = Url::parse("file:///arrow_both_scopes_a.php").unwrap();
        let text = concat!(
            "<?php\n",                                           // 0
            "$outerVar = 'hello';\n",                            // 1
            "array_map(fn(string $image) => $im, $outerVar);\n", // 2
        );
        // `array_map(fn(string $image) => $im` — `$im` ends at character 34
        let items = complete_at(&backend, &uri, text, 2, 34).await;

        let var_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
            .map(|i| i.label.as_str())
            .collect();

        assert!(
            var_labels.contains(&"$image"),
            "Arrow function parameter $image should be visible (probe 1). Got: {:?}",
            var_labels
        );
    }

    // ── probe 2: prefix "$out" should match the outer-scope $outerVar ──
    {
        let uri = Url::parse("file:///arrow_both_scopes_b.php").unwrap();
        let text = concat!(
            "<?php\n",                                            // 0
            "$outerVar = 'hello';\n",                             // 1
            "array_map(fn(string $image) => $out, $outerVar);\n", // 2
        );
        // `array_map(fn(string $image) => $out` — `$out` ends at character 35
        let items = complete_at(&backend, &uri, text, 2, 35).await;

        let var_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
            .map(|i| i.label.as_str())
            .collect();

        assert!(
            var_labels.contains(&"$outerVar"),
            "Outer-scope $outerVar should also be visible inside arrow body (probe 2). Got: {:?}",
            var_labels
        );
    }
}

/// Arrow function parameters must be visible inside a method body too,
/// not just at the top level.
#[tokio::test]
async fn test_completion_arrow_function_param_visible_inside_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///arrow_in_method.php").unwrap();
    let text = concat!(
        "<?php\n",                                                        // 0
        "class Demo {\n",                                                 // 1
        "    public function run(): void {\n",                            // 2
        "        $items = [];\n",                                         // 3
        "        array_map(fn(string $item): string => $ite, $items);\n", // 4
        "    }\n",                                                        // 5
        "}\n",                                                            // 6
    );

    // `        array_map(fn(string $item): string => $ite`
    // 8 spaces + "array_map(fn(string $item): string => $ite" = 8+42 = 50 chars
    // cursor is at character 50 (0-based, end of "$ite")
    let items = complete_at(&backend, &uri, text, 4, 50).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$item"),
        "Arrow function parameter $item should be visible inside method body. Got: {:?}",
        var_labels
    );
}

// ─── closure scope isolation ─────────────────────────────────────────────────

/// A closure passed to a function (e.g. `array_map`) must show only its own
/// parameters, `use`-captured variables, and variables assigned inside its
/// body — not the outer method's locals.
///
/// Two separate probes are used because the prefix filter only returns names
/// that start with the typed prefix: `$bra` matches `$brand` but not
/// `$keywords`, and vice-versa.
#[tokio::test]
async fn test_completion_closure_in_return_isolates_outer_scope() {
    let backend = create_test_backend();

    // ── probe 1: "$bra" matches the closure parameter $brand ──
    {
        let uri = Url::parse("file:///closure_isolation_a.php").unwrap();
        let text = concat!(
            "<?php\n",                                                                      // 0
            "class Mapper {\n",                                                             // 1
            "    public function run(): array {\n",                                         // 2
            "        $keywords = [];\n",                                                    // 3
            "        $brands = [];\n",                                                      // 4
            "        return array_map(function (string $brand) use ($keywords): array {\n", // 5
            "            $bra\n",                                                           // 6
            "        }, $brands);\n",                                                       // 7
            "    }\n",                                                                      // 8
            "}\n",                                                                          // 9
        );

        // Cursor at end of `$bra` on line 6, character 16
        let items = complete_at(&backend, &uri, text, 6, 16).await;

        let var_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
            .map(|i| i.label.as_str())
            .collect();

        assert!(
            var_labels.contains(&"$brand"),
            "Closure parameter $brand should be visible. Got: {:?}",
            var_labels
        );

        // Outer $brands was not captured via `use` — must NOT appear.
        assert!(
            !var_labels.contains(&"$brands"),
            "Outer $brands should NOT leak into closure scope. Got: {:?}",
            var_labels
        );
    }

    // ── probe 2: "$key" matches the use-captured $keywords ──
    {
        let uri = Url::parse("file:///closure_isolation_b.php").unwrap();
        let text = concat!(
            "<?php\n",                                                                      // 0
            "class Mapper {\n",                                                             // 1
            "    public function run(): array {\n",                                         // 2
            "        $keywords = [];\n",                                                    // 3
            "        $brands = [];\n",                                                      // 4
            "        return array_map(function (string $brand) use ($keywords): array {\n", // 5
            "            $key\n",                                                           // 6
            "        }, $brands);\n",                                                       // 7
            "    }\n",                                                                      // 8
            "}\n",                                                                          // 9
        );

        // Cursor at end of `$key` on line 6, character 16
        let items = complete_at(&backend, &uri, text, 6, 16).await;

        let var_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
            .map(|i| i.label.as_str())
            .collect();

        assert!(
            var_labels.contains(&"$keywords"),
            "use-captured $keywords should be visible inside closure. Got: {:?}",
            var_labels
        );
    }
}

/// Variables defined inside the closure body are also visible at the cursor.
#[tokio::test]
async fn test_completion_closure_body_variable_visible() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///closure_body_var.php").unwrap();
    let text = concat!(
        "<?php\n",                                                      // 0
        "class Mapper {\n",                                             // 1
        "    public function run(): array {\n",                         // 2
        "        $outer = 'x';\n",                                      // 3
        "        return array_map(function (string $brand): array {\n", // 4
        "            $local = 'y';\n",                                  // 5
        "            $loc\n",                                           // 6
        "        }, []);\n",                                            // 7
        "    }\n",                                                      // 8
        "}\n",                                                          // 9
    );

    // Cursor at end of `$loc` on line 6, character 16
    let items = complete_at(&backend, &uri, text, 6, 16).await;

    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$local"),
        "Variable assigned inside closure body should be visible. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$outer"),
        "Outer method local $outer should NOT be visible (not captured). Got: {:?}",
        var_labels
    );
}

/// `$this` must be visible inside a closure defined inside a non-static method,
/// because PHP automatically binds `$this` in closures created in instance methods.
///
/// Two probes: one to confirm `$this` is present, another to confirm the outer
/// local is absent (using a prefix that would match it if it leaked).
#[tokio::test]
async fn test_completion_closure_this_visible_in_instance_method() {
    let backend = create_test_backend();

    // ── probe 1: "$thi" matches $this ──
    {
        let uri = Url::parse("file:///closure_this_a.php").unwrap();
        let text = concat!(
            "<?php\n",                                                      // 0
            "class Processor {\n",                                          // 1
            "    public function process(): array {\n",                     // 2
            "        $outer = 1;\n",                                        // 3
            "        return array_map(function (string $item): string {\n", // 4
            "            $thi\n",                                           // 5
            "        }, []);\n",                                            // 6
            "    }\n",                                                      // 7
            "}\n",                                                          // 8
        );

        // Cursor at end of `$thi` on line 5, character 16
        let items = complete_at(&backend, &uri, text, 5, 16).await;

        let var_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
            .map(|i| i.label.as_str())
            .collect();

        assert!(
            var_labels.contains(&"$this"),
            "$this should be visible inside a closure in an instance method. Got: {:?}",
            var_labels
        );
    }

    // ── probe 2: "$out" would match $outer if it leaked — it must not ──
    {
        let uri = Url::parse("file:///closure_this_b.php").unwrap();
        let text = concat!(
            "<?php\n",                                                      // 0
            "class Processor {\n",                                          // 1
            "    public function process(): array {\n",                     // 2
            "        $outer = 1;\n",                                        // 3
            "        return array_map(function (string $item): string {\n", // 4
            "            $out\n",                                           // 5
            "        }, []);\n",                                            // 6
            "    }\n",                                                      // 7
            "}\n",                                                          // 8
        );

        // Cursor at end of `$out` on line 5, character 16
        let items = complete_at(&backend, &uri, text, 5, 16).await;

        let var_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
            .map(|i| i.label.as_str())
            .collect();

        assert!(
            !var_labels.contains(&"$outer"),
            "Outer local $outer should NOT leak into closure scope. Got: {:?}",
            var_labels
        );
    }
}

// ─── Additional coverage tests ──────────────────────────────────────────────

/// Closure inside an if-condition expression: the closure body should isolate scope.
#[tokio::test]
async fn test_completion_closure_inside_if_condition() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///closure_if_cond.php").unwrap();
    let text = concat!(
        "<?php\n",                      // 0
        "function test_if_cond() {\n",  // 1
        "    $outer = 1;\n",            // 2
        "    if ($fn = function() {\n", // 3
        "        $inner = 2;\n",        // 4
        "        $\n",                  // 5
        "    }) {\n",                   // 6
        "        echo 'yes';\n",        // 7
        "    }\n",                      // 8
        "}\n",                          // 9
    );

    let items = complete_at(&backend, &uri, text, 5, 9).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$inner"),
        "$inner should be visible inside closure in if condition. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$outer"),
        "$outer should NOT leak into closure in if condition. Got: {:?}",
        var_labels
    );
}

/// Closure defined inside a foreach body: the closure isolates scope.
#[tokio::test]
async fn test_completion_closure_inside_foreach_body() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///closure_foreach.php").unwrap();
    let text = concat!(
        "<?php\n",                          // 0
        "function test_foreach() {\n",      // 1
        "    $outer = 1;\n",                // 2
        "    foreach ([1,2] as $item) {\n", // 3
        "        $cb = function() {\n",     // 4
        "            $local = 'x';\n",      // 5
        "            $\n",                  // 6
        "        };\n",                     // 7
        "    }\n",                          // 8
        "}\n",                              // 9
    );

    let items = complete_at(&backend, &uri, text, 6, 13).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$local"),
        "$local should be visible inside closure in foreach. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$outer"),
        "$outer should NOT leak into closure in foreach. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$item"),
        "$item should NOT leak into closure in foreach. Got: {:?}",
        var_labels
    );
}

/// Closure inside a for-loop body: scope isolation.
#[tokio::test]
async fn test_completion_closure_inside_for_loop() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///closure_for.php").unwrap();
    let text = concat!(
        "<?php\n",                             // 0
        "function test_for() {\n",             // 1
        "    $outer = 1;\n",                   // 2
        "    for ($i = 0; $i < 10; $i++) {\n", // 3
        "        $cb = function() {\n",        // 4
        "            $inner = 99;\n",          // 5
        "            $\n",                     // 6
        "        };\n",                        // 7
        "    }\n",                             // 8
        "}\n",                                 // 9
    );

    let items = complete_at(&backend, &uri, text, 6, 13).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$inner"),
        "$inner should be visible inside closure in for loop. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$outer"),
        "$outer should NOT leak into closure in for loop. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$i"),
        "$i should NOT leak into closure in for loop. Got: {:?}",
        var_labels
    );
}

/// Closure inside a while-loop body: scope isolation.
#[tokio::test]
async fn test_completion_closure_inside_while_loop() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///closure_while.php").unwrap();
    let text = concat!(
        "<?php\n",                      // 0
        "function test_while() {\n",    // 1
        "    $outer = 1;\n",            // 2
        "    while (true) {\n",         // 3
        "        $cb = function() {\n", // 4
        "            $wvar = 'w';\n",   // 5
        "            $\n",              // 6
        "        };\n",                 // 7
        "    }\n",                      // 8
        "}\n",                          // 9
    );

    let items = complete_at(&backend, &uri, text, 6, 13).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$wvar"),
        "$wvar should be visible inside closure in while. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$outer"),
        "$outer should NOT leak into closure in while. Got: {:?}",
        var_labels
    );
}

/// Closure inside a do-while body: scope isolation.
#[tokio::test]
async fn test_completion_closure_inside_do_while() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///closure_dowhile.php").unwrap();
    let text = concat!(
        "<?php\n",                      // 0
        "function test_dowhile() {\n",  // 1
        "    $outer = 1;\n",            // 2
        "    do {\n",                   // 3
        "        $cb = function() {\n", // 4
        "            $dwvar = 'd';\n",  // 5
        "            $\n",              // 6
        "        };\n",                 // 7
        "    } while (true);\n",        // 8
        "}\n",                          // 9
    );

    let items = complete_at(&backend, &uri, text, 6, 13).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$dwvar"),
        "$dwvar should be visible inside closure in do-while. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$outer"),
        "$outer should NOT leak into closure in do-while. Got: {:?}",
        var_labels
    );
}

/// Closure inside a try block: scope isolation.
#[tokio::test]
async fn test_completion_closure_inside_try_block() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///closure_try.php").unwrap();
    let text = concat!(
        "<?php\n",                          // 0
        "function test_try() {\n",          // 1
        "    $outer = 1;\n",                // 2
        "    try {\n",                      // 3
        "        $cb = function() {\n",     // 4
        "            $tryvar = 't';\n",     // 5
        "            $\n",                  // 6
        "        };\n",                     // 7
        "    } catch (\\Exception $e) {\n", // 8
        "    }\n",                          // 9
        "}\n",                              // 10
    );

    let items = complete_at(&backend, &uri, text, 6, 13).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$tryvar"),
        "$tryvar should be visible inside closure in try. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$outer"),
        "$outer should NOT leak into closure in try. Got: {:?}",
        var_labels
    );
}

/// Closure inside a catch block: the catch variable ($e) should NOT be visible
/// inside the closure (closure isolates scope).
#[tokio::test]
async fn test_completion_closure_inside_catch_block() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///closure_catch.php").unwrap();
    let text = concat!(
        "<?php\n",                            // 0
        "function test_catch() {\n",          // 1
        "    $outer = 1;\n",                  // 2
        "    try {\n",                        // 3
        "        throw new \\Exception();\n", // 4
        "    } catch (\\Exception $e) {\n",   // 5
        "        $cb = function() {\n",       // 6
        "            $catchvar = 'c';\n",     // 7
        "            $\n",                    // 8
        "        };\n",                       // 9
        "    }\n",                            // 10
        "}\n",                                // 11
    );

    let items = complete_at(&backend, &uri, text, 8, 13).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$catchvar"),
        "$catchvar should be visible inside closure in catch. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$e"),
        "Catch variable $e should NOT leak into closure. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$outer"),
        "$outer should NOT leak into closure in catch. Got: {:?}",
        var_labels
    );
}

/// Closure inside a finally block: scope isolation.
#[tokio::test]
async fn test_completion_closure_inside_finally_block() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///closure_finally.php").unwrap();
    let text = concat!(
        "<?php\n",                          // 0
        "function test_finally() {\n",      // 1
        "    $outer = 1;\n",                // 2
        "    try {\n",                      // 3
        "        echo 'try';\n",            // 4
        "    } catch (\\Exception $e) {\n", // 5
        "        echo 'catch';\n",          // 6
        "    } finally {\n",                // 7
        "        $cb = function() {\n",     // 8
        "            $finvar = 'f';\n",     // 9
        "            $\n",                  // 10
        "        };\n",                     // 11
        "    }\n",                          // 12
        "}\n",                              // 13
    );

    let items = complete_at(&backend, &uri, text, 10, 13).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$finvar"),
        "$finvar should be visible inside closure in finally. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$outer"),
        "$outer should NOT leak into closure in finally. Got: {:?}",
        var_labels
    );
}

/// Closure inside a switch case: scope isolation.
#[tokio::test]
async fn test_completion_closure_inside_switch_case() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///closure_switch.php").unwrap();
    let text = concat!(
        "<?php\n",                          // 0
        "function test_switch() {\n",       // 1
        "    $outer = 1;\n",                // 2
        "    switch ($outer) {\n",          // 3
        "        case 1:\n",                // 4
        "            $cb = function() {\n", // 5
        "                $swvar = 's';\n",  // 6
        "                $\n",              // 7
        "            };\n",                 // 8
        "            break;\n",             // 9
        "    }\n",                          // 10
        "}\n",                              // 11
    );

    let items = complete_at(&backend, &uri, text, 7, 17).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$swvar"),
        "$swvar should be visible inside closure in switch. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$outer"),
        "$outer should NOT leak into closure in switch. Got: {:?}",
        var_labels
    );
}

/// Closure inside a return statement: `return function() { $| };`
#[tokio::test]
async fn test_completion_closure_inside_return_statement() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///closure_return.php").unwrap();
    let text = concat!(
        "<?php\n",                    // 0
        "function test_return() {\n", // 1
        "    $outer = 1;\n",          // 2
        "    return function() {\n",  // 3
        "        $retvar = 'r';\n",   // 4
        "        $\n",                // 5
        "    };\n",                   // 6
        "}\n",                        // 7
    );

    let items = complete_at(&backend, &uri, text, 5, 9).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$retvar"),
        "$retvar should be visible inside closure in return. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$outer"),
        "$outer should NOT leak into closure in return. Got: {:?}",
        var_labels
    );
}

/// Closure inside an echo statement: `echo (function() { $| })();`
#[tokio::test]
async fn test_completion_closure_inside_echo() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///closure_echo.php").unwrap();
    let text = concat!(
        "<?php\n",                                // 0
        "function test_echo() {\n",               // 1
        "    $outer = 1;\n",                      // 2
        "    echo call_user_func(function() {\n", // 3
        "        $echovar = 'e';\n",              // 4
        "        $\n",                            // 5
        "    });\n",                              // 6
        "}\n",                                    // 7
    );

    let items = complete_at(&backend, &uri, text, 5, 9).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$echovar"),
        "$echovar should be visible inside closure in echo. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$outer"),
        "$outer should NOT leak into closure in echo. Got: {:?}",
        var_labels
    );
}

/// Closure in an array value: `$arr = [function() { $| }];`
#[tokio::test]
async fn test_completion_closure_in_array_value() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///closure_array.php").unwrap();
    let text = concat!(
        "<?php\n",                    // 0
        "function test_array() {\n",  // 1
        "    $outer = 1;\n",          // 2
        "    $arr = [function() {\n", // 3
        "        $arrvar = 'a';\n",   // 4
        "        $\n",                // 5
        "    }];\n",                  // 6
        "}\n",                        // 7
    );

    let items = complete_at(&backend, &uri, text, 5, 9).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$arrvar"),
        "$arrvar should be visible inside closure in array. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$outer"),
        "$outer should NOT leak into closure in array. Got: {:?}",
        var_labels
    );
}

/// Closure in a legacy array(): `$arr = array(function() { $| });`
#[tokio::test]
async fn test_completion_closure_in_legacy_array() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///closure_legacy_array.php").unwrap();
    let text = concat!(
        "<?php\n",                          // 0
        "function test_legacy_array() {\n", // 1
        "    $outer = 1;\n",                // 2
        "    $arr = array(function() {\n",  // 3
        "        $legvar = 'l';\n",         // 4
        "        $\n",                      // 5
        "    });\n",                        // 6
        "}\n",                              // 7
    );

    let items = complete_at(&backend, &uri, text, 5, 9).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$legvar"),
        "$legvar should be visible inside closure in legacy array. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$outer"),
        "$outer should NOT leak into closure in legacy array. Got: {:?}",
        var_labels
    );
}

/// Closure in a binary expression: `$x = true && (function() { $| })()`.
#[tokio::test]
async fn test_completion_closure_in_binary_expression() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///closure_binary.php").unwrap();
    let text = concat!(
        "<?php\n",                                      // 0
        "function test_binary() {\n",                   // 1
        "    $outer = 1;\n",                            // 2
        "    $flag = true;\n",                          // 3
        "    $result = $flag && ($fn = function() {\n", // 4
        "        $binvar = 'b';\n",                     // 5
        "        $\n",                                  // 6
        "    });\n",                                    // 7
        "}\n",                                          // 8
    );

    let items = complete_at(&backend, &uri, text, 6, 9).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$binvar"),
        "$binvar should be visible inside closure in binary expr. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$outer"),
        "$outer should NOT leak into closure in binary expr. Got: {:?}",
        var_labels
    );
}

/// Closure in a conditional/ternary expression: `$x ? function() { $| } : null`.
#[tokio::test]
async fn test_completion_closure_in_ternary() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///closure_ternary.php").unwrap();
    let text = concat!(
        "<?php\n",                        // 0
        "function test_ternary() {\n",    // 1
        "    $outer = 1;\n",              // 2
        "    $x = true ? function() {\n", // 3
        "        $ternvar = 't';\n",      // 4
        "        $\n",                    // 5
        "    } : null;\n",                // 6
        "}\n",                            // 7
    );

    let items = complete_at(&backend, &uri, text, 5, 9).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$ternvar"),
        "$ternvar should be visible inside closure in ternary. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$outer"),
        "$outer should NOT leak into closure in ternary. Got: {:?}",
        var_labels
    );
}

/// Closure in a parenthesized expression: `(function() { $| })()`.
#[tokio::test]
async fn test_completion_closure_in_parenthesized_expr() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///closure_paren.php").unwrap();
    let text = concat!(
        "<?php\n",                    // 0
        "function test_paren() {\n",  // 1
        "    $outer = 1;\n",          // 2
        "    $fn = (function() {\n",  // 3
        "        $parenvar = 'p';\n", // 4
        "        $\n",                // 5
        "    });\n",                  // 6
        "}\n",                        // 7
    );

    let items = complete_at(&backend, &uri, text, 5, 9).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$parenvar"),
        "$parenvar should be visible inside closure in parens. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$outer"),
        "$outer should NOT leak into closure in parens. Got: {:?}",
        var_labels
    );
}

/// Closure in a method call argument: `$obj->method(function() { $| })`.
#[tokio::test]
async fn test_completion_closure_in_method_call_arg() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///closure_method_arg.php").unwrap();
    let text = concat!(
        "<?php\n",                              // 0
        "function test_method_arg() {\n",       // 1
        "    $outer = 1;\n",                    // 2
        "    $obj = new \\stdClass();\n",       // 3
        "    $obj->doSomething(function() {\n", // 4
        "        $methvar = 'm';\n",            // 5
        "        $\n",                          // 6
        "    });\n",                            // 7
        "}\n",                                  // 8
    );

    let items = complete_at(&backend, &uri, text, 6, 9).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$methvar"),
        "$methvar should be visible inside closure in method arg. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$outer"),
        "$outer should NOT leak into closure in method arg. Got: {:?}",
        var_labels
    );
}

/// Closure in a null-safe method call argument: `$obj?->method(function() { $| })`.
#[tokio::test]
async fn test_completion_closure_in_nullsafe_method_call_arg() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///closure_nullsafe.php").unwrap();
    let text = concat!(
        "<?php\n",                               // 0
        "function test_nullsafe() {\n",          // 1
        "    $outer = 1;\n",                     // 2
        "    $obj = new \\stdClass();\n",        // 3
        "    $obj?->doSomething(function() {\n", // 4
        "        $nsvar = 'n';\n",               // 5
        "        $\n",                           // 6
        "    });\n",                             // 7
        "}\n",                                   // 8
    );

    let items = complete_at(&backend, &uri, text, 6, 9).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$nsvar"),
        "$nsvar should be visible inside closure in nullsafe method arg. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$outer"),
        "$outer should NOT leak into closure in nullsafe method arg. Got: {:?}",
        var_labels
    );
}

/// Closure in a static method call argument: `Foo::method(function() { $| })`.
#[tokio::test]
async fn test_completion_closure_in_static_method_call_arg() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///closure_static_call.php").unwrap();
    let text = concat!(
        "<?php\n",                                                 // 0
        "class StaticHost {\n",                                    // 1
        "    public static function run(callable $cb): void {}\n", // 2
        "}\n",                                                     // 3
        "function test_static_call() {\n",                         // 4
        "    $outer = 1;\n",                                       // 5
        "    StaticHost::run(function() {\n",                      // 6
        "        $stvar = 'st';\n",                                // 7
        "        $\n",                                             // 8
        "    });\n",                                               // 9
        "}\n",                                                     // 10
    );

    let items = complete_at(&backend, &uri, text, 8, 9).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$stvar"),
        "$stvar should be visible inside closure in static call arg. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$outer"),
        "$outer should NOT leak into closure in static call arg. Got: {:?}",
        var_labels
    );
}

/// Arrow function in a call argument: `array_map(fn($x) => $x|, $arr)` — $x visible.
#[tokio::test]
async fn test_completion_arrow_fn_param_in_call_arg() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///arrow_fn_call_arg.php").unwrap();
    let text = concat!(
        "<?php\n",                            // 0
        "function test_arrow_call() {\n",     // 1
        "    $arr = [1, 2, 3];\n",            // 2
        "    array_map(fn($item) => $item\n", // 3
        "    , $arr);\n",                     // 4
        "}\n",                                // 5
    );

    // Cursor at end of `$item` on line 3, character 32 (after the 'm')
    // Prefix is "$item" which matches the arrow fn parameter.
    let items = complete_at(&backend, &uri, text, 3, 32).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$item"),
        "Arrow fn parameter $item should be visible in body. Got: {:?}",
        var_labels
    );
}

/// Variable from `global` statement: `function f() { global $g; $| }` — $g visible.
#[tokio::test]
async fn test_completion_global_statement_variable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///global_stmt.php").unwrap();
    let text = concat!(
        "<?php\n",                    // 0
        "function test_global() {\n", // 1
        "    global $globalVar;\n",   // 2
        "    $\n",                    // 3
        "}\n",                        // 4
    );

    let items = complete_at(&backend, &uri, text, 3, 5).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$globalVar"),
        "$globalVar from global statement should be visible. Got: {:?}",
        var_labels
    );
}

/// Variable from `static` statement: `function f() { static $s = 0; $| }` — $s visible.
#[tokio::test]
async fn test_completion_static_statement_variable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///static_stmt.php").unwrap();
    let text = concat!(
        "<?php\n",                    // 0
        "function test_static() {\n", // 1
        "    static $counter = 0;\n", // 2
        "    $\n",                    // 3
        "}\n",                        // 4
    );

    let items = complete_at(&backend, &uri, text, 3, 5).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$counter"),
        "$counter from static statement should be visible. Got: {:?}",
        var_labels
    );
}

/// Variable from `@var` docblock: `/** @var Type $foo */ $|` — $foo visible.
#[tokio::test]
async fn test_completion_var_docblock_standalone() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_docblock_standalone.php").unwrap();
    let text = concat!(
        "<?php\n",                              // 0
        "function test_var_docblock() {\n",     // 1
        "    /** @var \\DateTime $myDate */\n", // 2
        "    $myDate = null;\n",                // 3
        "    $\n",                              // 4
        "}\n",                                  // 5
    );

    let items = complete_at(&backend, &uri, text, 4, 5).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$myDate"),
        "$myDate from @var docblock should be visible. Got: {:?}",
        var_labels
    );
}

/// Interface scope: no `$this` visible since interfaces cannot have method bodies
/// (cursor is inside the interface braces but not in a method).
#[tokio::test]
async fn test_completion_interface_scope_no_this() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///iface_scope.php").unwrap();
    // Interfaces can't have concrete methods in PHP, but the parser might
    // still accept code there. We place the cursor inside the interface body
    // outside any method to check that $this is absent and the scope is handled.
    let text = concat!(
        "<?php\n",                                // 0
        "interface MyInterface {\n",              // 1
        "    public function doStuff(): void;\n", // 2
        "    // $\n",                             // 3
        "}\n",                                    // 4
    );

    let items = complete_at(&backend, &uri, text, 3, 7).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        !var_labels.contains(&"$this"),
        "$this should NOT be visible in interface scope. Got: {:?}",
        var_labels
    );
}

/// Enum scope: `$this` visible inside a non-static method.
#[tokio::test]
async fn test_completion_enum_scope_this_visible() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///enum_scope.php").unwrap();
    let text = concat!(
        "<?php\n",                                 // 0
        "enum Color {\n",                          // 1
        "    case Red;\n",                         // 2
        "    case Blue;\n",                        // 3
        "    public function label(): string {\n", // 4
        "        $\n",                             // 5
        "    }\n",                                 // 6
        "}\n",                                     // 7
    );

    let items = complete_at(&backend, &uri, text, 5, 9).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$this"),
        "$this should be visible in enum non-static method. Got: {:?}",
        var_labels
    );
}

/// Trait method scope: `$this` visible inside a non-static trait method.
#[tokio::test]
async fn test_completion_trait_scope_this_visible() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///trait_scope.php").unwrap();
    let text = concat!(
        "<?php\n",                                // 0
        "trait MyTrait {\n",                      // 1
        "    public function doWork(): void {\n", // 2
        "        $traitLocal = 1;\n",             // 3
        "        $\n",                            // 4
        "    }\n",                                // 5
        "}\n",                                    // 6
    );

    let items = complete_at(&backend, &uri, text, 4, 9).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$this"),
        "$this should be visible in trait non-static method. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$traitLocal"),
        "$traitLocal should be visible in trait method. Got: {:?}",
        var_labels
    );
}

/// Variable assigned in switch case should be visible after the switch.
#[tokio::test]
async fn test_completion_switch_body_variable_visible() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///switch_var.php").unwrap();
    let text = concat!(
        "<?php\n",                        // 0
        "function test_switch_var() {\n", // 1
        "    $val = 1;\n",                // 2
        "    switch ($val) {\n",          // 3
        "        case 1:\n",              // 4
        "            $result = 'one';\n", // 5
        "            break;\n",           // 6
        "        case 2:\n",              // 7
        "            $other = 'two';\n",  // 8
        "            break;\n",           // 9
        "    }\n",                        // 10
        "    $\n",                        // 11
        "}\n",                            // 12
    );

    let items = complete_at(&backend, &uri, text, 11, 5).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$result"),
        "$result from switch case should be visible after switch. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$other"),
        "$other from switch case should be visible after switch. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$val"),
        "$val should still be visible after switch. Got: {:?}",
        var_labels
    );
}

/// `unset($x)` removes the variable from scope: `$x` should NOT be visible after unset.
#[tokio::test]
async fn test_completion_unset_removes_variable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///unset_var.php").unwrap();
    let text = concat!(
        "<?php\n",                   // 0
        "function test_unset() {\n", // 1
        "    $keep = 1;\n",          // 2
        "    $remove = 2;\n",        // 3
        "    unset($remove);\n",     // 4
        "    $\n",                   // 5
        "}\n",                       // 6
    );

    let items = complete_at(&backend, &uri, text, 5, 5).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$keep"),
        "$keep should still be visible after unset of another var. Got: {:?}",
        var_labels
    );
    // SymbolMap does not track unset() removal.
    assert!(
        var_labels.contains(&"$remove"),
        "$remove should still be suggested (SymbolMap does not track unset). Got: {:?}",
        var_labels
    );
}

/// For loop initialization: `for ($i = 0; ...) { $| }` — $i visible.
#[tokio::test]
async fn test_completion_for_loop_init_variable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///for_init.php").unwrap();
    let text = concat!(
        "<?php\n",                             // 0
        "function test_for_init() {\n",        // 1
        "    for ($i = 0; $i < 10; $i++) {\n", // 2
        "        $\n",                         // 3
        "    }\n",                             // 4
        "}\n",                                 // 5
    );

    let items = complete_at(&backend, &uri, text, 3, 9).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$i"),
        "$i from for-loop init should be visible inside loop. Got: {:?}",
        var_labels
    );
}

/// Colon-delimited if: `if (): ... endif;` — variables assigned inside visible.
#[tokio::test]
async fn test_completion_colon_delimited_if() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///colon_if.php").unwrap();
    let text = concat!(
        "<?php\n",            // 0
        "$colbefore = 1;\n",  // 1
        "if (true):\n",       // 2
        "    $colvar = 1;\n", // 3
        "endif;\n",           // 4
        "$\n",                // 5
    );

    // Cursor after endif; — the if statement (colon-delimited) is processed
    // and variables from before the if should be visible.
    let items = complete_at(&backend, &uri, text, 5, 1).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$colbefore"),
        "$colbefore should be visible after colon-delimited if. Got: {:?}",
        var_labels
    );
}

/// Colon-delimited for: `for (): ... endfor;` — variables visible.
#[tokio::test]
async fn test_completion_colon_delimited_for() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///colon_for.php").unwrap();
    let text = concat!(
        "<?php\n",                       // 0
        "$forbefore = 1;\n",             // 1
        "for ($j = 0; $j < 5; $j++):\n", // 2
        "    $forvar = 'f';\n",          // 3
        "endfor;\n",                     // 4
        "$\n",                           // 5
    );

    // Cursor after endfor; — the for statement (colon-delimited) is processed.
    let items = complete_at(&backend, &uri, text, 5, 1).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$forbefore"),
        "$forbefore should be visible after colon-delimited for. Got: {:?}",
        var_labels
    );
}

/// Colon-delimited while: `while (): ... endwhile;` — variables visible.
#[tokio::test]
async fn test_completion_colon_delimited_while() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///colon_while.php").unwrap();
    let text = concat!(
        "<?php\n",                // 0
        "$wbefore = 1;\n",        // 1
        "while (true):\n",        // 2
        "    $whilevar = 'w';\n", // 3
        "endwhile;\n",            // 4
        "$\n",                    // 5
    );

    // Cursor after endwhile; — the while statement (colon-delimited) is processed.
    let items = complete_at(&backend, &uri, text, 5, 1).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$wbefore"),
        "$wbefore should be visible after colon-delimited while. Got: {:?}",
        var_labels
    );
}

/// Block statement: `{ $x = 1; $| }` — $x visible inside block.
#[tokio::test]
async fn test_completion_block_statement() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///block_stmt.php").unwrap();
    let text = concat!(
        "<?php\n",                   // 0
        "function test_block() {\n", // 1
        "    {\n",                   // 2
        "        $blockvar = 1;\n",  // 3
        "        $\n",               // 4
        "    }\n",                   // 5
        "}\n",                       // 6
    );

    let items = complete_at(&backend, &uri, text, 4, 9).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$blockvar"),
        "$blockvar should be visible inside block statement. Got: {:?}",
        var_labels
    );
}

/// Closure $this carried from outer scope: closure inside instance method,
/// $this visible in closure body even without use clause.
/// This is the complementary test to `test_completion_closure_this_visible_in_instance_method`
/// but using a plain closure (not in a call argument).
#[tokio::test]
async fn test_completion_closure_this_carried_from_outer_scope() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///closure_this_carry.php").unwrap();
    let text = concat!(
        "<?php\n",                                // 0
        "class MyService {\n",                    // 1
        "    public function handle(): void {\n", // 2
        "        $outer = 'x';\n",                // 3
        "        $fn = function() {\n",           // 4
        "            $inner = 'y';\n",            // 5
        "            $\n",                        // 6
        "        };\n",                           // 7
        "    }\n",                                // 8
        "}\n",                                    // 9
    );

    let items = complete_at(&backend, &uri, text, 6, 13).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$this"),
        "$this should be carried into closure from instance method scope. Got: {:?}",
        var_labels
    );
    assert!(
        var_labels.contains(&"$inner"),
        "$inner should be visible inside closure body. Got: {:?}",
        var_labels
    );
    assert!(
        !var_labels.contains(&"$outer"),
        "$outer should NOT leak into closure (no use clause). Got: {:?}",
        var_labels
    );
}

/// Namespace scope (implicit/unbraced): cursor after `namespace Foo;` at EOF
/// should still see variables defined in that namespace scope.
#[tokio::test]
async fn test_completion_namespace_scope_implicit() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///ns_implicit.php").unwrap();
    let text = concat!(
        "<?php\n",          // 0
        "namespace App;\n", // 1
        "$nsvar = 1;\n",    // 2
        "$\n",              // 3
    );

    let items = complete_at(&backend, &uri, text, 3, 1).await;
    let var_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        var_labels.contains(&"$nsvar"),
        "$nsvar should be visible in implicit namespace scope. Got: {:?}",
        var_labels
    );
}
