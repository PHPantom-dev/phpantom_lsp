//! Integration tests for the "Sort use statements" code action.
//!
//! These exercise the full pipeline: request code actions at a cursor
//! position on a `use` line or for the whole document (as organize-on-save
//! does), resolve the deferred edit, and verify the resulting source.

use crate::common::{
    apply_edits, create_test_backend, extract_edits, find_action, get_code_actions_in_range,
    get_code_actions_on_line, resolve_action,
};
use tower_lsp::lsp_types::*;

fn whole_document(content: &str) -> Range {
    Range::new(
        Position::new(0, 0),
        Position::new(content.lines().count() as u32, 0),
    )
}

#[test]
fn offers_sort_action_when_block_is_unsorted() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nuse Zebra\\Foo;\nuse Aardvark\\Bar;\n\nclass Test {}\n";
    backend.update_ast(uri, content);

    let actions = get_code_actions_on_line(&backend, uri, content, 1);
    let action = find_action(&actions, "Sort use statements").expect("should offer sort action");

    assert_eq!(action.kind, Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS));
    assert!(action.edit.is_none(), "the edit is computed on resolve");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);
    assert_eq!(
        result,
        "<?php\nuse Aardvark\\Bar;\nuse Zebra\\Foo;\n\nclass Test {}\n"
    );
}

#[test]
fn no_action_when_already_sorted() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nuse Aardvark\\Bar;\nuse Zebra\\Foo;\n\nclass Test {}\n";
    backend.update_ast(uri, content);

    let actions = get_code_actions_on_line(&backend, uri, content, 1);
    assert!(
        find_action(&actions, "Sort use statements").is_none(),
        "should not offer sort action when the block is already sorted"
    );
}

#[test]
fn no_action_when_cursor_not_on_use_line() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nuse Zebra\\Foo;\nuse Aardvark\\Bar;\n\nclass Test {}\n";
    backend.update_ast(uri, content);

    let actions = get_code_actions_on_line(&backend, uri, content, 4);
    assert!(
        find_action(&actions, "Sort use statements").is_none(),
        "should not offer sort action when cursor is away from the use block"
    );
}

#[test]
fn no_action_for_single_import() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nuse Foo\\Bar;\n\nclass Test {}\n";
    backend.update_ast(uri, content);

    let actions = get_code_actions_on_line(&backend, uri, content, 1);
    assert!(find_action(&actions, "Sort use statements").is_none());
}

#[test]
fn offered_for_a_whole_document_request() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nuse Zebra\\Foo;\nuse Aardvark\\Bar;\n\nclass Test {}\n";
    backend.update_ast(uri, content);

    let actions = get_code_actions_in_range(&backend, uri, content, whole_document(content));
    assert!(find_action(&actions, "Sort use statements").is_some());
}

/// Organize-on-save applies "Remove all unused imports" and then "Sort use
/// statements", resolving each against the text the one before left.
#[test]
fn sorts_what_remove_all_unused_imports_left() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nuse Zebra\\Foo;\nuse Middle\\Unused;\nuse Aardvark\\Bar;\n\nclass Test extends Foo implements Bar {}\n";
    backend.update_ast(uri, content);

    let actions = get_code_actions_in_range(&backend, uri, content, whole_document(content));
    let remove = find_action(&actions, "Remove all unused imports").expect("should offer removal");
    let sort = find_action(&actions, "Sort use statements").expect("should offer sort");

    let resolved = resolve_action(&backend, uri, content, remove);
    let pruned = apply_edits(content, &extract_edits(&resolved));
    backend.update_ast(uri, &pruned);
    let resolved = resolve_action(&backend, uri, &pruned, sort);
    let sorted = apply_edits(&pruned, &extract_edits(&resolved));

    assert_eq!(
        sorted,
        "<?php\nuse Aardvark\\Bar;\nuse Zebra\\Foo;\n\nclass Test extends Foo implements Bar {}\n"
    );
}

#[test]
fn keeps_crlf_line_endings() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\r\nuse Zebra\\Foo;\r\nuse Aardvark\\Bar;\r\n\r\nclass Test {}\r\n";
    backend.update_ast(uri, content);

    let actions = get_code_actions_on_line(&backend, uri, content, 1);
    let action = find_action(&actions, "Sort use statements").expect("should offer sort action");
    let resolved = resolve_action(&backend, uri, content, action);
    assert_eq!(
        apply_edits(content, &extract_edits(&resolved)),
        "<?php\r\nuse Aardvark\\Bar;\r\nuse Zebra\\Foo;\r\n\r\nclass Test {}\r\n"
    );
}
