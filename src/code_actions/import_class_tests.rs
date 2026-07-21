use std::sync::Arc;

use super::*;

// ── compute_use_line_ranges ─────────────────────────────────────────

#[test]
fn use_line_ranges_lf() {
    let content = "<?php\nuse App\\Foo;\nnew Foo();\n";
    let ranges = compute_use_line_ranges(content);
    assert_eq!(ranges.len(), 1);
    let (start, end) = ranges[0];
    // The `use App\Foo;` line starts right after "<?php\n" (6 bytes).
    assert_eq!(&content[start as usize..end as usize], "use App\\Foo;");
}

#[test]
fn use_line_ranges_crlf() {
    let content = "<?php\r\nuse App\\Foo;\r\nnew Foo();\r\n";
    let ranges = compute_use_line_ranges(content);
    assert_eq!(ranges.len(), 1);
    let (start, end) = ranges[0];
    // The slice must still land exactly on the use statement even
    // though each preceding line carries a two-byte `\r\n` terminator.
    assert_eq!(&content[start as usize..end as usize], "use App\\Foo;");
}

// ── find_all_unresolved_class_names ─────────────────────────────────

/// Helper: extract just the names from the `(name, context)` tuples.
fn unresolved_names(list: &[(String, ClassRefContext)]) -> Vec<&str> {
    list.iter().map(|(n, _)| n.as_str()).collect()
}

#[test]
fn finds_multiple_unresolved_names() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "<?php\nnamespace App;\n\nnew Request();\nnew Collection();\n";
    backend.update_ast(uri, content);

    let unresolved = backend.find_all_unresolved_class_names(uri, content);
    let names = unresolved_names(&unresolved);
    assert!(
        names.contains(&"Collection"),
        "expected Collection in {:?}",
        names
    );
    assert!(
        names.contains(&"Request"),
        "expected Request in {:?}",
        names
    );
}

#[test]
fn skips_already_imported_names() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "<?php\nnamespace App;\n\nuse Illuminate\\Http\\Request;\n\nnew Request();\nnew Collection();\n";
    backend.update_ast(uri, content);

    let unresolved = backend.find_all_unresolved_class_names(uri, content);
    let names = unresolved_names(&unresolved);
    assert!(
        !names.contains(&"Request"),
        "Request should not be unresolved: {:?}",
        names
    );
    assert!(
        names.contains(&"Collection"),
        "expected Collection in {:?}",
        names
    );
}

// ── collect_import_all_classes_action ────────────────────────────────

#[test]
fn bulk_import_offered_when_multiple_unresolved() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "<?php\nnamespace App;\n\nnew Request();\nnew Collection();\n";
    backend.update_ast(uri, content);

    // Add candidates.
    {
        let mut cmap = backend.fqn_uri_index.write();
        cmap.insert(
            "Illuminate\\Http\\Request".to_string(),
            "file:///vendor/Request.php".to_string(),
        );
        cmap.insert(
            "Illuminate\\Support\\Collection".to_string(),
            "file:///vendor/Collection.php".to_string(),
        );
    }

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(3, 4),
            end: Position::new(3, 11),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let mut actions = Vec::new();
    backend.collect_import_all_classes_action(uri, content, &params, &mut actions);

    assert!(
        actions.iter().any(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                ca.title == "Import all missing classes"
            } else {
                false
            }
        }),
        "expected bulk import action, got: {:?}",
        actions
            .iter()
            .map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) => ca.title.clone(),
                CodeActionOrCommand::Command(c) => c.title.clone(),
            })
            .collect::<Vec<_>>()
    );
}

#[test]
fn bulk_import_not_offered_for_single_unresolved() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "<?php\nnamespace App;\n\nnew Request();\n";
    backend.update_ast(uri, content);

    {
        let mut cmap = backend.fqn_uri_index.write();
        cmap.insert(
            "Illuminate\\Http\\Request".to_string(),
            "file:///vendor/Request.php".to_string(),
        );
    }

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(3, 4),
            end: Position::new(3, 11),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let mut actions = Vec::new();
    backend.collect_import_all_classes_action(uri, content, &params, &mut actions);
    assert!(
        actions.is_empty(),
        "should not offer bulk import for single unresolved class"
    );
}

#[test]
fn bulk_import_not_offered_when_cursor_elsewhere() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    // Two unresolved names exist, but cursor is on line 2 (the
    // namespace declaration), not on either unresolved reference.
    let content = "<?php\nnamespace App;\n\nnew Request();\nnew Collection();\n";
    backend.update_ast(uri, content);

    {
        let mut cmap = backend.fqn_uri_index.write();
        cmap.insert(
            "Illuminate\\Http\\Request".to_string(),
            "file:///vendor/Request.php".to_string(),
        );
        cmap.insert(
            "Illuminate\\Support\\Collection".to_string(),
            "file:///vendor/Collection.php".to_string(),
        );
    }

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(1, 0),
            end: Position::new(1, 0),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let mut actions = Vec::new();
    backend.collect_import_all_classes_action(uri, content, &params, &mut actions);
    assert!(
        actions.is_empty(),
        "should not offer bulk import when cursor is not on an unresolved class"
    );
}

#[test]
fn resolve_import_all_inserts_use_statements() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "<?php\nnamespace App;\n\nnew Request();\nnew Collection();\n";
    backend.update_ast(uri, content);

    {
        let mut cmap = backend.fqn_uri_index.write();
        cmap.insert(
            "Illuminate\\Http\\Request".to_string(),
            "file:///vendor/Request.php".to_string(),
        );
        cmap.insert(
            "Illuminate\\Support\\Collection".to_string(),
            "file:///vendor/Collection.php".to_string(),
        );
    }

    // Store the file content so resolve can read it.
    {
        let mut files = backend.open_files.write();
        files.insert(uri.to_string(), Arc::new(content.to_string()));
    }

    let data = super::super::CodeActionData {
        action_kind: "source.importAllClasses".to_string(),
        uri: uri.to_string(),
        range: Range::default(),
        extra: serde_json::json!({}),
    };

    let edit = backend.resolve_import_all_classes(&data, content);
    assert!(edit.is_some(), "expected a WorkspaceEdit");

    let ws_edit = edit.unwrap();
    let changes = ws_edit.changes.unwrap();
    let edits = changes.get(&uri.parse::<Url>().unwrap()).unwrap();

    // Should have two use-statement insertions.
    assert_eq!(edits.len(), 2, "expected 2 edits, got {:?}", edits);

    let combined: String = edits.iter().map(|e| e.new_text.as_str()).collect();
    assert!(
        combined.contains("use Illuminate\\Support\\Collection;"),
        "expected Collection import in {:?}",
        combined
    );
    assert!(
        combined.contains("use Illuminate\\Http\\Request;"),
        "expected Request import in {:?}",
        combined
    );
}

#[test]
fn resolve_import_all_adds_blank_line_after_namespace() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    // No blank line after namespace, no existing use statements.
    let content = "<?php\nnamespace App\\Http\\Data\\Payment;\nfinal class Foo extends Data\n{\n    public function __construct(\n        #[MapInputName('shop_orderid')]\n        public readonly string $shopOrderId,\n        #[WithCast(DecimalCast::class)]\n        public readonly Decimal $amount,\n    ) {}\n}\n";
    backend.update_ast(uri, content);

    {
        let mut cmap = backend.fqn_uri_index.write();
        cmap.insert(
            "Spatie\\LaravelData\\Attributes\\MapInputName".to_string(),
            "file:///vendor/MapInputName.php".to_string(),
        );
        cmap.insert(
            "Spatie\\LaravelData\\Attributes\\WithCast".to_string(),
            "file:///vendor/WithCast.php".to_string(),
        );
    }

    {
        let mut files = backend.open_files.write();
        files.insert(uri.to_string(), Arc::new(content.to_string()));
    }

    let data = super::super::CodeActionData {
        action_kind: "source.importAllClasses".to_string(),
        uri: uri.to_string(),
        range: Range::default(),
        extra: serde_json::json!({}),
    };

    let edit = backend.resolve_import_all_classes(&data, content);
    assert!(edit.is_some(), "expected a WorkspaceEdit");

    let ws_edit = edit.unwrap();
    let changes = ws_edit.changes.unwrap();
    let edits = changes.get(&uri.parse::<Url>().unwrap()).unwrap();

    // The first edit should have a \n prefix (blank line after namespace).
    // Subsequent edits should NOT have it.
    let first = &edits[0];
    assert!(
        first.new_text.starts_with('\n'),
        "first edit should start with blank line separator, got: {:?}",
        first.new_text
    );

    for te in &edits[1..] {
        assert!(
            !te.new_text.starts_with('\n'),
            "subsequent edits should NOT have blank line prefix, got: {:?}",
            te.new_text
        );
    }

    let combined: String = edits.iter().map(|e| e.new_text.as_str()).collect();
    assert!(
        combined.contains("use Spatie\\LaravelData\\Attributes\\MapInputName;"),
        "expected MapInputName import in {:?}",
        combined
    );
    assert!(
        combined.contains("use Spatie\\LaravelData\\Attributes\\WithCast;"),
        "expected WithCast import in {:?}",
        combined
    );
}

#[test]
fn resolve_import_all_interleaves_with_existing_imports() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "\
<?php
namespace App\\Http\\Data;

use Acme\\Core\\Data\\DecimalCast;
use Acme\\Decimal\\Decimal;
use Spatie\\LaravelData\\Data;

final class Foo extends Data
{
    public function __construct(
        #[MapInputName('shop_orderid')]
        public readonly string $shopOrderId,
        #[WithCast(DecimalCast::class)]
        public readonly Decimal $amount,
    ) {}
}
";
    backend.update_ast(uri, content);

    // Add candidates for the two unresolved attribute names.
    {
        let mut cmap = backend.fqn_uri_index.write();
        cmap.insert(
            "Spatie\\LaravelData\\Attributes\\MapInputName".to_string(),
            "file:///vendor/MapInputName.php".to_string(),
        );
        cmap.insert(
            "Spatie\\LaravelData\\Attributes\\WithCast".to_string(),
            "file:///vendor/WithCast.php".to_string(),
        );
    }

    {
        let mut files = backend.open_files.write();
        files.insert(uri.to_string(), Arc::new(content.to_string()));
    }

    let data = super::super::CodeActionData {
        action_kind: "source.importAllClasses".to_string(),
        uri: uri.to_string(),
        range: Range::default(),
        extra: serde_json::json!({}),
    };

    let edit = backend.resolve_import_all_classes(&data, content);
    assert!(edit.is_some(), "expected a WorkspaceEdit");

    let ws_edit = edit.unwrap();
    let changes = ws_edit.changes.unwrap();
    let edits = changes.get(&uri.parse::<Url>().unwrap()).unwrap();

    // Both new imports should be inserted into the existing use
    // block, not scattered through the class body.  All edits
    // must target lines within the use block region (lines 3-5
    // in the original, so insertions at lines 3-6).
    for te in edits {
        assert!(
            te.range.start.line <= 6,
            "edit at line {} is outside the use block region: {:?}",
            te.range.start.line,
            te
        );
    }

    let combined: String = edits.iter().map(|e| e.new_text.as_str()).collect();
    assert!(
        combined.contains("use Spatie\\LaravelData\\Attributes\\MapInputName;"),
        "expected MapInputName import in {:?}",
        combined
    );
    assert!(
        combined.contains("use Spatie\\LaravelData\\Attributes\\WithCast;"),
        "expected WithCast import in {:?}",
        combined
    );
}

#[test]
fn resolve_import_all_skips_conflicting_short_names() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    // Two references to "Exception" — but we have two candidate FQNs
    // with the same short name.  Only the first should be imported.
    let content = "<?php\nnamespace App;\n\nnew Exception();\nnew Request();\n";
    backend.update_ast(uri, content);

    {
        let mut cmap = backend.fqn_uri_index.write();
        // Two different classes named "Exception"
        cmap.insert(
            "Exception".to_string(),
            "file:///vendor/Exception.php".to_string(),
        );
        cmap.insert(
            "Illuminate\\Http\\Request".to_string(),
            "file:///vendor/Request.php".to_string(),
        );
    }

    {
        let mut files = backend.open_files.write();
        files.insert(uri.to_string(), Arc::new(content.to_string()));
    }

    let data = super::super::CodeActionData {
        action_kind: "source.importAllClasses".to_string(),
        uri: uri.to_string(),
        range: Range::default(),
        extra: serde_json::json!({}),
    };

    let edit = backend.resolve_import_all_classes(&data, content);
    // Should still produce edits (at least the Request one).
    assert!(edit.is_some(), "expected a WorkspaceEdit");

    let ws_edit = edit.unwrap();
    let changes = ws_edit.changes.unwrap();
    let edits = changes.get(&uri.parse::<Url>().unwrap()).unwrap();

    let combined: String = edits.iter().map(|e| e.new_text.as_str()).collect();
    assert!(
        combined.contains("use Illuminate\\Http\\Request;"),
        "expected Request import in {:?}",
        combined
    );
}

#[test]
fn resolve_import_all_skips_ambiguous_names() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    // Both names have multiple candidates — neither should be
    // auto-imported because the user needs to choose.
    let content = "<?php\nnamespace App;\n\nnew Request();\nnew Collection();\n";
    backend.update_ast(uri, content);

    {
        let mut cmap = backend.fqn_uri_index.write();
        cmap.insert(
            "Illuminate\\Http\\Request".to_string(),
            "file:///vendor/Request.php".to_string(),
        );
        cmap.insert(
            "Symfony\\Component\\HttpFoundation\\Request".to_string(),
            "file:///vendor/SymfonyRequest.php".to_string(),
        );
        cmap.insert(
            "Illuminate\\Support\\Collection".to_string(),
            "file:///vendor/Collection.php".to_string(),
        );
        cmap.insert(
            "Doctrine\\Common\\Collections\\Collection".to_string(),
            "file:///vendor/DoctrineCollection.php".to_string(),
        );
    }

    {
        let mut files = backend.open_files.write();
        files.insert(uri.to_string(), Arc::new(content.to_string()));
    }

    let data = super::super::CodeActionData {
        action_kind: "source.importAllClasses".to_string(),
        uri: uri.to_string(),
        range: Range::default(),
        extra: serde_json::json!({}),
    };

    let edit = backend.resolve_import_all_classes(&data, content);
    // Both names are ambiguous — nothing to import.
    assert!(
        edit.is_none(),
        "should not produce edits when all names are ambiguous"
    );
}

#[test]
fn resolve_import_all_imports_unambiguous_skips_ambiguous() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    // Request has two candidates (ambiguous), Collection has one
    // (unambiguous).  Only Collection should be imported.
    let content = "<?php\nnamespace App;\n\nnew Request();\nnew Collection();\n";
    backend.update_ast(uri, content);

    {
        let mut cmap = backend.fqn_uri_index.write();
        cmap.insert(
            "Illuminate\\Http\\Request".to_string(),
            "file:///vendor/Request.php".to_string(),
        );
        cmap.insert(
            "Symfony\\Component\\HttpFoundation\\Request".to_string(),
            "file:///vendor/SymfonyRequest.php".to_string(),
        );
        cmap.insert(
            "Illuminate\\Support\\Collection".to_string(),
            "file:///vendor/Collection.php".to_string(),
        );
    }

    {
        let mut files = backend.open_files.write();
        files.insert(uri.to_string(), Arc::new(content.to_string()));
    }

    let data = super::super::CodeActionData {
        action_kind: "source.importAllClasses".to_string(),
        uri: uri.to_string(),
        range: Range::default(),
        extra: serde_json::json!({}),
    };

    let edit = backend.resolve_import_all_classes(&data, content);
    assert!(edit.is_some(), "expected a WorkspaceEdit");

    let ws_edit = edit.unwrap();
    let changes = ws_edit.changes.unwrap();
    let edits = changes.get(&uri.parse::<Url>().unwrap()).unwrap();

    let combined: String = edits.iter().map(|e| e.new_text.as_str()).collect();
    assert!(
        combined.contains("use Illuminate\\Support\\Collection;"),
        "expected Collection import in {:?}",
        combined
    );
    assert!(
        !combined.contains("Request"),
        "should not import ambiguous Request, got {:?}",
        combined
    );
}

// ── find_import_candidates smoke test ───────────────────────────────

#[test]
fn find_candidates_from_fqn_uri_index() {
    let backend = crate::Backend::new_test();
    // Populate class index with a known class.
    {
        let mut cmap = backend.fqn_uri_index.write();
        cmap.insert(
            "App\\Models\\User".to_string(),
            "file:///fake/path/User.php".to_string(),
        );
        cmap.insert(
            "App\\Http\\Request".to_string(),
            "file:///fake/path/Request.php".to_string(),
        );
    }

    let table = std::collections::HashMap::new();
    let candidates = backend.find_import_candidates("User", &table);
    assert!(candidates.contains(&"App\\Models\\User".to_string()));
    assert!(!candidates.contains(&"App\\Http\\Request".to_string()));
}

#[test]
fn find_candidates_case_insensitive() {
    let backend = crate::Backend::new_test();
    {
        let mut cmap = backend.fqn_uri_index.write();
        cmap.insert(
            "Vendor\\Obscure\\ZYGOMORPHIC".to_string(),
            "file:///fake/path.php".to_string(),
        );
    }

    let table = std::collections::HashMap::new();
    let candidates = backend.find_import_candidates("Zygomorphic", &table);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0], "Vendor\\Obscure\\ZYGOMORPHIC");
}

#[test]
fn find_candidates_deduplicates() {
    let backend = crate::Backend::new_test();
    // Add the same FQN to fqn_uri_index — should only appear once.
    {
        let mut idx = backend.fqn_uri_index.write();
        idx.insert("App\\Foo".to_string(), "file:///foo.php".to_string());
    }

    let table = std::collections::HashMap::new();
    let candidates = backend.find_import_candidates("Foo", &table);
    let count = candidates.iter().filter(|c| *c == "App\\Foo").count();
    assert_eq!(count, 1, "should not have duplicates");
}

// ── Integration-style test with code action collection ──────────────

#[test]
fn import_action_offered_for_unresolved_class() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "<?php\nnamespace App;\n\nnew Request();\n";

    // Parse the file so the symbol map is populated.
    backend.update_ast(uri, content);

    // Add a candidate to the class index.
    {
        let mut cmap = backend.fqn_uri_index.write();
        cmap.insert(
            "Illuminate\\Http\\Request".to_string(),
            "file:///vendor/laravel/framework/src/Illuminate/Http/Request.php".to_string(),
        );
    }

    // Build a request range covering "Request" on line 3.
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(3, 4),
            end: Position::new(3, 11),
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
    assert!(
        actions.iter().any(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                ca.title.contains("Illuminate\\Http\\Request")
            } else {
                false
            }
        }),
        "expected an import action for Illuminate\\Http\\Request, got: {:?}",
        actions
            .iter()
            .map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) => ca.title.clone(),
                CodeActionOrCommand::Command(c) => c.title.clone(),
            })
            .collect::<Vec<_>>()
    );
}

#[test]
fn no_import_action_when_already_imported() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "<?php\nnamespace App;\n\nuse Illuminate\\Http\\Request;\n\nnew Request();\n";

    backend.update_ast(uri, content);

    {
        let mut cmap = backend.fqn_uri_index.write();
        cmap.insert(
            "Illuminate\\Http\\Request".to_string(),
            "file:///vendor/laravel/framework/src/Illuminate/Http/Request.php".to_string(),
        );
    }

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(5, 4),
            end: Position::new(5, 11),
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
    // No import actions should be offered — Request is already imported.
    let import_actions: Vec<_> = actions
        .iter()
        .filter(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title.starts_with("Import"),
            _ => false,
        })
        .collect();
    assert!(
        import_actions.is_empty(),
        "should not offer import when already imported, got: {:?}",
        import_actions
    );
}

#[test]
fn no_import_action_for_fqn_reference() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "<?php\nnamespace App;\n\nnew \\Illuminate\\Http\\Request();\n";

    backend.update_ast(uri, content);

    {
        let mut cmap = backend.fqn_uri_index.write();
        cmap.insert(
            "Illuminate\\Http\\Request".to_string(),
            "file:///vendor/laravel/framework/src/Illuminate/Http/Request.php".to_string(),
        );
    }

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(3, 5),
            end: Position::new(3, 35),
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
    let import_actions: Vec<_> = actions
        .iter()
        .filter(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title.starts_with("Import"),
            _ => false,
        })
        .collect();
    assert!(
        import_actions.is_empty(),
        "should not offer import for FQN reference"
    );
}

#[test]
fn import_action_inserts_use_statement() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "<?php\nnamespace App;\n\nnew Request();\n";

    backend.update_ast(uri, content);

    {
        let mut cmap = backend.fqn_uri_index.write();
        cmap.insert(
            "Illuminate\\Http\\Request".to_string(),
            "file:///vendor/laravel/framework/src/Illuminate/Http/Request.php".to_string(),
        );
    }

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(3, 4),
            end: Position::new(3, 11),
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
    let action = actions
        .iter()
        .find_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca)
                if ca.title.contains("Illuminate\\Http\\Request") =>
            {
                Some(ca)
            }
            _ => None,
        })
        .expect("expected import action");

    // Verify the edit inserts a use statement.
    let edit = action.edit.as_ref().expect("expected workspace edit");
    let changes = edit.changes.as_ref().expect("expected changes");
    let file_edits = changes
        .get(&uri.parse::<Url>().unwrap())
        .expect("expected edits for the file");
    assert_eq!(file_edits.len(), 1);
    assert_eq!(file_edits[0].new_text, "\nuse Illuminate\\Http\\Request;\n");
}

#[test]
fn import_skips_conflict_with_existing_import() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    // Already importing a *different* Request class.
    let content = "<?php\nnamespace App;\n\nuse Symfony\\Component\\HttpFoundation\\Request;\n\nnew Request();\n";

    backend.update_ast(uri, content);

    {
        let mut cmap = backend.fqn_uri_index.write();
        cmap.insert(
            "Illuminate\\Http\\Request".to_string(),
            "file:///vendor/laravel/framework/src/Illuminate/Http/Request.php".to_string(),
        );
    }

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(5, 4),
            end: Position::new(5, 11),
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
    // Should not offer importing Illuminate\Http\Request because
    // Symfony's Request is already imported with the same short name.
    let import_actions: Vec<_> = actions
        .iter()
        .filter(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title.contains("Illuminate\\Http\\Request"),
            _ => false,
        })
        .collect();
    assert!(
        import_actions.is_empty(),
        "should not offer conflicting import"
    );
}

// ── No-namespace file tests ─────────────────────────────────────────

#[test]
fn import_action_offered_in_no_namespace_file_for_new_expression() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    // File has NO namespace declaration.
    let content = "<?php\n\nnew Request();\n";

    backend.update_ast(uri, content);

    {
        let mut cmap = backend.fqn_uri_index.write();
        cmap.insert(
            "Illuminate\\Http\\Request".to_string(),
            "file:///vendor/laravel/framework/src/Illuminate/Http/Request.php".to_string(),
        );
    }

    // Range covering "Request" on line 2.
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(2, 4),
            end: Position::new(2, 11),
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
    assert!(
        actions.iter().any(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                ca.title.contains("Illuminate\\Http\\Request")
            } else {
                false
            }
        }),
        "expected an import action for Illuminate\\Http\\Request in no-namespace file, got: {:?}",
        actions
            .iter()
            .map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) => ca.title.clone(),
                CodeActionOrCommand::Command(c) => c.title.clone(),
            })
            .collect::<Vec<_>>()
    );
}

#[test]
fn import_action_offered_in_no_namespace_file_for_static_call() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    // File has NO namespace — reproduces issue #59.
    let content = "<?php\n\nfunction () {\n    return Carbon::now();\n};\n";

    backend.update_ast(uri, content);

    {
        let mut cmap = backend.fqn_uri_index.write();
        cmap.insert(
            "Carbon\\Carbon".to_string(),
            "file:///vendor/nesbot/carbon/src/Carbon/Carbon.php".to_string(),
        );
    }

    // Range covering "Carbon" on line 3 (the class name in Carbon::now()).
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(3, 11),
            end: Position::new(3, 17),
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
    assert!(
        actions.iter().any(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                ca.title.contains("Carbon\\Carbon")
            } else {
                false
            }
        }),
        "expected an import action for Carbon\\Carbon in no-namespace file, got: {:?}",
        actions
            .iter()
            .map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) => ca.title.clone(),
                CodeActionOrCommand::Command(c) => c.title.clone(),
            })
            .collect::<Vec<_>>()
    );
}

#[test]
fn import_action_inserts_use_after_php_open_in_no_namespace_file() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "<?php\n\nnew Request();\n";

    backend.update_ast(uri, content);

    {
        let mut cmap = backend.fqn_uri_index.write();
        cmap.insert(
            "Illuminate\\Http\\Request".to_string(),
            "file:///vendor/laravel/framework/src/Illuminate/Http/Request.php".to_string(),
        );
    }

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(2, 4),
            end: Position::new(2, 11),
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
    let action = actions
        .iter()
        .find_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca)
                if ca.title.contains("Illuminate\\Http\\Request") =>
            {
                Some(ca)
            }
            _ => None,
        })
        .expect("expected import action");

    let edit = action.edit.as_ref().expect("expected workspace edit");
    let changes = edit.changes.as_ref().expect("expected changes");
    let file_edits = changes
        .get(&uri.parse::<Url>().unwrap())
        .expect("expected edits for the file");
    assert_eq!(file_edits.len(), 1);
    assert_eq!(file_edits[0].new_text, "use Illuminate\\Http\\Request;\n");
    // Should insert after `<?php` (line 1), not line 0.
    assert_eq!(file_edits[0].range.start.line, 1);
}

#[test]
fn no_import_action_for_known_global_class_in_no_namespace_file() {
    let backend = crate::Backend::new_test();
    let uri_dep = "file:///dep.php";
    let content_dep = "<?php\nclass Helper {}\n";
    backend.update_ast(uri_dep, content_dep);

    {
        let mut idx = backend.fqn_uri_index.write();
        idx.insert("Helper".to_string(), uri_dep.to_string());
    }

    let uri = "file:///test.php";
    let content = "<?php\n\nnew Helper();\n";
    backend.update_ast(uri, content);

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(2, 4),
            end: Position::new(2, 10),
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
    let import_actions: Vec<_> = actions
        .iter()
        .filter(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title.starts_with("Import"),
            _ => false,
        })
        .collect();
    assert!(
        import_actions.is_empty(),
        "should not offer import for a known global class in no-namespace file, got: {:?}",
        import_actions
            .iter()
            .map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) => ca.title.clone(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
    );
}

#[test]
fn import_action_offered_when_namespaced_class_in_uri_classes_index() {
    // Reproduces issue #59: when a namespaced class like `Carbon\Carbon`
    // is already parsed and in the uri_classes_index, `find_or_load_class("Carbon")`
    // must NOT match it — the bare name `"Carbon"` is a global-scope
    // lookup and should not resolve to `Carbon\Carbon`.
    //
    // Without the fix, `find_class_in_uri_classes_index("Carbon")` ignores the
    // namespace filter when `expected_ns` is `None`, so ANY class with
    // short name `Carbon` matches.  The import action then skips it
    // thinking "this class resolves in global scope".
    let backend = crate::Backend::new_test();

    // Parse the dependency file so Carbon\Carbon is in the uri_classes_index.
    let uri_dep = "file:///vendor/carbon.php";
    let content_dep = "<?php\nnamespace Carbon;\n\nclass Carbon {}\n";
    backend.update_ast(uri_dep, content_dep);
    {
        let mut idx = backend.fqn_uri_index.write();
        idx.insert("Carbon\\Carbon".to_string(), uri_dep.to_string());
    }

    // The file under edit has NO namespace.
    let uri = "file:///test.php";
    let content = "<?php\n\nfunction () {\n    return Carbon::now();\n};\n";
    backend.update_ast(uri, content);

    // Range covering "Carbon" on line 3.
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(3, 11),
            end: Position::new(3, 17),
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
    assert!(
        actions.iter().any(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                ca.title.contains("Carbon\\Carbon")
            } else {
                false
            }
        }),
        "expected an import action for Carbon\\Carbon when the namespaced class is in uri_classes_index, got: {:?}",
        actions
            .iter()
            .map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) => ca.title.clone(),
                CodeActionOrCommand::Command(c) => c.title.clone(),
            })
            .collect::<Vec<_>>()
    );
}
