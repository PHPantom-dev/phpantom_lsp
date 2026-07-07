//! Integration tests for the "Extract interface" code action.

use std::sync::Arc;

use crate::common::create_test_backend;
use tower_lsp::lsp_types::*;

/// Helper: send a code action request at the given line/character.
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

/// Find the "Extract interface" code action from a list.
fn find_extract_interface(actions: &[CodeActionOrCommand]) -> Option<&CodeAction> {
    actions.iter().find_map(|a| match a {
        CodeActionOrCommand::CodeAction(ca) if ca.title == "Extract interface" => Some(ca),
        _ => None,
    })
}

#[test]
fn offered_on_class_with_public_methods() {
    let content = r#"<?php
namespace App\Models;

class User
{
    public function getName(): string
    {
        return 'test';
    }

    public function setName(string $name): void
    {
    }
}
"#;

    let uri = "file:///tmp/User.php";
    let backend = create_test_backend();

    // Cursor on the class body.
    backend.update_ast(uri, content);
    let actions = get_code_actions(&backend, uri, content, 5, 4);
    let action = find_extract_interface(&actions);
    assert!(action.is_some(), "Should offer Extract interface");
}

#[test]
fn not_offered_on_interface() {
    let content = r#"<?php
interface UserInterface
{
    public function getName(): string;
}
"#;

    let uri = "file:///tmp/UserInterface.php";
    let backend = create_test_backend();

    backend.update_ast(uri, content);
    let actions = get_code_actions(&backend, uri, content, 3, 4);
    let action = find_extract_interface(&actions);
    assert!(action.is_none(), "Should not offer on interfaces");
}

#[test]
fn not_offered_without_public_methods() {
    let content = r#"<?php
class User
{
    private function secret(): void {}
}
"#;

    let uri = "file:///tmp/User.php";
    let backend = create_test_backend();

    backend.update_ast(uri, content);
    let actions = get_code_actions(&backend, uri, content, 3, 4);
    let action = find_extract_interface(&actions);
    assert!(action.is_none(), "Should not offer without public methods");
}

#[test]
fn resolve_creates_interface_file() {
    let content = r#"<?php
namespace App\Models;

class User
{
    public function getName(): string
    {
        return 'test';
    }

    public static function create(string $name): self
    {
        return new self();
    }
}
"#;

    let uri = "file:///tmp/User.php";
    let backend = create_test_backend();

    backend.update_ast(uri, content);
    let actions = get_code_actions(&backend, uri, content, 5, 4);
    let action = find_extract_interface(&actions).expect("should have action");

    // Resolve the deferred action.
    backend
        .open_files()
        .write()
        .insert(uri.to_string(), Arc::new(content.to_string()));
    let (resolved, _) = backend.resolve_code_action(action.clone());
    let edit = resolved.edit.expect("should produce edit");

    let doc_changes = edit.document_changes.expect("should have document_changes");
    match doc_changes {
        DocumentChanges::Operations(ops) => {
            // Should have CreateFile + two TextDocumentEdits.
            assert_eq!(ops.len(), 3);

            // First op: CreateFile.
            match &ops[0] {
                DocumentChangeOperation::Op(ResourceOp::Create(cf)) => {
                    assert!(
                        cf.uri.as_str().ends_with("UserInterface.php"),
                        "new file should be UserInterface.php, got: {}",
                        cf.uri
                    );
                }
                _ => panic!("First op should be CreateFile"),
            }

            // Second op: TextDocumentEdit writing interface content.
            match &ops[1] {
                DocumentChangeOperation::Edit(tde) => {
                    let text = &tde.edits[0];
                    let new_text = match text {
                        OneOf::Left(te) => &te.new_text,
                        _ => panic!("expected TextEdit"),
                    };
                    assert!(new_text.contains("interface UserInterface"));
                    assert!(new_text.contains("namespace App\\Models;"));
                    assert!(new_text.contains("public function getName(): string;"));
                    assert!(
                        new_text.contains("public static function create(string $name): self;")
                    );
                }
                _ => panic!("Second op should be TextDocumentEdit"),
            }

            // Third op: TextDocumentEdit adding implements clause.
            match &ops[2] {
                DocumentChangeOperation::Edit(tde) => {
                    let text = match &tde.edits[0] {
                        OneOf::Left(te) => &te.new_text,
                        _ => panic!("expected TextEdit"),
                    };
                    assert!(text.contains("implements UserInterface"));
                }
                _ => panic!("Third op should be TextDocumentEdit"),
            }
        }
        _ => panic!("expected Operations variant"),
    }
}
