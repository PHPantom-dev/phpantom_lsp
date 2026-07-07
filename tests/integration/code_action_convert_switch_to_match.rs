//! Integration tests for the "Convert to match expression" code action.

use crate::common::create_test_backend;
use tower_lsp::lsp_types::*;

fn get_code_actions(
    backend: &phpantom_lsp::Backend,
    uri: &str,
    content: &str,
    line: u32,
    character: u32,
) -> Vec<CodeActionOrCommand> {
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(line, character),
            end: Position::new(line, character),
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

    backend.handle_code_action(uri, content, &params)
}

fn find_convert_action(actions: &[CodeActionOrCommand]) -> Option<&CodeAction> {
    actions.iter().find_map(|a| match a {
        CodeActionOrCommand::CodeAction(ca) if ca.title == "Convert to match expression" => {
            Some(ca)
        }
        _ => None,
    })
}

fn extract_edit_text(action: &CodeAction) -> String {
    let edit = action.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    let edits: Vec<&TextEdit> = changes.values().flat_map(|v| v.iter()).collect();
    assert_eq!(edits.len(), 1);
    edits[0].new_text.clone()
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
    let actions = get_code_actions(&backend, uri, content, 2, 4);
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
    let actions = get_code_actions(&backend, uri, content, 2, 4);
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
    let actions = get_code_actions(&backend, uri, content, 2, 4);
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
    let actions = get_code_actions(&backend, uri, content, 2, 4);
    assert!(find_convert_action(&actions).is_none());
}
