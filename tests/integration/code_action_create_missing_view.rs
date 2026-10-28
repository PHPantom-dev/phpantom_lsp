//! Integration tests for the "Create missing view" code action.

use crate::common::{create_psr4_workspace, find_action, get_code_actions_in_range};
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "autoload": { "psr-4": { "App\\": "app/" } }
}"#;

/// Helper: send a code action request covering the given line.
fn actions_on_line(
    backend: &phpantom_lsp::Backend,
    uri: &str,
    content: &str,
    line: u32,
) -> Vec<CodeActionResponse> {
    get_code_actions_in_range(
        backend,
        uri,
        content,
        Range::new(Position::new(line, 0), Position::new(line, 200)),
    )
}

const CONTROLLER: &str = "<?php\nnamespace App\\Http\\Controllers;\n\nclass HomeController\n{\n    public function index(): string\n    {\n        return view('missing.page');\n    }\n}\n";

#[test]
fn offered_and_falls_back_to_conventional_dir_when_none_exists() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[("app/HomeController.php", CONTROLLER)]);

    let uri_path = dir.path().join("app/HomeController.php");
    let uri = Uri::from_file_path(&uri_path).unwrap();
    backend.update_ast(uri.as_str(), CONTROLLER);

    let actions = actions_on_line(&backend, uri.as_str(), CONTROLLER, 7);
    let action = find_action(&actions, "Create missing view").expect("should offer the action");

    let edit = action.edit.as_ref().expect("action should carry an edit");
    let ops = edit.document_changes.as_ref().expect("document_changes");
    assert_eq!(ops.len(), 1);
    match &ops[0] {
        DocumentChange::CreateFile(cf) => {
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
    let uri = Uri::from_file_path(&uri_path).unwrap();
    backend.update_ast(uri.as_str(), CONTROLLER);

    let actions = actions_on_line(&backend, uri.as_str(), CONTROLLER, 7);
    let action = find_action(&actions, "Create missing view").expect("should offer the action");

    let edit = action.edit.as_ref().expect("action should carry an edit");
    let ops = edit.document_changes.as_ref().expect("document_changes");
    match &ops[0] {
        DocumentChange::CreateFile(cf) => {
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
    let uri = Uri::from_file_path(&uri_path).unwrap();
    backend.update_ast(uri.as_str(), CONTROLLER);

    let actions = actions_on_line(&backend, uri.as_str(), CONTROLLER, 7);
    assert!(
        find_action(&actions, "Create missing view").is_none(),
        "should not offer to create a view that already exists"
    );
}

#[test]
fn not_offered_when_cursor_is_elsewhere() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[("app/HomeController.php", CONTROLLER)]);

    let uri_path = dir.path().join("app/HomeController.php");
    let uri = Uri::from_file_path(&uri_path).unwrap();
    backend.update_ast(uri.as_str(), CONTROLLER);

    // Line 3 is the class declaration, nowhere near the view() call.
    let actions = actions_on_line(&backend, uri.as_str(), CONTROLLER, 3);
    assert!(find_action(&actions, "Create missing view").is_none());
}

const TYPO_CONTROLLER: &str = "<?php\nnamespace App\\Http\\Controllers;\n\nclass MailController\n{\n    public function index(): string\n    {\n        return view('emails.welcom');\n    }\n}\n";

#[test]
fn a_close_view_name_is_offered_before_creating_one() {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/MailController.php", TYPO_CONTROLLER),
            ("resources/views/emails/welcome.blade.php", "<p>hi</p>\n"),
            ("resources/views/emails/goodbye.blade.php", "<p>bye</p>\n"),
        ],
    );

    let uri_path = dir.path().join("app/MailController.php");
    let uri = Uri::from_file_path(&uri_path).unwrap();
    backend.update_ast(uri.as_str(), TYPO_CONTROLLER);

    let actions = actions_on_line(&backend, uri.as_str(), TYPO_CONTROLLER, 7);
    let titles: Vec<&str> = actions
        .iter()
        .filter_map(|a| match a {
            CodeActionResponse::CodeAction(ca) => Some(ca.title.as_str()),
            _ => None,
        })
        .filter(|t| t.starts_with("Change to") || t.starts_with("Create missing view"))
        .collect();
    assert_eq!(
        titles,
        vec![
            "Change to 'emails.welcome'",
            "Create missing view 'emails.welcom'"
        ]
    );

    let change = find_action(&actions, "Change to 'emails.welcome'").unwrap();
    assert_eq!(change.is_preferred, Some(true));
    let edits = change
        .edit
        .as_ref()
        .and_then(|e| e.changes.as_ref())
        .and_then(|c| c.get(&uri))
        .expect("an edit to the controller");
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "emails.welcome");
    assert_eq!(
        edits[0].range,
        Range::new(Position::new(7, 21), Position::new(7, 34))
    );

    let create = find_action(&actions, "Create missing view").unwrap();
    assert_eq!(create.is_preferred, None);
}

#[test]
fn no_name_is_offered_when_none_is_close() {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/HomeController.php", CONTROLLER),
            ("resources/views/unrelated.blade.php", "<p>hi</p>\n"),
        ],
    );

    let uri_path = dir.path().join("app/HomeController.php");
    let uri = Uri::from_file_path(&uri_path).unwrap();
    backend.update_ast(uri.as_str(), CONTROLLER);

    let actions = actions_on_line(&backend, uri.as_str(), CONTROLLER, 7);
    assert!(find_action(&actions, "Change to").is_none());
    let create = find_action(&actions, "Create missing view").unwrap();
    assert_eq!(create.is_preferred, Some(true));
}
