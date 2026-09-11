//! Integration tests for the "Convert to match expression" code action.

use crate::common::{create_test_backend, extract_edit_text, get_code_actions_at};
use tower_lsp::lsp_types::*;

fn find_convert_action(actions: &[CodeActionOrCommand]) -> Option<&CodeAction> {
    actions.iter().find_map(|a| match a {
        CodeActionOrCommand::CodeAction(ca) if ca.title == "Convert to match expression" => {
            Some(ca)
        }
        _ => None,
    })
}

#[test]
fn offered_on_return_switch() {
    let content = r#"<?php
function test($x) {
    switch ($x) {
        case 1:
            return 'one';
        case 2:
            return 'two';
        default:
            return 'other';
    }
}
"#;
    let backend = create_test_backend();
    let uri = "file:///test.php";
    backend.update_ast(uri, content);
    let actions = get_code_actions_at(&backend, uri, content, 2, 4);
    let action = find_convert_action(&actions).expect("action should be offered");
    let text = extract_edit_text(action);
    assert!(text.contains("return match ("));
    assert!(text.contains("1 => 'one'"));
    assert!(text.contains("default => 'other'"));
}

#[test]
fn offered_on_assignment_switch() {
    let content = r#"<?php
function test($status) {
    switch ($status) {
        case 'active':
            $label = 'Active';
            break;
        case 'inactive':
            $label = 'Inactive';
            break;
    }
}
"#;
    let backend = create_test_backend();
    let uri = "file:///test.php";
    backend.update_ast(uri, content);
    let actions = get_code_actions_at(&backend, uri, content, 2, 4);
    let action = find_convert_action(&actions).expect("action should be offered");
    let text = extract_edit_text(action);
    assert!(text.contains("$label = match ("));
    assert!(text.contains("'active' => 'Active'"));
}

#[test]
fn not_offered_when_mixed_modes() {
    let content = r#"<?php
function test($x) {
    switch ($x) {
        case 1:
            return 'one';
        case 2:
            $y = 'two';
            break;
    }
}
"#;
    let backend = create_test_backend();
    let uri = "file:///test.php";
    backend.update_ast(uri, content);
    let actions = get_code_actions_at(&backend, uri, content, 2, 4);
    assert!(find_convert_action(&actions).is_none());
}

#[test]
fn not_offered_on_php74() {
    let content = r#"<?php
function test($x) {
    switch ($x) {
        case 1:
            return 'one';
        default:
            return 'other';
    }
}
"#;
    let backend = create_test_backend();
    backend.set_php_version(phpantom_lsp::types::PhpVersion::new(7, 4));
    let uri = "file:///test.php";
    backend.update_ast(uri, content);
    let actions = get_code_actions_at(&backend, uri, content, 2, 4);
    assert!(find_convert_action(&actions).is_none());
}
