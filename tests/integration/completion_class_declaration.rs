use crate::common::{complete_at, create_test_backend};
use tower_lsp::lsp_types::*;

// ─── Helper ─────────────────────────────────────────────────────────────────

fn labels(items: &[CompletionItem]) -> Vec<&str> {
    items.iter().map(|i| i.label.as_str()).collect()
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_class_declaration_offers_filename() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///Test.php").unwrap();
    let items = complete_at(&backend, &uri, "<?php\n\nclass T", 2, 7).await;
    assert_eq!(labels(&items), vec!["Test"]);
    assert_eq!(items[0].kind, Some(CompletionItemKind::CLASS));
}

#[tokio::test]
async fn test_class_declaration_no_partial() {
    // Even with no partial typed, the filename should be offered.
    let backend = create_test_backend();
    let uri = Url::parse("file:///MyService.php").unwrap();
    let items = complete_at(&backend, &uri, "<?php\n\nclass ", 2, 6).await;
    assert_eq!(labels(&items), vec!["MyService"]);
}

#[tokio::test]
async fn test_interface_declaration_offers_filename() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///Printable.php").unwrap();
    let items = complete_at(&backend, &uri, "<?php\n\ninterface P", 2, 11).await;
    assert_eq!(labels(&items), vec!["Printable"]);
}

#[tokio::test]
async fn test_trait_declaration_offers_filename() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///HasTimestamps.php").unwrap();
    let items = complete_at(&backend, &uri, "<?php\n\ntrait H", 2, 7).await;
    assert_eq!(labels(&items), vec!["HasTimestamps"]);
}

#[tokio::test]
async fn test_enum_declaration_offers_filename() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///Status.php").unwrap();
    let items = complete_at(&backend, &uri, "<?php\n\nenum S", 2, 6).await;
    assert_eq!(labels(&items), vec!["Status"]);
}

#[tokio::test]
async fn test_abstract_class_declaration_offers_filename() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///BaseController.php").unwrap();
    let items = complete_at(&backend, &uri, "<?php\n\nabstract class B", 2, 16).await;
    assert_eq!(labels(&items), vec!["BaseController"]);
}

#[tokio::test]
async fn test_final_class_declaration_offers_filename() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///Singleton.php").unwrap();
    let items = complete_at(&backend, &uri, "<?php\n\nfinal class S", 2, 13).await;
    assert_eq!(labels(&items), vec!["Singleton"]);
}

#[tokio::test]
async fn test_readonly_class_declaration_offers_filename() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///ValueObject.php").unwrap();
    let items = complete_at(&backend, &uri, "<?php\n\nreadonly class V", 2, 16).await;
    assert_eq!(labels(&items), vec!["ValueObject"]);
}

#[tokio::test]
async fn test_final_readonly_class_declaration_offers_filename() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///Money.php").unwrap();
    let items = complete_at(&backend, &uri, "<?php\n\nfinal readonly class M", 2, 22).await;
    assert_eq!(labels(&items), vec!["Money"]);
}

#[tokio::test]
async fn test_anonymous_class_not_triggered() {
    // `new class` should NOT offer the filename completion.
    let backend = create_test_backend();
    let uri = Url::parse("file:///Foo.php").unwrap();
    let items = complete_at(&backend, &uri, "<?php\n\n$x = new class ", 2, 16).await;
    // Should not contain the filename "Foo"
    let has_foo = items.iter().any(|i| i.label == "Foo");
    assert!(
        !has_foo,
        "anonymous class context should not offer filename"
    );
}

#[tokio::test]
async fn test_extends_not_triggered() {
    // `extends` context should NOT trigger class declaration completion.
    let backend = create_test_backend();
    let uri = Url::parse("file:///Child.php").unwrap();
    let items = complete_at(&backend, &uri, "<?php\n\nclass Child extends B", 2, 21).await;
    // The items should not be a single "Child" item — this is extends context.
    let is_declaration_only = items.len() == 1
        && items[0].label == "Child"
        && items[0].detail.as_deref() == Some("Match filename");
    assert!(
        !is_declaration_only,
        "extends context should not trigger class declaration completion"
    );
}

#[tokio::test]
async fn test_implements_not_triggered() {
    // `implements` context should NOT trigger class declaration completion.
    let backend = create_test_backend();
    let uri = Url::parse("file:///MyClass.php").unwrap();
    let items = complete_at(&backend, &uri, "<?php\n\nclass MyClass implements S", 2, 26).await;
    let is_declaration_only = items.len() == 1
        && items[0].label == "MyClass"
        && items[0].detail.as_deref() == Some("Match filename");
    assert!(
        !is_declaration_only,
        "implements context should not trigger class declaration completion"
    );
}

#[tokio::test]
async fn test_only_one_item_returned() {
    // Class declaration completion should return exactly one item.
    let backend = create_test_backend();
    let uri = Url::parse("file:///Test.php").unwrap();
    let items = complete_at(&backend, &uri, "<?php\n\nclass T", 2, 7).await;
    assert_eq!(items.len(), 1, "should return exactly one completion item");
}

#[tokio::test]
async fn test_completion_item_detail() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///UserRepository.php").unwrap();
    let items = complete_at(&backend, &uri, "<?php\n\nclass U", 2, 7).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].detail.as_deref(), Some("Match filename"));
}

#[tokio::test]
async fn test_nested_path_uses_filename_only() {
    // A deeply nested path should still only suggest the filename stem.
    let backend = create_test_backend();
    let uri = Url::parse("file:///home/user/project/src/Models/User.php").unwrap();
    let items = complete_at(&backend, &uri, "<?php\n\nclass U", 2, 7).await;
    assert_eq!(labels(&items), vec!["User"]);
}

#[tokio::test]
async fn test_class_declaration_with_namespace() {
    // Namespace declaration before class should not interfere.
    let backend = create_test_backend();
    let uri = Url::parse("file:///OrderService.php").unwrap();
    let items = complete_at(
        &backend,
        &uri,
        "<?php\n\nnamespace App\\Services;\n\nclass O",
        4,
        7,
    )
    .await;
    assert_eq!(labels(&items), vec!["OrderService"]);
}
