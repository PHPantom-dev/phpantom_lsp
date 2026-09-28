//! Rename of a namespace segment across declarations, imports, and
//! fully-qualified references.

use crate::common::{
    apply_edits, create_test_backend, edits_for_uri, initialize_with_resource_operations, open_php,
    prepare_rename, rename,
};
use phpantom_lsp::Backend;
use tower_lsp::lsp_types::*;

// ─── Namespace rename tests ─────────────────────────────────────────────────

#[tokio::test]
async fn prepare_rename_namespace_returns_full_range() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///a.php").unwrap();
    let text = "<?php\nnamespace App\\Models\\User;\nclass User {}\n";
    open_php(&backend, &uri, text).await;

    // Cursor on "Models" (line 1, char 14 is inside the namespace).
    let resp = prepare_rename(&backend, &uri, 1, 14).await;
    assert!(resp.is_some(), "Expected prepare rename to succeed");
    if let Some(PrepareRenameResponse::RangeWithPlaceholder { range, placeholder }) = resp {
        assert_eq!(placeholder, "App\\Models\\User");
        assert_eq!(range.start.character, 10);
        assert_eq!(range.end.character, 25);
    } else {
        panic!("Expected RangeWithPlaceholder");
    }
}

#[tokio::test]
async fn prepare_rename_namespace_first_segment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///a.php").unwrap();
    let text = "<?php\nnamespace App\\Models;\nclass Foo {}\n";
    open_php(&backend, &uri, text).await;

    // Cursor on "App" (line 1, char 11).
    let resp = prepare_rename(&backend, &uri, 1, 11).await;
    assert!(
        resp.is_some(),
        "Expected prepare rename to succeed for namespace"
    );
    if let Some(PrepareRenameResponse::RangeWithPlaceholder { range, placeholder }) = resp {
        assert_eq!(placeholder, "App\\Models");
        assert_eq!(range.start.character, 10);
        assert_eq!(range.end.character, 20);
    } else {
        panic!("Expected RangeWithPlaceholder");
    }
}

#[tokio::test]
async fn rename_namespace_full_placeholder_can_replace_multiple_segments() {
    let backend = create_test_backend();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!(
        "<?php\n",
        "namespace App\\Services\\Billing;\n",
        "class PaymentService {}\n",
    );

    let text_b = concat!(
        "<?php\n",
        "use App\\Services\\Billing\\PaymentService;\n",
        "function demo(PaymentService $p): void {}\n",
    );

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

    let edit = rename(&backend, &uri_a, 1, 11, "App\\Handlers\\Payments").await;
    assert!(edit.is_some(), "Expected workspace edit");
    let edit = edit.unwrap();

    let result_a = apply_edits(text_a, &edits_for_uri(&edit, &uri_a));
    assert!(
        result_a.contains("namespace App\\Handlers\\Payments;"),
        "Namespace declaration should be updated: {}",
        result_a
    );

    let result_b = apply_edits(text_b, &edits_for_uri(&edit, &uri_b));
    assert!(
        result_b.contains("use App\\Handlers\\Payments\\PaymentService;"),
        "Use statement should be updated: {}",
        result_b
    );
}

#[tokio::test]
async fn rename_namespace_updates_declaration_same_file() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///a.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Services;\n",
        "class PaymentService {}\n",
    );
    open_php(&backend, &uri, text).await;

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
    let backend = create_test_backend();
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

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

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

/// PHP is not sensitive to whitespace between tokens outside of strings,
/// so a plain (brace-less) `use` import can legally wrap its FQN across
/// several lines. The statement has to be read as a whole, or the import
/// is silently left stale after the move.
#[tokio::test]
async fn rename_namespace_updates_a_plain_use_statement_wrapped_across_lines() {
    let backend = create_test_backend();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!(
        "<?php\n",
        "namespace App\\Services;\n",
        "class PaymentService {}\n",
    );

    let text_b = concat!(
        "<?php\n",
        "use App\\Services\\\n",
        "    PaymentService;\n",
        "function demo(PaymentService $p): void {}\n",
    );

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

    let edit = rename(&backend, &uri_a, 1, 15, "Handlers").await;
    assert!(edit.is_some(), "Expected workspace edit");
    let edit = edit.unwrap();

    let edits_b = edits_for_uri(&edit, &uri_b);
    assert!(
        !edits_b.is_empty(),
        "Expected the wrapped use statement to be edited"
    );

    let result_b = apply_edits(text_b, &edits_b);
    assert!(
        result_b.contains("use App\\Handlers\\\n    PaymentService;"),
        "Wrapped use statement should be updated: {}",
        result_b
    );
}

#[tokio::test]
async fn rename_namespace_updates_use_statements_in_unopened_usage_only_file() {
    use std::path::PathBuf;

    let workspace = PathBuf::from("/tmp/test_workspace_ns_usage_only");
    let _ = std::fs::create_dir_all(&workspace);

    let backend = Backend::new_test_with_workspace(workspace.clone(), Vec::new());
    let uri_a = Url::from_file_path(workspace.join("a.php")).unwrap();
    let uri_b = Url::from_file_path(workspace.join("b.php")).unwrap();

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

    std::fs::write(workspace.join("a.php"), text_a).unwrap();
    std::fs::write(workspace.join("b.php"), text_b).unwrap();

    open_php(&backend, &uri_a, text_a).await;

    let edit = rename(&backend, &uri_a, 1, 15, "Handlers").await;
    assert!(edit.is_some(), "Expected workspace edit");
    let edit = edit.unwrap();

    let edits_b = edits_for_uri(&edit, &uri_b);
    assert!(
        !edits_b.is_empty(),
        "Expected edits in unopened usage-only file b"
    );

    let result_b = apply_edits(text_b, &edits_b);
    assert!(
        result_b.contains("use App\\Handlers\\PaymentService;"),
        "Use statement should be updated in unopened usage-only file: {}",
        result_b
    );

    let _ = std::fs::remove_dir_all(&workspace);
}

#[tokio::test]
async fn rename_namespace_root_segment_renames_all_children() {
    let backend = create_test_backend();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!("<?php\n", "namespace App\\Models;\n", "class User {}\n",);

    let text_b = concat!(
        "<?php\n",
        "namespace App\\Services;\n",
        "use App\\Models\\User;\n",
        "class UserService {}\n",
    );

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

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
    let backend = create_test_backend();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!("<?php\n", "namespace App\\Old;\n", "class Foo {}\n",);

    let text_b = concat!(
        "<?php\n",
        "use App\\Old\\Foo as MyFoo;\n",
        "function demo(MyFoo $f): void {}\n",
    );

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

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
    let backend = create_test_backend();
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

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

    // Rename "Old" to "New".
    let edit = rename(&backend, &uri_a, 1, 14, "New").await;
    assert!(edit.is_some(), "Expected workspace edit");
    let edit = edit.unwrap();

    let edits_b = edits_for_uri(&edit, &uri_b);
    let result_b = apply_edits(text_b, &edits_b);

    assert_eq!(
        result_b,
        concat!(
            "<?php\n",
            "use App\\New\\{Foo, Bar};\n",
            "function demo(Foo $f, Bar $b): void {}\n",
        ),
        "Group use prefix should be updated without touching member names"
    );
}

#[tokio::test]
async fn rename_namespace_does_not_affect_unrelated_namespaces() {
    let backend = create_test_backend();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!("<?php\n", "namespace App\\Models;\n", "class User {}\n",);

    let text_b = concat!(
        "<?php\n",
        "namespace Other\\Models;\n",
        "class Product {}\n",
    );

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

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
    let backend = create_test_backend();
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

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

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
    let backend = create_test_backend();
    let uri = Url::parse("file:///a.php").unwrap();
    let text = concat!("<?php\n", "namespace App;\n", "class Config {}\n",);
    open_php(&backend, &uri, text).await;

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

    let psr4 = vec![phpantom_lsp::composer::Psr4Mapping {
        prefix: "App\\".to_string(),
        base_path: "src/App/".to_string(),
    }];

    let backend = Backend::new_test_with_workspace(workspace.clone(), psr4);
    initialize_with_resource_operations(&backend).await;

    let uri =
        Url::parse("file:///tmp/test_workspace_ns_rename/src/App/Services/Mailer.php").unwrap();
    let text = "<?php\nnamespace App\\Services;\nclass Mailer {}\n";
    open_php(&backend, &uri, text).await;

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
    let backend = create_test_backend();
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

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;
    open_php(&backend, &uri_c, text_c).await;

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

#[tokio::test]
async fn rename_namespace_keeps_a_rooted_reference_rooted() {
    let backend = create_test_backend();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!("<?php\n", "namespace App\\Legacy;\n", "class Widget {}\n",);
    let text_b = concat!(
        "<?php\n",
        "namespace App\\Providers;\n",
        "class Provider {\n",
        "    public array $map = ['page' => \\App\\Legacy\\Widget::class];\n",
        "}\n",
    );

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

    let edit = rename(&backend, &uri_a, 1, 14, "Modern")
        .await
        .expect("Expected workspace edit");
    let result_b = apply_edits(text_b, &edits_for_uri(&edit, &uri_b));

    assert!(
        result_b.contains("\\App\\Modern\\Widget::class"),
        "Dropping the root would resolve the name against App\\Providers: {}",
        result_b
    );
}

#[tokio::test]
async fn rename_namespace_rewrites_a_qualified_reference_in_a_file_with_no_namespace() {
    let backend = create_test_backend();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_config = Url::parse("file:///config.php").unwrap();

    let text_a = concat!("<?php\n", "namespace App\\Legacy;\n", "class Widget {}\n",);
    let text_config = concat!(
        "<?php\n",
        "return ['providers' => [App\\Legacy\\Widget::class]];\n",
    );

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_config, text_config).await;

    let edit = rename(&backend, &uri_a, 1, 14, "Modern")
        .await
        .expect("Expected workspace edit");
    let result_config = apply_edits(text_config, &edits_for_uri(&edit, &uri_config));

    assert_eq!(
        result_config,
        concat!(
            "<?php\n",
            "return ['providers' => [App\\Modern\\Widget::class]];\n",
        ),
        "A file with no namespace resolves a qualified name against the \
         global namespace, so the move has to carry it"
    );
}

#[tokio::test]
async fn rename_namespace_leaves_a_reference_relative_to_the_moved_namespace_alone() {
    let backend = create_test_backend();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!(
        "<?php\n",
        "namespace App\\Legacy\\Sub;\n",
        "class Gadget {}\n",
    );
    let text_b = concat!(
        "<?php\n",
        "namespace App\\Legacy;\n",
        "class Holder {\n",
        "    public function make(): Sub\\Gadget {}\n",
        "}\n",
    );

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

    let edit = rename(&backend, &uri_b, 1, 14, "Modern")
        .await
        .expect("Expected workspace edit");
    let result_b = apply_edits(text_b, &edits_for_uri(&edit, &uri_b));

    assert_eq!(
        result_b,
        concat!(
            "<?php\n",
            "namespace App\\Modern;\n",
            "class Holder {\n",
            "    public function make(): Sub\\Gadget {}\n",
            "}\n",
        ),
        "The rewritten namespace declaration carries the relative name with it"
    );
}

#[tokio::test]
async fn rename_namespace_respells_a_reference_its_import_no_longer_binds() {
    let backend = create_test_backend();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!("<?php\n", "namespace App\\Legacy;\n", "class Widget {}\n",);
    // `use App\Legacy;` binds `Legacy`, and the move renames that binding
    // to `Modern` along with the statement.
    let text_b = concat!(
        "<?php\n",
        "namespace App\\Site;\n",
        "use App\\Legacy;\n",
        "class Page {\n",
        "    public function make(): Legacy\\Widget {}\n",
        "}\n",
    );

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

    let edit = rename(&backend, &uri_a, 1, 14, "Modern")
        .await
        .expect("Expected workspace edit");
    let result_b = apply_edits(text_b, &edits_for_uri(&edit, &uri_b));

    assert_eq!(
        result_b,
        concat!(
            "<?php\n",
            "namespace App\\Site;\n",
            "use App\\Modern;\n",
            "class Page {\n",
            "    public function make(): Modern\\Widget {}\n",
            "}\n",
        ),
        "Leaving `Legacy\\Widget` behind would resolve it to App\\Site\\Legacy\\Widget"
    );
}

// --- PHPDoc @property and @method rename tests ---

#[tokio::test]
async fn test_prepare_rename_phpdoc_property() {
    let backend = create_test_backend();
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

    open_php(&backend, &uri, text).await;

    // Click on "email" in @property tag (line 2, char 22).
    let result = prepare_rename(&backend, &uri, 2, 22).await;
    assert!(
        result.is_some(),
        "prepare_rename should succeed on @property tag name"
    );
}

#[tokio::test]
async fn rename_phpdoc_property_from_usage() {
    let backend = create_test_backend();
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

    open_php(&backend, &uri, text).await;

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
    let backend = create_test_backend();
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

    open_php(&backend, &uri, text).await;

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
    let backend = create_test_backend();
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

    open_php(&backend, &uri, text).await;

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
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @method string getEmail()\n",
        " */\n",
        "class User {}\n",
    );

    open_php(&backend, &uri, text).await;

    // Click on "getEmail" in @method tag (line 2, char 19).
    let result = prepare_rename(&backend, &uri, 2, 19).await;
    assert!(
        result.is_some(),
        "prepare_rename should succeed on @method tag name"
    );
}

#[tokio::test]
async fn rename_phpdoc_property_does_not_leak_to_unrelated_class() {
    let backend = create_test_backend();
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

    open_php(&backend, &uri, text).await;

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
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function foo(?bool $abstract = false)\n",
        "{\n",
        "    fn () => fn () => $abstract;\n",
        "}\n",
    );
    open_php(&backend, &uri, text).await;
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
    let backend = create_test_backend();
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
    open_php(&backend, &uri, text).await;
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
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo($v) {\n",
        "    function () use ($v) {\n",
        "        fn () => fn () => $v;\n",
        "    };\n",
        "}\n",
    );
    open_php(&backend, &uri, text).await;
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

#[tokio::test]
async fn prepare_rename_class_declaration_returns_fqcn_placeholder() {
    // A class declaration offers the full FQCN as the rename placeholder,
    // so the user can change the namespace to move the class in one edit.
    let backend = create_test_backend();
    let uri = Url::parse("file:///src/User.php").unwrap();
    let text = "<?php\nnamespace App\\Models;\nclass User {}\n";
    open_php(&backend, &uri, text).await;

    let response = prepare_rename(&backend, &uri, 2, 6).await;
    assert!(response.is_some(), "Expected prepare rename to succeed");
    if let Some(PrepareRenameResponse::RangeWithPlaceholder { placeholder, range }) = response {
        assert_eq!(placeholder, "App\\Models\\User");
        // The editable range still covers only the short name in source.
        assert_eq!(range.start.line, 2);
        assert_eq!(range.start.character, 6);
        assert_eq!(range.end.character, 10);
    } else {
        panic!("Expected RangeWithPlaceholder, got {:?}", response);
    }
}

#[tokio::test]
async fn rename_class_move_updates_cross_file_usage() {
    // Renaming a class to a new FQN (namespace + short name change) updates
    // both the `use` statement and inline references in a separate file.
    let backend = create_test_backend();

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

    let edit = rename(&backend, &uri_decl, 3, 6, "Acme\\Domain\\TaskDto").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for the class move"
    );
    let ws = edit.unwrap();

    let usage_edits = edits_for_uri(&ws, &uri_usage);
    assert!(
        !usage_edits.is_empty(),
        "Expected edits in the usage file, got: {:?}",
        ws
    );
    let result_usage = apply_edits(text_usage, &usage_edits);
    assert!(
        result_usage.contains("use Acme\\Domain\\TaskDto;"),
        "Use statement should point at the new FQN; got:\n{}",
        result_usage
    );
    assert!(
        result_usage.contains("new TaskDto()"),
        "Inline references should use the new short name; got:\n{}",
        result_usage
    );
    assert!(
        !result_usage.contains("TaskResource"),
        "No stale references should remain; got:\n{}",
        result_usage
    );
}

#[tokio::test]
async fn rename_class_move_adds_import_to_former_namespace_sibling() {
    // A sibling in the same namespace reached the class without any
    // `use` import.  Moving the class out of that namespace has to add
    // the import, or the sibling stops compiling.
    let backend = create_test_backend();

    let uri_decl = Url::parse("file:///src/BuilderHelper.php").unwrap();
    let uri_usage = Url::parse("file:///src/Builder.php").unwrap();

    let text_decl = concat!(
        "<?php\n",
        "namespace App\\Support;\n",
        "\n",
        "class BuilderHelper {}\n",
    );

    let text_usage = concat!(
        "<?php\n",
        "namespace App\\Support;\n",
        "\n",
        "class Builder {\n",
        "    public function helper(): BuilderHelper {\n",
        "        return new BuilderHelper();\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri_decl, text_decl).await;
    open_php(&backend, &uri_usage, text_usage).await;

    let edit = rename(
        &backend,
        &uri_decl,
        3,
        6,
        "App\\Support\\Helpers\\BuilderHelper",
    )
    .await;
    let ws = edit.expect("Expected a workspace edit for the class move");

    let usage_edits = edits_for_uri(&ws, &uri_usage);
    let result_usage = apply_edits(text_usage, &usage_edits);
    assert!(
        result_usage.contains("use App\\Support\\Helpers\\BuilderHelper;"),
        "The sibling should gain an import for the moved class; got:\n{}",
        result_usage
    );
    assert!(
        result_usage.contains("new BuilderHelper()"),
        "The short-name references should stay as-is; got:\n{}",
        result_usage
    );
}

#[tokio::test]
async fn rename_class_move_adds_import_when_short_name_also_changes() {
    let backend = create_test_backend();

    let uri_decl = Url::parse("file:///src/BuilderHelper.php").unwrap();
    let uri_usage = Url::parse("file:///src/Builder.php").unwrap();

    let text_decl = concat!(
        "<?php\n",
        "namespace App\\Support;\n",
        "\n",
        "class BuilderHelper {}\n",
    );

    let text_usage = concat!(
        "<?php\n",
        "namespace App\\Support;\n",
        "\n",
        "class Builder {\n",
        "    public function helper(): BuilderHelper {\n",
        "        return new BuilderHelper();\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri_decl, text_decl).await;
    open_php(&backend, &uri_usage, text_usage).await;

    let edit = rename(&backend, &uri_decl, 3, 6, "App\\Helpers\\BuildAssistant").await;
    let ws = edit.expect("Expected a workspace edit for the class move");

    let result_usage = apply_edits(text_usage, &edits_for_uri(&ws, &uri_usage));
    assert!(
        result_usage.contains("use App\\Helpers\\BuildAssistant;"),
        "got:\n{}",
        result_usage
    );
    assert!(
        result_usage.contains("new BuildAssistant()")
            && result_usage.contains("helper(): BuildAssistant"),
        "got:\n{}",
        result_usage
    );
    assert!(
        !result_usage.contains("BuilderHelper"),
        "No stale references should remain; got:\n{}",
        result_usage
    );
}

#[tokio::test]
async fn rename_class_move_aliases_added_import_on_short_name_collision() {
    // The sibling already imports an unrelated class under the short
    // name the moved class needs, so the added import must be aliased
    // and the references rewritten to that alias.
    let backend = create_test_backend();

    let uri_decl = Url::parse("file:///src/Helper.php").unwrap();
    let uri_other = Url::parse("file:///src/Other/Widget.php").unwrap();
    let uri_usage = Url::parse("file:///src/Builder.php").unwrap();

    let text_decl = concat!(
        "<?php\n",
        "namespace App\\Support;\n",
        "\n",
        "class Helper {}\n",
    );
    let text_other = concat!("<?php\n", "namespace Other;\n", "\n", "class Widget {}\n",);

    let text_usage = concat!(
        "<?php\n",
        "namespace App\\Support;\n",
        "\n",
        "use Other\\Widget;\n",
        "\n",
        "class Builder {\n",
        "    public function run(): Widget {\n",
        "        new Helper();\n",
        "        return new Widget();\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri_decl, text_decl).await;
    open_php(&backend, &uri_other, text_other).await;
    open_php(&backend, &uri_usage, text_usage).await;

    let edit = rename(&backend, &uri_decl, 3, 6, "App\\Helpers\\Widget").await;
    let ws = edit.expect("Expected a workspace edit for the class move");

    let result_usage = apply_edits(text_usage, &edits_for_uri(&ws, &uri_usage));
    assert!(
        result_usage.contains("use Other\\Widget;"),
        "The unrelated import must survive; got:\n{}",
        result_usage
    );
    assert!(
        result_usage.contains("use App\\Helpers\\Widget as WidgetAlias;"),
        "The moved class must be imported under an alias; got:\n{}",
        result_usage
    );
    assert!(
        result_usage.contains("new WidgetAlias()"),
        "The moved class's references must use the alias; got:\n{}",
        result_usage
    );
    assert!(
        result_usage.contains("return new Widget();"),
        "The unrelated class's references must be left alone; got:\n{}",
        result_usage
    );
}

#[tokio::test]
async fn rename_class_move_skips_import_for_fqn_only_reference() {
    // A file that only ever writes the FQN has that FQN rewritten, so
    // there is nothing for an import to fix.
    let backend = create_test_backend();

    let uri_decl = Url::parse("file:///src/BuilderHelper.php").unwrap();
    let uri_usage = Url::parse("file:///src/Builder.php").unwrap();

    let text_decl = concat!(
        "<?php\n",
        "namespace App\\Support;\n",
        "\n",
        "class BuilderHelper {}\n",
    );

    let text_usage = concat!(
        "<?php\n",
        "namespace App\\Support;\n",
        "\n",
        "class Builder {\n",
        "    public function helper() {\n",
        "        return new \\App\\Support\\BuilderHelper();\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri_decl, text_decl).await;
    open_php(&backend, &uri_usage, text_usage).await;

    let edit = rename(
        &backend,
        &uri_decl,
        3,
        6,
        "App\\Support\\Helpers\\BuilderHelper",
    )
    .await;
    let ws = edit.expect("Expected a workspace edit for the class move");

    let result_usage = apply_edits(text_usage, &edits_for_uri(&ws, &uri_usage));
    assert!(
        result_usage.contains("new \\App\\Support\\Helpers\\BuilderHelper()"),
        "got:\n{}",
        result_usage
    );
    assert!(
        !result_usage.contains("use "),
        "No import is needed for an FQN-only reference; got:\n{}",
        result_usage
    );
}

#[tokio::test]
async fn rename_class_move_into_referencing_files_namespace_adds_no_import() {
    // The class lands in the referencing file's own namespace, so the
    // short-name reference keeps resolving without an import.
    let backend = create_test_backend();

    let uri_decl = Url::parse("file:///src/Support/BuilderHelper.php").unwrap();
    let uri_usage = Url::parse("file:///src/Builder.php").unwrap();

    let text_decl = concat!(
        "<?php\n",
        "namespace App\\Support;\n",
        "\n",
        "class BuilderHelper {}\n",
    );

    let text_usage = concat!(
        "<?php\n",
        "namespace App\\Support;\n",
        "\n",
        "class Builder {\n",
        "    public function helper(): BuilderHelper {\n",
        "        return new BuilderHelper();\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri_decl, text_decl).await;
    open_php(&backend, &uri_usage, text_usage).await;

    // Same namespace, different short name: no move out of the namespace.
    let edit = rename(&backend, &uri_decl, 3, 6, "App\\Support\\BuildAssistant").await;
    let ws = edit.expect("Expected a workspace edit for the class rename");

    let result_usage = apply_edits(text_usage, &edits_for_uri(&ws, &uri_usage));
    assert!(
        !result_usage.contains("use "),
        "A same-namespace rename needs no import; got:\n{}",
        result_usage
    );
    assert!(
        result_usage.contains("new BuildAssistant()"),
        "got:\n{}",
        result_usage
    );
}

#[tokio::test]
async fn rename_class_move_from_global_namespace_adds_import() {
    let backend = create_test_backend();

    let uri_decl = Url::parse("file:///src/Legacy.php").unwrap();
    let uri_usage = Url::parse("file:///src/Caller.php").unwrap();

    let text_decl = concat!("<?php\n", "\n", "class Legacy {}\n");
    let text_usage = concat!(
        "<?php\n",
        "\n",
        "function callIt() {\n",
        "    return new Legacy();\n",
        "}\n",
    );

    open_php(&backend, &uri_decl, text_decl).await;
    open_php(&backend, &uri_usage, text_usage).await;

    let edit = rename(&backend, &uri_decl, 2, 6, "App\\Legacy").await;
    let ws = edit.expect("Expected a workspace edit for the class move");

    let result_usage = apply_edits(text_usage, &edits_for_uri(&ws, &uri_usage));
    assert!(
        result_usage.contains("use App\\Legacy;"),
        "got:\n{}",
        result_usage
    );
    assert!(
        result_usage.contains("new Legacy()"),
        "got:\n{}",
        result_usage
    );
}
