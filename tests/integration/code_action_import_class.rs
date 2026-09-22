//! Integration tests for the "Import class" code action.
//!
//! These exercise the full pipeline: parse a file that names a class no
//! import, namespace, or local declaration resolves, register a candidate
//! by parsing the file that declares it, request code actions on the
//! name, and verify the offered import and its edit.

use crate::common::{
    create_test_backend, extract_edits, find_action, get_code_actions_at, get_code_actions_in_range,
};
use phpantom_lsp::Backend;
use tower_lsp::lsp_types::*;

const REQUEST_URI: &str = "file:///vendor/laravel/framework/src/Illuminate/Http/Request.php";
const CARBON_URI: &str = "file:///vendor/nesbot/carbon/src/Carbon/Carbon.php";

/// Make `Illuminate\Http\Request` an import candidate.
fn declare_request(backend: &Backend) {
    backend.update_ast(
        REQUEST_URI,
        "<?php\nnamespace Illuminate\\Http;\n\nclass Request {}\n",
    );
}

/// Make `Carbon\Carbon` an import candidate.
fn declare_carbon(backend: &Backend) {
    backend.update_ast(CARBON_URI, "<?php\nnamespace Carbon;\n\nclass Carbon {}\n");
}

fn range(line: u32, start: u32, end: u32) -> Range {
    Range::new(Position::new(line, start), Position::new(line, end))
}

fn titles(actions: &[CodeActionOrCommand]) -> Vec<String> {
    actions
        .iter()
        .map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title.clone(),
            CodeActionOrCommand::Command(c) => c.title.clone(),
        })
        .collect()
}

/// The titles of the "Import class" actions offered, leaving out the
/// "Import class … and shorten usages" rewrite of a qualified name.
fn import_titles(actions: &[CodeActionOrCommand]) -> Vec<String> {
    titles(actions)
        .into_iter()
        .filter(|t| t.starts_with("Import `"))
        .collect()
}

#[test]
fn import_action_offered_for_unresolved_class() {
    let backend = create_test_backend();
    declare_request(&backend);
    let uri = "file:///test.php";
    let content = "<?php\nnamespace App;\n\nnew Request();\n";
    backend.update_ast(uri, content);

    let actions = get_code_actions_in_range(&backend, uri, content, range(3, 4, 11));
    assert!(
        find_action(&actions, "Import `Illuminate\\Http\\Request`").is_some(),
        "expected an import action for Illuminate\\Http\\Request, got: {:?}",
        titles(&actions)
    );
}

/// A normal-mode cursor sends a point range; the first character of the
/// name counts as inside it.
#[test]
fn import_action_offered_for_point_request_at_first_character() {
    let backend = create_test_backend();
    declare_request(&backend);
    let uri = "file:///test.php";
    let content = "<?php\nnamespace App;\n\nnew Request();\n";
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 3, 4);
    assert!(
        find_action(&actions, "Import `Illuminate\\Http\\Request`").is_some(),
        "got: {:?}",
        titles(&actions)
    );
}

#[test]
fn import_action_offered_for_static_point_request_at_first_character() {
    let backend = create_test_backend();
    declare_carbon(&backend);
    let uri = "file:///test.php";
    let content = "<?php\nnamespace App;\n\nCarbon::now();\n";
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 3, 0);
    assert!(
        find_action(&actions, "Import `Carbon\\Carbon`").is_some(),
        "got: {:?}",
        titles(&actions)
    );
}

#[test]
fn no_import_action_when_already_imported() {
    let backend = create_test_backend();
    declare_request(&backend);
    let uri = "file:///test.php";
    let content = "<?php\nnamespace App;\n\nuse Illuminate\\Http\\Request;\n\nnew Request();\n";
    backend.update_ast(uri, content);

    let actions = get_code_actions_in_range(&backend, uri, content, range(5, 4, 11));
    assert!(
        import_titles(&actions).is_empty(),
        "should not offer import when already imported, got: {:?}",
        import_titles(&actions)
    );
}

#[test]
fn no_import_action_for_fqn_reference() {
    let backend = create_test_backend();
    declare_request(&backend);
    let uri = "file:///test.php";
    let content = "<?php\nnamespace App;\n\nnew \\Illuminate\\Http\\Request();\n";
    backend.update_ast(uri, content);

    let actions = get_code_actions_in_range(&backend, uri, content, range(3, 5, 35));
    assert!(
        import_titles(&actions).is_empty(),
        "should not offer import for FQN reference, got: {:?}",
        import_titles(&actions)
    );
}

#[test]
fn import_action_inserts_use_statement() {
    let backend = create_test_backend();
    declare_request(&backend);
    let uri = "file:///test.php";
    let content = "<?php\nnamespace App;\n\nnew Request();\n";
    backend.update_ast(uri, content);

    let actions = get_code_actions_in_range(&backend, uri, content, range(3, 4, 11));
    let action = find_action(&actions, "Import `Illuminate\\Http\\Request`")
        .expect("expected import action");

    let edits = extract_edits(action);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "\nuse Illuminate\\Http\\Request;\n");
}

/// A different class already imported under the same short name blocks
/// the import.
#[test]
fn import_skips_conflict_with_existing_import() {
    let backend = create_test_backend();
    declare_request(&backend);
    let uri = "file:///test.php";
    let content = "<?php\nnamespace App;\n\nuse Symfony\\Component\\HttpFoundation\\Request;\n\nnew Request();\n";
    backend.update_ast(uri, content);

    let actions = get_code_actions_in_range(&backend, uri, content, range(5, 4, 11));
    assert!(
        find_action(&actions, "Import `Illuminate\\Http\\Request`").is_none(),
        "should not offer conflicting import, got: {:?}",
        titles(&actions)
    );
}

// ── Files without a namespace ────────────────────────────────────────

#[test]
fn import_action_offered_in_no_namespace_file_for_new_expression() {
    let backend = create_test_backend();
    declare_request(&backend);
    let uri = "file:///test.php";
    let content = "<?php\n\nnew Request();\n";
    backend.update_ast(uri, content);

    let actions = get_code_actions_in_range(&backend, uri, content, range(2, 4, 11));
    assert!(
        find_action(&actions, "Import `Illuminate\\Http\\Request`").is_some(),
        "expected an import action in a no-namespace file, got: {:?}",
        titles(&actions)
    );
}

/// Reproduces issue #59: a static call on an unimported class in a file
/// without a namespace.
#[test]
fn import_action_offered_in_no_namespace_file_for_static_call() {
    let backend = create_test_backend();
    declare_carbon(&backend);
    let uri = "file:///test.php";
    let content = "<?php\n\nfunction () {\n    return Carbon::now();\n};\n";
    backend.update_ast(uri, content);

    let actions = get_code_actions_in_range(&backend, uri, content, range(3, 11, 17));
    assert!(
        find_action(&actions, "Import `Carbon\\Carbon`").is_some(),
        "expected an import action for Carbon\\Carbon in a no-namespace file, got: {:?}",
        titles(&actions)
    );
}

#[test]
fn import_action_inserts_use_after_php_open_in_no_namespace_file() {
    let backend = create_test_backend();
    declare_request(&backend);
    let uri = "file:///test.php";
    let content = "<?php\n\nnew Request();\n";
    backend.update_ast(uri, content);

    let actions = get_code_actions_in_range(&backend, uri, content, range(2, 4, 11));
    let action = find_action(&actions, "Import `Illuminate\\Http\\Request`")
        .expect("expected import action");

    let edits = extract_edits(action);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "use Illuminate\\Http\\Request;\n");
    // After `<?php` (line 1), not at line 0.
    assert_eq!(edits[0].range.start.line, 1);
}

#[test]
fn no_import_action_for_known_global_class_in_no_namespace_file() {
    let backend = create_test_backend();
    backend.update_ast("file:///dep.php", "<?php\nclass Helper {}\n");
    let uri = "file:///test.php";
    let content = "<?php\n\nnew Helper();\n";
    backend.update_ast(uri, content);

    let actions = get_code_actions_in_range(&backend, uri, content, range(2, 4, 10));
    assert!(
        import_titles(&actions).is_empty(),
        "should not offer import for a known global class, got: {:?}",
        import_titles(&actions)
    );
}

/// Reproduces issue #59: a parsed `Carbon\Carbon` must not satisfy the
/// global-scope lookup for the bare name `Carbon`, or the import is never
/// offered because the name looks resolved.
#[test]
fn import_action_offered_when_namespaced_class_in_uri_classes_index() {
    let backend = create_test_backend();
    declare_carbon(&backend);
    let uri = "file:///test.php";
    let content = "<?php\n\nfunction () {\n    return Carbon::now();\n};\n";
    backend.update_ast(uri, content);

    let actions = get_code_actions_in_range(&backend, uri, content, range(3, 11, 17));
    assert!(
        find_action(&actions, "Import `Carbon\\Carbon`").is_some(),
        "expected an import action for Carbon\\Carbon, got: {:?}",
        titles(&actions)
    );
}
