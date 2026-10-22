//! Edits into a file saved with `\r\n` line endings end the lines they add
//! with `\r\n` too, whichever feature built them.

use crate::common::{
    apply_edits, apply_workspace_edit, complete_at, create_test_backend, extract_edits,
    find_action, find_action_titled, get_code_actions_at, get_code_actions_in_range,
    resolve_action,
};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// Assert every line break in `result` is `\r\n`.
fn assert_crlf_only(result: &str) {
    assert!(
        !result.replace("\r\n", "").contains('\n'),
        "every line should end with \\r\\n:\n{result:?}"
    );
}

#[test]
fn generated_constructor_uses_crlf() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\r\nclass Foo {\r\n    public string $name;\r\n}\r\n";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_action_titled(&actions, "Generate constructor")
        .expect("should offer generate constructor action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(result.contains("__construct(string $name)"), "{result}");
    assert_crlf_only(&result);
}

#[test]
fn import_class_action_uses_crlf() {
    let backend = create_test_backend();
    backend.update_ast(
        "file:///vendor/acme/Widget.php",
        "<?php\nnamespace Acme;\n\nclass Widget {}\n",
    );
    let uri = "file:///test.php";
    let content = "<?php\r\nnamespace App;\r\n\r\nnew Widget();\r\n";
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 3, 6);
    let action =
        find_action(&actions, "Import `Acme\\Widget`").expect("should offer the import action");
    let result = apply_edits(content, &extract_edits(action));

    assert!(result.contains("use Acme\\Widget;"), "{result}");
    assert_crlf_only(&result);
}

#[test]
fn extracted_function_uses_crlf() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\r\nclass Foo {\r\n    public function run(): void {\r\n        $a = 1;\r\n        echo $a;\r\n    }\r\n}\r\n";
    backend.update_ast(uri, content);

    let range = Range::new(Position::new(3, 8), Position::new(4, 16));
    let actions = get_code_actions_in_range(&backend, uri, content, range);
    let action = find_action(&actions, "Extract function").expect("should offer extract function");
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edits(content, &extract_edits(&resolved));

    assert!(result.contains("function renderRun()"), "{result}");
    assert_crlf_only(&result);
}

#[tokio::test]
async fn completion_auto_import_uses_crlf() {
    let backend = create_test_backend();
    backend.update_ast(
        "file:///vendor/acme/Widget.php",
        "<?php\nnamespace Acme;\n\nclass Widget {}\n",
    );
    let uri = Url::parse("file:///test.php").unwrap();
    let content = "<?php\r\nnamespace App;\r\n\r\nnew Widg\r\n";

    let items = complete_at(&backend, &uri, content, 3, 8).await;
    let widget = items
        .iter()
        .find(|i| i.detail.as_deref() == Some("Acme\\Widget"))
        .expect("should offer Acme\\Widget");
    let edits = widget
        .additional_text_edits
        .as_ref()
        .expect("should carry the import edit");
    let result = apply_edits(content, edits);

    assert!(result.contains("use Acme\\Widget;"), "{result}");
    assert_crlf_only(&result);
}

#[tokio::test]
async fn docblock_on_enter_uses_crlf() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let content = "<?php\r\n/** */\r\nfunction greet($name): string {}\r\n";
    crate::common::open_php(&backend, &uri, content).await;

    let edits = backend
        .on_type_formatting(DocumentOnTypeFormattingParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position::new(1, 4),
            },
            ch: "\n".to_string(),
            options: FormattingOptions {
                tab_size: 4,
                insert_spaces: true,
                ..FormattingOptions::default()
            },
        })
        .await
        .unwrap()
        .expect("should generate a docblock");
    let result = apply_edits(content, &edits);

    assert!(result.contains("@param"), "{result}");
    assert_crlf_only(&result);
}
