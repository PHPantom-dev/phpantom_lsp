//! Integration tests for the "Convert to string interpolation" code action.

use crate::common::{create_test_backend, get_code_actions_at};
use tower_lsp::lsp_types::*;

fn find_action(actions: &[CodeActionOrCommand]) -> Option<&CodeAction> {
    actions.iter().find_map(|a| match a {
        CodeActionOrCommand::CodeAction(ca) if ca.title == "Convert to string interpolation" => {
            Some(ca)
        }
        _ => None,
    })
}

/// Apply the action's single edit to `content` and return the result.
fn apply(action: &CodeAction, content: &str) -> String {
    let edit = action.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    let edits: Vec<&TextEdit> = changes.values().flat_map(|v| v.iter()).collect();
    assert_eq!(edits.len(), 1);

    let lines: Vec<&str> = content.split('\n').collect();
    let offset = |pos: Position| -> usize {
        let mut o = 0;
        for line in lines.iter().take(pos.line as usize) {
            o += line.len() + 1;
        }
        o + pos.character as usize
    };
    let range = edits[0].range;
    let mut result = String::from(&content[..offset(range.start)]);
    result.push_str(&edits[0].new_text);
    result.push_str(&content[offset(range.end)..]);
    result
}

fn actions_at(content: &str, line: u32, character: u32) -> Vec<CodeActionOrCommand> {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    backend.update_ast(uri, content);
    get_code_actions_at(&backend, uri, content, line, character)
}

#[test]
fn offered_on_assignment_rhs() {
    let content = "<?php\nfunction greet(string $name): string {\n    $greeting = 'Hello ' . $name . ', welcome!';\n    return $greeting;\n}\n";
    let actions = actions_at(content, 2, 25);
    let action = find_action(&actions).expect("action should be offered");
    assert!(
        apply(action, content).contains(r#"$greeting = "Hello {$name}, welcome!";"#),
        "unexpected result: {}",
        apply(action, content)
    );
}

#[test]
fn offered_on_return_value() {
    let content = "<?php\nfunction label(object $item): string {\n    return '#' . $item->id;\n}\n";
    let actions = actions_at(content, 2, 16);
    let action = find_action(&actions).expect("action should be offered");
    assert!(apply(action, content).contains(r##"return "#{$item->id}";"##));
}

#[test]
fn offered_on_echo() {
    let content = "<?php\nfunction show(string $name): void {\n    echo 'Hi ' . $name;\n}\n";
    let actions = actions_at(content, 2, 12);
    let action = find_action(&actions).expect("action should be offered");
    assert!(apply(action, content).contains(r#"echo "Hi {$name}";"#));
}

#[test]
fn not_offered_inside_call_argument() {
    let content = "<?php\nfunction show(string $name): void {\n    printf('Hi ' . $name);\n}\n";
    let actions = actions_at(content, 2, 14);
    assert!(find_action(&actions).is_none());
}

#[test]
fn not_offered_for_all_literal_chain() {
    let content = "<?php\nfunction show(): string {\n    return 'a' . 'b';\n}\n";
    let actions = actions_at(content, 2, 14);
    assert!(find_action(&actions).is_none());
}

#[test]
fn not_offered_away_from_the_concatenation() {
    let content = "<?php\nfunction greet(string $name): string {\n    $greeting = 'Hello ' . $name;\n    return $greeting;\n}\n";
    let actions = actions_at(content, 3, 8);
    assert!(find_action(&actions).is_none());
}
