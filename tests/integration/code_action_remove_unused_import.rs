//! Integration tests for the "Remove unused import" code actions.
//!
//! These exercise the full pipeline: parse a file, request code actions on
//! an import line, and verify which removal actions are offered and what
//! the bulk action's edit does to the file once resolved.

use crate::common::{
    apply_edits, create_test_backend, extract_edits, find_action, get_code_actions_at,
    get_code_actions_in_range, resolve_action,
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
fn bulk_remove_offered_for_a_whole_document_request() {
    let backend = create_test_backend();
    let content = "<?php\nuse Foo\\Bar;\n\nclass Test {}\n";
    backend.update_ast(URI, content);
    // `editor.codeActionsOnSave` sends the whole document as the range.
    let range = Range::new(Position::new(0, 0), Position::new(4, 0));
    let actions = get_code_actions_in_range(&backend, URI, content, range);
    assert!(
        find_action(&actions, BULK).is_some(),
        "should offer bulk remove for a range spanning the imports"
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

/// Each `namespace` block's imports are checked against the references in
/// that block, so an import used only by another block is still unused.
#[test]
fn an_import_used_only_in_another_namespace_block_is_unused() {
    let backend = create_test_backend();
    let content = concat!(
        "<?php\n",
        "namespace A {\n",
        "    use X\\Foo;\n",
        "}\n",
        "namespace B {\n",
        "    use X\\Foo;\n",
        "    function b(Foo $f) {}\n",
        "}\n",
    );
    backend.update_ast(URI, content);

    let mut diags = Vec::new();
    backend.collect_unused_import_diagnostics(URI, content, &mut diags);
    let lines: Vec<u32> = diags.iter().map(|d| d.range.start.line).collect();
    assert_eq!(lines, [2], "only block A's import is unused: {diags:?}");
}

/// A call whose `)` has not been typed yet still uses the imports it names,
/// and so does the rest of the class it is in.
#[test]
fn an_import_used_by_a_call_missing_its_closing_paren_is_used() {
    let content = r#"<?php
namespace App;

use Lib\Gadget;
use Lib\Unused;
use Lib\Widget;

class Shop {
    public function make() {
        $w = new Widget(
    }

    public function gadget(): Gadget {}
}
"#;
    assert_eq!(unused_imports(content), ["Unused import 'Lib\\Unused'"]);
}

/// A statement the end of the file cuts off, still missing its `)` and
/// `;`, uses the imports it names.
#[test]
fn an_import_used_by_a_call_at_the_end_of_the_file_missing_its_closing_paren_is_used() {
    let content = r#"<?php
namespace App;

use App\Http\Controllers\HomeController;
use App\Http\Controllers\Unused;

Route::get('/', [HomeController::class, 'index']
"#;
    assert_eq!(
        unused_imports(content),
        ["Unused import 'App\\Http\\Controllers\\Unused'"]
    );
}

/// A call cut off right after its `(`, with or without a newline after
/// it, uses the imports it names.
#[test]
fn an_import_used_by_an_empty_call_at_the_end_of_the_file_is_used() {
    let head =
        "<?php\nnamespace App;\n\nuse App\\Services\\Mailer;\nuse App\\Services\\Unused;\n\n";
    for last in ["$mailer = new Mailer(\n", "$mailer = new Mailer("] {
        assert_eq!(
            unused_imports(&format!("{head}{last}")),
            ["Unused import 'App\\Services\\Unused'"],
            "{last:?}"
        );
    }
}

/// A `::` access cut off before its member, on its own or inside a call,
/// uses the import it names.
#[test]
fn an_import_used_by_a_static_access_at_the_end_of_the_file_is_used() {
    let head =
        "<?php\nnamespace App;\n\nuse App\\Services\\Mailer;\nuse App\\Services\\Unused;\n\n";
    for last in [
        "$mailer = Mailer::\n",
        "$mailer = Mailer::",
        "foo(Mailer::\n",
        "foo(Mailer::",
    ] {
        assert_eq!(
            unused_imports(&format!("{head}{last}")),
            ["Unused import 'App\\Services\\Unused'"],
            "{last:?}"
        );
    }
}

/// The `Unused import '…'` messages for `content`.
fn unused_imports(content: &str) -> Vec<String> {
    let backend = create_test_backend();
    let mut messages: Vec<String> = crate::common::collect_diagnostics_with(
        &backend,
        content,
        Backend::collect_unused_import_diagnostics,
    )
    .into_iter()
    .map(|d| d.message)
    .collect();
    messages.sort();
    messages
}

#[test]
fn function_and_constant_imports_are_checked_for_use() {
    let content = r#"<?php
namespace App;

use function Lib\used_fn;
use function Lib\unused_fn;
use const Lib\USED_CONST;
use const Lib\UNUSED_CONST;

used_fn(USED_CONST);
"#;
    assert_eq!(
        unused_imports(content),
        [
            "Unused import 'Lib\\UNUSED_CONST'",
            "Unused import 'Lib\\unused_fn'",
        ]
    );
}

/// A function import is used by a call, not by a class or constant of the
/// same name, and the other way round.
#[test]
fn an_import_is_used_only_by_its_own_kind() {
    let content = r#"<?php
namespace App;

use Lib\Table;
use function Lib\table;
use const Lib\TABLE;

table();
"#;
    assert_eq!(
        unused_imports(content),
        ["Unused import 'Lib\\TABLE'", "Unused import 'Lib\\Table'"]
    );
}

/// Each `namespace` block's function imports are checked against the calls
/// in that block alone.
#[test]
fn function_imports_are_checked_per_namespace_block() {
    let content = r#"<?php
namespace A {
    use function Lib\helper;
    helper();
}

namespace B {
    use function Lib\helper;
}
"#;
    let backend = create_test_backend();
    let diagnostics = crate::common::collect_diagnostics_with(
        &backend,
        content,
        Backend::collect_unused_import_diagnostics,
    );
    assert_eq!(diagnostics.len(), 1, "got {diagnostics:?}");
    assert_eq!(diagnostics[0].range.start.line, 7);
}

/// A `case` label is a reference like any other expression: a class
/// constant or an imported constant matched there keeps its import.
#[test]
fn an_import_used_only_in_a_switch_case_is_kept() {
    let backend = create_test_backend();
    let content = "<?php\nuse App\\Status;\nuse const E_STRICT;\n\nfunction f(int $s): void {\n    switch ($s) {\n        case Status::Open:\n        case E_STRICT:\n            break;\n    }\n}\n";
    for line in [1, 2] {
        let actions = actions_at(&backend, content, line, 4);
        assert!(
            find_action(&actions, SINGLE).is_none(),
            "the import on line {line} is used by a case label"
        );
    }
}

/// The name written in a string, in docblock prose, or as a property name
/// is not a reference to the imported class.
#[test]
fn a_name_that_only_appears_as_text_does_not_keep_its_import() {
    let backend = create_test_backend();
    let content = "<?php\nuse App\\Address;\n\n/** @param object $o An Address or similar */\nfunction f(object $o): string {\n    return 'Address' . $o->Address;\n}\n";
    let actions = actions_at(&backend, content, 1, 4);
    assert!(find_action(&actions, SINGLE).is_some());
}
