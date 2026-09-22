//! Rename of class-like declarations and the `use` imports that name them.

use crate::common::{
    apply_edits, create_test_backend, doc_change_edits_for_uri, edits_for_uri, extract_rename_file,
    initialize_with_resource_operations, open_php, prepare_rename, rename,
};
use tower_lsp::lsp_types::*;

// ─── Class Rename ───────────────────────────────────────────────────────────

#[tokio::test]
async fn rename_class_same_file() {
    let backend = create_test_backend();
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

    open_php(&backend, &uri, text).await;

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
    let backend = create_test_backend();
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

    open_php(&backend, &uri, text).await;

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
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {}\n",
        "function demo(Foo $f): void {}\n",
    );

    open_php(&backend, &uri, text).await;

    let response = prepare_rename(&backend, &uri, 1, 7).await;
    assert!(response.is_some());

    if let Some(PrepareRenameResponse::RangeWithPlaceholder { placeholder, .. }) = response {
        assert_eq!(placeholder, "Foo");
    } else {
        panic!("Expected RangeWithPlaceholder response");
    }
}

// ─── Use-Statement-Aware Class Rename ───────────────────────────────────────

#[tokio::test]
async fn rename_class_updates_use_import() {
    // Renaming a class should update the `use` statement FQN (last segment)
    // as well as all in-code references.
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
        "    protected static string $service = TaskResource::class;\n",
        "}\n",
    );

    open_php(&backend, &uri_decl, text_decl).await;
    open_php(&backend, &uri_usage, text_usage).await;

    // Rename from the declaration site (line 3, col 6 = "TaskResource").
    let edit = rename(&backend, &uri_decl, 3, 6, "TaskResourceService").await;
    assert!(edit.is_some(), "Expected a workspace edit for class rename");

    let ws = edit.unwrap();
    let edits_usage = edits_for_uri(&ws, &uri_usage);
    assert!(!edits_usage.is_empty(), "Expected edits in the usage file");

    let result = apply_edits(text_usage, &edits_usage);

    // The use statement should have the FQN last segment updated.
    assert!(
        result.contains("use Acme\\Tasks\\Resources\\TaskResourceService;"),
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
    let backend = create_test_backend();
    let uri_decl = Url::parse("file:///src/TaskResource.php").unwrap();
    let uri_usage = Url::parse("file:///src/Controller.php").unwrap();

    let text_decl = concat!(
        "<?php\n",
        "namespace Acme\\Tasks\\Resources;\n",
        "\n",
        "class TaskResource {}\n",
    );

    let text_usage = concat!(
        "<?php\n",
        "namespace Acme\\Tasks\\Http;\n",
        "\n",
        "use Acme\\Tasks\\Resources\\TaskResource as ResourceService;\n",
        "\n",
        "class Controller {\n",
        "    private ResourceService $service;\n",
        "}\n",
    );

    open_php(&backend, &uri_decl, text_decl).await;
    open_php(&backend, &uri_usage, text_usage).await;

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
        result.contains("use Acme\\Tasks\\Resources\\TaskResourceService as ResourceService;"),
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
    let backend = create_test_backend();
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

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;
    open_php(&backend, &uri_usage, text_usage).await;

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
    let backend = create_test_backend();
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

    open_php(&backend, &uri, text).await;

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
    let backend = create_test_backend();
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

    open_php(&backend, &uri, text).await;

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
async fn rename_class_rewrites_differently_cased_references() {
    // PHP resolves class names case-insensitively, so `new WIDGET()` is a
    // reference to `Widget` and has to be rewritten with the rest.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Widget {}\n",
        "$a = new WIDGET();\n",
        "$b = new Widget();\n",
        "$c = new widget();\n",
    );

    open_php(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 1, 7, "Gadget").await;
    assert!(edit.is_some(), "Expected a workspace edit");

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    assert_eq!(
        result.matches("Gadget").count(),
        4,
        "Every spelling of Widget should become Gadget; got:\n{}",
        result
    );
    assert!(
        !result.to_ascii_lowercase().contains("widget"),
        "No spelling of the old name should remain; got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_from_a_differently_cased_reference_uses_the_declared_name() {
    // A same-namespace reference resolves to an FQN spelled the way the
    // *reference* writes it, so starting the rename from `WIDGET` yields
    // `Acme\Parts\WIDGET`.  Everything downstream reads the old short name
    // back out of that FQN, and the file rename compares it to the file
    // stem, so the name has to be canonicalized to the declaration first.
    let backend = create_test_backend();
    initialize_with_resource_operations(&backend).await;

    let uri_decl = Url::parse("file:///src/Widget.php").unwrap();
    let uri_usage = Url::parse("file:///src/Usage.php").unwrap();

    let text_decl = concat!(
        "<?php\n",
        "namespace Acme\\Parts;\n",
        "\n",
        "class Widget {}\n",
    );

    let text_usage = concat!(
        "<?php\n",
        "namespace Acme\\Parts;\n",
        "\n",
        "class Usage {\n",
        "    public function make(): void {\n",
        "        $w = new WIDGET();\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri_decl, text_decl).await;
    open_php(&backend, &uri_usage, text_usage).await;

    // Line 5, col 21 is inside `WIDGET`.
    let edit = rename(&backend, &uri_usage, 5, 21, "Gadget").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit from the mis-cased reference site"
    );

    let ws = edit.unwrap();

    let rf = extract_rename_file(&ws)
        .expect("the declaration file is named after the class, so it should be renamed with it");
    assert_eq!(rf.old_uri.to_string(), "file:///src/Widget.php");
    assert_eq!(rf.new_uri.to_string(), "file:///src/Gadget.php");

    let result = apply_edits(text_usage, &doc_change_edits_for_uri(&ws, &uri_usage));
    assert!(
        result.contains("new Gadget()"),
        "The mis-cased reference should be rewritten; got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_class_updates_use_import_from_reference_site() {
    // Trigger rename from a reference site (not the declaration) and
    // verify the use statement is still updated.
    let backend = create_test_backend();
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

    open_php(&backend, &uri_decl, text_decl).await;
    open_php(&backend, &uri_usage, text_usage).await;

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
    let backend = create_test_backend();
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

    open_php(&backend, &uri_decl, text_decl).await;
    open_php(&backend, &uri_usage, text_usage).await;

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
async fn rename_class_updates_group_import_member() {
    // Renaming a class imported through a multi-member group statement
    // should rewrite only that member, leaving its siblings alone.
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
        "use Acme\\Tasks\\Resources\\{TaskResource, TaskDto};\n",
        "\n",
        "class Task {\n",
        "    protected static string $service = TaskResource::class;\n",
        "}\n",
    );

    open_php(&backend, &uri_decl, text_decl).await;
    open_php(&backend, &uri_usage, text_usage).await;

    let edit = rename(&backend, &uri_decl, 3, 6, "TaskResourceService").await;
    let ws = edit.expect("Expected a workspace edit for class rename");
    let result = apply_edits(text_usage, &edits_for_uri(&ws, &uri_usage));

    assert!(
        result.contains("use Acme\\Tasks\\Resources\\{TaskResourceService, TaskDto};"),
        "Only the renamed member should change, sibling preserved; got:\n{}",
        result
    );
    assert!(
        result.contains("TaskResourceService::class"),
        "got:\n{}",
        result
    );
    assert!(!result.contains("TaskResource::class"), "got:\n{}", result);
}

#[tokio::test]
async fn rename_class_updates_wrapped_group_import_member() {
    // The same as above, but the group is wrapped over several lines the
    // way a long member list usually is.
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
        "use Acme\\Tasks\\Resources\\{\n",
        "    TaskResource,\n",
        "    TaskDto,\n",
        "};\n",
        "\n",
        "class Task {\n",
        "    protected static string $service = TaskResource::class;\n",
        "}\n",
    );

    open_php(&backend, &uri_decl, text_decl).await;
    open_php(&backend, &uri_usage, text_usage).await;

    let edit = rename(&backend, &uri_decl, 3, 6, "TaskResourceService").await;
    let ws = edit.expect("Expected a workspace edit for class rename");
    let result = apply_edits(text_usage, &edits_for_uri(&ws, &uri_usage));

    assert!(
        result.contains("    TaskResourceService,\n"),
        "got:\n{}",
        result
    );
    assert!(result.contains("    TaskDto,\n"), "got:\n{}", result);
    assert!(!result.contains("TaskResource,\n"), "got:\n{}", result);
}

#[tokio::test]
async fn rename_class_collapses_a_single_member_group_import() {
    // A one-member group has nothing left to keep, so it is rewritten as
    // a plain `use` statement rather than an empty `{}`.
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
        "use Acme\\Tasks\\Resources\\{TaskResource};\n",
        "\n",
        "class Task {\n",
        "    protected static string $service = TaskResource::class;\n",
        "}\n",
    );

    open_php(&backend, &uri_decl, text_decl).await;
    open_php(&backend, &uri_usage, text_usage).await;

    let edit = rename(&backend, &uri_decl, 3, 6, "TaskResourceService").await;
    let ws = edit.expect("Expected a workspace edit for class rename");
    let result = apply_edits(text_usage, &edits_for_uri(&ws, &uri_usage));

    assert!(
        result.contains("use Acme\\Tasks\\Resources\\TaskResourceService;"),
        "got:\n{}",
        result
    );
    assert!(!result.contains("Resources\\{"), "got:\n{}", result);
}

#[tokio::test]
async fn rename_class_move_within_group_prefix_updates_member() {
    // A move that stays under the group's shared prefix rewrites the
    // member's relative name in place.
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
        "use Acme\\Tasks\\Resources\\{TaskResource, TaskDto};\n",
        "\n",
        "class Task {\n",
        "    protected static string $service = TaskResource::class;\n",
        "}\n",
    );

    open_php(&backend, &uri_decl, text_decl).await;
    open_php(&backend, &uri_usage, text_usage).await;

    let edit = rename(
        &backend,
        &uri_decl,
        3,
        6,
        "Acme\\Tasks\\Resources\\V2\\TaskResource",
    )
    .await;
    let ws = edit.expect("Expected a workspace edit for the class move");
    let result = apply_edits(text_usage, &edits_for_uri(&ws, &uri_usage));

    assert!(
        result.contains("use Acme\\Tasks\\Resources\\{V2\\TaskResource, TaskDto};"),
        "got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_class_move_out_of_group_prefix_splits_the_import() {
    // A move whose new namespace no longer fits the group's shared prefix
    // cannot stay a member of that group: it is dropped from the group
    // (keeping the sibling) and re-added as its own `use` statement.
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
        "use Acme\\Tasks\\Resources\\{TaskResource, TaskDto};\n",
        "\n",
        "class Task {\n",
        "    protected static string $service = TaskResource::class;\n",
        "}\n",
    );

    open_php(&backend, &uri_decl, text_decl).await;
    open_php(&backend, &uri_usage, text_usage).await;

    let edit = rename(&backend, &uri_decl, 3, 6, "Vendor\\Other\\Task2").await;
    let ws = edit.expect("Expected a workspace edit for the class move");
    let result = apply_edits(text_usage, &edits_for_uri(&ws, &uri_usage));

    assert!(
        result.contains("use Acme\\Tasks\\Resources\\{TaskDto};")
            || result.contains("use Acme\\Tasks\\Resources\\{TaskDto}"),
        "The sibling should stay imported without the moved member; got:\n{}",
        result
    );
    assert!(
        result.contains("use Vendor\\Other\\Task2;"),
        "The moved class should get its own import; got:\n{}",
        result
    );
    assert!(
        result.contains("Task2::class"),
        "In-code reference should be renamed; got:\n{}",
        result
    );
    assert!(
        !result.contains("TaskResource"),
        "The old name should not remain anywhere; got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_class_updates_a_brace_less_comma_import_list() {
    // A plain multi-import statement without braces
    // (`use Foo\Bar, Baz\Qux;`) is a group in spirit even without the
    // `{}` syntax: only the renamed item should change.
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
        "use Acme\\Tasks\\Resources\\TaskResource, Acme\\Tasks\\Resources\\TaskDto;\n",
        "\n",
        "class Task {\n",
        "    protected static string $service = TaskResource::class;\n",
        "}\n",
    );

    open_php(&backend, &uri_decl, text_decl).await;
    open_php(&backend, &uri_usage, text_usage).await;

    let edit = rename(&backend, &uri_decl, 3, 6, "TaskResourceService").await;
    let ws = edit.expect("Expected a workspace edit for class rename");
    let result = apply_edits(text_usage, &edits_for_uri(&ws, &uri_usage));

    assert!(
        result.contains(
            "use Acme\\Tasks\\Resources\\TaskResourceService, Acme\\Tasks\\Resources\\TaskDto;"
        ),
        "Only the renamed item should change, sibling preserved; got:\n{}",
        result
    );
    assert!(
        result.contains("TaskResourceService::class"),
        "got:\n{}",
        result
    );
}

#[tokio::test]
async fn rename_class_fqn_inline_reference() {
    // When a file uses the class via an inline FQN (no use statement),
    // only the last segment should be renamed.
    let backend = create_test_backend();
    let uri_decl = Url::parse("file:///src/Item.php").unwrap();
    let uri_usage = Url::parse("file:///src/other.php").unwrap();

    let text_decl = concat!("<?php\n", "namespace Shop;\n", "\n", "class Item {}\n",);

    let text_usage = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $x = new \\Shop\\Item();\n",
        "}\n",
    );

    open_php(&backend, &uri_decl, text_decl).await;
    open_php(&backend, &uri_usage, text_usage).await;

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
    let backend = create_test_backend();
    let uri_a = Url::parse("file:///src/Foo.php").unwrap();
    let uri_b = Url::parse("file:///src/Bar.php").unwrap();

    let text_a = concat!("<?php\n", "namespace App;\n", "\n", "class Foo {}\n",);

    let text_b = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class Bar extends Foo {}\n",
    );

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

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
