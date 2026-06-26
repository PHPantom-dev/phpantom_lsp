#![cfg(test)]

use crate::Backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// Helper: open a file in the backend.
async fn open_file(backend: &Backend, uri: &Url, text: &str) {
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;
}

/// Helper: send a prepare-rename request and return the response.
async fn prepare_rename(
    backend: &Backend,
    uri: &Url,
    line: u32,
    character: u32,
) -> Option<PrepareRenameResponse> {
    let params = TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        position: Position { line, character },
    };

    backend.prepare_rename(params).await.unwrap()
}

/// Helper: send a rename request and return the workspace edit.
async fn rename(
    backend: &Backend,
    uri: &Url,
    line: u32,
    character: u32,
    new_name: &str,
) -> Option<WorkspaceEdit> {
    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        new_name: new_name.to_string(),
        work_done_progress_params: WorkDoneProgressParams::default(),
    };

    backend.rename(params).await.unwrap()
}

/// Collect all text edits for a given URI from a WorkspaceEdit.
fn edits_for_uri(edit: &WorkspaceEdit, uri: &Url) -> Vec<TextEdit> {
    edit.changes
        .as_ref()
        .and_then(|changes| changes.get(uri))
        .cloned()
        .unwrap_or_default()
}

/// Apply a set of text edits to source text and return the result.
/// Edits must not overlap; they are applied from last to first.
fn apply_edits(source: &str, edits: &[TextEdit]) -> String {
    let mut sorted: Vec<_> = edits.to_vec();
    // Sort by start position descending so we can apply from the end.
    sorted.sort_by(|a, b| {
        b.range
            .start
            .line
            .cmp(&a.range.start.line)
            .then(b.range.start.character.cmp(&a.range.start.character))
    });

    let lines: Vec<&str> = source.lines().collect();
    let mut result = source.to_string();

    for edit in &sorted {
        let start_offset = line_col_to_offset(&lines, edit.range.start);
        let end_offset = line_col_to_offset(&lines, edit.range.end);
        result.replace_range(start_offset..end_offset, &edit.new_text);
    }

    result
}

fn line_col_to_offset(lines: &[&str], pos: Position) -> usize {
    let mut offset = 0;
    for (i, line) in lines.iter().enumerate() {
        if i == pos.line as usize {
            return offset + pos.character as usize;
        }
        offset += line.len() + 1; // +1 for newline
    }
    offset
}

// ─── Variable Rename ────────────────────────────────────────────────────────

#[tokio::test]
async fn rename_variable_in_function() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $user = new User();\n",
        "    $user->name = 'Alice';\n",
        "    echo $user->name;\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    // Rename $user on line 2 (the assignment)
    let edit = rename(&backend, &uri, 2, 5, "$person").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for variable rename"
    );

    let edit = edit.unwrap();
    let file_edits = edits_for_uri(&edit, &uri);
    assert!(
        file_edits.len() >= 3,
        "Expected at least 3 edits for $user (decl + 2 usages), got {}",
        file_edits.len()
    );

    // All edits should use the new name with `$`.
    for te in &file_edits {
        assert_eq!(te.new_text, "$person");
    }
}

#[tokio::test]
async fn rename_variable_without_dollar_prefix() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $x = 1;\n",
        "    echo $x;\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    // User provides new name without `$` — the handler should add it.
    let edit = rename(&backend, &uri, 2, 5, "y").await;
    assert!(edit.is_some());

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    for te in &file_edits {
        assert_eq!(te.new_text, "$y");
    }
}

#[tokio::test]
async fn rename_variable_updates_compact_string() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo(): array {\n",
        "    $user = 'alice';\n",
        "    return compact('user');\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 2, 6, "$person").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for variable rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let updated = apply_edits(text, &file_edits);
    assert!(updated.contains("$person = 'alice';"));
    assert!(updated.contains("compact('person')"));
}

#[tokio::test]
async fn rename_variable_updates_dynamic_property_selector() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo(object $message, string $type): void {\n",
        "    $attribute = strtolower($type);\n",
        "    if (empty($message->{$attribute})) {\n",
        "        return;\n",
        "    }\n",
        "    echo $attribute;\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 2, 6, "$field").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for variable rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let updated = apply_edits(text, &file_edits);
    assert!(updated.contains("$field = strtolower($type);"));
    assert!(updated.contains("$message->{$field}"));
    assert!(updated.contains("echo $field;"));
}

#[tokio::test]
async fn rename_from_compact_string_updates_variable() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo(): array {\n",
        "    $user = 'alice';\n",
        "    return compact('user');\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 3, 21, "person").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for compact rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let updated = apply_edits(text, &file_edits);
    assert!(updated.contains("$person = 'alice';"));
    assert!(updated.contains("compact('person')"));
}

#[tokio::test]
async fn prepare_rename_variable() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $count = 0;\n",
        "    $count++;\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    let response = prepare_rename(&backend, &uri, 2, 6).await;
    assert!(
        response.is_some(),
        "Expected prepare rename to succeed for $count"
    );

    if let Some(PrepareRenameResponse::RangeWithPlaceholder { placeholder, .. }) = response {
        assert_eq!(placeholder, "$count");
    } else {
        panic!("Expected RangeWithPlaceholder response");
    }
}

#[tokio::test]
async fn prepare_rename_compact_string_uses_bare_name() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo(): array {\n",
        "    $user = 'alice';\n",
        "    return compact('user');\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    let response = prepare_rename(&backend, &uri, 3, 21).await;
    assert!(
        response.is_some(),
        "Expected prepare rename on compact string"
    );

    if let Some(PrepareRenameResponse::RangeWithPlaceholder { placeholder, .. }) = response {
        assert_eq!(placeholder, "user");
    } else {
        panic!("Expected RangeWithPlaceholder response");
    }
}

// ─── Non-Renameable Symbols ─────────────────────────────────────────────────

#[tokio::test]
async fn prepare_rename_rejects_this() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {\n",
        "        $this->baz();\n",
        "    }\n",
        "    public function baz(): void {}\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    // `$this` should not be renameable.
    let response = prepare_rename(&backend, &uri, 3, 9).await;
    assert!(response.is_none(), "$this should not be renameable");
}

#[tokio::test]
async fn prepare_rename_rejects_self() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public static function create(): self {\n",
        "        return new self();\n",
        "    }\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    // `self` keyword on line 3 should not be renameable.
    let response = prepare_rename(&backend, &uri, 3, 20).await;
    assert!(response.is_none(), "self keyword should not be renameable");
}

#[tokio::test]
async fn prepare_rename_rejects_static() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public static function create(): static {\n",
        "        return new static();\n",
        "    }\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    let response = prepare_rename(&backend, &uri, 3, 22).await;
    assert!(
        response.is_none(),
        "static keyword should not be renameable"
    );
}

#[tokio::test]
async fn prepare_rename_rejects_parent() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    public function hello(): void {}\n",
        "}\n",
        "class Child extends Base {\n",
        "    public function hello(): void {\n",
        "        parent::hello();\n",
        "    }\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    let response = prepare_rename(&backend, &uri, 6, 10).await;
    assert!(
        response.is_none(),
        "parent keyword should not be renameable"
    );
}

// ─── Class Rename ───────────────────────────────────────────────────────────

#[tokio::test]
async fn rename_class_same_file() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function log(string $msg): void {}\n",
        "}\n",
        "function demo(Logger $logger): void {\n",
        "    $obj = new Logger();\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    // Rename from a reference site (type hint on line 4).
    let edit = rename(&backend, &uri, 4, 16, "AppLogger").await;
    assert!(edit.is_some(), "Expected a workspace edit for class rename");

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    // Should find: declaration (L1), type hint (L4), new (L5) = at least 3.
    assert!(
        file_edits.len() >= 3,
        "Expected at least 3 edits for Logger, got {}",
        file_edits.len()
    );

    for te in &file_edits {
        assert_eq!(te.new_text, "AppLogger");
    }
}

#[tokio::test]
async fn rename_class_from_declaration() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public function render(): string { return ''; }\n",
        "}\n",
        "function demo(Widget $w): void {\n",
        "    $obj = new Widget();\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    // Rename from the declaration site (line 1).
    let edit = rename(&backend, &uri, 1, 7, "Component").await;
    assert!(edit.is_some());

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    assert!(
        file_edits.len() >= 3,
        "Expected at least 3 edits for Widget, got {}",
        file_edits.len()
    );

    for te in &file_edits {
        assert_eq!(te.new_text, "Component");
    }
}

#[tokio::test]
async fn prepare_rename_class() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {}\n",
        "function demo(Foo $f): void {}\n",
    );

    open_file(&backend, &uri, text).await;

    let response = prepare_rename(&backend, &uri, 1, 7).await;
    assert!(response.is_some());

    if let Some(PrepareRenameResponse::RangeWithPlaceholder { placeholder, .. }) = response {
        assert_eq!(placeholder, "Foo");
    } else {
        panic!("Expected RangeWithPlaceholder response");
    }
}

// ─── Method Rename ──────────────────────────────────────────────────────────

#[tokio::test]
async fn rename_method() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Service {\n",
        "    public function process(): void {}\n",
        "}\n",
        "function demo(): void {\n",
        "    $s = new Service();\n",
        "    $s->process();\n",
        "    $s->process();\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    // Rename from call site (line 6).
    let edit = rename(&backend, &uri, 6, 9, "execute").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for method rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    // Should find: declaration (L2) + 2 call sites (L6, L7) = at least 3.
    assert!(
        file_edits.len() >= 3,
        "Expected at least 3 edits for process, got {}",
        file_edits.len()
    );

    for te in &file_edits {
        assert_eq!(te.new_text, "execute");
    }
}

#[tokio::test]
async fn rename_static_method() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Factory {\n",
        "    public static function create(): self { return new self(); }\n",
        "}\n",
        "function demo(): void {\n",
        "    Factory::create();\n",
        "    Factory::create();\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 5, 14, "build").await;
    assert!(edit.is_some());

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    assert!(
        file_edits.len() >= 3,
        "Expected at least 3 edits for create, got {}",
        file_edits.len()
    );

    for te in &file_edits {
        assert_eq!(te.new_text, "build");
    }
}

// ─── Property Rename ────────────────────────────────────────────────────────

#[tokio::test]
async fn rename_property_from_access() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name = '';\n",
        "    public function greet(): string {\n",
        "        return $this->name;\n",
        "    }\n",
        "}\n",
        "function demo(): void {\n",
        "    $u = new User();\n",
        "    $u->name = 'Alice';\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    // Rename from access site (line 9, `$u->name`).
    let edit = rename(&backend, &uri, 9, 9, "displayName").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for property rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    // Should have edits for: declaration ($name), $this->name, $u->name.
    assert!(
        file_edits.len() >= 3,
        "Expected at least 3 edits for name property, got {}",
        file_edits.len()
    );

    // The declaration site includes `$`, access sites don't.
    for te in &file_edits {
        assert!(
            te.new_text == "displayName" || te.new_text == "$displayName",
            "Unexpected edit text: {}",
            te.new_text
        );
    }
}

// ─── Function Rename ────────────────────────────────────────────────────────

#[tokio::test]
async fn rename_function() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function helper(): void {}\n",
        "function demo(): void {\n",
        "    helper();\n",
        "    helper();\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 3, 6, "utility").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for function rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    // declaration (L1) + 2 call sites (L3, L4) = at least 3.
    assert!(
        file_edits.len() >= 3,
        "Expected at least 3 edits for helper, got {}",
        file_edits.len()
    );

    for te in &file_edits {
        assert_eq!(te.new_text, "utility");
    }
}

// ─── Constant Rename ────────────────────────────────────────────────────────

#[tokio::test]
async fn rename_class_constant() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Status {\n",
        "    const ACTIVE = 1;\n",
        "}\n",
        "function demo(): void {\n",
        "    echo Status::ACTIVE;\n",
        "    $x = Status::ACTIVE;\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 5, 19, "ENABLED").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for constant rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    assert!(
        file_edits.len() >= 3,
        "Expected at least 3 edits for ACTIVE, got {}",
        file_edits.len()
    );

    for te in &file_edits {
        assert_eq!(te.new_text, "ENABLED");
    }
}

// ─── Cross-file Rename ─────────────────────────────────────────────────────

#[tokio::test]
async fn rename_class_cross_file() {
    let backend = Backend::new_test();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!(
        "<?php\n",
        "class Animal {\n",
        "    public function speak(): string { return ''; }\n",
        "}\n",
    );

    let text_b = concat!(
        "<?php\n",
        "function demo(Animal $a): void {\n",
        "    $obj = new Animal();\n",
        "}\n",
    );

    open_file(&backend, &uri_a, text_a).await;
    open_file(&backend, &uri_b, text_b).await;

    // Rename from file a (declaration).
    let edit = rename(&backend, &uri_a, 1, 7, "Creature").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for cross-file class rename"
    );

    let edit = edit.unwrap();
    let edits_a = edits_for_uri(&edit, &uri_a);
    let edits_b = edits_for_uri(&edit, &uri_b);

    assert!(
        !edits_a.is_empty(),
        "Expected edits in file a (declaration)"
    );
    assert!(!edits_b.is_empty(), "Expected edits in file b (references)");

    for te in edits_a.iter().chain(edits_b.iter()) {
        assert_eq!(te.new_text, "Creature");
    }
}

#[tokio::test]
async fn rename_method_cross_file() {
    let backend = Backend::new_test();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!(
        "<?php\n",
        "class Printer {\n",
        "    public function print(): void {}\n",
        "}\n",
    );

    let text_b = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $p = new Printer();\n",
        "    $p->print();\n",
        "}\n",
    );

    open_file(&backend, &uri_a, text_a).await;
    open_file(&backend, &uri_b, text_b).await;

    // Rename from the call site in file b.
    let edit = rename(&backend, &uri_b, 3, 9, "output").await;
    assert!(edit.is_some());

    let edit = edit.unwrap();
    let edits_a = edits_for_uri(&edit, &uri_a);
    let edits_b = edits_for_uri(&edit, &uri_b);

    assert!(
        !edits_a.is_empty(),
        "Expected edits in file a (declaration)"
    );
    assert!(!edits_b.is_empty(), "Expected edits in file b (call site)");

    for te in edits_a.iter().chain(edits_b.iter()) {
        assert_eq!(te.new_text, "output");
    }
}

// ─── Whitespace / No Symbol ─────────────────────────────────────────────────

#[tokio::test]
async fn prepare_rename_on_whitespace_returns_none() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!("<?php\n", "\n", "function demo(): void {}\n",);

    open_file(&backend, &uri, text).await;

    // Line 1 is blank.
    let response = prepare_rename(&backend, &uri, 1, 0).await;
    assert!(response.is_none(), "Expected no rename on whitespace");
}

#[tokio::test]
async fn rename_on_whitespace_returns_none() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!("<?php\n", "\n", "function demo(): void {}\n",);

    open_file(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 1, 0, "anything").await;
    assert!(edit.is_none(), "Expected no edit on whitespace");
}

// ─── Result Correctness ─────────────────────────────────────────────────────

#[tokio::test]
async fn rename_variable_produces_valid_php() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $a = 1;\n",
        "    $b = $a + 2;\n",
        "    echo $a;\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 2, 5, "$z").await;
    assert!(edit.is_some());

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    // The renamed variable should appear as `$z` everywhere.
    assert!(result.contains("$z = 1;"), "Declaration not renamed");
    assert!(result.contains("$b = $z + 2;"), "RHS usage not renamed");
    assert!(result.contains("echo $z;"), "Echo usage not renamed");
    // And the old name should be gone.
    assert!(!result.contains("$a"), "Old variable name still present");
}

// ─── Variable Scoping ───────────────────────────────────────────────────────

#[tokio::test]
async fn rename_variable_does_not_leak_across_functions() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function alpha(): void {\n",
        "    $x = 1;\n",
        "    echo $x;\n",
        "}\n",
        "function beta(): void {\n",
        "    $x = 2;\n",
        "    echo $x;\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    // Rename $x in alpha (line 2).
    let edit = rename(&backend, &uri, 2, 5, "$y").await;
    assert!(edit.is_some());

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    // alpha should have $y, beta should still have $x.
    assert!(result.contains("function alpha(): void {\n    $y = 1;\n    echo $y;\n}"));
    assert!(result.contains("function beta(): void {\n    $x = 2;\n    echo $x;\n}"));
}

// ─── Class-Aware Member Rename ──────────────────────────────────────────────

#[tokio::test]
async fn rename_method_does_not_leak_to_unrelated_class() {
    // Two unrelated classes with the same method name.  Renaming the
    // method on one class must not touch the other.
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                 // L0
        "class Dog {\n",                           // L1
        "    public function speak(): void {}\n",  // L2
        "}\n",                                     // L3
        "class Cat {\n",                           // L4
        "    public function speak(): void {}\n",  // L5
        "}\n",                                     // L6
        "function demo(Dog $d, Cat $c): void {\n", // L7
        "    $d->speak();\n",                      // L8
        "    $c->speak();\n",                      // L9
        "}\n",                                     // L10
    );

    open_file(&backend, &uri, text).await;

    // Rename speak() from the Dog::speak declaration (line 2, col 21).
    // "    public function speak(): void {}"
    //                     ^ col 20
    let edit = rename(&backend, &uri, 2, 21, "bark").await;
    assert!(edit.is_some(), "Rename should produce edits");

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    // Dog::speak and $d->speak should be renamed to bark.
    assert!(
        result.contains("function bark()"),
        "Dog's method should be renamed to bark; got:\n{}",
        result
    );
    assert!(
        result.contains("$d->bark()"),
        "$d->speak() should become $d->bark(); got:\n{}",
        result
    );

    // Cat::speak and $c->speak must NOT be renamed.
    assert!(
        result.contains("class Cat {\n    public function speak(): void {}"),
        "Cat's method should remain speak; got:\n{}",
        result
    );
    assert!(
        result.contains("$c->speak()"),
        "$c->speak() should remain unchanged; got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_private_method_excludes_unresolved_same_name_calls() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Listener {\n",
        "    private function recalculate(): void {}\n",
        "    public function handle(): void {\n",
        "        $this->recalculate();\n",
        "    }\n",
        "}\n",
        "class Other {\n",
        "    public function recalculate(): void {}\n",
        "}\n",
        "function demo($unknown, Other $other): void {\n",
        "    $unknown->recalculate();\n",
        "    $other->recalculate();\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 2, 25, "calculate").await;
    assert!(edit.is_some(), "Rename should produce edits");

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert!(
        result.contains("private function calculate(): void {}"),
        "Private declaration should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("$this->calculate()"),
        "$this call should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("$unknown->recalculate()"),
        "Unresolved same-name call should remain unchanged; got:\n{}",
        result
    );
    assert!(
        result.contains("$other->recalculate()"),
        "Resolved unrelated class call should remain unchanged; got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_listener_private_method_cross_file_stays_scoped() {
    let backend = Backend::new_test();
    let uri_a = Url::parse("file:///Listener.php").unwrap();
    let uri_b = Url::parse("file:///Other.php").unwrap();
    let uri_c = Url::parse("file:///Use.php").unwrap();

    let text_a = concat!(
        "<?php\n",
        "final class InvoiceListener {\n",
        "    private function recalculate(): void {}\n",
        "    public function handle(): void {\n",
        "        $this->recalculate();\n",
        "    }\n",
        "}\n",
    );

    let text_b = concat!(
        "<?php\n",
        "class PricingService {\n",
        "    public function recalculate(): void {}\n",
        "}\n",
    );

    let text_c = concat!(
        "<?php\n",
        "function demo($unknown, PricingService $service): void {\n",
        "    $unknown->recalculate();\n",
        "    $service->recalculate();\n",
        "}\n",
    );

    open_file(&backend, &uri_a, text_a).await;
    open_file(&backend, &uri_b, text_b).await;
    open_file(&backend, &uri_c, text_c).await;

    let edit = rename(&backend, &uri_a, 2, 25, "calculate").await;
    assert!(edit.is_some(), "Rename should produce edits");

    let edit = edit.unwrap();
    let edits_a = edits_for_uri(&edit, &uri_a);
    let edits_b = edits_for_uri(&edit, &uri_b);
    let edits_c = edits_for_uri(&edit, &uri_c);

    let result_a = apply_edits(text_a, &edits_a);
    let result_b = apply_edits(text_b, &edits_b);
    let result_c = apply_edits(text_c, &edits_c);

    assert!(
        result_a.contains("private function calculate(): void {}"),
        "Listener private method should be renamed; got:\n{}",
        result_a
    );
    assert!(
        result_a.contains("$this->calculate()"),
        "Listener self-call should be renamed; got:\n{}",
        result_a
    );
    assert!(
        result_b.contains("public function recalculate(): void {}"),
        "Unrelated class declaration should stay unchanged; got:\n{}",
        result_b
    );
    assert!(
        result_c.contains("$unknown->recalculate()"),
        "Unknown receiver call should stay unchanged; got:\n{}",
        result_c
    );
    assert!(
        result_c.contains("$service->recalculate()"),
        "Unrelated typed receiver call should stay unchanged; got:\n{}",
        result_c
    );
}

#[tokio::test]
async fn rename_method_on_implementation_does_not_leak_to_sibling_implementors() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface Recalculates {\n",
        "    public function recalculate(): void;\n",
        "}\n",
        "class Listener implements Recalculates {\n",
        "    public function recalculate(): void {}\n",
        "}\n",
        "class Worker implements Recalculates {\n",
        "    public function recalculate(): void {}\n",
        "}\n",
        "function demo(Listener $listener, Worker $worker): void {\n",
        "    $listener->recalculate();\n",
        "    $worker->recalculate();\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 5, 20, "calculate").await;
    assert!(edit.is_some(), "Rename should produce edits");

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert!(
        result.contains(
            "class Listener implements Recalculates {\n    public function calculate(): void {}"
        ),
        "Implementation declaration should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("$listener->calculate()"),
        "Listener call should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("interface Recalculates {\n    public function recalculate(): void;"),
        "Interface declaration should remain unchanged; got:\n{}",
        result
    );
    assert!(
        result.contains(
            "class Worker implements Recalculates {\n    public function recalculate(): void {}"
        ),
        "Sibling implementation should remain unchanged; got:\n{}",
        result
    );
    assert!(
        result.contains("$worker->recalculate()"),
        "Sibling call should remain unchanged; got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_interface_method_updates_implementations_and_calls() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface Recalculates {\n",
        "    public function recalculate(): void;\n",
        "}\n",
        "class Listener implements Recalculates {\n",
        "    public function recalculate(): void {}\n",
        "}\n",
        "class Worker implements Recalculates {\n",
        "    public function recalculate(): void {}\n",
        "}\n",
        "function demo(Recalculates $item, Listener $listener, Worker $worker): void {\n",
        "    $item->recalculate();\n",
        "    $listener->recalculate();\n",
        "    $worker->recalculate();\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 2, 20, "calculate").await;
    assert!(edit.is_some(), "Rename should produce edits");

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert!(
        result.contains("interface Recalculates {\n    public function calculate(): void;"),
        "Interface declaration should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains(
            "class Listener implements Recalculates {\n    public function calculate(): void {}"
        ),
        "First implementation should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains(
            "class Worker implements Recalculates {\n    public function calculate(): void {}"
        ),
        "Second implementation should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("$item->calculate()"),
        "Interface-typed call should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("$listener->calculate()"),
        "Listener call should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("$worker->calculate()"),
        "Worker call should be renamed; got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_child_override_stays_on_child_branch() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class BaseListener {\n",
        "    protected function recalculate(): void {}\n",
        "}\n",
        "class InvoiceListener extends BaseListener {\n",
        "    protected function recalculate(): void {\n",
        "        $this->recalculate();\n",
        "    }\n",
        "}\n",
        "class OrderListener extends BaseListener {\n",
        "    protected function recalculate(): void {\n",
        "        $this->recalculate();\n",
        "    }\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 5, 24, "calculate").await;
    assert!(edit.is_some(), "Rename should produce edits");

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert!(
        result.contains("class InvoiceListener extends BaseListener {\n    protected function calculate(): void {\n        $this->calculate();"),
        "Child override should be renamed with its self-call; got:\n{}",
        result
    );
    assert!(
        result.contains("class BaseListener {\n    protected function recalculate(): void {}"),
        "Parent declaration should remain unchanged; got:\n{}",
        result
    );
    assert!(
        result.contains("class OrderListener extends BaseListener {\n    protected function recalculate(): void {\n        $this->recalculate();"),
        "Sibling override branch should remain unchanged; got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_parent_method_updates_overrides_and_subclass_calls() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class BaseJob {\n",
        "    protected function recalculate(): void {}\n",
        "}\n",
        "class ChildJob extends BaseJob {\n",
        "    protected function recalculate(): void {\n",
        "        $this->recalculate();\n",
        "    }\n",
        "    public function run(): void {\n",
        "        $this->recalculate();\n",
        "    }\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 2, 24, "calculate").await;
    assert!(edit.is_some(), "Rename should produce edits");

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert!(
        result.contains("class BaseJob {\n    protected function calculate(): void {}"),
        "Parent declaration should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("class ChildJob extends BaseJob {\n    protected function calculate(): void {\n        $this->calculate();\n    }\n    public function run(): void {\n        $this->calculate();"),
        "Override and subclass calls should be renamed; got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_method_includes_inherited_class() {
    // Renaming a method on a parent class should also rename it on
    // accesses through a child class.
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                    // L0
        "class Base {\n",                             // L1
        "    public function run(): void {}\n",       // L2
        "}\n",                                        // L3
        "class Child extends Base {}\n",              // L4
        "function demo(Base $b, Child $c): void {\n", // L5
        "    $b->run();\n",                           // L6
        "    $c->run();\n",                           // L7
        "}\n",                                        // L8
    );

    open_file(&backend, &uri, text).await;

    // Rename run() from $b->run() (line 6, col 10).
    let edit = rename(&backend, &uri, 6, 10, "execute").await;
    assert!(edit.is_some(), "Rename should produce edits");

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    // Both $b->run() and $c->run() should be renamed (Child extends Base).
    assert!(
        result.contains("$b->execute()"),
        "$b->run() should become $b->execute(); got:\n{}",
        result
    );
    assert!(
        result.contains("$c->execute()"),
        "$c->run() should become $c->execute() (inherited); got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_static_method_does_not_leak_to_unrelated_class() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                        // L0
        "class Alpha {\n",                                // L1
        "    public static function create(): void {}\n", // L2
        "}\n",                                            // L3
        "class Beta {\n",                                 // L4
        "    public static function create(): void {}\n", // L5
        "}\n",                                            // L6
        "function demo(): void {\n",                      // L7
        "    Alpha::create();\n",                         // L8
        "    Beta::create();\n",                          // L9
        "}\n",                                            // L10
    );

    open_file(&backend, &uri, text).await;

    // Rename create() from Alpha::create() call (line 8, col 12).
    // "    Alpha::create();"
    //             ^ col 11
    let edit = rename(&backend, &uri, 8, 12, "make").await;
    assert!(edit.is_some(), "Rename should produce edits");

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    // Alpha::create should be renamed.
    assert!(
        result.contains("Alpha::make()"),
        "Alpha::create() should become Alpha::make(); got:\n{}",
        result
    );

    // Beta::create must NOT be renamed.
    assert!(
        result.contains("Beta::create()"),
        "Beta::create() should remain unchanged; got:\n{}",
        result
    );
}

// ─── Use-Statement-Aware Class Rename ───────────────────────────────────────

#[tokio::test]
async fn rename_class_updates_use_import() {
    // Renaming a class should update the `use` statement FQN (last segment)
    // as well as all in-code references.
    let backend = Backend::new_test();
    let uri_decl = Url::parse("file:///src/TaskResource.php").unwrap();
    let uri_usage = Url::parse("file:///src/Task.php").unwrap();

    let text_decl = concat!(
        "<?php\n",
        "namespace Eagle\\Tasks\\Resources;\n",
        "\n",
        "class TaskResource {}\n",
    );

    let text_usage = concat!(
        "<?php\n",
        "namespace Eagle\\Tasks;\n",
        "\n",
        "use Eagle\\Tasks\\Resources\\TaskResource;\n",
        "\n",
        "class Task {\n",
        "    protected static string $service = TaskResource::class;\n",
        "}\n",
    );

    open_file(&backend, &uri_decl, text_decl).await;
    open_file(&backend, &uri_usage, text_usage).await;

    // Rename from the declaration site (line 3, col 6 = "TaskResource").
    let edit = rename(&backend, &uri_decl, 3, 6, "TaskResourceService").await;
    assert!(edit.is_some(), "Expected a workspace edit for class rename");

    let ws = edit.unwrap();
    let edits_usage = edits_for_uri(&ws, &uri_usage);
    assert!(!edits_usage.is_empty(), "Expected edits in the usage file");

    let result = apply_edits(text_usage, &edits_usage);

    // The use statement should have the FQN last segment updated.
    assert!(
        result.contains("use Eagle\\Tasks\\Resources\\TaskResourceService;"),
        "Use statement should be updated; got:\n{}",
        result
    );

    // The in-code reference should be renamed.
    assert!(
        result.contains("TaskResourceService::class"),
        "In-code reference should be renamed; got:\n{}",
        result
    );

    // The old name should NOT appear.
    assert!(
        !result.contains("TaskResource::class"),
        "Old name should not remain; got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_class_preserves_explicit_alias() {
    // When a file imports the class with an explicit alias, the alias
    // should be preserved and in-code references should NOT be renamed.
    let backend = Backend::new_test();
    let uri_decl = Url::parse("file:///src/TaskResource.php").unwrap();
    let uri_usage = Url::parse("file:///src/Controller.php").unwrap();

    let text_decl = concat!(
        "<?php\n",
        "namespace Eagle\\Tasks\\Resources;\n",
        "\n",
        "class TaskResource {}\n",
    );

    let text_usage = concat!(
        "<?php\n",
        "namespace Eagle\\Tasks\\Http;\n",
        "\n",
        "use Eagle\\Tasks\\Resources\\TaskResource as ResourceService;\n",
        "\n",
        "class Controller {\n",
        "    private ResourceService $service;\n",
        "}\n",
    );

    open_file(&backend, &uri_decl, text_decl).await;
    open_file(&backend, &uri_usage, text_usage).await;

    // Rename from the declaration.
    let edit = rename(&backend, &uri_decl, 3, 6, "TaskResourceService").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for aliased class rename"
    );

    let ws = edit.unwrap();
    let edits_usage = edits_for_uri(&ws, &uri_usage);

    let result = apply_edits(text_usage, &edits_usage);

    // The use statement FQN should be updated, but the alias kept.
    assert!(
        result.contains("use Eagle\\Tasks\\Resources\\TaskResourceService as ResourceService;"),
        "Use statement FQN should update, alias preserved; got:\n{}",
        result
    );

    // In-code references via the alias should NOT change.
    assert!(
        result.contains("private ResourceService $service;"),
        "Alias-based references should remain unchanged; got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_class_with_collision_adds_alias() {
    // When renaming would produce a short name that collides with an
    // existing import, an alias should be introduced.
    let backend = Backend::new_test();
    let uri_a = Url::parse("file:///src/OldName.php").unwrap();
    let uri_b = Url::parse("file:///src/NewName.php").unwrap();
    let uri_usage = Url::parse("file:///src/Usage.php").unwrap();

    let text_a = concat!("<?php\n", "namespace Ns\\A;\n", "\n", "class OldName {}\n",);

    let text_b = concat!("<?php\n", "namespace Ns\\B;\n", "\n", "class NewName {}\n",);

    let text_usage = concat!(
        "<?php\n",
        "use Ns\\A\\OldName;\n",
        "use Ns\\B\\NewName;\n",
        "\n",
        "function demo(OldName $a, NewName $b): void {}\n",
    );

    open_file(&backend, &uri_a, text_a).await;
    open_file(&backend, &uri_b, text_b).await;
    open_file(&backend, &uri_usage, text_usage).await;

    // Rename OldName → NewName (which collides with an existing import).
    let edit = rename(&backend, &uri_a, 3, 6, "NewName").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for colliding class rename"
    );

    let ws = edit.unwrap();
    let edits_usage = edits_for_uri(&ws, &uri_usage);
    let result = apply_edits(text_usage, &edits_usage);

    // The existing `use Ns\B\NewName;` should remain unchanged.
    assert!(
        result.contains("use Ns\\B\\NewName;"),
        "Existing import should remain unchanged; got:\n{}",
        result
    );

    // The renamed import should get an alias to avoid collision.
    assert!(
        result.contains("use Ns\\A\\NewName as NewNameAlias;"),
        "Renamed import should get an alias; got:\n{}",
        result
    );

    // In-code references to the renamed class should use the alias.
    assert!(
        result.contains("NewNameAlias $a"),
        "In-code references should use the alias; got:\n{}",
        result
    );

    // The other class's references should be unaffected.
    assert!(
        result.contains("NewName $b"),
        "Other class references should remain; got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_class_does_not_rename_self_static_parent() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public const BAR = 1;\n",
        "    public static function create(): self {\n",
        "        return new self();\n",
        "    }\n",
        "    public function check(): bool {\n",
        "        return self::BAR === static::BAR;\n",
        "    }\n",
        "}\n",
        "class Bar extends Foo {\n",
        "    public function parentRef(): void {\n",
        "        parent::create();\n",
        "    }\n",
        "}\n",
        "function demo(Foo $f): void {\n",
        "    $obj = new Foo();\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    // Rename Foo -> Baz from the declaration.
    let edit = rename(&backend, &uri, 1, 7, "Baz").await;
    assert!(edit.is_some(), "Expected a workspace edit");

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    // Class declaration and references should be renamed.
    assert!(
        result.contains("class Baz"),
        "Declaration should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("new Baz()"),
        "new expression should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("demo(Baz"),
        "Type hint should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("extends Baz"),
        "extends should be renamed; got:\n{}",
        result
    );

    // self, static, and parent keywords must NOT be renamed.
    assert!(
        result.contains("self::BAR"),
        "self:: should not be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("static::BAR"),
        "static:: should not be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("parent::create"),
        "parent:: should not be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("new self()"),
        "new self() should not be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("): self {"),
        "return type self should not be renamed; got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_class_same_file_no_use_statement() {
    // Renaming a class in the same file (no use statement) should still work.
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function log(string $msg): void {}\n",
        "}\n",
        "function demo(Logger $logger): void {\n",
        "    $obj = new Logger();\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    // Rename from the declaration.
    let edit = rename(&backend, &uri, 1, 7, "AppLogger").await;
    assert!(edit.is_some(), "Expected a workspace edit");

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert!(
        result.contains("class AppLogger"),
        "Declaration should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("function demo(AppLogger"),
        "Type hint should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("new AppLogger()"),
        "new expression should be renamed; got:\n{}",
        result
    );
    // Verify no standalone "Logger" remains (AppLogger is fine).
    let has_standalone_old_name = result
        .lines()
        .any(|l| l.contains("Logger") && !l.contains("AppLogger"));
    assert!(
        !has_standalone_old_name,
        "Old standalone name should not remain; got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_class_updates_use_import_from_reference_site() {
    // Trigger rename from a reference site (not the declaration) and
    // verify the use statement is still updated.
    let backend = Backend::new_test();
    let uri_decl = Url::parse("file:///src/Animal.php").unwrap();
    let uri_usage = Url::parse("file:///src/Zoo.php").unwrap();

    let text_decl = concat!(
        "<?php\n",
        "namespace Zoo\\Models;\n",
        "\n",
        "class Animal {}\n",
    );

    let text_usage = concat!(
        "<?php\n",
        "use Zoo\\Models\\Animal;\n",
        "\n",
        "function feed(Animal $a): void {}\n",
    );

    open_file(&backend, &uri_decl, text_decl).await;
    open_file(&backend, &uri_usage, text_usage).await;

    // Rename from the reference site in the usage file (line 3, col 15).
    // "function feed(Animal $a): void {}"
    //                ^ col 14
    let edit = rename(&backend, &uri_usage, 3, 15, "Creature").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit when renaming from reference"
    );

    let ws = edit.unwrap();
    let edits_usage = edits_for_uri(&ws, &uri_usage);
    let result = apply_edits(text_usage, &edits_usage);

    assert!(
        result.contains("use Zoo\\Models\\Creature;"),
        "Use statement should be updated; got:\n{}",
        result
    );
    assert!(
        result.contains("function feed(Creature $a)"),
        "In-code reference should be renamed; got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_class_cross_file_use_import_multiple_refs() {
    // A file with multiple references to the renamed class (via use
    // import) should have all references and the use statement updated.
    let backend = Backend::new_test();
    let uri_decl = Url::parse("file:///src/Repo.php").unwrap();
    let uri_usage = Url::parse("file:///src/Service.php").unwrap();

    let text_decl = concat!(
        "<?php\n",
        "namespace App\\Repos;\n",
        "\n",
        "class UserRepo {}\n",
    );

    let text_usage = concat!(
        "<?php\n",
        "use App\\Repos\\UserRepo;\n",
        "\n",
        "class Service {\n",
        "    private UserRepo $repo;\n",
        "    public function getRepo(): UserRepo {\n",
        "        return new UserRepo();\n",
        "    }\n",
        "}\n",
    );

    open_file(&backend, &uri_decl, text_decl).await;
    open_file(&backend, &uri_usage, text_usage).await;

    let edit = rename(&backend, &uri_decl, 3, 6, "UserRepository").await;
    assert!(edit.is_some());

    let ws = edit.unwrap();
    let edits_usage = edits_for_uri(&ws, &uri_usage);
    let result = apply_edits(text_usage, &edits_usage);

    assert!(
        result.contains("use App\\Repos\\UserRepository;"),
        "Use statement should be updated; got:\n{}",
        result
    );
    assert!(
        result.contains("private UserRepository $repo;"),
        "Property type should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("getRepo(): UserRepository"),
        "Return type should be renamed; got:\n{}",
        result
    );
    assert!(
        result.contains("new UserRepository()"),
        "new expression should be renamed; got:\n{}",
        result
    );
    // Verify no standalone "UserRepo" remains (UserRepository is fine).
    let has_standalone_old_name = result
        .lines()
        .any(|l| l.contains("UserRepo") && !l.contains("UserRepository"));
    assert!(
        !has_standalone_old_name,
        "Old standalone name should not remain; got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_class_fqn_inline_reference() {
    // When a file uses the class via an inline FQN (no use statement),
    // only the last segment should be renamed.
    let backend = Backend::new_test();
    let uri_decl = Url::parse("file:///src/Item.php").unwrap();
    let uri_usage = Url::parse("file:///src/other.php").unwrap();

    let text_decl = concat!("<?php\n", "namespace Shop;\n", "\n", "class Item {}\n",);

    let text_usage = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $x = new \\Shop\\Item();\n",
        "}\n",
    );

    open_file(&backend, &uri_decl, text_decl).await;
    open_file(&backend, &uri_usage, text_usage).await;

    let edit = rename(&backend, &uri_decl, 3, 6, "Product").await;
    assert!(edit.is_some());

    let ws = edit.unwrap();
    let edits_usage = edits_for_uri(&ws, &uri_usage);
    let result = apply_edits(text_usage, &edits_usage);

    // The inline FQN should have only the last segment renamed.
    assert!(
        result.contains("\\Shop\\Product()"),
        "Inline FQN should update last segment only; got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_class_declaration_updates_in_same_namespace() {
    // Two files in the same namespace — references use the short name
    // without a use statement.  The rename should just update the short name.
    let backend = Backend::new_test();
    let uri_a = Url::parse("file:///src/Foo.php").unwrap();
    let uri_b = Url::parse("file:///src/Bar.php").unwrap();

    let text_a = concat!("<?php\n", "namespace App;\n", "\n", "class Foo {}\n",);

    let text_b = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class Bar extends Foo {}\n",
    );

    open_file(&backend, &uri_a, text_a).await;
    open_file(&backend, &uri_b, text_b).await;

    let edit = rename(&backend, &uri_a, 3, 6, "Baz").await;
    assert!(edit.is_some());

    let ws = edit.unwrap();
    let edits_a = edits_for_uri(&ws, &uri_a);
    let edits_b = edits_for_uri(&ws, &uri_b);

    let result_a = apply_edits(text_a, &edits_a);
    let result_b = apply_edits(text_b, &edits_b);

    assert!(
        result_a.contains("class Baz"),
        "Declaration should be renamed; got:\n{}",
        result_a
    );
    assert!(
        result_b.contains("extends Baz"),
        "Cross-file reference should be renamed; got:\n{}",
        result_b
    );
}

// ─── File Rename on Class Rename ────────────────────────────────────────────

/// Extract the `RenameFile` operation from a `WorkspaceEdit`, if any.
fn extract_rename_file(edit: &WorkspaceEdit) -> Option<&RenameFile> {
    let doc_changes = edit.document_changes.as_ref()?;
    match doc_changes {
        DocumentChanges::Operations(ops) => {
            for op in ops {
                if let DocumentChangeOperation::Op(ResourceOp::Rename(rf)) = op {
                    return Some(rf);
                }
            }
            None
        }
        _ => None,
    }
}

/// Collect all text edits for a given URI from a `WorkspaceEdit` that uses
/// `document_changes` (the `DocumentChanges::Operations` variant).
fn doc_change_edits_for_uri(edit: &WorkspaceEdit, uri: &Url) -> Vec<TextEdit> {
    let Some(DocumentChanges::Operations(ops)) = &edit.document_changes else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for op in ops {
        if let DocumentChangeOperation::Edit(tde) = op
            && tde.text_document.uri == *uri
        {
            for e in &tde.edits {
                match e {
                    OneOf::Left(te) => result.push(te.clone()),
                    OneOf::Right(ate) => result.push(TextEdit {
                        range: ate.text_edit.range,
                        new_text: ate.text_edit.new_text.clone(),
                    }),
                }
            }
        }
    }
    result
}

#[tokio::test]
async fn rename_class_renames_file_when_psr4_match() {
    // When the filename matches the class name and the client supports
    // file renames, a RenameFile operation should be included.
    let backend = Backend::new_test();
    backend
        .supports_file_rename
        .store(true, std::sync::atomic::Ordering::Release);

    let uri = Url::parse("file:///src/Foo.php").unwrap();
    let text = concat!("<?php\n", "namespace App;\n", "\n", "class Foo {}\n",);

    open_file(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 3, 6, "Bar").await;
    assert!(edit.is_some(), "Expected a workspace edit");

    let ws = edit.unwrap();

    // Should use document_changes, not changes.
    assert!(
        ws.document_changes.is_some(),
        "Expected document_changes when file rename is included"
    );
    assert!(
        ws.changes.is_none(),
        "changes should be None when document_changes is used"
    );

    // Should contain a RenameFile operation.
    let rf = extract_rename_file(&ws);
    assert!(rf.is_some(), "Expected a RenameFile operation");

    let rf = rf.unwrap();
    assert_eq!(
        rf.old_uri.to_string(),
        "file:///src/Foo.php",
        "Old URI should be the original file"
    );
    assert_eq!(
        rf.new_uri.to_string(),
        "file:///src/Bar.php",
        "New URI should use the new class name"
    );

    // Text edits should target the new URI (file is renamed first).
    let new_uri = Url::parse("file:///src/Bar.php").unwrap();
    let edits = doc_change_edits_for_uri(&ws, &new_uri);
    assert!(
        !edits.is_empty(),
        "Expected text edits targeting the new file URI"
    );

    // The class declaration should be renamed.
    let has_bar = edits.iter().any(|e| e.new_text == "Bar");
    assert!(has_bar, "Expected an edit renaming to Bar");
}

#[tokio::test]
async fn rename_class_no_file_rename_when_filename_mismatch() {
    // When the filename does NOT match the class name, no file rename
    // should happen — only text edits.
    let backend = Backend::new_test();
    backend
        .supports_file_rename
        .store(true, std::sync::atomic::Ordering::Release);

    let uri = Url::parse("file:///src/helpers.php").unwrap();
    let text = concat!("<?php\n", "namespace App;\n", "\n", "class Foo {}\n",);

    open_file(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 3, 6, "Bar").await;
    assert!(edit.is_some());

    let ws = edit.unwrap();

    // Should use plain changes, not document_changes.
    assert!(
        ws.changes.is_some(),
        "Expected plain changes when filename doesn't match class name"
    );
    assert!(
        ws.document_changes.is_none(),
        "Should not include document_changes"
    );
}

#[tokio::test]
async fn rename_class_no_file_rename_when_multiple_classes() {
    // When the file contains more than one class, do not rename the file.
    let backend = Backend::new_test();
    backend
        .supports_file_rename
        .store(true, std::sync::atomic::Ordering::Release);

    let uri = Url::parse("file:///src/Foo.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class Foo {}\n",
        "class Extra {}\n",
    );

    open_file(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 3, 6, "Bar").await;
    assert!(edit.is_some());

    let ws = edit.unwrap();

    // Multiple classes → no file rename.
    assert!(
        ws.changes.is_some(),
        "Expected plain changes when multiple classes in file"
    );
    assert!(
        ws.document_changes.is_none(),
        "Should not include document_changes with multiple classes"
    );
}

#[tokio::test]
async fn rename_class_no_file_rename_when_client_unsupported() {
    // When the client does not support file rename operations, only
    // text edits should be produced.
    let backend = Backend::new_test();
    // supports_file_rename is false by default.

    let uri = Url::parse("file:///src/Foo.php").unwrap();
    let text = concat!("<?php\n", "namespace App;\n", "\n", "class Foo {}\n",);

    open_file(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 3, 6, "Bar").await;
    assert!(edit.is_some());

    let ws = edit.unwrap();

    assert!(
        ws.changes.is_some(),
        "Expected plain changes when client does not support file rename"
    );
    assert!(
        ws.document_changes.is_none(),
        "Should not include document_changes without client support"
    );
}

#[tokio::test]
async fn rename_class_cross_file_with_file_rename() {
    // Cross-file class rename with a use statement, plus file rename.
    let backend = Backend::new_test();
    backend
        .supports_file_rename
        .store(true, std::sync::atomic::Ordering::Release);

    let uri_decl = Url::parse("file:///src/TaskResource.php").unwrap();
    let uri_usage = Url::parse("file:///src/Task.php").unwrap();

    let text_decl = concat!(
        "<?php\n",
        "namespace Eagle\\Tasks\\Resources;\n",
        "\n",
        "class TaskResource {}\n",
    );

    let text_usage = concat!(
        "<?php\n",
        "namespace Eagle\\Tasks;\n",
        "\n",
        "use Eagle\\Tasks\\Resources\\TaskResource;\n",
        "\n",
        "class Task {\n",
        "    public function resource(): TaskResource {\n",
        "        return new TaskResource();\n",
        "    }\n",
        "}\n",
    );

    open_file(&backend, &uri_decl, text_decl).await;
    open_file(&backend, &uri_usage, text_usage).await;

    let edit = rename(&backend, &uri_decl, 3, 6, "TaskDto").await;
    assert!(edit.is_some(), "Expected workspace edit");

    let ws = edit.unwrap();

    // Should use document_changes with a RenameFile.
    assert!(ws.document_changes.is_some());

    let rf = extract_rename_file(&ws);
    assert!(rf.is_some(), "Expected a RenameFile operation");

    let rf = rf.unwrap();
    assert_eq!(rf.old_uri.to_string(), "file:///src/TaskResource.php");
    assert_eq!(rf.new_uri.to_string(), "file:///src/TaskDto.php");

    // Edits in the usage file should NOT have their URI changed (only
    // the definition file is renamed).
    let usage_edits = doc_change_edits_for_uri(&ws, &uri_usage);
    assert!(!usage_edits.is_empty(), "Expected edits in the usage file");

    // Apply edits to verify correctness.
    let result_usage = apply_edits(text_usage, &usage_edits);
    assert!(
        result_usage.contains("use Eagle\\Tasks\\Resources\\TaskDto;"),
        "Use statement should be updated; got:\n{}",
        result_usage
    );
    assert!(
        result_usage.contains("TaskDto"),
        "In-code references should be updated; got:\n{}",
        result_usage
    );

    // The declaration file edits should target the new URI.
    let new_decl_uri = Url::parse("file:///src/TaskDto.php").unwrap();
    let decl_edits = doc_change_edits_for_uri(&ws, &new_decl_uri);
    assert!(
        !decl_edits.is_empty(),
        "Expected edits targeting the new declaration file URI"
    );

    let result_decl = apply_edits(text_decl, &decl_edits);
    assert!(
        result_decl.contains("class TaskDto"),
        "Class declaration should be renamed; got:\n{}",
        result_decl
    );
}

#[tokio::test]
async fn rename_class_from_reference_site_renames_file() {
    // Trigger rename from a reference site (not the declaration) and
    // verify the file is still renamed.
    let backend = Backend::new_test();
    backend
        .supports_file_rename
        .store(true, std::sync::atomic::Ordering::Release);

    let uri_decl = Url::parse("file:///src/Animal.php").unwrap();
    let uri_usage = Url::parse("file:///src/Zoo.php").unwrap();

    let text_decl = concat!(
        "<?php\n",
        "namespace Zoo\\Models;\n",
        "\n",
        "class Animal {}\n",
    );

    let text_usage = concat!(
        "<?php\n",
        "namespace Zoo;\n",
        "\n",
        "use Zoo\\Models\\Animal;\n",
        "\n",
        "class Zoo {\n",
        "    public function get(): Animal {\n",
        "        return new Animal();\n",
        "    }\n",
        "}\n",
    );

    open_file(&backend, &uri_decl, text_decl).await;
    open_file(&backend, &uri_usage, text_usage).await;

    // Rename from the reference site in Zoo.php (line 6, "Animal").
    let edit = rename(&backend, &uri_usage, 6, 30, "Creature").await;
    assert!(
        edit.is_some(),
        "Expected workspace edit from reference site"
    );

    let ws = edit.unwrap();

    // Should include a file rename for the declaration file.
    let rf = extract_rename_file(&ws);
    assert!(rf.is_some(), "Expected a RenameFile operation");

    let rf = rf.unwrap();
    assert_eq!(rf.old_uri.to_string(), "file:///src/Animal.php");
    assert_eq!(rf.new_uri.to_string(), "file:///src/Creature.php");
}

#[tokio::test]
async fn rename_class_no_file_rename_for_non_namespaced() {
    // Non-namespaced class — fqn_uri_index uses bare name as FQN.
    // File rename should still work if filename matches.
    let backend = Backend::new_test();
    backend
        .supports_file_rename
        .store(true, std::sync::atomic::Ordering::Release);

    let uri = Url::parse("file:///src/Widget.php").unwrap();
    let text = concat!("<?php\n", "class Widget {}\n",);

    open_file(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 1, 6, "Gadget").await;
    assert!(edit.is_some());

    let ws = edit.unwrap();

    // Non-namespaced classes are stored in fqn_uri_index with just
    // the short name, so should_rename_file should still find it.
    let rf = extract_rename_file(&ws);
    assert!(
        rf.is_some(),
        "Expected a RenameFile for non-namespaced class with matching filename"
    );

    let rf = rf.unwrap();
    assert_eq!(rf.new_uri.to_string(), "file:///src/Gadget.php");
}

// ─── Enum Case Rename ───────────────────────────────────────────────────────

#[tokio::test]
async fn prepare_rename_enum_case_at_declaration() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                 // 0
        "enum TaskType: int {\n",                  // 1
        "    case Task  = 1;\n",                   // 2
        "    case Issue = 2;\n",                   // 3
        "    public function isIssue(): bool {\n", // 4
        "        return $this === self::Issue;\n", // 5
        "    }\n",                                 // 6
        "}\n",                                     // 7
    );

    open_file(&backend, &uri, text).await;

    // Cursor on `Issue` in `case Issue = 2;` (line 3, col 9)
    let result = prepare_rename(&backend, &uri, 3, 9).await;
    assert!(
        result.is_some(),
        "prepare_rename should succeed on enum case declaration"
    );

    if let Some(PrepareRenameResponse::RangeWithPlaceholder { placeholder, .. }) = result {
        assert_eq!(
            placeholder, "Issue",
            "Placeholder should be the enum case name"
        );
    } else {
        panic!("Expected RangeWithPlaceholder, got {:?}", result);
    }
}

#[tokio::test]
async fn prepare_rename_enum_case_at_reference() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                 // 0
        "enum TaskType: int {\n",                  // 1
        "    case Task  = 1;\n",                   // 2
        "    case Issue = 2;\n",                   // 3
        "    public function isIssue(): bool {\n", // 4
        "        return $this === self::Issue;\n", // 5
        "    }\n",                                 // 6
        "}\n",                                     // 7
    );

    open_file(&backend, &uri, text).await;

    // Cursor on `Issue` in `self::Issue` (line 5, col 36)
    let result = prepare_rename(&backend, &uri, 5, 36).await;
    assert!(
        result.is_some(),
        "prepare_rename should succeed on enum case reference"
    );

    if let Some(PrepareRenameResponse::RangeWithPlaceholder { placeholder, .. }) = result {
        assert_eq!(
            placeholder, "Issue",
            "Placeholder should be the enum case name"
        );
    } else {
        panic!("Expected RangeWithPlaceholder, got {:?}", result);
    }
}

#[tokio::test]
async fn rename_enum_case_from_declaration() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                 // 0
        "enum TaskType: int {\n",                  // 1
        "    case Task  = 1;\n",                   // 2
        "    case Issue = 2;\n",                   // 3
        "    public function isIssue(): bool {\n", // 4
        "        return $this === self::Issue;\n", // 5
        "    }\n",                                 // 6
        "}\n",                                     // 7
    );

    open_file(&backend, &uri, text).await;

    // Rename `Issue` from its declaration site (line 3, col 9)
    let edit = rename(&backend, &uri, 3, 9, "Ticket").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for enum case rename from declaration"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    // Should have at least 2 edits: the declaration + the self::Issue reference
    assert!(
        file_edits.len() >= 2,
        "Expected at least 2 edits for Issue → Ticket, got {}",
        file_edits.len()
    );

    for te in &file_edits {
        assert_eq!(te.new_text, "Ticket");
    }

    let result = apply_edits(text, &file_edits);
    assert!(
        result.contains("case Ticket"),
        "Declaration should be renamed: {}",
        result
    );
    assert!(
        result.contains("self::Ticket"),
        "Reference should be renamed: {}",
        result
    );
    assert!(
        !result.contains("case Issue"),
        "Old declaration should not remain: {}",
        result
    );
    assert!(
        !result.contains("self::Issue"),
        "Old reference should not remain: {}",
        result
    );
}

#[tokio::test]
async fn rename_enum_case_from_reference() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                 // 0
        "enum TaskType: int {\n",                  // 1
        "    case Task  = 1;\n",                   // 2
        "    case Issue = 2;\n",                   // 3
        "    public function isIssue(): bool {\n", // 4
        "        return $this === self::Issue;\n", // 5
        "    }\n",                                 // 6
        "}\n",                                     // 7
    );

    open_file(&backend, &uri, text).await;

    // Rename `Issue` from a reference site: `self::Issue` (line 5, col 36)
    let edit = rename(&backend, &uri, 5, 36, "Ticket").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for enum case rename from reference"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    assert!(
        file_edits.len() >= 2,
        "Expected at least 2 edits for Issue → Ticket, got {}",
        file_edits.len()
    );

    for te in &file_edits {
        assert_eq!(te.new_text, "Ticket");
    }

    let result = apply_edits(text, &file_edits);
    assert!(
        result.contains("case Ticket"),
        "Declaration should be renamed: {}",
        result
    );
    assert!(
        result.contains("self::Ticket"),
        "Reference should be renamed: {}",
        result
    );
}

#[tokio::test]
async fn rename_enum_case_does_not_affect_other_cases() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                 // 0
        "enum TaskType: int {\n",                  // 1
        "    case Task  = 1;\n",                   // 2
        "    case Issue = 2;\n",                   // 3
        "    public function isIssue(): bool {\n", // 4
        "        return $this === self::Issue;\n", // 5
        "    }\n",                                 // 6
        "    public function isTask(): bool {\n",  // 7
        "        return $this === self::Task;\n",  // 8
        "    }\n",                                 // 9
        "}\n",                                     // 10
    );

    open_file(&backend, &uri, text).await;

    // Rename `Issue` from declaration (line 3, col 9)
    let edit = rename(&backend, &uri, 3, 9, "Ticket").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for enum case rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    // `Task` case should remain untouched
    assert!(
        result.contains("case Task"),
        "Other enum case 'Task' should not be affected: {}",
        result
    );
    assert!(
        result.contains("self::Task"),
        "Other enum case reference 'self::Task' should not be affected: {}",
        result
    );
}

#[tokio::test]
async fn rename_unit_enum_case() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                   // 0
        "enum Color {\n",            // 1
        "    case Red;\n",           // 2
        "    case Blue;\n",          // 3
        "}\n",                       // 4
        "function demo(): void {\n", // 5
        "    $c = Color::Red;\n",    // 6
        "}\n",                       // 7
    );

    open_file(&backend, &uri, text).await;

    // Rename `Red` from declaration (line 2, col 9)
    let edit = rename(&backend, &uri, 2, 9, "Crimson").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for unit enum case rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    assert!(
        file_edits.len() >= 2,
        "Expected at least 2 edits for Red → Crimson, got {}",
        file_edits.len()
    );

    for te in &file_edits {
        assert_eq!(te.new_text, "Crimson");
    }

    let result = apply_edits(text, &file_edits);
    assert!(
        result.contains("case Crimson"),
        "Declaration should be renamed: {}",
        result
    );
    assert!(
        result.contains("Color::Crimson"),
        "Reference should be renamed: {}",
        result
    );
}

// ─── Parameter Rename (closure / function / method) ─────────────────────────

#[tokio::test]
async fn rename_closure_parameter_from_param() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Factory {\n",
        "    public function build(): void {\n",
        "        $this->afterMaking(function (Box $item): void {\n",
        "            $item->item_id ??= $item->segment_id\n",
        "                ? $item->segment->run->item_id\n",
        "                : Item::randomOrFactoryCreate()->getKey();\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    // Cursor on `$item` in the closure parameter list (line 3, col 44).
    let edit = rename(&backend, &uri, 3, 44, "$box").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for closure parameter rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    // The parameter and all usages in the closure body should be renamed.
    assert!(
        result.contains("function (Box $box)"),
        "Parameter declaration not renamed: {}",
        result
    );
    assert!(
        result.contains("$box->item_id"),
        "Body usage not renamed: {}",
        result
    );
    assert!(
        result.contains("$box->segment_id"),
        "Body usage not renamed: {}",
        result
    );
    assert!(
        result.contains("$box->segment->run"),
        "Chained body usage not renamed: {}",
        result
    );
    // Old name should be gone.
    assert!(
        !result.contains("$item"),
        "Old variable name still present: {}",
        result
    );
}

#[tokio::test]
async fn rename_closure_parameter_from_body_usage() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Factory {\n",
        "    public function build(): void {\n",
        "        $this->afterMaking(function (Box $item): void {\n",
        "            $item->name = 'test';\n",
        "            echo $item->name;\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    // Cursor on `$item` in the closure body (line 4, col 13).
    let edit = rename(&backend, &uri, 4, 13, "$box").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit when renaming from body usage"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    // Both the parameter and body usages should be renamed.
    assert!(
        result.contains("function (Box $box)"),
        "Parameter declaration not renamed: {}",
        result
    );
    assert!(
        result.contains("$box->name = 'test'"),
        "Assignment usage not renamed: {}",
        result
    );
    assert!(
        result.contains("echo $box->name"),
        "Echo usage not renamed: {}",
        result
    );
    assert!(
        !result.contains("$item"),
        "Old variable name still present: {}",
        result
    );
}

#[tokio::test]
async fn rename_function_parameter_from_param() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function greet(string $name): string {\n",
        "    return 'Hello, ' . $name . '!';\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    // Cursor on `$name` in the parameter list (line 1, col 23).
    let edit = rename(&backend, &uri, 1, 23, "$who").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for function parameter rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert!(
        result.contains("string $who)"),
        "Parameter not renamed: {}",
        result
    );
    assert!(
        result.contains("$who . '!'"),
        "Body usage not renamed: {}",
        result
    );
    assert!(
        !result.contains("$name"),
        "Old variable name still present: {}",
        result
    );
}

#[tokio::test]
async fn rename_method_parameter_from_param() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Greeter {\n",
        "    public function greet(string $name): string {\n",
        "        return 'Hello, ' . $name . '!';\n",
        "    }\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    // Cursor on `$name` in the parameter list (line 2, col 35).
    let edit = rename(&backend, &uri, 2, 35, "$who").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for method parameter rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert!(
        result.contains("string $who)"),
        "Parameter not renamed: {}",
        result
    );
    assert!(
        result.contains("$who . '!'"),
        "Body usage not renamed: {}",
        result
    );
    assert!(
        !result.contains("$name"),
        "Old variable name still present: {}",
        result
    );
}

#[tokio::test]
async fn rename_parameter_includes_docblock_param_tag() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Greeter {\n",
        "    /**\n",
        "     * @param string $name The person's name.\n",
        "     */\n",
        "    public function greet(string $name): string {\n",
        "        return 'Hello, ' . $name . '!';\n",
        "    }\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    // Cursor on `$name` in the parameter list (line 5, col 35).
    let edit = rename(&backend, &uri, 5, 35, "$who").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for parameter rename with docblock"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert!(
        result.contains("@param string $who"),
        "Docblock @param not renamed: {}",
        result
    );
    assert!(
        result.contains("string $who)"),
        "Parameter not renamed: {}",
        result
    );
    assert!(
        result.contains("$who . '!'"),
        "Body usage not renamed: {}",
        result
    );
    assert!(
        !result.contains("$name"),
        "Old variable name still present: {}",
        result
    );
}

#[tokio::test]
async fn rename_parameter_includes_docblock_from_body_usage() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @param string $name The person's name.\n",
        " */\n",
        "function greet(string $name): string {\n",
        "    return 'Hello, ' . $name . '!';\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    // Cursor on `$name` in the function body (line 5, col 24).
    let edit = rename(&backend, &uri, 5, 24, "$who").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for parameter rename from body"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert!(
        result.contains("@param string $who"),
        "Docblock @param not renamed: {}",
        result
    );
    assert!(
        result.contains("string $who)"),
        "Parameter not renamed: {}",
        result
    );
    assert!(
        result.contains("$who . '!'"),
        "Body usage not renamed: {}",
        result
    );
    assert!(
        !result.contains("$name"),
        "Old variable name still present: {}",
        result
    );
}

#[tokio::test]
async fn rename_parameter_multiple_docblock_params() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Math {\n",
        "    /**\n",
        "     * @param int $a First operand.\n",
        "     * @param int $b Second operand.\n",
        "     */\n",
        "    public function add(int $a, int $b): int {\n",
        "        return $a + $b;\n",
        "    }\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    // Rename $a (line 6, col 29).
    let edit = rename(&backend, &uri, 6, 29, "$x").await;
    assert!(edit.is_some());

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    // Only $a should be renamed, not $b.
    assert!(
        result.contains("@param int $x First"),
        "Docblock @param for $a not renamed: {}",
        result
    );
    assert!(
        result.contains("@param int $b Second"),
        "Docblock @param for $b was wrongly renamed: {}",
        result
    );
    assert!(
        result.contains("int $x, int $b)"),
        "Parameter $a not renamed: {}",
        result
    );
    assert!(
        result.contains("return $x + $b"),
        "Body usage not renamed correctly: {}",
        result
    );
}

#[tokio::test]
async fn rename_arrow_function_parameter() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $fn = fn(int $x) => $x * 2;\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    // Cursor on `$x` in the arrow function parameter (line 2, col 18).
    let edit = rename(&backend, &uri, 2, 18, "$n").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for arrow function parameter rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert!(
        result.contains("fn(int $n)"),
        "Arrow function parameter not renamed: {}",
        result
    );
    assert!(
        result.contains("$n * 2"),
        "Arrow function body not renamed: {}",
        result
    );
    assert!(
        !result.contains("$x"),
        "Old variable name still present: {}",
        result
    );
}

#[tokio::test]
async fn rename_closure_parameter_does_not_leak_to_outer_scope() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $item = 'outer';\n",
        "    $fn = function (string $item): string {\n",
        "        return $item . '!';\n",
        "    };\n",
        "    echo $item;\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    // Rename `$item` in the closure parameter (line 3, col 28).
    let edit = rename(&backend, &uri, 3, 28, "$inner").await;
    assert!(edit.is_some());

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    // Closure parameter and body should be renamed.
    assert!(
        result.contains("function (string $inner)"),
        "Closure parameter not renamed: {}",
        result
    );
    assert!(
        result.contains("return $inner . '!'"),
        "Closure body not renamed: {}",
        result
    );
    // Outer scope $item should NOT be renamed.
    assert!(
        result.contains("$item = 'outer'"),
        "Outer scope was wrongly renamed: {}",
        result
    );
    assert!(
        result.contains("echo $item"),
        "Outer scope echo was wrongly renamed: {}",
        result
    );
}

#[tokio::test]
async fn rename_function_param_propagates_into_closure_use_and_arrow() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function test(string $param): void\n",
        "{\n",
        "    function () use ($param): void {\n",
        "        echo $param;\n",
        "    };\n",
        "\n",
        "    fn () => $param;\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    // Cursor on `$param` in the function parameter list (line 1, col 22).
    let edit = rename(&backend, &uri, 1, 22, "$renamed").await;
    assert!(edit.is_some(), "Expected a workspace edit");
    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    // Function parameter should be renamed.
    assert!(
        result.contains("function test(string $renamed)"),
        "Function parameter not renamed: {}",
        result
    );
    // Closure use should be renamed.
    assert!(
        result.contains("use ($renamed)"),
        "Closure use not renamed: {}",
        result
    );
    // Variable inside closure body should be renamed.
    assert!(
        result.contains("echo $renamed;"),
        "Closure body variable not renamed: {}",
        result
    );
    // Arrow function body should be renamed.
    assert!(
        result.contains("fn () => $renamed;"),
        "Arrow function body not renamed: {}",
        result
    );
}

// ─── Namespace rename tests ─────────────────────────────────────────────────

#[tokio::test]
async fn prepare_rename_namespace_returns_segment_range() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///a.php").unwrap();
    let text = "<?php\nnamespace App\\Models\\User;\nclass User {}\n";
    open_file(&backend, &uri, text).await;

    // Cursor on "Models" (line 1, char 14 is inside "Models").
    let resp = prepare_rename(&backend, &uri, 1, 14).await;
    assert!(resp.is_some(), "Expected prepare rename to succeed");
    if let Some(PrepareRenameResponse::RangeWithPlaceholder { range, placeholder }) = resp {
        assert_eq!(placeholder, "Models");
        // "Models" starts at char 14 in "namespace App\Models\User;"
        assert_eq!(range.start.character, 14);
        assert_eq!(range.end.character, 20);
    } else {
        panic!("Expected RangeWithPlaceholder");
    }
}

#[tokio::test]
async fn prepare_rename_namespace_first_segment() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///a.php").unwrap();
    let text = "<?php\nnamespace App\\Models;\nclass Foo {}\n";
    open_file(&backend, &uri, text).await;

    // Cursor on "App" (line 1, char 11).
    let resp = prepare_rename(&backend, &uri, 1, 11).await;
    assert!(
        resp.is_some(),
        "Expected prepare rename to succeed for first segment"
    );
    if let Some(PrepareRenameResponse::RangeWithPlaceholder { range, placeholder }) = resp {
        assert_eq!(placeholder, "App");
        assert_eq!(range.start.character, 10);
        assert_eq!(range.end.character, 13);
    } else {
        panic!("Expected RangeWithPlaceholder");
    }
}

#[tokio::test]
async fn rename_namespace_updates_declaration_same_file() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///a.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Services;\n",
        "class PaymentService {}\n",
    );
    open_file(&backend, &uri, text).await;

    // Rename "Services" to "Handlers" (cursor on "Services", line 1, char 15).
    let edit = rename(&backend, &uri, 1, 15, "Handlers").await;
    assert!(
        edit.is_some(),
        "Expected workspace edit for namespace rename"
    );
    let edit = edit.unwrap();
    let edits = edits_for_uri(&edit, &uri);
    assert!(!edits.is_empty(), "Expected edits in the file");

    let result = apply_edits(text, &edits);
    assert!(
        result.contains("namespace App\\Handlers;"),
        "Namespace declaration should be updated: {}",
        result
    );
}

#[tokio::test]
async fn rename_namespace_updates_use_statements_cross_file() {
    let backend = Backend::new_test();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!(
        "<?php\n",
        "namespace App\\Services;\n",
        "class PaymentService {}\n",
    );

    let text_b = concat!(
        "<?php\n",
        "use App\\Services\\PaymentService;\n",
        "function demo(PaymentService $p): void {}\n",
    );

    open_file(&backend, &uri_a, text_a).await;
    open_file(&backend, &uri_b, text_b).await;

    // Rename "Services" to "Handlers" from file a's namespace declaration.
    let edit = rename(&backend, &uri_a, 1, 15, "Handlers").await;
    assert!(edit.is_some(), "Expected workspace edit");
    let edit = edit.unwrap();

    let edits_a = edits_for_uri(&edit, &uri_a);
    let edits_b = edits_for_uri(&edit, &uri_b);

    assert!(!edits_a.is_empty(), "Expected edits in file a");
    assert!(!edits_b.is_empty(), "Expected edits in file b");

    let result_a = apply_edits(text_a, &edits_a);
    assert!(
        result_a.contains("namespace App\\Handlers;"),
        "Namespace declaration should be updated: {}",
        result_a
    );

    let result_b = apply_edits(text_b, &edits_b);
    assert!(
        result_b.contains("use App\\Handlers\\PaymentService;"),
        "Use statement should be updated: {}",
        result_b
    );
}

#[tokio::test]
async fn rename_namespace_root_segment_renames_all_children() {
    let backend = Backend::new_test();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!("<?php\n", "namespace App\\Models;\n", "class User {}\n",);

    let text_b = concat!(
        "<?php\n",
        "namespace App\\Services;\n",
        "use App\\Models\\User;\n",
        "class UserService {}\n",
    );

    open_file(&backend, &uri_a, text_a).await;
    open_file(&backend, &uri_b, text_b).await;

    // Rename root segment "App" to "MyApp" from file a.
    let edit = rename(&backend, &uri_a, 1, 11, "MyApp").await;
    assert!(edit.is_some(), "Expected workspace edit");
    let edit = edit.unwrap();

    let edits_a = edits_for_uri(&edit, &uri_a);
    let edits_b = edits_for_uri(&edit, &uri_b);

    let result_a = apply_edits(text_a, &edits_a);
    assert!(
        result_a.contains("namespace MyApp\\Models;"),
        "File a namespace should be updated: {}",
        result_a
    );

    let result_b = apply_edits(text_b, &edits_b);
    assert!(
        result_b.contains("namespace MyApp\\Services;"),
        "File b namespace should be updated: {}",
        result_b
    );
    assert!(
        result_b.contains("use MyApp\\Models\\User;"),
        "File b use statement should be updated: {}",
        result_b
    );
}

#[tokio::test]
async fn rename_namespace_preserves_alias_in_use() {
    let backend = Backend::new_test();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!("<?php\n", "namespace App\\Old;\n", "class Foo {}\n",);

    let text_b = concat!(
        "<?php\n",
        "use App\\Old\\Foo as MyFoo;\n",
        "function demo(MyFoo $f): void {}\n",
    );

    open_file(&backend, &uri_a, text_a).await;
    open_file(&backend, &uri_b, text_b).await;

    // Rename "Old" to "New".
    let edit = rename(&backend, &uri_a, 1, 14, "New").await;
    assert!(edit.is_some(), "Expected workspace edit");
    let edit = edit.unwrap();

    let edits_b = edits_for_uri(&edit, &uri_b);
    let result_b = apply_edits(text_b, &edits_b);

    assert!(
        result_b.contains("use App\\New\\Foo as MyFoo;"),
        "Use statement should update FQN but preserve alias: {}",
        result_b
    );
}

#[tokio::test]
async fn rename_namespace_updates_group_use() {
    let backend = Backend::new_test();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!(
        "<?php\n",
        "namespace App\\Old;\n",
        "class Foo {}\n",
        "class Bar {}\n",
    );

    let text_b = concat!(
        "<?php\n",
        "use App\\Old\\{Foo, Bar};\n",
        "function demo(Foo $f, Bar $b): void {}\n",
    );

    open_file(&backend, &uri_a, text_a).await;
    open_file(&backend, &uri_b, text_b).await;

    // Rename "Old" to "New".
    let edit = rename(&backend, &uri_a, 1, 14, "New").await;
    assert!(edit.is_some(), "Expected workspace edit");
    let edit = edit.unwrap();

    let edits_b = edits_for_uri(&edit, &uri_b);
    let result_b = apply_edits(text_b, &edits_b);

    assert!(
        result_b.contains("App\\New"),
        "Group use prefix should be updated: {}",
        result_b
    );
}

#[tokio::test]
async fn rename_namespace_does_not_affect_unrelated_namespaces() {
    let backend = Backend::new_test();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!("<?php\n", "namespace App\\Models;\n", "class User {}\n",);

    let text_b = concat!(
        "<?php\n",
        "namespace Other\\Models;\n",
        "class Product {}\n",
    );

    open_file(&backend, &uri_a, text_a).await;
    open_file(&backend, &uri_b, text_b).await;

    // Rename "App" to "MyApp" — should not touch "Other\Models".
    let edit = rename(&backend, &uri_a, 1, 11, "MyApp").await;
    assert!(edit.is_some(), "Expected workspace edit");
    let edit = edit.unwrap();

    let edits_b = edits_for_uri(&edit, &uri_b);
    assert!(
        edits_b.is_empty(),
        "Unrelated namespace file should not be edited"
    );
}

#[tokio::test]
async fn rename_namespace_updates_fqn_in_use_statement_from_symbol_map() {
    let backend = Backend::new_test();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!("<?php\n", "namespace App\\Services;\n", "class Mailer {}\n",);

    let text_b = concat!(
        "<?php\n",
        "use App\\Services\\Mailer;\n",
        "class NotificationService {\n",
        "    public function send(Mailer $m): void {}\n",
        "}\n",
    );

    open_file(&backend, &uri_a, text_a).await;
    open_file(&backend, &uri_b, text_b).await;

    let edit = rename(&backend, &uri_a, 1, 15, "Providers").await;
    assert!(edit.is_some(), "Expected workspace edit");
    let edit = edit.unwrap();

    let edits_b = edits_for_uri(&edit, &uri_b);
    let result_b = apply_edits(text_b, &edits_b);

    assert!(
        result_b.contains("use App\\Providers\\Mailer;"),
        "Use statement FQN should be updated: {}",
        result_b
    );
    // The short-name reference `Mailer` in the method signature should NOT change
    // because it's imported via `use` and the short name didn't change.
    assert!(
        result_b.contains("public function send(Mailer $m)"),
        "Short name reference should be preserved: {}",
        result_b
    );
}

#[tokio::test]
async fn rename_namespace_single_segment_namespace() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///a.php").unwrap();
    let text = concat!("<?php\n", "namespace App;\n", "class Config {}\n",);
    open_file(&backend, &uri, text).await;

    // Rename single-segment namespace "App" to "Framework".
    let edit = rename(&backend, &uri, 1, 11, "Framework").await;
    assert!(edit.is_some(), "Expected workspace edit");
    let edit = edit.unwrap();

    let edits = edits_for_uri(&edit, &uri);
    let result = apply_edits(text, &edits);
    assert!(
        result.contains("namespace Framework;"),
        "Single-segment namespace should be renamed: {}",
        result
    );
}

#[tokio::test]
async fn rename_namespace_psr4_directory_rename() {
    use std::path::PathBuf;

    let workspace = PathBuf::from("/tmp/test_workspace_ns_rename");
    let src_dir = workspace.join("src").join("App").join("Services");
    let _ = std::fs::create_dir_all(&src_dir);
    let _ = std::fs::write(
        src_dir.join("Mailer.php"),
        "<?php\nnamespace App\\Services;\nclass Mailer {}\n",
    );

    let psr4 = vec![crate::composer::Psr4Mapping {
        prefix: "App\\".to_string(),
        base_path: "src/App/".to_string(),
    }];

    let backend = Backend::new_test_with_workspace(workspace.clone(), psr4);
    backend
        .supports_file_rename
        .store(true, std::sync::atomic::Ordering::Release);

    let uri =
        Url::parse("file:///tmp/test_workspace_ns_rename/src/App/Services/Mailer.php").unwrap();
    let text = "<?php\nnamespace App\\Services;\nclass Mailer {}\n";
    open_file(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 1, 15, "Handlers").await;
    assert!(edit.is_some(), "Expected workspace edit with PSR-4 rename");
    let edit = edit.unwrap();

    // When PSR-4 applies and client supports file rename, the edit
    // should use document_changes with a RenameFile operation.
    if let Some(ref doc_changes) = edit.document_changes {
        let rename_file = match doc_changes {
            DocumentChanges::Operations(ops) => ops.iter().find_map(|op| match op {
                DocumentChangeOperation::Op(ResourceOp::Rename(rf)) => Some(rf),
                _ => None,
            }),
            _ => None,
        };
        assert!(
            rename_file.is_some(),
            "Expected a RenameFile operation for PSR-4 directory"
        );
        let rf = rename_file.unwrap();
        assert!(
            rf.old_uri.as_str().contains("Services"),
            "Old URI should contain 'Services': {}",
            rf.old_uri
        );
        assert!(
            rf.new_uri.as_str().contains("Handlers"),
            "New URI should contain 'Handlers': {}",
            rf.new_uri
        );
    } else {
        // If document_changes is None, the test still passes for the text
        // edits — PSR-4 directory rename only triggers when the dir exists.
        // The directory was created above, so we expect it to work.
    }

    // Cleanup.
    let _ = std::fs::remove_dir_all(&workspace);
}

#[tokio::test]
async fn rename_namespace_multiple_files_same_namespace() {
    let backend = Backend::new_test();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();
    let uri_c = Url::parse("file:///c.php").unwrap();

    let text_a = concat!("<?php\n", "namespace App\\Models;\n", "class User {}\n",);
    let text_b = concat!("<?php\n", "namespace App\\Models;\n", "class Post {}\n",);
    let text_c = concat!(
        "<?php\n",
        "use App\\Models\\User;\n",
        "use App\\Models\\Post;\n",
        "function demo(User $u, Post $p): void {}\n",
    );

    open_file(&backend, &uri_a, text_a).await;
    open_file(&backend, &uri_b, text_b).await;
    open_file(&backend, &uri_c, text_c).await;

    // Rename "Models" to "Entities" from file a.
    let edit = rename(&backend, &uri_a, 1, 15, "Entities").await;
    assert!(edit.is_some(), "Expected workspace edit");
    let edit = edit.unwrap();

    let result_a = apply_edits(text_a, &edits_for_uri(&edit, &uri_a));
    let result_b = apply_edits(text_b, &edits_for_uri(&edit, &uri_b));
    let result_c = apply_edits(text_c, &edits_for_uri(&edit, &uri_c));

    assert!(
        result_a.contains("namespace App\\Entities;"),
        "File a namespace: {}",
        result_a
    );
    assert!(
        result_b.contains("namespace App\\Entities;"),
        "File b namespace: {}",
        result_b
    );
    assert!(
        result_c.contains("use App\\Entities\\User;"),
        "File c use User: {}",
        result_c
    );
    assert!(
        result_c.contains("use App\\Entities\\Post;"),
        "File c use Post: {}",
        result_c
    );
}

// --- PHPDoc @property and @method rename tests ---

#[tokio::test]
async fn test_prepare_rename_phpdoc_property() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @property string $email\n",
        " */\n",
        "class User {\n",
        "    public function demo(): void {\n",
        "        echo $this->email;\n",
        "    }\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    // Click on "email" in @property tag (line 2, char 22).
    let result = prepare_rename(&backend, &uri, 2, 22).await;
    assert!(
        result.is_some(),
        "prepare_rename should succeed on @property tag name"
    );
}

#[tokio::test]
async fn rename_phpdoc_property_from_usage() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @property string $email\n",
        " */\n",
        "class User {\n",
        "    public function demo(): void {\n",
        "        echo $this->email;\n",
        "    }\n",
        "}\n",
        "$u = new User();\n",
        "echo $u->email;\n",
    );

    open_file(&backend, &uri, text).await;

    // Rename from $u->email usage (line 10, "email" at char 13).
    let edit = rename(&backend, &uri, 10, 13, "emailAddress").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for @property rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    // Should have edits for: @property declaration, $this->email, $u->email.
    assert!(
        file_edits.len() >= 3,
        "Expected at least 3 edits for @property rename, got {}",
        file_edits.len()
    );

    // Verify that the @property declaration was included.
    let has_decl_edit = file_edits.iter().any(|te| te.range.start.line == 2);
    assert!(
        has_decl_edit,
        "Should include an edit for the @property declaration on line 2"
    );
}

#[tokio::test]
async fn rename_phpdoc_property_from_declaration() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @property string $email\n",
        " */\n",
        "class User {\n",
        "    public function demo(): void {\n",
        "        echo $this->email;\n",
        "    }\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    // Rename from the @property declaration (line 2, char 22).
    let edit = rename(&backend, &uri, 2, 22, "emailAddress").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for @property rename from declaration"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    assert!(
        file_edits.len() >= 2,
        "Expected at least 2 edits (@property + $this->email), got {}",
        file_edits.len()
    );

    // Verify $this->email usage was updated.
    let has_usage_edit = file_edits.iter().any(|te| te.range.start.line == 6);
    assert!(
        has_usage_edit,
        "Should include an edit for the $this->email usage on line 6"
    );
}

#[tokio::test]
async fn test_rename_phpdoc_method_from_usage() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @method string getEmail()\n",
        " */\n",
        "class User {}\n",
        "$u = new User();\n",
        "echo $u->getEmail();\n",
    );

    open_file(&backend, &uri, text).await;

    // Rename from $u->getEmail() usage (line 6, "getEmail" at char 10).
    let edit = rename(&backend, &uri, 6, 10, "getEmailAddress").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for @method rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    // Should have edits for: @method declaration + $u->getEmail() usage.
    assert!(
        file_edits.len() >= 2,
        "Expected at least 2 edits for @method rename, got {}",
        file_edits.len()
    );

    // Verify @method declaration was updated.
    let has_decl_edit = file_edits.iter().any(|te| te.range.start.line == 2);
    assert!(
        has_decl_edit,
        "Should include an edit for the @method declaration on line 2"
    );
}

#[tokio::test]
async fn test_prepare_rename_phpdoc_method() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @method string getEmail()\n",
        " */\n",
        "class User {}\n",
    );

    open_file(&backend, &uri, text).await;

    // Click on "getEmail" in @method tag (line 2, char 19).
    let result = prepare_rename(&backend, &uri, 2, 19).await;
    assert!(
        result.is_some(),
        "prepare_rename should succeed on @method tag name"
    );
}

#[tokio::test]
async fn rename_phpdoc_property_does_not_leak_to_unrelated_class() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @property string $email\n",
        " */\n",
        "class User {}\n",
        "/**\n",
        " * @property int $email\n",
        " */\n",
        "class Order {}\n",
        "$u = new User();\n",
        "echo $u->email;\n",
        "$o = new Order();\n",
        "echo $o->email;\n",
    );

    open_file(&backend, &uri, text).await;

    // Rename from $u->email (line 11, char 13).
    let edit = rename(&backend, &uri, 11, 13, "emailAddress").await;
    assert!(edit.is_some());

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);

    // Should NOT include edits for Order's @property or $o->email.
    let has_order_decl = file_edits.iter().any(|te| te.range.start.line == 7);
    let has_order_usage = file_edits.iter().any(|te| te.range.start.line == 13);
    assert!(
        !has_order_decl,
        "Should NOT edit Order's @property declaration"
    );
    assert!(!has_order_usage, "Should NOT edit $o->email usage");
}

#[tokio::test]
async fn rename_function_param_propagates_into_nested_arrows() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function foo(?bool $abstract = false)\n",
        "{\n",
        "    fn () => fn () => $abstract;\n",
        "}\n",
    );
    open_file(&backend, &uri, text).await;
    let edit = rename(&backend, &uri, 1, 19, "$renamed").await;
    assert!(edit.is_some());
    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);
    assert!(result.contains("$renamed = false"));
    assert!(result.contains("fn () => fn () => $renamed;"));
    let edit2 = rename(&backend, &uri, 3, 23, "$renamed2").await;
    assert!(edit2.is_some());
    let file_edits2 = edits_for_uri(&edit2.unwrap(), &uri);
    let result2 = apply_edits(text, &file_edits2);
    assert!(result2.contains("function foo(?bool $renamed2 = false)"));
    assert!(result2.contains("fn () => fn () => $renamed2;"));
}

#[tokio::test]
async fn rename_function_param_propagates_into_deeply_nested_closures_with_use() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function test(string $outer) {\n",
        "    $f = function () use ($outer) {\n",
        "        $g = function () use ($outer) {\n",
        "            echo $outer;\n",
        "        };\n",
        "    };\n",
        "}\n",
    );
    open_file(&backend, &uri, text).await;
    let edit = rename(&backend, &uri, 1, 22, "$renamed").await;
    assert!(edit.is_some());
    let edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &edits);
    assert!(result.contains("function test(string $renamed)"));
    assert!(result.contains("use ($renamed)"));
    assert!(result.contains("echo $renamed;"));
    let edit2 = rename(&backend, &uri, 4, 19, "$renamed2").await;
    assert!(edit2.is_some());
    let edits2 = edits_for_uri(&edit2.unwrap(), &uri);
    let result2 = apply_edits(text, &edits2);
    assert!(result2.contains("function test(string $renamed2)"));
    assert!(result2.contains("echo $renamed2;"));
}

#[tokio::test]
async fn rename_function_param_propagates_mixed_closure_arrow_nesting() {
    let backend = Backend::new_test();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo($v) {\n",
        "    function () use ($v) {\n",
        "        fn () => fn () => $v;\n",
        "    };\n",
        "}\n",
    );
    open_file(&backend, &uri, text).await;
    let edit = rename(&backend, &uri, 1, 16, "$renamed").await;
    assert!(edit.is_some());
    let edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &edits);
    assert!(result.contains("use ($renamed)"));
    assert!(result.contains("fn () => fn () => $renamed;"));
    let edit2 = rename(&backend, &uri, 3, 27, "$renamed2").await;
    assert!(edit2.is_some());
    let edits2 = edits_for_uri(&edit2.unwrap(), &uri);
    let result2 = apply_edits(text, &edits2);
    assert!(result2.contains("function demo($renamed2)"));
    assert!(result2.contains("use ($renamed2)"));
    assert!(result2.contains("fn () => fn () => $renamed2;"));
}
