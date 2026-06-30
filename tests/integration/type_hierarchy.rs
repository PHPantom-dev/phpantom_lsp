use crate::common::{create_psr4_workspace, create_test_backend};
use phpantom_lsp::Backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

async fn open(backend: &Backend, uri: &Url, text: &str) {
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;
}

async fn prepare_at(
    backend: &Backend,
    uri: &Url,
    line: u32,
    character: u32,
) -> Vec<TypeHierarchyItem> {
    let params = TypeHierarchyPrepareParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    };

    backend
        .prepare_type_hierarchy(params)
        .await
        .unwrap()
        .unwrap_or_default()
}

async fn supertypes_of(backend: &Backend, item: &TypeHierarchyItem) -> Vec<TypeHierarchyItem> {
    let params = TypeHierarchySupertypesParams {
        item: item.clone(),
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    backend
        .supertypes(params)
        .await
        .unwrap()
        .unwrap_or_default()
}

async fn subtypes_of(backend: &Backend, item: &TypeHierarchyItem) -> Vec<TypeHierarchyItem> {
    let params = TypeHierarchySubtypesParams {
        item: item.clone(),
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    backend.subtypes(params).await.unwrap().unwrap_or_default()
}

/// Extract the FQN stored in the item's data field.
fn item_fqn(item: &TypeHierarchyItem) -> String {
    item.data
        .as_ref()
        .and_then(|d| d.get("fqn"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn item_names(items: &[TypeHierarchyItem]) -> Vec<String> {
    let mut names: Vec<String> = items.iter().map(|i| i.name.clone()).collect();
    names.sort();
    names
}

// ─── Prepare: class declaration ─────────────────────────────────────────────

#[tokio::test]
async fn prepare_on_class_declaration() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",           // 0
        "class MyClass {\n", // 1
        "}\n",               // 2
    );
    open(&backend, &uri, text).await;

    let items = prepare_at(&backend, &uri, 1, 8).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "MyClass");
    assert_eq!(items[0].kind, SymbolKind::CLASS);
}

#[tokio::test]
async fn prepare_on_interface_declaration() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                   // 0
        "interface MyInterface {\n", // 1
        "}\n",                       // 2
    );
    open(&backend, &uri, text).await;

    let items = prepare_at(&backend, &uri, 1, 12).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "MyInterface");
    assert_eq!(items[0].kind, SymbolKind::INTERFACE);
}

#[tokio::test]
async fn prepare_on_enum_declaration() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",         // 0
        "enum MyEnum {\n", // 1
        "    case A;\n",   // 2
        "}\n",             // 3
    );
    open(&backend, &uri, text).await;

    let items = prepare_at(&backend, &uri, 1, 7).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "MyEnum");
    assert_eq!(items[0].kind, SymbolKind::ENUM);
}

#[tokio::test]
async fn prepare_on_trait_declaration() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",           // 0
        "trait MyTrait {\n", // 1
        "}\n",               // 2
    );
    open(&backend, &uri, text).await;

    let items = prepare_at(&backend, &uri, 1, 8).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "MyTrait");
    // Traits map to STRUCT in the LSP symbol kind.
    assert_eq!(items[0].kind, SymbolKind::STRUCT);
}

// ─── Prepare: class reference ───────────────────────────────────────────────

#[tokio::test]
async fn prepare_on_class_reference_in_extends() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                      // 0
        "class Base {\n",               // 1
        "}\n",                          // 2
        "class Child extends Base {\n", // 3
        "}\n",                          // 4
    );
    open(&backend, &uri, text).await;

    // Cursor on "Base" in the extends clause on line 3.
    let items = prepare_at(&backend, &uri, 3, 22).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "Base");
}

#[tokio::test]
async fn prepare_on_class_reference_in_implements() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "interface Renderable {\n",                              // 1
        "    public function render(): string;\n",               // 2
        "}\n",                                                   // 3
        "class View implements Renderable {\n",                  // 4
        "    public function render(): string { return ''; }\n", // 5
        "}\n",                                                   // 6
    );
    open(&backend, &uri, text).await;

    // Cursor on "Renderable" in implements clause on line 4.
    let items = prepare_at(&backend, &uri, 4, 26).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "Renderable");
    assert_eq!(items[0].kind, SymbolKind::INTERFACE);
}

// ─── Prepare: self / static / parent ────────────────────────────────────────

#[tokio::test]
async fn prepare_on_self_keyword() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                              // 0
        "class Foo {\n",                        // 1
        "    public function test(): self {\n", // 2
        "        return new self();\n",         // 3
        "    }\n",                              // 4
        "}\n",                                  // 5
    );
    open(&backend, &uri, text).await;

    // Cursor on "self" on line 3.
    let items = prepare_at(&backend, &uri, 3, 20).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "Foo");
}

#[tokio::test]
async fn prepare_on_parent_keyword() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                 // 0
        "class Base {\n",                          // 1
        "    public function greet(): string {\n", // 2
        "        return 'hi';\n",                  // 3
        "    }\n",                                 // 4
        "}\n",                                     // 5
        "class Child extends Base {\n",            // 6
        "    public function greet(): string {\n", // 7
        "        return parent::greet();\n",       // 8
        "    }\n",                                 // 9
        "}\n",                                     // 10
    );
    open(&backend, &uri, text).await;

    // Cursor on "parent" on line 8.
    let items = prepare_at(&backend, &uri, 8, 17).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "Base");
}

// ─── Prepare: returns nothing for non-class symbols ─────────────────────────

#[tokio::test]
async fn prepare_on_variable_returns_none() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",           // 0
        "$x = new Foo();\n", // 1
    );
    open(&backend, &uri, text).await;

    // Cursor on "$x".
    let items = prepare_at(&backend, &uri, 1, 1).await;
    assert!(items.is_empty());
}

// ─── Prepare: detail contains namespace ─────────────────────────────────────

#[tokio::test]
async fn prepare_includes_namespace_in_detail() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                  // 0
        "namespace App\\Models;\n", // 1
        "class User {\n",           // 2
        "}\n",                      // 3
    );
    open(&backend, &uri, text).await;

    let items = prepare_at(&backend, &uri, 2, 7).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "User");
    assert_eq!(items[0].detail.as_deref(), Some("App\\Models"));
    assert_eq!(item_fqn(&items[0]), "App\\Models\\User");
}

// ─── Prepare: deprecated class gets tag ─────────────────────────────────────

#[tokio::test]
async fn prepare_deprecated_class_has_tag() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                      // 0
        "/** @deprecated Use New */\n", // 1
        "class OldClass {\n",           // 2
        "}\n",                          // 3
    );
    open(&backend, &uri, text).await;

    let items = prepare_at(&backend, &uri, 2, 8).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].tags, Some(SymbolTag::DEPRECATED));
}

// ─── Supertypes: parent class ───────────────────────────────────────────────

#[tokio::test]
async fn supertypes_returns_parent_class() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                      // 0
        "class Base {\n",               // 1
        "}\n",                          // 2
        "class Child extends Base {\n", // 3
        "}\n",                          // 4
    );
    open(&backend, &uri, text).await;

    let items = prepare_at(&backend, &uri, 3, 7).await;
    assert_eq!(items.len(), 1);

    let supers = supertypes_of(&backend, &items[0]).await;
    let names = item_names(&supers);
    assert!(
        names.contains(&"Base".to_string()),
        "Should include parent class Base, got: {:?}",
        names
    );
}

// ─── Supertypes: interfaces ─────────────────────────────────────────────────

#[tokio::test]
async fn supertypes_returns_interfaces() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                         // 0
        "interface Printable {\n",                         // 1
        "    public function print(): void;\n",            // 2
        "}\n",                                             // 3
        "interface Loggable {\n",                          // 4
        "    public function log(): void;\n",              // 5
        "}\n",                                             // 6
        "class Report implements Printable, Loggable {\n", // 7
        "    public function print(): void {}\n",          // 8
        "    public function log(): void {}\n",            // 9
        "}\n",                                             // 10
    );
    open(&backend, &uri, text).await;

    let items = prepare_at(&backend, &uri, 7, 7).await;
    assert_eq!(items.len(), 1);

    let supers = supertypes_of(&backend, &items[0]).await;
    let names = item_names(&supers);
    assert!(
        names.contains(&"Printable".to_string()),
        "Should include interface Printable, got: {:?}",
        names
    );
    assert!(
        names.contains(&"Loggable".to_string()),
        "Should include interface Loggable, got: {:?}",
        names
    );
}

// ─── Supertypes: parent + interfaces combined ───────────────────────────────

#[tokio::test]
async fn supertypes_returns_parent_and_interfaces() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                  // 0
        "interface Serializable {\n",                               // 1
        "    public function serialize(): string;\n",               // 2
        "}\n",                                                      // 3
        "class Base {\n",                                           // 4
        "}\n",                                                      // 5
        "class Model extends Base implements Serializable {\n",     // 6
        "    public function serialize(): string { return ''; }\n", // 7
        "}\n",                                                      // 8
    );
    open(&backend, &uri, text).await;

    let items = prepare_at(&backend, &uri, 6, 7).await;
    assert_eq!(items.len(), 1);

    let supers = supertypes_of(&backend, &items[0]).await;
    let names = item_names(&supers);
    assert!(
        names.contains(&"Base".to_string()),
        "Should include parent class Base, got: {:?}",
        names
    );
    assert!(
        names.contains(&"Serializable".to_string()),
        "Should include interface Serializable, got: {:?}",
        names
    );
}

// ─── Supertypes: interface extends interface ────────────────────────────────

#[tokio::test]
async fn supertypes_interface_extends_interface() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                           // 0
        "interface Countable {\n",                           // 1
        "    public function count(): int;\n",               // 2
        "}\n",                                               // 3
        "interface AdvancedCountable extends Countable {\n", // 4
        "    public function isEmpty(): bool;\n",            // 5
        "}\n",                                               // 6
    );
    open(&backend, &uri, text).await;

    let items = prepare_at(&backend, &uri, 4, 12).await;
    assert_eq!(items.len(), 1);

    let supers = supertypes_of(&backend, &items[0]).await;
    let names = item_names(&supers);
    assert!(
        names.contains(&"Countable".to_string()),
        "Should include parent interface Countable, got: {:?}",
        names
    );
}

// ─── Supertypes: class with no parent ───────────────────────────────────────

#[tokio::test]
async fn supertypes_no_parent_returns_empty() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",          // 0
        "class Orphan {\n", // 1
        "}\n",              // 2
    );
    open(&backend, &uri, text).await;

    let items = prepare_at(&backend, &uri, 1, 7).await;
    assert_eq!(items.len(), 1);

    let supers = supertypes_of(&backend, &items[0]).await;
    assert!(
        supers.is_empty(),
        "Class with no parent should have no supertypes, got: {:?}",
        item_names(&supers)
    );
}

// ─── Subtypes: concrete subclasses ──────────────────────────────────────────

#[tokio::test]
async fn subtypes_returns_subclasses() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                      // 0
        "class Animal {\n",             // 1
        "}\n",                          // 2
        "class Dog extends Animal {\n", // 3
        "}\n",                          // 4
        "class Cat extends Animal {\n", // 5
        "}\n",                          // 6
        "class Unrelated {\n",          // 7
        "}\n",                          // 8
    );
    open(&backend, &uri, text).await;

    let items = prepare_at(&backend, &uri, 1, 7).await;
    assert_eq!(items.len(), 1);

    let subs = subtypes_of(&backend, &items[0]).await;
    let names = item_names(&subs);
    assert!(
        names.contains(&"Dog".to_string()),
        "Should include subclass Dog, got: {:?}",
        names
    );
    assert!(
        names.contains(&"Cat".to_string()),
        "Should include subclass Cat, got: {:?}",
        names
    );
    assert!(
        !names.contains(&"Unrelated".to_string()),
        "Should NOT include Unrelated, got: {:?}",
        names
    );
}

// ─── Subtypes: interface implementors ───────────────────────────────────────

#[tokio::test]
async fn subtypes_returns_interface_implementors() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "interface Renderable {\n",                              // 1
        "    public function render(): string;\n",               // 2
        "}\n",                                                   // 3
        "class HtmlView implements Renderable {\n",              // 4
        "    public function render(): string { return ''; }\n", // 5
        "}\n",                                                   // 6
        "class JsonView implements Renderable {\n",              // 7
        "    public function render(): string { return ''; }\n", // 8
        "}\n",                                                   // 9
    );
    open(&backend, &uri, text).await;

    let items = prepare_at(&backend, &uri, 1, 12).await;
    assert_eq!(items.len(), 1);

    let subs = subtypes_of(&backend, &items[0]).await;
    let names = item_names(&subs);
    assert!(
        names.contains(&"HtmlView".to_string()),
        "Should include HtmlView, got: {:?}",
        names
    );
    assert!(
        names.contains(&"JsonView".to_string()),
        "Should include JsonView, got: {:?}",
        names
    );
}

// ─── Subtypes: includes abstract subclasses ─────────────────────────────────

#[tokio::test]
async fn subtypes_includes_abstract_subclasses() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                             // 0
        "interface Shape {\n",                                 // 1
        "    public function area(): float;\n",                // 2
        "}\n",                                                 // 3
        "abstract class AbstractShape implements Shape {\n",   // 4
        "}\n",                                                 // 5
        "class Circle extends AbstractShape {\n",              // 6
        "    public function area(): float { return 0.0; }\n", // 7
        "}\n",                                                 // 8
    );
    open(&backend, &uri, text).await;

    // Direct subtypes of Shape: only AbstractShape (implements Shape).
    // Circle extends AbstractShape, NOT Shape directly, so it is NOT
    // a direct subtype.  The client would find Circle by expanding
    // AbstractShape's subtypes.
    let items = prepare_at(&backend, &uri, 1, 12).await;
    assert_eq!(items.len(), 1);

    let subs = subtypes_of(&backend, &items[0]).await;
    let names = item_names(&subs);
    assert!(
        names.contains(&"AbstractShape".to_string()),
        "Should include abstract class AbstractShape, got: {:?}",
        names
    );
    assert!(
        !names.contains(&"Circle".to_string()),
        "Should NOT include Circle (transitive, not direct), got: {:?}",
        names
    );

    // Expanding AbstractShape should reveal Circle.
    let abstract_item = subs.iter().find(|i| i.name == "AbstractShape").unwrap();
    let abstract_subs = subtypes_of(&backend, abstract_item).await;
    let abstract_sub_names = item_names(&abstract_subs);
    assert!(
        abstract_sub_names.contains(&"Circle".to_string()),
        "AbstractShape subtypes should include Circle, got: {:?}",
        abstract_sub_names
    );
}

// ─── Subtypes: final class has no subtypes ──────────────────────────────────

#[tokio::test]
async fn subtypes_final_class_has_no_subtypes() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                // 0
        "final class Sealed {\n", // 1
        "}\n",                    // 2
    );
    open(&backend, &uri, text).await;

    let items = prepare_at(&backend, &uri, 1, 13).await;
    assert_eq!(items.len(), 1);

    let subs = subtypes_of(&backend, &items[0]).await;
    assert!(
        subs.is_empty(),
        "Final class should have no subtypes, got: {:?}",
        item_names(&subs)
    );
}

// ─── Full hierarchy navigation ──────────────────────────────────────────────

#[tokio::test]
async fn full_hierarchy_navigation_up_and_down() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                // 0
        "interface HasName {\n",                                  // 1
        "    public function getName(): string;\n",               // 2
        "}\n",                                                    // 3
        "abstract class Entity implements HasName {\n",           // 4
        "}\n",                                                    // 5
        "class User extends Entity {\n",                          // 6
        "    public function getName(): string { return ''; }\n", // 7
        "}\n",                                                    // 8
        "class Admin extends User {\n",                           // 9
        "}\n",                                                    // 10
    );
    open(&backend, &uri, text).await;

    // Start from User (line 6).
    let items = prepare_at(&backend, &uri, 6, 7).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "User");

    // Go up: User → supertypes should include Entity.
    let supers = supertypes_of(&backend, &items[0]).await;
    let super_names = item_names(&supers);
    assert!(
        super_names.contains(&"Entity".to_string()),
        "User supertypes should include Entity, got: {:?}",
        super_names
    );

    // Go up from Entity → should include HasName.
    let entity_item = supers.iter().find(|i| i.name == "Entity").unwrap();
    let entity_supers = supertypes_of(&backend, entity_item).await;
    let entity_super_names = item_names(&entity_supers);
    assert!(
        entity_super_names.contains(&"HasName".to_string()),
        "Entity supertypes should include HasName, got: {:?}",
        entity_super_names
    );

    // Go down: User → subtypes should include Admin.
    let user_subs = subtypes_of(&backend, &items[0]).await;
    let sub_names = item_names(&user_subs);
    assert!(
        sub_names.contains(&"Admin".to_string()),
        "User subtypes should include Admin, got: {:?}",
        sub_names
    );
}

// ─── Enum implements interface ──────────────────────────────────────────────

#[tokio::test]
async fn subtypes_enum_implementing_interface() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                 // 0
        "interface HasLabel {\n",                  // 1
        "    public function label(): string;\n",  // 2
        "}\n",                                     // 3
        "enum Status implements HasLabel {\n",     // 4
        "    case Active;\n",                      // 5
        "    case Inactive;\n",                    // 6
        "    public function label(): string {\n", // 7
        "        return $this->name;\n",           // 8
        "    }\n",                                 // 9
        "}\n",                                     // 10
    );
    open(&backend, &uri, text).await;

    // Subtypes of HasLabel should include Status enum.
    let items = prepare_at(&backend, &uri, 1, 12).await;
    assert_eq!(items.len(), 1);

    let subs = subtypes_of(&backend, &items[0]).await;
    let names = item_names(&subs);
    assert!(
        names.contains(&"Status".to_string()),
        "Should include enum Status, got: {:?}",
        names
    );

    // Supertypes of Status enum should include HasLabel.
    let enum_items = prepare_at(&backend, &uri, 4, 7).await;
    assert_eq!(enum_items.len(), 1);
    assert_eq!(enum_items[0].kind, SymbolKind::ENUM);

    let enum_supers = supertypes_of(&backend, &enum_items[0]).await;
    let super_names = item_names(&enum_supers);
    assert!(
        super_names.contains(&"HasLabel".to_string()),
        "Enum supertypes should include HasLabel, got: {:?}",
        super_names
    );
}

// ─── Cross-file with PSR-4 ─────────────────────────────────────────────────

#[tokio::test]
async fn prepare_and_supertypes_cross_file_psr4() {
    let composer = r#"{
        "autoload": {
            "psr-4": {
                "App\\": "src/"
            }
        }
    }"#;
    let files = &[
        (
            "src/Contracts/Repository.php",
            "<?php\nnamespace App\\Contracts;\ninterface Repository {\n    public function find(int $id): mixed;\n}\n",
        ),
        (
            "src/Models/UserRepository.php",
            "<?php\nnamespace App\\Models;\nuse App\\Contracts\\Repository;\nclass UserRepository implements Repository {\n    public function find(int $id): mixed { return null; }\n}\n",
        ),
    ];

    let (backend, _dir) = create_psr4_workspace(composer, files);

    let uri = Url::parse("file:///main.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Models\\UserRepository;\n",
        "class Service {\n",
        "    public function test(UserRepository $repo): void {}\n",
        "}\n",
    );
    open(&backend, &uri, text).await;

    // Prepare on UserRepository reference on line 3.
    let items = prepare_at(&backend, &uri, 3, 35).await;
    assert_eq!(items.len(), 1, "Should prepare on UserRepository reference");
    assert_eq!(items[0].name, "UserRepository");

    // Supertypes should include Repository interface.
    let supers = supertypes_of(&backend, &items[0]).await;
    let names = item_names(&supers);
    assert!(
        names.contains(&"Repository".to_string()),
        "UserRepository supertypes should include Repository, got: {:?}",
        names
    );
}

// ─── Data field roundtrip ───────────────────────────────────────────────────

#[tokio::test]
async fn data_field_contains_fqn() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",              // 0
        "namespace My\\App;\n", // 1
        "class Controller {\n", // 2
        "}\n",                  // 3
    );
    open(&backend, &uri, text).await;

    let items = prepare_at(&backend, &uri, 2, 7).await;
    assert_eq!(items.len(), 1);
    assert_eq!(item_fqn(&items[0]), "My\\App\\Controller");
}

// ─── Transitive hierarchy ───────────────────────────────────────────────────

#[tokio::test]
async fn subtypes_transitive_via_parent_chain() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                     // 0
        "interface Loggable {\n",                      // 1
        "    public function log(): void;\n",          // 2
        "}\n",                                         // 3
        "abstract class Base implements Loggable {\n", // 4
        "}\n",                                         // 5
        "class Concrete extends Base {\n",             // 6
        "    public function log(): void {}\n",        // 7
        "}\n",                                         // 8
    );
    open(&backend, &uri, text).await;

    // Direct subtypes of Loggable: only Base (implements Loggable).
    // Concrete extends Base but does NOT directly implement Loggable,
    // so it should NOT appear here.  The client finds Concrete by
    // expanding Base's subtypes.
    let items = prepare_at(&backend, &uri, 1, 12).await;
    assert_eq!(items.len(), 1);

    let subs = subtypes_of(&backend, &items[0]).await;
    let names = item_names(&subs);
    assert!(
        names.contains(&"Base".to_string()),
        "Should include Base, got: {:?}",
        names
    );
    assert!(
        !names.contains(&"Concrete".to_string()),
        "Should NOT include Concrete (transitive, not direct), got: {:?}",
        names
    );

    // Expanding Base should reveal Concrete.
    let base_item = subs.iter().find(|i| i.name == "Base").unwrap();
    let base_subs = subtypes_of(&backend, base_item).await;
    let base_sub_names = item_names(&base_subs);
    assert!(
        base_sub_names.contains(&"Concrete".to_string()),
        "Base subtypes should include Concrete, got: {:?}",
        base_sub_names
    );
}

// ─── Multiple interfaces ────────────────────────────────────────────────────

#[tokio::test]
async fn supertypes_class_with_multiple_interfaces() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                       // 0
        "interface JsonSerializable {\n",                                // 1
        "    public function jsonSerialize(): mixed;\n",                 // 2
        "}\n",                                                           // 3
        "interface Stringable {\n",                                      // 4
        "    public function __toString(): string;\n",                   // 5
        "}\n",                                                           // 6
        "class Value implements JsonSerializable, Stringable {\n",       // 7
        "    public function jsonSerialize(): mixed { return null; }\n", // 8
        "    public function __toString(): string { return ''; }\n",     // 9
        "}\n",                                                           // 10
    );
    open(&backend, &uri, text).await;

    let items = prepare_at(&backend, &uri, 7, 7).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "Value");

    let supers = supertypes_of(&backend, &items[0]).await;
    assert_eq!(
        supers.len(),
        2,
        "Value should have 2 supertypes, got {}",
        supers.len()
    );
    let names = item_names(&supers);
    assert!(names.contains(&"JsonSerializable".to_string()));
    assert!(names.contains(&"Stringable".to_string()));
}

// ─── Symbol kind correctness in hierarchy items ─────────────────────────────

#[tokio::test]
async fn supertypes_have_correct_symbol_kinds() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                          // 0
        "interface Iface {\n",                              // 1
        "}\n",                                              // 2
        "class Parent1 {\n",                                // 3
        "}\n",                                              // 4
        "class Child extends Parent1 implements Iface {\n", // 5
        "}\n",                                              // 6
    );
    open(&backend, &uri, text).await;

    let items = prepare_at(&backend, &uri, 5, 7).await;
    assert_eq!(items.len(), 1);

    let supers = supertypes_of(&backend, &items[0]).await;
    let parent_item = supers.iter().find(|i| i.name == "Parent1");
    let iface_item = supers.iter().find(|i| i.name == "Iface");

    assert!(parent_item.is_some(), "Should find Parent1 in supertypes");
    assert!(iface_item.is_some(), "Should find Iface in supertypes");

    assert_eq!(parent_item.unwrap().kind, SymbolKind::CLASS);
    assert_eq!(iface_item.unwrap().kind, SymbolKind::INTERFACE);
}

// ─── Subtypes items carry correct data for further navigation ───────────────

#[tokio::test]
async fn subtype_items_have_data_for_further_navigation() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                       // 0
        "class Base {\n",                // 1
        "}\n",                           // 2
        "class Middle extends Base {\n", // 3
        "}\n",                           // 4
        "class Leaf extends Middle {\n", // 5
        "}\n",                           // 6
    );
    open(&backend, &uri, text).await;

    // Get subtypes of Base.
    let items = prepare_at(&backend, &uri, 1, 7).await;
    assert_eq!(items.len(), 1);

    let subs = subtypes_of(&backend, &items[0]).await;
    let middle_item = subs.iter().find(|i| i.name == "Middle");
    assert!(middle_item.is_some(), "Should find Middle in subtypes");

    // Now use Middle to get its subtypes — should find Leaf.
    let middle_subs = subtypes_of(&backend, middle_item.unwrap()).await;
    let middle_sub_names = item_names(&middle_subs);
    assert!(
        middle_sub_names.contains(&"Leaf".to_string()),
        "Middle subtypes should include Leaf, got: {:?}",
        middle_sub_names
    );
}

// ─── Direct-only subtypes ───────────────────────────────────────────────────

/// Subtypes returns only direct children, not transitive descendants.
/// Base → Middle → Leaf: subtypes of Base should be [Middle] only.
/// The client gets Leaf by expanding Middle.
#[tokio::test]
async fn subtypes_returns_only_direct_children() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                       // 0
        "class Base {\n",                // 1
        "}\n",                           // 2
        "class Middle extends Base {\n", // 3
        "}\n",                           // 4
        "class Leaf extends Middle {\n", // 5
        "}\n",                           // 6
    );
    open(&backend, &uri, text).await;

    let items = prepare_at(&backend, &uri, 1, 7).await;
    assert_eq!(items.len(), 1);

    let subs = subtypes_of(&backend, &items[0]).await;
    let all_names: Vec<String> = subs.iter().map(|i| i.name.clone()).collect();
    assert_eq!(
        all_names,
        vec!["Middle".to_string()],
        "Only direct child Middle should appear, got: {:?}",
        all_names
    );

    // Expanding Middle should reveal Leaf.
    let middle_item = subs.iter().find(|i| i.name == "Middle").unwrap();
    let middle_subs = subtypes_of(&backend, middle_item).await;
    let middle_sub_names = item_names(&middle_subs);
    assert!(
        middle_sub_names.contains(&"Leaf".to_string()),
        "Middle subtypes should include Leaf, got: {:?}",
        middle_sub_names
    );
}

// ─── Selection range points to the class name ───────────────────────────────

#[tokio::test]
async fn prepare_selection_range_covers_class_name() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    //                     0         1         2
    //                     0123456789012345678901234
    let text = concat!(
        "<?php\n",           // 0  (offset 0-5)
        "class MyClass {\n", // 1  (offset 6-21) — "class" at 6, "MyClass" at 12
        "}\n",               // 2
    );
    open(&backend, &uri, text).await;

    let items = prepare_at(&backend, &uri, 1, 8).await;
    assert_eq!(items.len(), 1);

    let sel = items[0].selection_range;
    // "MyClass" starts at column 6 on line 1 (after "class ")
    assert_eq!(sel.start.line, 1, "selection start line");
    assert_eq!(sel.start.character, 6, "selection start character");
    assert_eq!(sel.end.line, 1, "selection end line");
    assert_eq!(
        sel.end.character, 13,
        "selection end character (6 + len('MyClass') = 13)"
    );
}

#[tokio::test]
async fn subtypes_items_have_nonzero_selection_range() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "interface Renderable {\n",                              // 1
        "    public function render(): string;\n",               // 2
        "}\n",                                                   // 3
        "class HtmlView implements Renderable {\n",              // 4
        "    public function render(): string { return ''; }\n", // 5
        "}\n",                                                   // 6
    );
    open(&backend, &uri, text).await;

    let items = prepare_at(&backend, &uri, 1, 12).await;
    assert_eq!(items.len(), 1);

    let subs = subtypes_of(&backend, &items[0]).await;
    assert!(!subs.is_empty(), "Should have at least one subtype");

    for sub in &subs {
        // The selection range must not be 0,0 — it should point to the
        // actual class name in the source.
        assert!(
            sub.selection_range.start.line > 0 || sub.selection_range.start.character > 0,
            "Subtype {} has zero selection_range {:?}",
            sub.name,
            sub.selection_range
        );
    }
}

#[tokio::test]
async fn supertypes_items_have_nonzero_selection_range() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                      // 0
        "class Base {\n",               // 1
        "}\n",                          // 2
        "class Child extends Base {\n", // 3
        "}\n",                          // 4
    );
    open(&backend, &uri, text).await;

    let items = prepare_at(&backend, &uri, 3, 7).await;
    assert_eq!(items.len(), 1);

    let supers = supertypes_of(&backend, &items[0]).await;
    assert!(!supers.is_empty(), "Should have at least one supertype");

    for sup in &supers {
        assert!(
            sup.selection_range.start.line > 0 || sup.selection_range.start.character > 0,
            "Supertype {} has zero selection_range {:?}",
            sup.name,
            sup.selection_range
        );
    }
}

// ─── Subtypes: completeness when some children already indexed ──────────────

/// Subtypes must list every direct child even when one child file is
/// already parsed (so it lives in the reverse inheritance index) while
/// another child file has only been discovered on disk and not yet
/// parsed.  Returning only the indexed children silently hides the rest.
#[tokio::test]
async fn subtypes_includes_unopened_children_when_some_are_indexed() {
    let composer = r#"{
        "autoload": {
            "psr-4": {
                "App\\": "src/"
            }
        }
    }"#;
    let files = &[
        (
            "src/Repository.php",
            "<?php\nnamespace App;\ninterface Repository {\n}\n",
        ),
        (
            "src/UserRepository.php",
            "<?php\nnamespace App;\nclass UserRepository implements Repository {\n}\n",
        ),
        (
            "src/AdminRepository.php",
            "<?php\nnamespace App;\nclass AdminRepository implements Repository {\n}\n",
        ),
    ];

    let (backend, dir) = create_psr4_workspace(composer, files);

    // Open the interface and ONE implementor.  Opening UserRepository
    // parses it and records App\UserRepository as a child of
    // App\Repository in the reverse inheritance index — but
    // AdminRepository stays on disk, discovered only by a wider scan.
    let iface_uri = Url::from_file_path(dir.path().join("src/Repository.php")).unwrap();
    open(
        &backend,
        &iface_uri,
        "<?php\nnamespace App;\ninterface Repository {\n}\n",
    )
    .await;
    let user_uri = Url::from_file_path(dir.path().join("src/UserRepository.php")).unwrap();
    open(
        &backend,
        &user_uri,
        "<?php\nnamespace App;\nclass UserRepository implements Repository {\n}\n",
    )
    .await;

    // Prepare on the `Repository` interface name (line 2, on the name).
    let items = prepare_at(&backend, &iface_uri, 2, 12).await;
    assert_eq!(items.len(), 1, "Should prepare on Repository interface");
    assert_eq!(item_fqn(&items[0]), "App\\Repository");

    let subs = subtypes_of(&backend, &items[0]).await;
    let names = item_names(&subs);
    assert!(
        names.contains(&"UserRepository".to_string()),
        "subtypes should include the already-indexed UserRepository, got: {:?}",
        names
    );
    assert!(
        names.contains(&"AdminRepository".to_string()),
        "subtypes should include the unopened AdminRepository, got: {:?}",
        names
    );
}
