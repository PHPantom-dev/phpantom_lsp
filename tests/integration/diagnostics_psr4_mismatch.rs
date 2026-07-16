//! Tests for the PSR-4 namespace and class-name mismatch diagnostics
//! and their quick fixes.

use crate::common::{create_psr4_workspace, create_psr4_workspace_with_exception_stubs};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER: &str = r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#;

/// Build a `file://` URI for a path relative to the workspace root and
/// collect fast diagnostics for the given content.
fn fast_diagnostics(
    backend: &phpantom_lsp::Backend,
    dir: &std::path::Path,
    rel_path: &str,
    content: &str,
) -> (String, Vec<Diagnostic>) {
    let full = dir.join(rel_path);
    let uri = Url::from_file_path(&full).unwrap().to_string();
    backend.update_ast(&uri, content);
    let mut out = Vec::new();
    backend.collect_namespace_mismatch_diagnostics(&uri, content, &mut out);
    backend.collect_class_name_mismatch_diagnostics(&uri, content, &mut out);
    (uri, out)
}

fn has_code(diags: &[Diagnostic], code: &str) -> bool {
    diags
        .iter()
        .any(|d| d.code == Some(NumberOrString::String(code.to_string())))
}

/// The canonical `App\Models\User` declaration, written to disk so PSR-4
/// resolution can find it when a consumer references it.
const USER_MODEL: &str = "<?php\nnamespace App\\Models;\nclass User {}\n";

/// Update a consumer file's AST and collect class-case-mismatch
/// diagnostics for it.
fn case_diagnostics(
    backend: &phpantom_lsp::Backend,
    dir: &std::path::Path,
    rel_path: &str,
    content: &str,
) -> Vec<Diagnostic> {
    let full = dir.join(rel_path);
    let uri = Url::from_file_path(&full).unwrap().to_string();
    backend.update_ast(&uri, content);
    let mut out = Vec::new();
    backend.collect_class_case_mismatch_diagnostics(&uri, content, &mut out);
    out
}

#[test]
fn namespace_mismatch_flagged_and_matching_is_clean() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[]);

    // Wrong namespace for the PSR-4 path.
    let (_, diags) = fast_diagnostics(
        &backend,
        dir.path(),
        "src/Models/User.php",
        "<?php\nnamespace App\\Wrong;\nclass User {}\n",
    );
    assert!(
        has_code(&diags, "namespace_mismatch"),
        "expected namespace_mismatch, got: {diags:?}"
    );

    // Correct namespace.
    let (_, diags) = fast_diagnostics(
        &backend,
        dir.path(),
        "src/Models/User.php",
        "<?php\nnamespace App\\Models;\nclass User {}\n",
    );
    assert!(
        !has_code(&diags, "namespace_mismatch"),
        "matching namespace should be clean, got: {diags:?}"
    );
}

#[test]
fn class_name_mismatch_flagged_and_matching_is_clean() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[]);

    // Class name does not match the filename.
    let (_, diags) = fast_diagnostics(
        &backend,
        dir.path(),
        "src/Models/User.php",
        "<?php\nnamespace App\\Models;\nclass Customer {}\n",
    );
    assert!(
        has_code(&diags, "class_name_mismatch"),
        "expected class_name_mismatch, got: {diags:?}"
    );

    // Class name matches the filename.
    let (_, diags) = fast_diagnostics(
        &backend,
        dir.path(),
        "src/Models/User.php",
        "<?php\nnamespace App\\Models;\nclass User {}\n",
    );
    assert!(
        !has_code(&diags, "class_name_mismatch"),
        "matching class name should be clean, got: {diags:?}"
    );
}

/// A file that is not under any PSR-4 mapping must not be flagged for
/// either mismatch. PSR-4's filename/namespace rules only apply inside
/// autoloaded roots; standalone scripts are exempt.
#[test]
fn no_mismatch_for_file_outside_psr4_roots() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[]);

    let (_, diags) = fast_diagnostics(
        &backend,
        dir.path(),
        "scripts/legacy.php",
        "<?php\nnamespace Whatever;\nclass DoesNotMatch {}\n",
    );
    assert!(
        !has_code(&diags, "class_name_mismatch"),
        "class name check must not fire outside PSR-4 roots, got: {diags:?}"
    );
    assert!(
        !has_code(&diags, "namespace_mismatch"),
        "namespace check must not fire outside PSR-4 roots, got: {diags:?}"
    );
}

/// A project without any PSR-4 mappings (no composer autoload) must not
/// produce class-name mismatch warnings. This is the regression guard:
/// the class-name check must be gated on PSR-4 membership exactly like
/// the namespace check.
#[test]
fn no_class_name_mismatch_without_psr4_mappings() {
    let (backend, dir) = create_psr4_workspace(r#"{}"#, &[]);
    assert!(
        backend.psr4_mappings().read().is_empty(),
        "test precondition: no PSR-4 mappings"
    );

    let (_, diags) = fast_diagnostics(
        &backend,
        dir.path(),
        "src/Models/User.php",
        "<?php\nclass Customer {}\n",
    );
    assert!(
        !has_code(&diags, "class_name_mismatch"),
        "class name check must not fire without PSR-4 mappings, got: {diags:?}"
    );
}

#[test]
fn fix_namespace_quick_fix_corrects_declaration() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[]);

    let content = "<?php\nnamespace App\\Wrong;\nclass User {}\n";
    let full = dir.path().join("src/Models/User.php");
    let uri = Url::from_file_path(&full).unwrap();
    let uri_str = uri.to_string();
    backend.update_ast(&uri_str, content);

    // Cursor on the namespace line.
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range: Range {
            start: Position {
                line: 1,
                character: 10,
            },
            end: Position {
                line: 1,
                character: 10,
            },
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = backend.handle_code_action(&uri_str, content, &params);
    let fix = actions.iter().find_map(|a| match a {
        CodeActionOrCommand::CodeAction(ca) if ca.title.starts_with("Fix namespace") => Some(ca),
        _ => None,
    });
    let fix = fix.expect("expected a Fix namespace quick fix");
    assert_eq!(fix.title, "Fix namespace to `App\\Models`");

    let edits = fix
        .edit
        .as_ref()
        .and_then(|e| e.changes.as_ref())
        .and_then(|c| c.get(&uri))
        .expect("expected edits for the file");
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "App\\Models");
}

#[test]
fn fix_class_name_quick_fix_corrects_declaration() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[]);

    let content = "<?php\nnamespace App\\Models;\nclass Customer {}\n";
    let full = dir.path().join("src/Models/User.php");
    let uri = Url::from_file_path(&full).unwrap();
    let uri_str = uri.to_string();
    backend.update_ast(&uri_str, content);

    // Cursor on the class declaration line.
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range: Range {
            start: Position {
                line: 2,
                character: 6,
            },
            end: Position {
                line: 2,
                character: 6,
            },
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = backend.handle_code_action(&uri_str, content, &params);
    let fix = actions.iter().find_map(|a| match a {
        CodeActionOrCommand::CodeAction(ca) if ca.title.starts_with("Fix class name") => Some(ca),
        _ => None,
    });
    let fix = fix.expect("expected a Fix class name quick fix");
    assert_eq!(fix.title, "Fix class name to `User`");

    let edits = fix
        .edit
        .as_ref()
        .and_then(|e| e.changes.as_ref())
        .and_then(|c| c.get(&uri))
        .expect("expected edits for the file");
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "User");
}

#[tokio::test]
async fn class_case_mismatch_flagged_for_wrong_case_use_import() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[("src/Models/User.php", USER_MODEL)]);
    backend.initialized(InitializedParams {}).await;

    let diags = case_diagnostics(
        &backend,
        dir.path(),
        "src/Consumers/Service.php",
        "<?php\nnamespace App\\Consumers;\nuse App\\Models\\user;\nclass Service {\n    public function make(): user { return new user(); }\n}\n",
    );

    // Exactly one diagnostic: on the `use` import. The inline references
    // inherit their casing from the import and must not be double-flagged.
    let case_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("class_case_mismatch".to_string())))
        .collect();
    assert_eq!(
        case_diags.len(),
        1,
        "expected exactly one case diagnostic (on the use import), got: {diags:?}"
    );
    assert!(case_diags[0].message.contains("App\\Models\\User"));
}

#[tokio::test]
async fn class_case_mismatch_flagged_for_inline_fqn() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[("src/Models/User.php", USER_MODEL)]);
    backend.initialized(InitializedParams {}).await;

    let diags = case_diagnostics(
        &backend,
        dir.path(),
        "src/Consumers/Service.php",
        "<?php\nnamespace App\\Consumers;\nclass Service {\n    public function make() { return new \\App\\Models\\USER(); }\n}\n",
    );
    assert!(
        has_code(&diags, "class_case_mismatch"),
        "expected inline FQN case mismatch, got: {diags:?}"
    );
}

#[tokio::test]
async fn class_case_mismatch_flagged_for_same_namespace_reference() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[("src/Models/User.php", USER_MODEL)]);
    backend.initialized(InitializedParams {}).await;

    // A sibling class in the same namespace referencing `user` (no import)
    // resolves via the current namespace; the casing is literal here.
    let diags = case_diagnostics(
        &backend,
        dir.path(),
        "src/Models/Repo.php",
        "<?php\nnamespace App\\Models;\nclass Repo {\n    public function u() { return new user(); }\n}\n",
    );
    assert!(
        has_code(&diags, "class_case_mismatch"),
        "expected same-namespace case mismatch, got: {diags:?}"
    );
}

#[tokio::test]
async fn no_case_mismatch_for_correct_casing() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[("src/Models/User.php", USER_MODEL)]);
    backend.initialized(InitializedParams {}).await;

    let diags = case_diagnostics(
        &backend,
        dir.path(),
        "src/Consumers/Service.php",
        "<?php\nnamespace App\\Consumers;\nuse App\\Models\\User;\nclass Service {\n    public function make(): User { return new User(); }\n}\n",
    );
    assert!(
        !has_code(&diags, "class_case_mismatch"),
        "correct casing must be clean, got: {diags:?}"
    );
}

#[tokio::test]
async fn no_case_mismatch_for_same_file_reference() {
    // A class referenced in the same file it is declared in is already
    // loaded, so its casing never reaches the autoloader.
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[]);
    backend.initialized(InitializedParams {}).await;
    let diags = case_diagnostics(
        &backend,
        dir.path(),
        "src/Widget.php",
        "<?php\nnamespace App;\nclass Widget {}\nclass Uses {\n    public function w(): widget { return new widget(); }\n}\n",
    );
    assert!(
        !has_code(&diags, "class_case_mismatch"),
        "same-file references must not be flagged, got: {diags:?}"
    );
}

#[tokio::test]
async fn no_case_mismatch_for_builtin_stub_class() {
    // Built-in classes are case-insensitive and always loaded (not PSR-4
    // autoloaded), so a lowercase reference must not be flagged.
    let (backend, dir) = create_psr4_workspace_with_exception_stubs(COMPOSER, &[]);
    backend.initialized(InitializedParams {}).await;
    let diags = case_diagnostics(
        &backend,
        dir.path(),
        "src/Boom.php",
        "<?php\nnamespace App;\nclass Boom {\n    public function go() { throw new \\exception('x'); }\n}\n",
    );
    assert!(
        !has_code(&diags, "class_case_mismatch"),
        "built-in stub classes must not be flagged, got: {diags:?}"
    );
}

#[tokio::test]
async fn fix_class_case_quick_fix_corrects_use_import() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[("src/Models/User.php", USER_MODEL)]);
    backend.initialized(InitializedParams {}).await;

    let content = "<?php\nnamespace App\\Consumers;\nuse App\\Models\\user;\nclass Service {\n    public function make(): user { return new user(); }\n}\n";
    let full = dir.path().join("src/Consumers/Service.php");
    let uri = Url::from_file_path(&full).unwrap();
    let uri_str = uri.to_string();
    backend.update_ast(&uri_str, content);

    // Cursor on the `use` import target (line 2, inside `user`).
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range: Range {
            start: Position {
                line: 2,
                character: 16,
            },
            end: Position {
                line: 2,
                character: 16,
            },
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = backend.handle_code_action(&uri_str, content, &params);
    let fix = actions.iter().find_map(|a| match a {
        CodeActionOrCommand::CodeAction(ca) if ca.title.starts_with("Fix case") => Some(ca),
        _ => None,
    });
    let fix = fix.expect("expected a Fix case quick fix on the use import");
    assert_eq!(fix.title, "Fix case to `App\\Models\\User`");

    let edits = fix
        .edit
        .as_ref()
        .and_then(|e| e.changes.as_ref())
        .and_then(|c| c.get(&uri))
        .expect("expected edits for the file");
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "App\\Models\\User");
}
