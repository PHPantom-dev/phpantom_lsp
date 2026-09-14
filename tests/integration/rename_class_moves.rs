//! Class renames that move the file on disk, and moves whose target is
//! already taken.

use crate::common::{
    apply_edits, create_test_backend, doc_change_edits_for_uri, edits_for_uri, extract_rename_file,
    initialize_with_resource_operations, open_php, rename, rename_result,
};
use phpantom_lsp::Backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── File Rename on Class Rename ────────────────────────────────────────────

#[tokio::test]
async fn rename_class_renames_file_when_psr4_match() {
    // When the filename matches the class name and the client supports
    // file renames, a RenameFile operation should be included.
    let backend = create_test_backend();
    initialize_with_resource_operations(&backend).await;

    let uri = Url::parse("file:///src/Foo.php").unwrap();
    let text = concat!("<?php\n", "namespace App;\n", "\n", "class Foo {}\n",);

    open_php(&backend, &uri, text).await;

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
    let backend = create_test_backend();
    initialize_with_resource_operations(&backend).await;

    let uri = Url::parse("file:///src/helpers.php").unwrap();
    let text = concat!("<?php\n", "namespace App;\n", "\n", "class Foo {}\n",);

    open_php(&backend, &uri, text).await;

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
    let backend = create_test_backend();
    initialize_with_resource_operations(&backend).await;

    let uri = Url::parse("file:///src/Foo.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class Foo {}\n",
        "class Extra {}\n",
    );

    open_php(&backend, &uri, text).await;

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
    let backend = create_test_backend();
    // supports_file_rename is false by default.

    let uri = Url::parse("file:///src/Foo.php").unwrap();
    let text = concat!("<?php\n", "namespace App;\n", "\n", "class Foo {}\n",);

    open_php(&backend, &uri, text).await;

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
    let backend = create_test_backend();
    initialize_with_resource_operations(&backend).await;

    let uri_decl = Url::parse("file:///src/TaskResource.php").unwrap();
    let uri_usage = Url::parse("file:///src/Task.php").unwrap();

    let text_decl = concat!(
        "<?php\n",
        "namespace Acme\\Tasks\\Resources;\n",
        "\n",
        "class TaskResource {}\n",
    );

    let text_usage = concat!(
        "<?php\n",
        "namespace Acme\\Tasks;\n",
        "\n",
        "use Acme\\Tasks\\Resources\\TaskResource;\n",
        "\n",
        "class Task {\n",
        "    public function resource(): TaskResource {\n",
        "        return new TaskResource();\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri_decl, text_decl).await;
    open_php(&backend, &uri_usage, text_usage).await;

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
        result_usage.contains("use Acme\\Tasks\\Resources\\TaskDto;"),
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
    let backend = create_test_backend();
    initialize_with_resource_operations(&backend).await;

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

    open_php(&backend, &uri_decl, text_decl).await;
    open_php(&backend, &uri_usage, text_usage).await;

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
    let backend = create_test_backend();
    initialize_with_resource_operations(&backend).await;

    let uri = Url::parse("file:///src/Widget.php").unwrap();
    let text = concat!("<?php\n", "class Widget {}\n",);

    open_php(&backend, &uri, text).await;

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

// ─── Moves onto an already-occupied target ──────────────────────────────────

/// A backend over a real on-disk PSR-4 workspace (`App\` → `src/`), with
/// every file indexed and client file-rename support enabled.  The
/// PSR-4 file-move logic stats the filesystem, so these cases cannot be
/// exercised against synthetic `file:///` URIs.
async fn psr4_move_workspace(files: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let backend = create_test_backend();
    initialize_with_resource_operations(&backend).await;
    *backend.workspace_root().write() = Some(dir.path().to_path_buf());
    *backend.psr4_mappings().write() = vec![phpantom_lsp::composer::Psr4Mapping {
        prefix: "App\\".to_string(),
        base_path: "src".to_string(),
    }];

    for (rel, content) in files {
        let full = dir.path().join(rel);
        std::fs::create_dir_all(full.parent().expect("parent")).expect("mkdir");
        std::fs::write(&full, content).expect("write");
        let uri = Url::from_file_path(&full).expect("uri");
        open_php(&backend, &uri, content).await;
    }

    (backend, dir)
}

fn ws_uri(dir: &tempfile::TempDir, rel: &str) -> Url {
    Url::from_file_path(dir.path().join(rel)).expect("uri")
}

/// Every `(old, new)` file operation a workspace edit carries.
fn file_moves(edit: &WorkspaceEdit) -> Vec<(String, String)> {
    let Some(DocumentChanges::Operations(ops)) = &edit.document_changes else {
        return Vec::new();
    };
    ops.iter()
        .filter_map(|op| match op {
            DocumentChangeOperation::Op(ResourceOp::Rename(r)) => {
                Some((r.old_uri.to_string(), r.new_uri.to_string()))
            }
            _ => None,
        })
        .collect()
}

/// Every URI a workspace edit targets with text edits.
fn edited_uris(edit: &WorkspaceEdit) -> Vec<String> {
    match &edit.document_changes {
        Some(DocumentChanges::Operations(ops)) => ops
            .iter()
            .filter_map(|op| match op {
                DocumentChangeOperation::Edit(e) => Some(e.text_document.uri.to_string()),
                _ => None,
            })
            .collect(),
        _ => edit
            .changes
            .iter()
            .flat_map(|c| c.keys())
            .map(|u| u.to_string())
            .collect(),
    }
}

#[tokio::test]
async fn class_move_onto_an_existing_class_is_refused_with_no_edits() {
    // The destination namespace already declares the name, so the move
    // would leave two classes claiming it.  Nothing is emitted.
    let (backend, dir) = psr4_move_workspace(&[
        (
            "src/Internal/Helper.php",
            "<?php\nnamespace App\\Internal;\n\nclass Helper {}\n",
        ),
        (
            "src/Support/Helper.php",
            "<?php\nnamespace App\\Support;\n\nclass Helper {}\n",
        ),
    ])
    .await;

    let uri = ws_uri(&dir, "src/Internal/Helper.php");
    let result = rename_result(&backend, &uri, 3, 8, "App\\Support\\Helper").await;

    let message = result.expect_err("the move should be refused");
    assert!(
        message.contains("App\\Support\\Helper") && message.contains("already"),
        "the refusal should name the class in the way: {message}"
    );
    assert!(
        std::fs::read_to_string(dir.path().join("src/Support/Helper.php"))
            .expect("the existing file should be untouched")
            .contains("namespace App\\Support;")
    );
}

#[tokio::test]
async fn class_move_onto_an_existing_file_is_refused() {
    // Nothing declares `App\Support\Helper`, but PSR-4 puts it in a file
    // that is already there, so the move would clobber it.
    let (backend, dir) = psr4_move_workspace(&[(
        "src/Internal/Helper.php",
        "<?php\nnamespace App\\Internal;\n\nclass Helper {}\n",
    )])
    .await;

    std::fs::create_dir_all(dir.path().join("src/Support")).expect("mkdir");
    std::fs::write(
        dir.path().join("src/Support/Helper.php"),
        "<?php\n// notes\n",
    )
    .expect("write");

    let uri = ws_uri(&dir, "src/Internal/Helper.php");
    let result = rename_result(&backend, &uri, 3, 8, "App\\Support\\Helper").await;

    let message = result.expect_err("the move should be refused");
    assert!(
        message.contains("already exists"),
        "the refusal should say the file is in the way: {message}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("src/Support/Helper.php")).expect("read"),
        "<?php\n// notes\n",
    );
}

#[tokio::test]
async fn class_move_to_a_free_name_still_works() {
    let (backend, dir) = psr4_move_workspace(&[
        (
            "src/Internal/Helper.php",
            "<?php\nnamespace App\\Internal;\n\nclass Helper {}\n",
        ),
        (
            "src/Support/Existing.php",
            "<?php\nnamespace App\\Support;\n\nclass Existing {}\n",
        ),
    ])
    .await;

    let uri = ws_uri(&dir, "src/Internal/Helper.php");
    let ws = rename(&backend, &uri, 3, 8, "App\\Support\\Helper")
        .await
        .expect("expected an edit");

    let moves = file_moves(&ws);
    assert_eq!(moves.len(), 1, "expected one file move, got {moves:?}");
    assert!(
        moves[0].1.ends_with("/src/Support/Helper.php"),
        "got {moves:?}"
    );
}

#[tokio::test]
async fn namespace_rename_into_an_existing_namespace_merges_file_by_file() {
    // `App\Internal` merges into an `App\Support` that already exists.
    // A directory rename would clobber or fail, so each file moves on
    // its own and the files already there are left alone.
    let (backend, dir) = psr4_move_workspace(&[
        (
            "src/Internal/Helper.php",
            "<?php\nnamespace App\\Internal;\n\nclass Helper {}\n",
        ),
        (
            "src/Internal/Nested/Deep.php",
            "<?php\nnamespace App\\Internal\\Nested;\n\nclass Deep {}\n",
        ),
        (
            "src/Support/Existing.php",
            "<?php\nnamespace App\\Support;\n\nclass Existing {}\n",
        ),
    ])
    .await;

    let uri = ws_uri(&dir, "src/Internal/Helper.php");
    let ws = rename(&backend, &uri, 1, 12, "App\\Support")
        .await
        .expect("expected an edit");

    let moves = file_moves(&ws);
    assert!(
        !moves
            .iter()
            .any(|(old, _)| old.ends_with("/src/Internal") || old.ends_with("/src/Support")),
        "the directory itself must not be renamed onto an existing one: {moves:?}"
    );

    let destinations: Vec<&str> = moves.iter().map(|(_, new)| new.as_str()).collect();
    assert!(
        destinations
            .iter()
            .any(|d| d.ends_with("/src/Support/Helper.php")),
        "got {destinations:?}"
    );
    assert!(
        destinations
            .iter()
            .any(|d| d.ends_with("/src/Support/Nested/Deep.php")),
        "a nested file keeps its relative path: {destinations:?}"
    );
    assert!(
        !destinations
            .iter()
            .any(|d| d.ends_with("/src/Support/Existing.php")),
        "the file already there is not touched: {destinations:?}"
    );

    // Every edited file is one the move actually produces.
    for edited in edited_uris(&ws) {
        assert!(
            !edited.contains("/src/Internal/"),
            "an edit still targets the old location: {edited}"
        );
    }
}

#[tokio::test]
async fn namespace_rename_onto_a_clashing_name_is_refused_with_no_edits() {
    let (backend, dir) = psr4_move_workspace(&[
        (
            "src/Internal/Helper.php",
            "<?php\nnamespace App\\Internal;\n\nclass Helper {}\n",
        ),
        (
            "src/Internal/Parser.php",
            "<?php\nnamespace App\\Internal;\n\nclass Parser {}\n",
        ),
        (
            "src/Support/Helper.php",
            "<?php\nnamespace App\\Support;\n\nclass Helper {}\n",
        ),
    ])
    .await;

    let uri = ws_uri(&dir, "src/Internal/Helper.php");
    let result = rename_result(&backend, &uri, 1, 12, "App\\Support").await;

    let message = result.expect_err("the merge should be refused");
    assert!(
        message.contains("App\\Support\\Helper"),
        "the refusal should name the clash: {message}"
    );
    assert!(
        message.contains("App\\Internal") && message.contains("App\\Support"),
        "the refusal should name both namespaces: {message}"
    );
}

#[tokio::test]
async fn namespace_rename_to_a_fresh_namespace_still_moves_the_directory() {
    // Nothing exists at the destination, so the whole directory moves in
    // one operation, as it did before merging was possible.
    let (backend, dir) = psr4_move_workspace(&[(
        "src/Internal/Helper.php",
        "<?php\nnamespace App\\Internal;\n\nclass Helper {}\n",
    )])
    .await;

    let uri = ws_uri(&dir, "src/Internal/Helper.php");
    let ws = rename(&backend, &uri, 1, 12, "App\\Core")
        .await
        .expect("expected an edit");

    let moves = file_moves(&ws);
    assert_eq!(moves.len(), 1, "expected one directory move: {moves:?}");
    assert!(moves[0].0.ends_with("/src/Internal"), "got {moves:?}");
    assert!(moves[0].1.ends_with("/src/Core"), "got {moves:?}");
}

#[tokio::test]
async fn namespace_rename_does_not_capture_a_sibling_directory_with_the_same_prefix() {
    // `src/Internal` must not claim the edits of `src/InternalTools`.
    let (backend, dir) = psr4_move_workspace(&[
        (
            "src/Internal/Helper.php",
            "<?php\nnamespace App\\Internal;\n\nclass Helper {}\n",
        ),
        (
            "src/InternalTools/Runner.php",
            concat!(
                "<?php\n",
                "namespace App\\InternalTools;\n",
                "\n",
                "use App\\Internal\\Helper;\n",
                "\n",
                "class Runner {\n",
                "    public function h(): Helper {\n",
                "        return new Helper();\n",
                "    }\n",
                "}\n",
            ),
        ),
    ])
    .await;

    let uri = ws_uri(&dir, "src/Internal/Helper.php");
    let ws = rename(&backend, &uri, 1, 12, "App\\Core")
        .await
        .expect("expected an edit");

    assert!(
        edited_uris(&ws)
            .iter()
            .any(|u| u.ends_with("/src/InternalTools/Runner.php")),
        "the sibling's edits must stay at its own path: {:?}",
        edited_uris(&ws)
    );
}

#[tokio::test]
async fn rename_class_updates_phpstan_type_and_import_type_tags() {
    // A class named inside a `@phpstan-type` definition or after a
    // `@phpstan-import-type`'s `from` is a real reference to it, so a
    // rename has to carry both along or the alias silently goes stale.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                       // 0
        "namespace App;\n",                              // 1
        "\n",                                            // 2
        "class User {}\n",                               // 3
        "\n",                                            // 4
        "/**\n",                                         // 5
        " * @phpstan-type UserRow array{owner: User}\n", // 6
        " * @phpstan-import-type Row from User\n",       // 7
        " */\n",                                         // 8
        "class Repo {}\n",                               // 9
    );

    open_php(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 3, 6, "Account")
        .await
        .expect("expected a workspace edit");
    let result = apply_edits(text, &edits_for_uri(&edit, &uri));

    assert!(
        result.contains("@phpstan-type UserRow array{owner: Account}"),
        "the alias definition should follow the rename:\n{result}"
    );
    assert!(
        result.contains("@phpstan-import-type Row from Account"),
        "the imported-from class should follow the rename:\n{result}"
    );
    assert!(
        result.contains("UserRow"),
        "the alias name is not the class and must not be rewritten:\n{result}"
    );
}

/// A PSR-4 workspace whose Blade templates are opened as templates, so
/// the preprocessor lowers them the way the editor would.
async fn blade_move_workspace(files: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let backend = create_test_backend();
    initialize_with_resource_operations(&backend).await;
    *backend.workspace_root().write() = Some(dir.path().to_path_buf());
    *backend.psr4_mappings().write() = vec![phpantom_lsp::composer::Psr4Mapping {
        prefix: "App\\".to_string(),
        base_path: "src".to_string(),
    }];

    for (rel, content) in files {
        let full = dir.path().join(rel);
        std::fs::create_dir_all(full.parent().expect("parent")).expect("mkdir");
        std::fs::write(&full, content).expect("write");
        let uri = Url::from_file_path(&full).expect("uri");
        let language_id = if rel.ends_with(".blade.php") {
            "blade"
        } else {
            "php"
        };
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri,
                    language_id: language_id.to_string(),
                    version: 1,
                    text: content.to_string(),
                },
            })
            .await;
    }

    (backend, dir)
}

/// Every way a template can name a class, in the shapes a real Laravel
/// view uses them: the `@use` directive, an import and a docblock inside
/// an `@php` block, an inline fully-qualified name, and one written
/// inside a directive's argument.
const TEMPLATE_NAMING_A_CLASS: &str = concat!(
    "@use('App\\Internal\\Widget')\n",
    "@php\n",
    "    /** @var \\App\\Internal\\Widget $widget */\n",
    "    $widget = new \\App\\Internal\\Widget();\n",
    "@endphp\n",
    "<p>{{ Widget::label() }}</p>\n",
    "<script>value = @json(\\App\\Internal\\Widget::label());</script>\n",
);

/// The other spelling of an import a template can carry: a `use` written
/// inside an `@php` block, which the preprocessor leaves where it stands
/// rather than hoisting.
const TEMPLATE_IMPORTING_IN_A_PHP_BLOCK: &str = concat!(
    "@php\n",
    "    use App\\Internal\\Widget as Alias;\n",
    "@endphp\n",
    "<p>{{ Alias::label() }}</p>\n",
);

#[tokio::test]
async fn a_namespace_move_rewrites_every_name_a_template_spells() {
    let (backend, dir) = blade_move_workspace(&[
        (
            "src/Internal/Widget.php",
            "<?php\nnamespace App\\Internal;\n\nclass Widget {\n    public static function label(): string { return ''; }\n}\n",
        ),
        ("resources/views/panel.blade.php", TEMPLATE_NAMING_A_CLASS),
    ])
    .await;

    let uri = ws_uri(&dir, "src/Internal/Widget.php");
    let ws = rename(&backend, &uri, 1, 12, "App\\Core")
        .await
        .expect("expected an edit");

    let view = ws_uri(&dir, "resources/views/panel.blade.php");
    let result = apply_edits(TEMPLATE_NAMING_A_CLASS, &edits_for_uri(&ws, &view));

    assert!(
        !result.contains("App\\Internal"),
        "the template must not still name the namespace it moved out of:\n{result}"
    );
    for expected in [
        "@use('App\\Core\\Widget')",
        "@var \\App\\Core\\Widget $widget",
        "new \\App\\Core\\Widget()",
        "@json(\\App\\Core\\Widget::label())",
    ] {
        assert!(
            result.contains(expected),
            "expected {expected:?} in:\n{result}"
        );
    }
}

#[tokio::test]
async fn an_open_template_no_longer_abandons_the_whole_rename() {
    // The template's symbol map describes the virtual PHP it lowers to,
    // which is longer than the file on disk.  Checking it against the
    // template's own bytes made the length guard fire and dropped every
    // file's edits, so even the moved namespace's own declaration was
    // left alone.
    let (backend, dir) = blade_move_workspace(&[
        (
            "src/Internal/Widget.php",
            "<?php\nnamespace App\\Internal;\n\nclass Widget {}\n",
        ),
        (
            "resources/views/panel.blade.php",
            "<p>nothing to do with the move</p>\n",
        ),
    ])
    .await;

    let uri = ws_uri(&dir, "src/Internal/Widget.php");
    let ws = rename(&backend, &uri, 1, 12, "App\\Core")
        .await
        .expect("expected an edit");

    // The declaration moves with the namespace, so its edits target the
    // path the file lands on rather than the one it left.
    let declaration = apply_edits(
        "<?php\nnamespace App\\Internal;\n\nclass Widget {}\n",
        &edits_for_uri(&ws, &ws_uri(&dir, "src/Core/Widget.php")),
    );
    assert!(
        declaration.contains("namespace App\\Core;"),
        "the declaration itself has to be rewritten:\n{declaration}"
    );
}

#[tokio::test]
async fn a_class_rename_carries_a_templates_use_directive_with_it() {
    // The preprocessor hoists `@use` into the prologue, which translates
    // back to no position in the template.  Rewriting the short names the
    // import binds without rewriting the import leaves the template
    // naming a class that no longer exists.
    let (backend, dir) = blade_move_workspace(&[
        (
            "src/Internal/Widget.php",
            "<?php\nnamespace App\\Internal;\n\nclass Widget {\n    public static function label(): string { return ''; }\n}\n",
        ),
        ("resources/views/panel.blade.php", TEMPLATE_NAMING_A_CLASS),
    ])
    .await;

    let uri = ws_uri(&dir, "src/Internal/Widget.php");
    let ws = rename(&backend, &uri, 3, 8, "Gadget")
        .await
        .expect("expected an edit");

    let view = ws_uri(&dir, "resources/views/panel.blade.php");
    let result = apply_edits(TEMPLATE_NAMING_A_CLASS, &edits_for_uri(&ws, &view));

    assert!(
        !result.contains("Widget"),
        "the old name must not survive anywhere in the template:\n{result}"
    );
    assert!(
        result.contains("@use('App\\Internal\\Gadget')"),
        "the import has to follow the rename:\n{result}"
    );
    assert!(
        result.contains("{{ Gadget::label() }}"),
        "the short name the import binds has to follow it:\n{result}"
    );
}

#[tokio::test]
async fn a_class_move_carries_a_templates_names_to_the_new_namespace() {
    let (backend, dir) = blade_move_workspace(&[
        (
            "src/Internal/Widget.php",
            "<?php\nnamespace App\\Internal;\n\nclass Widget {\n    public static function label(): string { return ''; }\n}\n",
        ),
        ("resources/views/panel.blade.php", TEMPLATE_NAMING_A_CLASS),
    ])
    .await;

    let uri = ws_uri(&dir, "src/Internal/Widget.php");
    let ws = rename(&backend, &uri, 3, 8, "App\\Core\\Widget")
        .await
        .expect("expected an edit");

    let view = ws_uri(&dir, "resources/views/panel.blade.php");
    let result = apply_edits(TEMPLATE_NAMING_A_CLASS, &edits_for_uri(&ws, &view));

    assert!(
        !result.contains("App\\Internal"),
        "the template must not still name the old namespace:\n{result}"
    );
    assert!(
        result.contains("@use('App\\Core\\Widget')"),
        "the import has to follow the move:\n{result}"
    );
}

#[tokio::test]
async fn a_member_rename_lands_on_the_templates_own_line() {
    // A method renamed from a template's call site has to come back
    // through the source map: the offsets the reference finder produces
    // index the virtual PHP, whose prologue the template never wrote.
    let (backend, dir) = blade_move_workspace(&[
        (
            "src/Internal/Widget.php",
            "<?php\nnamespace App\\Internal;\n\nclass Widget {\n    public static function label(): string { return ''; }\n}\n",
        ),
        ("resources/views/panel.blade.php", TEMPLATE_NAMING_A_CLASS),
    ])
    .await;

    let uri = ws_uri(&dir, "src/Internal/Widget.php");
    let ws = rename(&backend, &uri, 4, 28, "caption")
        .await
        .expect("expected an edit");

    let view = ws_uri(&dir, "resources/views/panel.blade.php");
    let result = apply_edits(TEMPLATE_NAMING_A_CLASS, &edits_for_uri(&ws, &view));

    assert!(
        result.contains("{{ Widget::caption() }}"),
        "the call in the template has to be rewritten in place:\n{result}"
    );
    assert!(
        result.contains("@json(\\App\\Internal\\Widget::caption())"),
        "so does the one inside a directive's argument:\n{result}"
    );
}

#[tokio::test]
async fn a_namespace_move_rewrites_an_import_inside_a_php_block() {
    let (backend, dir) = blade_move_workspace(&[
        (
            "src/Internal/Widget.php",
            "<?php\nnamespace App\\Internal;\n\nclass Widget {\n    public static function label(): string { return \'\'; }\n}\n",
        ),
        (
            "resources/views/panel.blade.php",
            TEMPLATE_IMPORTING_IN_A_PHP_BLOCK,
        ),
    ])
    .await;

    let uri = ws_uri(&dir, "src/Internal/Widget.php");
    let ws = rename(&backend, &uri, 1, 12, "App\\Core")
        .await
        .expect("expected an edit");

    let view = ws_uri(&dir, "resources/views/panel.blade.php");
    let result = apply_edits(
        TEMPLATE_IMPORTING_IN_A_PHP_BLOCK,
        &edits_for_uri(&ws, &view),
    );

    assert!(
        result.contains("    use App\\Core\\Widget as Alias;"),
        "the import has to follow the move, indentation and all:\n{result}"
    );
    assert!(
        result.contains("{{ Alias::label() }}"),
        "the alias names the same class and must be left alone:\n{result}"
    );
}
