//! Integration tests for the "import and shorten usages" code action.
//!
//! Each test parses a file that spells a symbol by its qualified name,
//! requests code actions on that name, and verifies the import the action
//! adds plus the shortened replacements it makes at every usage.

use crate::common::{create_test_backend, get_code_actions_in_range, position_of};
use tower_lsp::lsp_types::*;

const BULK_TITLE: &str = "Import all qualified symbols and shorten usages";

fn actions_at(content: &str, needle: &str) -> Vec<CodeAction> {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    backend.update_ast(uri, content);
    let pos = position_of(content, needle);
    get_code_actions_in_range(&backend, uri, content, Range::new(pos, pos))
        .into_iter()
        .filter_map(|action| match action {
            CodeActionOrCommand::CodeAction(action) if action.title.contains("shorten usages") => {
                Some(action)
            }
            _ => None,
        })
        .collect()
}

fn action(content: &str, needle: &str) -> CodeAction {
    actions_at(content, needle)
        .into_iter()
        .find(|action| action.title != BULK_TITLE)
        .expect("expected import-and-shorten action")
}

fn bulk_action(content: &str, needle: &str) -> Option<CodeAction> {
    actions_at(content, needle)
        .into_iter()
        .find(|action| action.title == BULK_TITLE)
}

fn new_texts(action: &CodeAction) -> Vec<&str> {
    action
        .edit
        .as_ref()
        .unwrap()
        .changes
        .as_ref()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .iter()
        .map(|edit| edit.new_text.as_str())
        .collect()
}

#[test]
fn imports_relative_class_and_replaces_all_usages() {
    let src = "<?php\nnamespace App;\n\nnew Node\\Expr\\Call();\nNode\\Expr\\Call::make();\n";
    let action = action(src, "Node\\Expr\\Call");
    let texts = new_texts(&action);
    assert!(texts.contains(&"\nuse App\\Node\\Expr\\Call;\n"));
    assert_eq!(texts.iter().filter(|text| **text == "Call").count(), 2);
}

#[test]
fn imports_absolute_function() {
    let src = "<?php\nnamespace App;\n\n\\Vendor\\Tools\\run();\n";
    let action = action(src, "Vendor\\Tools\\run");
    let texts = new_texts(&action);
    assert!(texts.contains(&"\nuse function Vendor\\Tools\\run;\n"));
    assert!(texts.contains(&"run"));
}

#[test]
fn imports_absolute_constant() {
    let src = "<?php\nnamespace App;\n\n$value = \\Vendor\\Config\\ENABLED;\n";
    let action = action(src, "Vendor\\Config\\ENABLED");
    let texts = new_texts(&action);
    assert!(texts.contains(&"\nuse const Vendor\\Config\\ENABLED;\n"));
    assert!(texts.contains(&"ENABLED"));
}

#[test]
fn aliases_conflicting_class_import() {
    let src = "<?php\nnamespace App;\n\nuse Other\\Call;\n\nnew \\Node\\Expr\\Call();\n";
    let action = action(src, "Node\\Expr\\Call");
    let texts = new_texts(&action);
    assert!(texts.contains(&"use Node\\Expr\\Call as ExprCall;\n"));
    assert!(texts.contains(&"ExprCall"));
}

#[test]
fn bulk_action_imports_every_qualified_symbol() {
    let src = "<?php\nnamespace App;\n\nuse Existing\\Thing;\n\nnew \\Vendor\\Alpha();\n\\Vendor\\Beta::make();\n\\Vendor\\Tools\\run();\n$v = \\Vendor\\Config\\ENABLED;\n";
    let action = bulk_action(src, "Vendor\\Alpha").expect("expected bulk action");
    let texts = new_texts(&action);
    assert!(texts.contains(&"use Vendor\\Alpha;\n"));
    assert!(texts.contains(&"use Vendor\\Beta;\n"));
    assert!(texts.contains(&"\nuse const Vendor\\Config\\ENABLED;\n"));
    assert!(texts.contains(&"\nuse function Vendor\\Tools\\run;\n"));
    assert!(texts.contains(&"Alpha"));
    assert!(texts.contains(&"Beta"));
    assert!(texts.contains(&"run"));
    assert!(texts.contains(&"ENABLED"));
}

#[test]
fn bulk_action_is_not_offered_for_a_lone_symbol() {
    let src = "<?php\nnamespace App;\n\nnew \\Vendor\\Alpha();\n\\Vendor\\Alpha::make();\n";
    assert!(bulk_action(src, "Vendor\\Alpha").is_none());
}

#[test]
fn bulk_action_aliases_short_name_collisions_within_the_batch() {
    let src = "<?php\nnamespace App;\n\nnew \\One\\Thing();\nnew \\Two\\Thing();\n";
    let action = bulk_action(src, "One\\Thing").expect("expected bulk action");
    let texts = new_texts(&action);
    assert!(texts.contains(&"\nuse One\\Thing;\n"));
    assert!(texts.contains(&"use Two\\Thing as TwoThing;\n"));
    assert!(texts.contains(&"Thing"));
    assert!(texts.contains(&"TwoThing"));
}

#[test]
fn bulk_action_separates_the_use_block_from_the_namespace_once() {
    let src = "<?php\nnamespace App;\n\nnew \\Vendor\\Alpha();\nnew \\Vendor\\Beta();\n";
    let action = bulk_action(src, "Vendor\\Alpha").expect("expected bulk action");
    let texts = new_texts(&action);
    assert_eq!(
        texts
            .iter()
            .filter(|text| text.starts_with("\nuse "))
            .count(),
        1
    );
    assert!(texts.contains(&"use Vendor\\Beta;\n"));
}

#[test]
fn bulk_action_skips_other_namespace_blocks() {
    let src = "<?php\nnamespace App {\n    new \\Vendor\\Alpha();\n    new \\Vendor\\Beta();\n}\nnamespace Other {\n    new \\Vendor\\Gamma();\n}\n";
    let action = bulk_action(src, "Vendor\\Alpha").expect("expected bulk action");
    let texts = new_texts(&action);
    assert!(texts.contains(&"Alpha"));
    assert!(texts.contains(&"Beta"));
    assert!(!texts.iter().any(|text| text.contains("Gamma")));
}
