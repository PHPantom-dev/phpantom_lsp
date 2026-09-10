//! Integration tests for the "Create missing view" code action.

use crate::common::create_psr4_workspace;
use tower_lsp::lsp_types::*;

const COMPOSER: &str = r#"{
    "autoload": { "psr-4": { "App\\": "app/" } }
}"#;

/// Helper: send a code action request covering the given line.
fn get_code_actions(
    backend: &phpantom_lsp::Backend,
    uri: &str,
    content: &str,
    line: u32,
) -> Vec<CodeActionOrCommand> {
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(line, 0),
            end: Position::new(line, 200),
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

fn find_create_missing_view(actions: &[CodeActionOrCommand]) -> Option<&CodeAction> {
    actions.iter().find_map(|a| match a {
        CodeActionOrCommand::CodeAction(ca) if ca.title.starts_with("Create missing view") => {
            Some(ca)
        }
        _ => None,
    })
}

const CONTROLLER: &str = "<?php\nnamespace App\\Http\\Controllers;\n\nclass HomeController\n{\n    public function index(): string\n    {\n        return view('missing.page');\n    }\n}\n";

#[test]
fn offered_and_falls_back_to_conventional_dir_when_none_exists() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[("app/HomeController.php", CONTROLLER)]);

    let uri_path = dir.path().join("app/HomeController.php");
    let uri = Url::from_file_path(&uri_path).unwrap();
    backend.update_ast(uri.as_str(), CONTROLLER);

    let actions = get_code_actions(&backend, uri.as_str(), CONTROLLER, 7);
    let action = find_create_missing_view(&actions).expect("should offer the action");

    let edit = action.edit.as_ref().expect("action should carry an edit");
    let ops = match edit.document_changes.as_ref().expect("document_changes") {
        DocumentChanges::Operations(ops) => ops,
        DocumentChanges::Edits(_) => panic!("expected operations, not plain edits"),
    };
    assert_eq!(ops.len(), 1);
    match &ops[0] {
        DocumentChangeOperation::Op(ResourceOp::Create(cf)) => {
            let path = cf.uri.to_file_path().unwrap();
            let expected = dir.path().join("resources/views/missing/page.blade.php");
            assert_eq!(path, expected);
            assert_eq!(cf.options.as_ref().unwrap().overwrite, Some(false));
            assert_eq!(cf.options.as_ref().unwrap().ignore_if_exists, Some(true));
        }
        _ => panic!("expected a CreateFile operation"),
    }
}

#[test]
fn offered_under_an_existing_view_root() {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/HomeController.php", CONTROLLER),
            ("resources/views/unrelated.blade.php", "<p>hi</p>\n"),
        ],
    );

    let uri_path = dir.path().join("app/HomeController.php");
    let uri = Url::from_file_path(&uri_path).unwrap();
    backend.update_ast(uri.as_str(), CONTROLLER);

    let actions = get_code_actions(&backend, uri.as_str(), CONTROLLER, 7);
    let action = find_create_missing_view(&actions).expect("should offer the action");

    let edit = action.edit.as_ref().expect("action should carry an edit");
    let ops = match edit.document_changes.as_ref().expect("document_changes") {
        DocumentChanges::Operations(ops) => ops,
        DocumentChanges::Edits(_) => panic!("expected operations, not plain edits"),
    };
    match &ops[0] {
        DocumentChangeOperation::Op(ResourceOp::Create(cf)) => {
            let path = cf.uri.to_file_path().unwrap();
            let expected = dir.path().join("resources/views/missing/page.blade.php");
            assert_eq!(path, expected);
        }
        _ => panic!("expected a CreateFile operation"),
    }
}

#[test]
fn not_offered_when_the_view_already_resolves() {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/HomeController.php", CONTROLLER),
            ("resources/views/missing/page.blade.php", "<p>hi</p>\n"),
        ],
    );

    let uri_path = dir.path().join("app/HomeController.php");
    let uri = Url::from_file_path(&uri_path).unwrap();
    backend.update_ast(uri.as_str(), CONTROLLER);

    let actions = get_code_actions(&backend, uri.as_str(), CONTROLLER, 7);
    assert!(
        find_create_missing_view(&actions).is_none(),
        "should not offer to create a view that already exists"
    );
}

#[test]
fn not_offered_when_cursor_is_elsewhere() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[("app/HomeController.php", CONTROLLER)]);

    let uri_path = dir.path().join("app/HomeController.php");
    let uri = Url::from_file_path(&uri_path).unwrap();
    backend.update_ast(uri.as_str(), CONTROLLER);

    // Line 3 is the class declaration, nowhere near the view() call.
    let actions = get_code_actions(&backend, uri.as_str(), CONTROLLER, 3);
    assert!(find_create_missing_view(&actions).is_none());
}
