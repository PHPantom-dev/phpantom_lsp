//! Integration tests for the "Remove unused import" code actions.
//!
//! These exercise the full pipeline: parse a file, request code actions on
//! an import line, and verify which removal actions are offered and what
//! the bulk action's edit does to the file once resolved.

use crate::common::{
    apply_edits, create_test_backend, extract_edits, find_action, get_code_actions_at,
    resolve_action,
};
use phpantom_lsp::Backend;
use tower_lsp::lsp_types::*;

const URI: &str = "file:///test.php";
const SINGLE: &str = "Remove unused import";
const BULK: &str = "Remove all unused imports";

/// Parse `content` and request code actions at `(line, character)`.
fn actions_at(
    backend: &Backend,
    content: &str,
    line: u32,
    character: u32,
) -> Vec<CodeActionOrCommand> {
    backend.update_ast(URI, content);
    get_code_actions_at(backend, URI, content, line, character)
}

/// Resolve the bulk action offered at `(line, character)` and apply its edits.
fn apply_bulk_remove(content: &str, line: u32, character: u32) -> String {
    let backend = create_test_backend();
    let actions = actions_at(&backend, content, line, character);
    let bulk = find_action(&actions, BULK).expect("should offer bulk remove");
    let resolved = resolve_action(&backend, URI, content, bulk);
    apply_edits(content, &extract_edits(&resolved))
}

#[test]
fn remove_action_offered_for_unused_import() {
    let backend = create_test_backend();
    let content = "<?php\nuse Foo\\Bar;\nuse Baz\\Qux;\n\nclass Test extends Qux {}\n";
    let actions = actions_at(&backend, content, 1, 4);
    assert!(
        find_action(&actions, SINGLE).is_some(),
        "should offer 'Remove unused import' action"
    );
}

#[test]
fn no_remove_action_for_used_import() {
    let backend = create_test_backend();
    let content = "<?php\nuse Foo\\Bar;\n\nclass Test extends Bar {}\n";
    let actions = actions_at(&backend, content, 1, 4);
    assert!(
        find_action(&actions, SINGLE).is_none(),
        "should NOT offer remove action for used import"
    );
}

#[test]
fn bulk_remove_offered_when_multiple_unused() {
    let backend = create_test_backend();
    let content = "<?php\nuse Foo\\Bar;\nuse Baz\\Qux;\n";
    let actions = actions_at(&backend, content, 1, 4);
    assert!(
        find_action(&actions, BULK).is_some(),
        "should offer 'Remove all unused imports' when multiple unused"
    );
}

#[test]
fn bulk_remove_offered_for_single_unused_import() {
    let backend = create_test_backend();
    let content = "<?php\nuse Foo\\Bar;\n\nclass Test {}\n";
    let actions = actions_at(&backend, content, 1, 4);
    assert!(
        find_action(&actions, BULK).is_some(),
        "should offer 'Remove all unused imports' even for a single unused import"
    );
}

#[test]
fn bulk_remove_not_offered_when_cursor_outside_import_block() {
    let backend = create_test_backend();
    let content = "<?php\nuse Foo\\Bar;\n\nclass Test {}\n";
    // Cursor on the `class Test` line, not on a `use` line.
    let actions = actions_at(&backend, content, 3, 0);
    assert!(
        find_action(&actions, BULK).is_none(),
        "should NOT offer bulk remove when cursor is not on a use line"
    );
    assert!(
        find_action(&actions, SINGLE).is_none(),
        "should NOT offer single remove when cursor is not on the unused import"
    );
}

#[test]
fn bulk_remove_offered_when_cursor_on_used_import() {
    let backend = create_test_backend();
    let content = "<?php\nuse Foo\\Bar;\nuse Baz\\Qux;\n\nclass Test extends Qux {}\n";
    // Cursor on the used import (Baz\Qux), not the unused one.
    let actions = actions_at(&backend, content, 2, 4);
    assert!(
        find_action(&actions, BULK).is_some(),
        "should offer bulk remove when cursor is on any use line"
    );
}

#[test]
fn bulk_remove_deletes_both_widely_separated_unused_imports() {
    let backend = create_test_backend();
    let content = "\
<?php

use App\\UnusedA;
use App\\UsedB;

class Foo extends UsedB
{
    public function bar(): void
    {
        // some code
    }
}

use App\\UnusedC;
";
    let actions = actions_at(&backend, content, 2, 4);
    let bulk = find_action(&actions, BULK).expect("should offer bulk remove");

    // Phase 1: no edit, has data.
    assert!(bulk.edit.is_none(), "Phase 1 should not have an edit");
    assert!(bulk.data.is_some(), "Phase 1 should have data");

    // Phase 2: resolve.
    let resolved = resolve_action(&backend, URI, content, bulk);
    let edits = extract_edits(&resolved);
    assert!(
        edits.len() >= 2,
        "should delete both unused imports, got {} edits",
        edits.len()
    );
}

#[test]
fn bulk_remove_in_braced_namespace_with_class_bodies_between() {
    let content = "\
<?php
use App\\UnusedAlpha;
use App\\UsedBravo;
use App\\UnusedCharlie;

class Demo extends UsedBravo
{
    public function method(): void
    {
    }
}
";
    let result = apply_bulk_remove(content, 1, 4);
    assert!(
        !result.contains("UnusedAlpha"),
        "UnusedAlpha should be removed:\n{result}"
    );
    assert!(
        !result.contains("UnusedCharlie"),
        "UnusedCharlie should be removed:\n{result}"
    );
    assert!(
        result.contains("UsedBravo"),
        "UsedBravo should be kept:\n{result}"
    );
}

#[test]
fn bulk_remove_consumes_separator_when_import_block_becomes_empty() {
    let content = "<?php\nuse Foo\\Bar;\nuse Baz\\Qux;\n\nclass Test {}\n";
    assert_eq!(apply_bulk_remove(content, 1, 4), "<?php\nclass Test {}\n");
}

#[test]
fn bulk_remove_collapses_gap_when_unused_import_is_between_used_ones() {
    let content = "<?php\nuse Foo\\Bar;\nuse Baz\\Qux;\n\nuse Quux\\Quuz;\n\nclass Test extends Bar\n{\n    public function make(): Quuz\n    {\n        return new Quuz();\n    }\n}\n";
    assert_eq!(
        apply_bulk_remove(content, 2, 4),
        "<?php\nuse Foo\\Bar;\nuse Quux\\Quuz;\n\nclass Test extends Bar\n{\n    public function make(): Quuz\n    {\n        return new Quuz();\n    }\n}\n"
    );
}
