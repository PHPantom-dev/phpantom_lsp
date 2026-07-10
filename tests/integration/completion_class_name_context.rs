use crate::common::{create_test_backend, create_test_backend_with_stubs};
use phpantom_lsp::Backend;
use phpantom_lsp::atom::atom;
use phpantom_lsp::php_type::PhpType;
use phpantom_lsp::types::{ClassInfo, ClassLikeKind};
use std::sync::Arc;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Helper ─────────────────────────────────────────────────────────────────

/// Open a file in the backend and request completion at the given position.
async fn complete_at(
    backend: &Backend,
    uri: &Url,
    text: &str,
    line: u32,
    character: u32,
) -> Vec<CompletionItem> {
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

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position { line, character },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    match result {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => vec![],
    }
}

/// Filter completion items to only those with kind == CLASS.
fn class_items(items: &[CompletionItem]) -> Vec<&CompletionItem> {
    items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::CLASS))
        .collect()
}

/// Extract labels from a list of completion items.
fn labels(items: &[CompletionItem]) -> Vec<&str> {
    items.iter().map(|i| i.label.as_str()).collect()
}

/// Find a completion item by its FQN (stored in the `detail` field).
fn find_by_fqn<'a>(items: &'a [&CompletionItem], fqn: &str) -> Option<&'a CompletionItem> {
    items
        .iter()
        .find(|i| i.detail.as_deref() == Some(fqn))
        .copied()
}

/// Extract FQNs from a list of completion items (via the `detail` field).
fn fqn_labels<'a>(items: &'a [&'a CompletionItem]) -> Vec<&'a str> {
    items.iter().filter_map(|i| i.detail.as_deref()).collect()
}

/// Load scaffolding classes into the backend's ast_map so the context
/// filter can inspect their `ClassLikeKind` / `is_final` / `is_abstract`.
async fn load_scaffolding(backend: &Backend) {
    let scaffolding_uri = Url::parse("file:///scaffolding.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: scaffolding_uri,
                language_id: "php".to_string(),
                version: 1,
                text: concat!(
                    "<?php\n",
                    "namespace Scaffold;\n",
                    "class ConcreteClass {}\n",
                    "final class FinalClass {}\n",
                    "abstract class AbstractClass {}\n",
                    "interface SomeInterface {}\n",
                    "interface AnotherInterface {}\n",
                    "trait SomeTrait {}\n",
                    "trait AnotherTrait {}\n",
                    "enum SomeEnum {}\n",
                )
                .to_string(),
            },
        })
        .await;
}

// ─── extends (class) ────────────────────────────────────────────────────────

/// `extends` in a class declaration should include non-final classes
/// and exclude interfaces, traits, enums, and final classes.
#[tokio::test]
async fn test_extends_class_excludes_interface() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_ext_cls.php").unwrap();
    let text = "<?php\nnamespace Scaffold;\nclass Foo extends Some";

    let items = complete_at(&backend, &uri, text, 2, 25).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.contains(&"SomeInterface"),
        "extends (class) should not offer interfaces, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"SomeTrait"),
        "extends (class) should not offer traits, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"SomeEnum"),
        "extends (class) should not offer enums, got: {lbls:?}"
    );
}

/// `extends` in a class should exclude final classes.
#[tokio::test]
async fn test_extends_class_excludes_final() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_ext_final.php").unwrap();
    let text = "<?php\nnamespace Scaffold;\nclass Foo extends Final";

    let items = complete_at(&backend, &uri, text, 2, 25).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.contains(&"FinalClass"),
        "extends (class) should not offer final classes, got: {lbls:?}"
    );
}

/// `extends` in a class should include abstract classes (they are valid
/// extension targets).
#[tokio::test]
async fn test_extends_class_includes_abstract() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_ext_abs.php").unwrap();
    let text = "<?php\nnamespace Scaffold;\nclass Foo extends Abstract";

    let items = complete_at(&backend, &uri, text, 2, 29).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"AbstractClass"),
        "extends (class) should offer abstract classes, got: {lbls:?}"
    );
}

/// `extends` in a class should include concrete non-final classes.
#[tokio::test]
async fn test_extends_class_includes_concrete() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_ext_conc.php").unwrap();
    let text = "<?php\nnamespace Scaffold;\nclass Foo extends Concrete";

    let items = complete_at(&backend, &uri, text, 2, 29).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"ConcreteClass"),
        "extends (class) should offer concrete classes, got: {lbls:?}"
    );
}

/// `extends` in an `abstract class` declaration should still be
/// detected as a class extends context.
#[tokio::test]
async fn test_extends_abstract_class_filters() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_ext_abscls.php").unwrap();
    let text = "<?php\nnamespace Scaffold;\nabstract class Foo extends Some";

    let items = complete_at(&backend, &uri, text, 2, 34).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.contains(&"SomeInterface"),
        "extends after abstract class should not offer interfaces, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"SomeTrait"),
        "extends after abstract class should not offer traits, got: {lbls:?}"
    );
}

/// `extends` after `final class` should also work correctly.
#[tokio::test]
async fn test_extends_final_class_filters() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    // Note: `final class Foo extends ...` is technically odd
    // (the result is a final class extending something), but PHP allows it.
    let uri = Url::parse("file:///test_ext_fincls.php").unwrap();
    let text = "<?php\nnamespace Scaffold;\nfinal class Foo extends Some";

    let items = complete_at(&backend, &uri, text, 2, 31).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.contains(&"SomeInterface"),
        "extends after final class should not offer interfaces, got: {lbls:?}"
    );
}

// ─── extends (interface) ────────────────────────────────────────────────────

/// `extends` in an interface declaration should only offer interfaces.
#[tokio::test]
async fn test_extends_interface_only_interfaces() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_ext_iface.php").unwrap();
    let text = "<?php\nnamespace Scaffold;\ninterface Foo extends Some";

    let items = complete_at(&backend, &uri, text, 2, 29).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"SomeInterface"),
        "extends (interface) should offer interfaces, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"ConcreteClass"),
        "extends (interface) should not offer classes, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"SomeTrait"),
        "extends (interface) should not offer traits, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"SomeEnum"),
        "extends (interface) should not offer enums, got: {lbls:?}"
    );
}

/// `extends` in an interface with comma-separated parents should still
/// filter to interfaces only.
#[tokio::test]
async fn test_extends_interface_comma_separated() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_ext_iface_comma.php").unwrap();
    let text = "<?php\nnamespace Scaffold;\ninterface Foo extends SomeInterface, Another";

    let items = complete_at(&backend, &uri, text, 2, 47).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"AnotherInterface"),
        "extends (interface) with comma should offer interfaces, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"AnotherTrait"),
        "extends (interface) with comma should not offer traits, got: {lbls:?}"
    );
}

// ─── implements ─────────────────────────────────────────────────────────────

/// `implements` should only offer interfaces.
#[tokio::test]
async fn test_implements_only_interfaces() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_impl.php").unwrap();
    let text = "<?php\nnamespace Scaffold;\nclass Foo implements Some";

    let items = complete_at(&backend, &uri, text, 2, 28).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"SomeInterface"),
        "implements should offer interfaces, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"ConcreteClass"),
        "implements should not offer classes, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"SomeTrait"),
        "implements should not offer traits, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"SomeEnum"),
        "implements should not offer enums, got: {lbls:?}"
    );
}

/// `implements` with comma-separated interfaces.
#[tokio::test]
async fn test_implements_comma_separated() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_impl_comma.php").unwrap();
    let text = "<?php\nnamespace Scaffold;\nclass Foo implements SomeInterface, Another";

    let items = complete_at(&backend, &uri, text, 2, 44).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"AnotherInterface"),
        "implements with comma should offer interfaces, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"AnotherTrait"),
        "implements with comma should not offer traits, got: {lbls:?}"
    );
}

/// `implements` should suppress constants and functions.
#[tokio::test]
async fn test_implements_excludes_constants_and_functions() {
    let backend = crate::common::create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///test_impl_no_const.php").unwrap();
    let text = "<?php\nclass Foo implements str";

    let items = complete_at(&backend, &uri, text, 1, 27).await;

    // Functions like str_contains should NOT appear.
    let func_items: Vec<&CompletionItem> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
        .collect();
    assert!(
        func_items.is_empty(),
        "implements should not offer functions, got: {:?}",
        labels(&items)
    );

    let const_items: Vec<&CompletionItem> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
        .collect();
    assert!(
        const_items.is_empty(),
        "implements should not offer constants, got: {:?}",
        labels(&items)
    );
}

// ─── use (trait) ────────────────────────────────────────────────────────────

/// `use` inside a class body should only offer traits.
#[tokio::test]
async fn test_trait_use_only_traits() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_trait_use.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Scaffold;\n",
        "class Bar {\n",
        "    use Some\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 12).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"SomeTrait"),
        "trait use should offer traits, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"SomeInterface"),
        "trait use should not offer interfaces, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"ConcreteClass"),
        "trait use should not offer classes, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"SomeEnum"),
        "trait use should not offer enums, got: {lbls:?}"
    );
}

/// `use` inside a class body with comma-separated traits.
#[tokio::test]
async fn test_trait_use_comma_separated() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_trait_use_comma.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Scaffold;\n",
        "class Bar {\n",
        "    use SomeTrait, Another\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 26).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"AnotherTrait"),
        "trait use with comma should offer traits, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"AnotherInterface"),
        "trait use with comma should not offer interfaces, got: {lbls:?}"
    );
}

/// Top-level `use` (namespace import) should NOT filter — show everything.
#[tokio::test]
async fn test_top_level_use_no_filter() {
    let backend = create_test_backend();

    // Load classes in a namespace so they appear in class name completions
    // when the test file types a `use` import for them.
    let scaffolding_uri = Url::parse("file:///ns_scaffolding.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: scaffolding_uri,
                language_id: "php".to_string(),
                version: 1,
                text: concat!(
                    "<?php\n",
                    "namespace Qx;\n",
                    "class QxConcreteClass {}\n",
                    "interface QxSomeInterface {}\n",
                    "trait QxSomeTrait {}\n",
                    "enum QxSomeEnum {}\n",
                )
                .to_string(),
            },
        })
        .await;

    let uri = Url::parse("file:///test_ns_use.php").unwrap();
    let text = "<?php\nuse Qx";

    let items = complete_at(&backend, &uri, text, 1, 6).await;
    let cls = class_items(&items);
    // Top-level `use` should offer all kinds: classes, interfaces,
    // traits, and enums — no kind-filtering should be applied.
    // Labels are FQNs in UseImport context because the user is writing
    // a fully-qualified import statement.
    // Labels may be short names or FQNs depending on whether the prefix
    // contains a namespace separator.  Use the `detail` field (which
    // always holds the FQN) for reliable lookup.
    let fqns = fqn_labels(&cls);
    assert!(
        fqns.contains(&"Qx\\QxConcreteClass"),
        "top-level use should offer classes, got: {fqns:?}"
    );
    assert!(
        fqns.contains(&"Qx\\QxSomeInterface"),
        "top-level use should offer interfaces, got: {fqns:?}"
    );
    assert!(
        fqns.contains(&"Qx\\QxSomeTrait"),
        "top-level use should offer traits, got: {fqns:?}"
    );
    assert!(
        fqns.contains(&"Qx\\QxSomeEnum"),
        "top-level use should offer enums, got: {fqns:?}"
    );
}

/// `use` inside a class body should suppress constants and functions.
#[tokio::test]
async fn test_trait_use_excludes_constants_and_functions() {
    let backend = crate::common::create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///test_trait_use_no_fn.php").unwrap();
    let text = concat!("<?php\n", "class Foo {\n", "    use str\n", "}\n",);

    let items = complete_at(&backend, &uri, text, 2, 11).await;

    let func_items: Vec<&CompletionItem> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
        .collect();
    assert!(
        func_items.is_empty(),
        "trait use should not offer functions, got: {:?}",
        labels(&items)
    );
}

// ─── instanceof ─────────────────────────────────────────────────────────────

/// `instanceof` should offer classes, interfaces, and enums but NOT traits.
#[tokio::test]
async fn test_instanceof_excludes_traits() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_instanceof.php").unwrap();
    let text = "<?php\nnamespace Scaffold;\n$x instanceof Some";

    let items = complete_at(&backend, &uri, text, 2, 18).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.contains(&"SomeTrait"),
        "instanceof should not offer traits, got: {lbls:?}"
    );
}

/// `instanceof` should include interfaces.
#[tokio::test]
async fn test_instanceof_includes_interfaces() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_instanceof_iface.php").unwrap();
    let text = "<?php\nnamespace Scaffold;\n$x instanceof Some";

    let items = complete_at(&backend, &uri, text, 2, 18).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"SomeInterface"),
        "instanceof should offer interfaces, got: {lbls:?}"
    );
}

/// `instanceof` should include classes.
#[tokio::test]
async fn test_instanceof_includes_classes() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_instanceof_cls.php").unwrap();
    let text = "<?php\nnamespace Scaffold;\n$x instanceof Concrete";

    let items = complete_at(&backend, &uri, text, 2, 21).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"ConcreteClass"),
        "instanceof should offer classes, got: {lbls:?}"
    );
}

/// `instanceof` should include enums.
#[tokio::test]
async fn test_instanceof_includes_enums() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_instanceof_enum.php").unwrap();
    let text = "<?php\nnamespace Scaffold;\n$x instanceof Some";

    let items = complete_at(&backend, &uri, text, 2, 18).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"SomeEnum"),
        "instanceof should offer enums, got: {lbls:?}"
    );
}

/// `instanceof` should suppress constants and functions.
#[tokio::test]
async fn test_instanceof_excludes_constants_and_functions() {
    let backend = crate::common::create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///test_instanceof_no_fn.php").unwrap();
    let text = "<?php\n$x instanceof str";

    let items = complete_at(&backend, &uri, text, 1, 17).await;

    let func_items: Vec<&CompletionItem> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
        .collect();
    assert!(
        func_items.is_empty(),
        "instanceof should not offer functions, got: {:?}",
        labels(&items)
    );
}

// ─── extends (class) suppresses constants/functions ─────────────────────────

/// `extends` should suppress constants and functions.
#[tokio::test]
async fn test_extends_excludes_constants_and_functions() {
    let backend = crate::common::create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///test_ext_no_fn.php").unwrap();
    let text = "<?php\nclass Foo extends str";

    let items = complete_at(&backend, &uri, text, 1, 24).await;

    let func_items: Vec<&CompletionItem> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
        .collect();
    assert!(
        func_items.is_empty(),
        "extends should not offer functions, got: {:?}",
        labels(&items)
    );
}

// ─── Multi-line declarations ────────────────────────────────────────────────

/// `implements` on the next line after the class declaration.
#[tokio::test]
async fn test_implements_multiline() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_impl_ml.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Scaffold;\n",
        "class Foo\n",
        "    implements Some\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 19).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"SomeInterface"),
        "multi-line implements should offer interfaces, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"ConcreteClass"),
        "multi-line implements should not offer classes, got: {lbls:?}"
    );
}

/// `extends` on the next line after `class Foo`.
#[tokio::test]
async fn test_extends_multiline() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_ext_ml.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Scaffold;\n",
        "class Foo\n",
        "    extends Concrete\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 20).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"ConcreteClass"),
        "multi-line extends should offer classes, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"SomeInterface"),
        "multi-line extends should not offer interfaces, got: {lbls:?}"
    );
}

/// `implements` with comma-separated interfaces across lines.
#[tokio::test]
async fn test_implements_multiline_comma() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_impl_ml_comma.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Scaffold;\n",
        "class Foo implements\n",
        "    SomeInterface,\n",
        "    Another\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 11).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"AnotherInterface"),
        "multi-line implements comma should offer interfaces, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"AnotherTrait"),
        "multi-line implements comma should not offer traits, got: {lbls:?}"
    );
}

// ─── new context still works ────────────────────────────────────────────────

/// The `new` context detection should still work via `detect_class_name_context`.
#[tokio::test]
async fn test_new_context_still_filters() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_new_ctx.php").unwrap();
    let text = "<?php\nnamespace Scaffold;\nnew Some";

    let items = complete_at(&backend, &uri, text, 2, 8).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.contains(&"SomeInterface"),
        "new should not offer interfaces, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"SomeTrait"),
        "new should not offer traits, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"SomeEnum"),
        "new should not offer enums, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"AbstractClass"),
        "new should not offer abstract classes, got: {lbls:?}"
    );
}

// ─── Default context ────────────────────────────────────────────────────────

/// In a plain context (no keyword), all class-like types should appear.
#[tokio::test]
async fn test_plain_context_offers_everything() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_plain.php").unwrap();
    let text = "<?php\nnamespace Scaffold;\nSome";

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"SomeInterface"),
        "plain context should offer interfaces, got: {lbls:?}"
    );
    assert!(
        lbls.contains(&"SomeTrait"),
        "plain context should offer traits, got: {lbls:?}"
    );
    assert!(
        lbls.contains(&"SomeEnum"),
        "plain context should offer enums, got: {lbls:?}"
    );
}

// ─── Unloaded classes pass through ──────────────────────────────────────────

/// Classes that are not loaded (only in class_index) should pass through
/// the filter since their kind is unknown.
#[tokio::test]
async fn test_unloaded_classes_pass_through_filter() {
    let backend = create_test_backend_with_stubs();

    // Put a class in the class_index but do NOT load it into ast_map.
    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "UnknownKind\\MysteryClass".to_string(),
            "file:///vendor/mystery.php".to_string(),
        );
    }
    // Also insert into fqn_class_index so check_context_match can verify the kind.
    {
        let mut idx = backend.fqn_class_index().write();
        idx.insert(
            "UnknownKind\\MysteryClass".to_string(),
            Arc::new(ClassInfo {
                kind: ClassLikeKind::Interface,
                name: atom("MysteryClass"),
                file_namespace: Some(atom("UnknownKind")),
                ..Default::default()
            }),
        );
    }

    let uri = Url::parse("file:///test_unloaded.php").unwrap();
    let text = "<?php\nclass Foo implements Mystery";

    let items = complete_at(&backend, &uri, text, 1, 31).await;
    let cls = class_items(&items);
    let fqns = fqn_labels(&cls);

    assert!(
        fqns.contains(&"UnknownKind\\MysteryClass"),
        "unloaded classes should pass through the filter, got: {fqns:?}"
    );
}

// ─── Stubs pass through ────────────────────────────────────────────────────

/// Stub interfaces are filtered out of extends-class context even before
/// being fully parsed, thanks to the lightweight source scanner.
#[tokio::test]
async fn test_stub_interface_filtered_in_extends_class() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///test_stub_filter.php").unwrap();
    // BackedEnum is an interface stub.  Even without a full parse, the
    // lightweight source scanner (`detect_stub_class_kind`) identifies it
    // as an interface and filters it out of extends-class context.
    let text = "<?php\nclass Foo extends Backed";

    let items = complete_at(&backend, &uri, text, 1, 27).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.contains(&"BackedEnum"),
        "stub interface should be filtered out of extends-class context, got: {lbls:?}"
    );
}

/// After a stub is parsed and loaded into ast_map, the context filter
/// still correctly excludes it (via the fast path in ast_map).
#[tokio::test]
async fn test_parsed_stub_interface_filtered_in_extends_class() {
    let backend = create_test_backend_with_stubs();

    // Force the BackedEnum stub to be parsed by requesting it.
    // We do this by opening a file that references it.
    let ref_uri = Url::parse("file:///ref.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: ref_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: concat!("<?php\n", "enum Color: string { case Red = 'red'; }\n",).to_string(),
            },
        })
        .await;

    // Trigger completion on Color to force enum stub parsing.
    let _ = complete_at(
        &backend,
        &ref_uri,
        concat!(
            "<?php\n",
            "enum Color: string { case Red = 'red'; }\n",
            "Color::Red->",
        ),
        2,
        12,
    )
    .await;

    // BackedEnum is an interface, so extends (class) should filter it
    // out whether detected via ast_map (fast path) or source scan.
    let uri = Url::parse("file:///test_stub_parsed.php").unwrap();
    let text = "<?php\nclass Foo extends Backed";
    let items = complete_at(&backend, &uri, text, 1, 27).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.contains(&"BackedEnum"),
        "parsed stub interface should be filtered out of extends-class context, got: {lbls:?}"
    );
}

// ─── Keyword at unusual positions ───────────────────────────────────────────

/// `instanceof` with extra whitespace before the name.
#[tokio::test]
async fn test_instanceof_extra_whitespace() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_instanceof_ws.php").unwrap();
    let text = "<?php\nnamespace Scaffold;\n$x instanceof    Some";

    let items = complete_at(&backend, &uri, text, 2, 21).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.contains(&"SomeTrait"),
        "instanceof with extra whitespace should still filter traits, got: {lbls:?}"
    );
}

/// `instanceof` should not match partial keyword like `notinstanceof`.
#[tokio::test]
async fn test_instanceof_not_partial_keyword() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_instanceof_partial.php").unwrap();
    // "notinstanceof" is not a valid keyword - make sure we don't
    // accidentally match.
    let text = "<?php\nnamespace Scaffold;\n$x = notinstanceof Some";

    let items = complete_at(&backend, &uri, text, 2, 25).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    // In a non-keyword context, traits should be offered.
    // (The "notinstanceof" is not a real keyword, so it falls through to Any.)
    assert!(
        lbls.contains(&"SomeTrait"),
        "partial keyword match should not filter, got: {lbls:?}"
    );
}

// ─── implements after extends ───────────────────────────────────────────────

/// `class Foo extends Bar implements Baz` — implements should filter.
#[tokio::test]
async fn test_implements_after_extends() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_impl_after_ext.php").unwrap();
    let text = "<?php\nnamespace Scaffold;\nclass Foo extends ConcreteClass implements Some";

    let items = complete_at(&backend, &uri, text, 2, 49).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"SomeInterface"),
        "implements after extends should offer interfaces, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"SomeTrait"),
        "implements after extends should not offer traits, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"ConcreteClass"),
        "implements after extends should not offer classes, got: {lbls:?}"
    );
}

// ─── enum implements ────────────────────────────────────────────────────────

/// Enum `implements` should also only offer interfaces.
#[tokio::test]
async fn test_enum_implements() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_enum_impl.php").unwrap();
    let text = "<?php\nnamespace Scaffold;\nenum Foo implements Some";

    let items = complete_at(&backend, &uri, text, 2, 27).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"SomeInterface"),
        "enum implements should offer interfaces, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"ConcreteClass"),
        "enum implements should not offer classes, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"SomeTrait"),
        "enum implements should not offer traits, got: {lbls:?}"
    );
}

// ─── Unloaded stubs are filtered via source scanning ────────────────────────
//
// The stub source is embedded in memory, so even without parsing into
// ast_map we can scan the raw PHP to determine the declaration kind.

/// A stub class should be excluded from `interface extends` even when
/// it has never been parsed (source scan detects `class` keyword).
#[tokio::test]
async fn test_unloaded_stub_class_excluded_from_extends_interface() {
    let mut stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    stubs.insert(
        "DirectoryIterator",
        "<?php\nclass DirectoryIterator extends SplFileInfo implements SeekableIterator {}\n",
    );
    stubs.insert(
        "SeekableIterator",
        "<?php\ninterface SeekableIterator extends Iterator {}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);

    // Do NOT trigger any resolution — stubs remain unloaded.
    let uri = Url::parse("file:///test_unloaded_stub_ext_iface.php").unwrap();
    let text = "<?php\ninterface A extends Directory";

    let items = complete_at(&backend, &uri, text, 1, 29).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.contains(&"DirectoryIterator"),
        "unloaded stub class should be excluded from interface extends, got: {lbls:?}"
    );
}

/// A stub interface should be included in `interface extends` even when
/// it has never been parsed.
#[tokio::test]
async fn test_unloaded_stub_interface_included_in_extends_interface() {
    let mut stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    stubs.insert(
        "SeekableIterator",
        "<?php\ninterface SeekableIterator extends Iterator {}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///test_unloaded_stub_iface_ok.php").unwrap();
    let text = "<?php\ninterface A extends Seekable";

    let items = complete_at(&backend, &uri, text, 1, 29).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"SeekableIterator"),
        "unloaded stub interface should be included in interface extends, got: {lbls:?}"
    );
}

/// A stub class should be excluded from `use` (trait) context even when
/// unloaded.
#[tokio::test]
async fn test_unloaded_stub_class_excluded_from_trait_use() {
    let mut stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    stubs.insert(
        "DirectoryIterator",
        "<?php\nclass DirectoryIterator extends SplFileInfo {}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///test_unloaded_stub_trait_use.php").unwrap();
    let text = concat!("<?php\n", "class Foo {\n", "    use Directory\n", "}\n",);

    let items = complete_at(&backend, &uri, text, 2, 19).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.contains(&"DirectoryIterator"),
        "unloaded stub class should be excluded from trait use, got: {lbls:?}"
    );
}

/// A stub interface should be excluded from `implements` when the stub
/// is actually a class (source scan detects `class` keyword).
#[tokio::test]
async fn test_unloaded_stub_class_excluded_from_implements() {
    let mut stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    stubs.insert(
        "DirectoryIterator",
        "<?php\nclass DirectoryIterator extends SplFileInfo implements SeekableIterator {}\n",
    );
    stubs.insert("JsonSerializable", "<?php\ninterface JsonSerializable {}\n");
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///test_unloaded_stub_impl.php").unwrap();
    let text = "<?php\nclass Foo implements Directory";

    let items = complete_at(&backend, &uri, text, 1, 31).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.contains(&"DirectoryIterator"),
        "unloaded stub class should be excluded from implements, got: {lbls:?}"
    );
}

/// A stub interface should be included in `implements` when unloaded.
#[tokio::test]
async fn test_unloaded_stub_interface_included_in_implements() {
    let mut stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    stubs.insert("JsonSerializable", "<?php\ninterface JsonSerializable {}\n");
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///test_unloaded_stub_impl_ok.php").unwrap();
    let text = "<?php\nclass Foo implements JsonSerializable";

    let items = complete_at(&backend, &uri, text, 1, 34).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"JsonSerializable"),
        "unloaded stub interface should be included in implements, got: {lbls:?}"
    );
}

/// An unloaded stub interface should be excluded from `new` context.
#[tokio::test]
async fn test_unloaded_stub_interface_excluded_from_new() {
    let mut stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    stubs.insert("SpanInterface", "<?php\ninterface SpanInterface {}\n");
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///test_unloaded_stub_new.php").unwrap();
    let text = "<?php\nnew SpanInterface";

    let items = complete_at(&backend, &uri, text, 1, 17).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.contains(&"SpanInterface"),
        "unloaded stub interface should be excluded from new context, got: {lbls:?}"
    );
}

/// An unloaded abstract stub class should be excluded from `new` context.
#[tokio::test]
async fn test_unloaded_stub_abstract_excluded_from_new() {
    let mut stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    stubs.insert(
        "DatePeriod",
        "<?php\nabstract class DatePeriod implements IteratorAggregate {}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///test_unloaded_stub_abs_new.php").unwrap();
    let text = "<?php\nnew DatePeriod";

    let items = complete_at(&backend, &uri, text, 1, 14).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.contains(&"DatePeriod"),
        "unloaded abstract stub class should be excluded from new context, got: {lbls:?}"
    );
}

/// An unloaded concrete stub class should be included in `new` context.
#[tokio::test]
async fn test_unloaded_stub_concrete_class_included_in_new() {
    let mut stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    stubs.insert(
        "DirectoryIterator",
        "<?php\nclass DirectoryIterator extends SplFileInfo {}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///test_unloaded_stub_cls_new.php").unwrap();
    let text = "<?php\nnew DirectoryIterator";

    let items = complete_at(&backend, &uri, text, 1, 21).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"DirectoryIterator"),
        "unloaded concrete stub class should be included in new context, got: {lbls:?}"
    );
}

/// An unloaded stub class should be excluded from `extends` (interface).
/// This is the exact scenario reported: `interface B extends DirectoryIterator`.
#[tokio::test]
async fn test_unloaded_stub_class_excluded_from_extends_interface_second_use() {
    let mut stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    stubs.insert(
        "DirectoryIterator",
        "<?php\nclass DirectoryIterator extends SplFileInfo {}\n",
    );
    stubs.insert("SpanInterface", "<?php\ninterface SpanInterface {}\n");
    let backend = Backend::new_test_with_stubs(stubs);

    // First use: class extends (class context) — should include the class.
    let uri1 = Url::parse("file:///test_first_use.php").unwrap();
    let text1 = "<?php\nclass A1 extends Directory";
    let items1 = complete_at(&backend, &uri1, text1, 1, 27).await;
    let cls1 = class_items(&items1);
    let lbls1: Vec<&str> = cls1.iter().map(|i| i.label.as_str()).collect();
    assert!(
        lbls1.contains(&"DirectoryIterator"),
        "class extends should include stub class, got: {lbls1:?}"
    );

    // Second use: interface extends — should exclude the class.
    let uri2 = Url::parse("file:///test_second_use.php").unwrap();
    let text2 = "<?php\ninterface B extends Directory";
    let items2 = complete_at(&backend, &uri2, text2, 1, 30).await;
    let cls2 = class_items(&items2);
    let lbls2: Vec<&str> = cls2.iter().map(|i| i.label.as_str()).collect();
    assert!(
        !lbls2.contains(&"DirectoryIterator"),
        "interface extends should exclude stub class, got: {lbls2:?}"
    );

    // Interface extends should include the interface.
    let uri3 = Url::parse("file:///test_third_use.php").unwrap();
    let text3 = "<?php\ninterface C extends Span";
    let items3 = complete_at(&backend, &uri3, text3, 1, 25).await;
    let cls3 = class_items(&items3);
    let lbls3: Vec<&str> = cls3.iter().map(|i| i.label.as_str()).collect();
    assert!(
        lbls3.contains(&"SpanInterface"),
        "interface extends should include stub interface, got: {lbls3:?}"
    );
}

/// `instanceof` should exclude stub traits (even when unloaded).
#[tokio::test]
async fn test_unloaded_stub_trait_excluded_from_instanceof() {
    let mut stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    stubs.insert("SomeSplTrait", "<?php\ntrait SomeSplTrait {}\n");
    stubs.insert("JsonSerializable", "<?php\ninterface JsonSerializable {}\n");
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///test_unloaded_instanceof_trait.php").unwrap();
    let text = "<?php\n$x instanceof SomeSpl";

    let items = complete_at(&backend, &uri, text, 1, 20).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.contains(&"SomeSplTrait"),
        "unloaded stub trait should be excluded from instanceof, got: {lbls:?}"
    );
}

/// `instanceof` should include stub interfaces (even when unloaded).
#[tokio::test]
async fn test_unloaded_stub_interface_included_in_instanceof() {
    let mut stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    stubs.insert("JsonSerializable", "<?php\ninterface JsonSerializable {}\n");
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///test_unloaded_instanceof_iface.php").unwrap();
    let text = "<?php\n$x instanceof JsonSerializable";

    let items = complete_at(&backend, &uri, text, 1, 28).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"JsonSerializable"),
        "unloaded stub interface should be included in instanceof, got: {lbls:?}"
    );
}

/// A stub file with multiple declarations: the scanner should find the
/// right one by name.
#[tokio::test]
async fn test_unloaded_stub_multi_declaration_file() {
    let mut stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    // Both point to the same source file (like real stubs do).
    let multi_source = concat!(
        "<?php\n",
        "interface Countable {\n",
        "    public function count(): int;\n",
        "}\n",
        "class ArrayObject implements IteratorAggregate, ArrayAccess, Countable {\n",
        "}\n",
    );
    stubs.insert("Countable", multi_source);
    stubs.insert("ArrayObject", multi_source);
    let backend = Backend::new_test_with_stubs(stubs);

    // `implements` should include Countable (interface) but exclude ArrayObject (class).
    let uri = Url::parse("file:///test_multi_decl.php").unwrap();
    let text = "<?php\nclass Foo implements Countable";
    let items = complete_at(&backend, &uri, text, 1, 30).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();
    assert!(
        lbls.contains(&"Countable"),
        "implements should include Countable (interface in multi-decl file), got: {lbls:?}"
    );

    let uri2 = Url::parse("file:///test_multi_decl2.php").unwrap();
    let text2 = "<?php\nclass Foo implements ArrayObject";
    let items2 = complete_at(&backend, &uri2, text2, 1, 31).await;
    let cls2 = class_items(&items2);
    let lbls2: Vec<&str> = cls2.iter().map(|i| i.label.as_str()).collect();
    assert!(
        !lbls2.contains(&"ArrayObject"),
        "implements should exclude ArrayObject (class in multi-decl file), got: {lbls2:?}"
    );
}

/// `extends` (class) should include a concrete stub class and exclude
/// a final stub class — all via source scanning, no parsing.
#[tokio::test]
async fn test_unloaded_stub_final_class_excluded_from_extends_class() {
    let mut stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    stubs.insert("FinalStubClass", "<?php\nfinal class FinalStubClass {}\n");
    stubs.insert("NormalStubClass", "<?php\nclass NormalStubClass {}\n");
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///test_final_stub.php").unwrap();
    let text = "<?php\nclass Foo extends FinalStub";
    let items = complete_at(&backend, &uri, text, 1, 27).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();
    assert!(
        !lbls.contains(&"FinalStubClass"),
        "extends (class) should exclude final stub class, got: {lbls:?}"
    );

    let uri2 = Url::parse("file:///test_normal_stub.php").unwrap();
    let text2 = "<?php\nclass Foo extends NormalStub";
    let items2 = complete_at(&backend, &uri2, text2, 1, 28).await;
    let cls2 = class_items(&items2);
    let lbls2: Vec<&str> = cls2.iter().map(|i| i.label.as_str()).collect();
    assert!(
        lbls2.contains(&"NormalStubClass"),
        "extends (class) should include normal stub class, got: {lbls2:?}"
    );
}

// ─── Loaded stubs are filtered ──────────────────────────────────────────────

/// Once a stub class is parsed and in the ast_map, the context filter
/// should exclude it from positions where its kind is invalid.
/// BadUrlException is a class — `interface A extends BadUrlException`
/// should not offer it.
#[tokio::test]
async fn test_loaded_stub_class_excluded_from_extends_interface() {
    let mut stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    stubs.insert(
        "BadUrlException",
        "<?php\nclass BadUrlException extends \\Exception {}\n",
    );
    stubs.insert(
        "SomeStubInterface",
        "<?php\ninterface SomeStubInterface {}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);

    // Force the stubs to be parsed by opening a file that references them.
    let ref_uri = Url::parse("file:///ref_stub.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: ref_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: concat!(
                    "<?php\n",
                    "class Preload extends BadUrlException {}\n",
                    "class Preload2 implements SomeStubInterface {}\n",
                )
                .to_string(),
            },
        })
        .await;

    // Trigger resolution so the stubs get parsed into ast_map.
    let _ = complete_at(
        &backend,
        &ref_uri,
        concat!(
            "<?php\n",
            "class Preload extends BadUrlException {}\n",
            "class Preload2 implements SomeStubInterface {}\n",
            "$x = new Preload(); $x->",
        ),
        3,
        24,
    )
    .await;

    // Now: interface extends should exclude the loaded class stub
    // but include the loaded interface stub.
    let uri = Url::parse("file:///test_loaded_stub.php").unwrap();
    // Use partial "S" which matches SomeStubInterface.
    let text = "<?php\ninterface A extends S";

    let items = complete_at(&backend, &uri, text, 1, 23).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"SomeStubInterface"),
        "loaded interface stub should be included in interface extends, got: {lbls:?}"
    );

    // Also check with partial "Bad" to verify the class is excluded.
    let uri2 = Url::parse("file:///test_loaded_stub2.php").unwrap();
    let text2 = "<?php\ninterface A extends Bad";

    let items2 = complete_at(&backend, &uri2, text2, 1, 25).await;
    let cls2 = class_items(&items2);
    let lbls2: Vec<&str> = cls2.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls2.contains(&"BadUrlException"),
        "loaded class stub should be excluded from interface extends, got: {lbls2:?}"
    );
}

/// A loaded class stub should be excluded from trait-use context.
#[tokio::test]
async fn test_loaded_stub_class_excluded_from_trait_use() {
    let mut stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    stubs.insert(
        "JsonException",
        "<?php\nclass JsonException extends \\Exception {}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);

    // Force the stub to be parsed.
    let ref_uri = Url::parse("file:///ref_stub2.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: ref_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: "<?php\nclass Preload extends JsonException {}\n".to_string(),
            },
        })
        .await;
    let _ = complete_at(
        &backend,
        &ref_uri,
        "<?php\nclass Preload extends JsonException {}\n$x = new Preload(); $x->",
        2,
        24,
    )
    .await;

    let uri = Url::parse("file:///test_loaded_trait_use.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class A extends JsonException {\n",
        "    use Json\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 12).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.contains(&"JsonException"),
        "loaded class stub should be excluded from trait use, got: {lbls:?}"
    );
}

/// After using a symbol once (forcing it to load), the filter should
/// apply on subsequent completions in a different file.
#[tokio::test]
async fn test_loaded_class_filtered_on_second_use() {
    let mut stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    stubs.insert(
        "BadUrlException",
        "<?php\nclass BadUrlException extends \\Exception {}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);

    // First use: `interface A extends BadUrlException {}` — loads the stub.
    let first_uri = Url::parse("file:///first_use.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: first_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: "<?php\nclass Preload extends BadUrlException {}\n".to_string(),
            },
        })
        .await;
    let _ = complete_at(
        &backend,
        &first_uri,
        "<?php\nclass Preload extends BadUrlException {}\n$x = new Preload(); $x->",
        2,
        24,
    )
    .await;

    // Second use in a different file:
    let second_uri = Url::parse("file:///second_use.php").unwrap();
    let text = "<?php\ninterface B extends BadUrl";

    let items = complete_at(&backend, &second_uri, text, 1, 27).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.contains(&"BadUrlException"),
        "after loading, class stub should be filtered from interface extends, got: {lbls:?}"
    );
}

// ─── Classmap entries are filtered when loaded ──────────────────────────────

/// A classmap entry that is loaded as an interface should be excluded
/// from class-extends context.
#[tokio::test]
async fn test_classmap_loaded_interface_excluded_from_extends_class() {
    let backend = create_test_backend();

    // Load an interface into ast_map.
    let iface_uri = Url::parse("file:///app/Contracts/Searchable.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: iface_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: "<?php\nnamespace App\\Contracts;\ninterface Searchable {}\n".to_string(),
            },
        })
        .await;

    // Put it in the class index.
    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "App\\Contracts\\Searchable".to_string(),
            iface_uri.to_string(),
        );
    }

    let uri = Url::parse("file:///test_cmap_filter.php").unwrap();
    let text = "<?php\nclass Foo extends Searchable";

    let items = complete_at(&backend, &uri, text, 1, 28).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.contains(&"App\\Contracts\\Searchable"),
        "loaded interface in classmap should be excluded from class extends, got: {lbls:?}"
    );
}

/// A classmap entry that is loaded as a trait should be excluded from
/// implements context.
#[tokio::test]
async fn test_classmap_loaded_trait_excluded_from_implements() {
    let backend = create_test_backend();

    let trait_uri = Url::parse("file:///app/Traits/Sortable.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: trait_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: "<?php\nnamespace App\\Traits;\ntrait Sortable {}\n".to_string(),
            },
        })
        .await;

    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert("App\\Traits\\Sortable".to_string(), trait_uri.to_string());
    }

    let uri = Url::parse("file:///test_cmap_trait.php").unwrap();
    let text = "<?php\nclass Foo implements Sortable";

    let items = complete_at(&backend, &uri, text, 1, 31).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.contains(&"App\\Traits\\Sortable"),
        "loaded trait in classmap should be excluded from implements, got: {lbls:?}"
    );
}

// ─── Heuristic sort demotion for unloaded classes ───────────────────────────

/// In `extends` (class) context, names that look like interfaces should
/// be demoted (higher sort_text prefix) behind normal names.
#[tokio::test]
async fn test_extends_class_demotes_interface_looking_names() {
    let backend = create_test_backend();

    // The heuristic matches on the SHORT name (last segment after \).
    // "ZxUserInterface" ends with "Interface" → demoted.
    // "IZxLogger" starts with I[A-Z] → demoted.
    // "ZxUserRepository" has no interface/abstract pattern → normal sort.
    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "App\\ZxUserInterface".to_string(),
            "file:///vendor/a.php".to_string(),
        );
        idx.insert(
            "App\\ZxUserRepository".to_string(),
            "file:///vendor/b.php".to_string(),
        );
        idx.insert(
            "App\\IZxLogger".to_string(),
            "file:///vendor/c.php".to_string(),
        );
    }

    let uri = Url::parse("file:///test_ext_demote.php").unwrap();
    // Partial "Zx" matches the first two; separate request for "IZx".
    let text = "<?php\nclass Foo extends Zx";
    let items_zx = complete_at(&backend, &uri, text, 1, 24).await;

    let uri2 = Url::parse("file:///test_ext_demote2.php").unwrap();
    let text2 = "<?php\nclass Foo extends IZx";
    let items_izx = complete_at(&backend, &uri2, text2, 1, 25).await;

    let mut items = items_zx;
    items.extend(items_izx);
    let cls = class_items(&items);

    let repo_item = find_by_fqn(&cls, "App\\ZxUserRepository");
    let iface_item = find_by_fqn(&cls, "App\\ZxUserInterface");
    let ilogger_item = find_by_fqn(&cls, "App\\IZxLogger");

    assert!(repo_item.is_some(), "ZxUserRepository should be present");
    assert!(
        iface_item.is_some(),
        "ZxUserInterface should be present (unloaded, passes through)"
    );
    assert!(
        ilogger_item.is_some(),
        "IZxLogger should be present (unloaded, passes through)"
    );

    // Interface-looking names should have a higher (worse) sort prefix.
    let repo_sort = repo_item.unwrap().sort_text.as_deref().unwrap_or("");
    let iface_sort = iface_item.unwrap().sort_text.as_deref().unwrap_or("");
    let ilogger_sort = ilogger_item.unwrap().sort_text.as_deref().unwrap_or("");

    assert!(
        repo_sort < iface_sort,
        "ZxUserRepository ({repo_sort}) should sort before ZxUserInterface ({iface_sort}) in extends-class context"
    );
    assert!(
        repo_sort < ilogger_sort,
        "ZxUserRepository ({repo_sort}) should sort before IZxLogger ({ilogger_sort}) in extends-class context"
    );
}

/// In `implements` context, names that look like abstract/base classes
/// should be demoted behind normal names.
#[tokio::test]
async fn test_implements_demotes_abstract_looking_names() {
    let backend = create_test_backend();

    // "YxLoggable" has no abstract pattern → normal sort.
    // "AbstractYxHandler" starts with "Abstract" → demoted.
    // "BaseYxController" starts with "Base[A-Z]" → demoted.
    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "App\\YxLoggable".to_string(),
            "file:///vendor/a.php".to_string(),
        );
        idx.insert(
            "App\\AbstractYxHandler".to_string(),
            "file:///vendor/b.php".to_string(),
        );
        idx.insert(
            "App\\BaseYxController".to_string(),
            "file:///vendor/c.php".to_string(),
        );
    }
    // Insert into fqn_class_index so check_context_match can verify their kind.
    {
        let mut idx = backend.fqn_class_index().write();
        idx.insert(
            "App\\YxLoggable".to_string(),
            Arc::new(ClassInfo {
                kind: ClassLikeKind::Interface,
                name: atom("YxLoggable"),
                file_namespace: Some(atom("App")),
                ..Default::default()
            }),
        );
        idx.insert(
            "App\\AbstractYxHandler".to_string(),
            Arc::new(ClassInfo {
                kind: ClassLikeKind::Interface,
                name: atom("AbstractYxHandler"),
                file_namespace: Some(atom("App")),
                ..Default::default()
            }),
        );
        idx.insert(
            "App\\BaseYxController".to_string(),
            Arc::new(ClassInfo {
                kind: ClassLikeKind::Interface,
                name: atom("BaseYxController"),
                file_namespace: Some(atom("App")),
                ..Default::default()
            }),
        );
    }

    let uri = Url::parse("file:///test_impl_demote.php").unwrap();
    // Partial "Yx" matches YxLoggable; separate requests for the others.
    let text_yx = "<?php\nclass Foo implements Yx";
    let items_yx = complete_at(&backend, &uri, text_yx, 1, 27).await;

    let uri2 = Url::parse("file:///test_impl_demote2.php").unwrap();
    let text_abs = "<?php\nclass Foo implements AbstractYx";
    let items_abs = complete_at(&backend, &uri2, text_abs, 1, 35).await;

    let uri3 = Url::parse("file:///test_impl_demote3.php").unwrap();
    let text_base = "<?php\nclass Foo implements BaseYx";
    let items_base = complete_at(&backend, &uri3, text_base, 1, 31).await;

    let mut items = items_yx;
    items.extend(items_abs);
    items.extend(items_base);
    let cls = class_items(&items);

    let loggable_item = find_by_fqn(&cls, "App\\YxLoggable");
    let abstract_item = find_by_fqn(&cls, "App\\AbstractYxHandler");
    let base_item = find_by_fqn(&cls, "App\\BaseYxController");

    assert!(loggable_item.is_some(), "YxLoggable should be present");
    assert!(
        abstract_item.is_some(),
        "AbstractYxHandler should be present (unloaded)"
    );
    assert!(
        base_item.is_some(),
        "BaseYxController should be present (unloaded)"
    );

    let loggable_sort = loggable_item.unwrap().sort_text.as_deref().unwrap_or("");
    let abstract_sort = abstract_item.unwrap().sort_text.as_deref().unwrap_or("");
    let base_sort = base_item.unwrap().sort_text.as_deref().unwrap_or("");

    assert!(
        loggable_sort < abstract_sort,
        "YxLoggable ({loggable_sort}) should sort before AbstractYxHandler ({abstract_sort}) in implements context"
    );
    assert!(
        loggable_sort < base_sort,
        "YxLoggable ({loggable_sort}) should sort before BaseYxController ({base_sort}) in implements context"
    );
}

/// In `use` (trait) context, names that look like interfaces or abstract
/// classes should be demoted.
#[tokio::test]
async fn test_trait_use_demotes_non_trait_looking_names() {
    let backend = create_test_backend();

    // "WxHasTimestamps" has no interface/abstract pattern → normal sort.
    // "WxUserInterface" ends with "Interface" → demoted (likely_non_instantiable).
    // "AbstractWxModel" starts with "Abstract" → demoted (likely_non_instantiable).
    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "App\\WxHasTimestamps".to_string(),
            "file:///vendor/a.php".to_string(),
        );
        idx.insert(
            "App\\WxUserInterface".to_string(),
            "file:///vendor/b.php".to_string(),
        );
        idx.insert(
            "App\\AbstractWxModel".to_string(),
            "file:///vendor/c.php".to_string(),
        );
    }
    // Insert into fqn_class_index so check_context_match can verify their kind.
    {
        let mut idx = backend.fqn_class_index().write();
        idx.insert(
            "App\\WxHasTimestamps".to_string(),
            Arc::new(ClassInfo {
                kind: ClassLikeKind::Trait,
                name: atom("WxHasTimestamps"),
                file_namespace: Some(atom("App")),
                ..Default::default()
            }),
        );
        idx.insert(
            "App\\WxUserInterface".to_string(),
            Arc::new(ClassInfo {
                kind: ClassLikeKind::Trait,
                name: atom("WxUserInterface"),
                file_namespace: Some(atom("App")),
                ..Default::default()
            }),
        );
        idx.insert(
            "App\\AbstractWxModel".to_string(),
            Arc::new(ClassInfo {
                kind: ClassLikeKind::Trait,
                name: atom("AbstractWxModel"),
                file_namespace: Some(atom("App")),
                ..Default::default()
            }),
        );
    }

    let uri = Url::parse("file:///test_use_demote.php").unwrap();
    // Partial "Wx" matches WxHasTimestamps and WxUserInterface.
    let text_wx = concat!("<?php\n", "class Bar {\n", "    use Wx\n", "}\n",);
    let items_wx = complete_at(&backend, &uri, text_wx, 2, 10).await;

    // Separate request for "AbstractWx".
    let uri2 = Url::parse("file:///test_use_demote2.php").unwrap();
    let text_abs = concat!("<?php\n", "class Bar {\n", "    use AbstractWx\n", "}\n",);
    let items_abs = complete_at(&backend, &uri2, text_abs, 2, 18).await;

    let mut items = items_wx;
    items.extend(items_abs);
    let cls = class_items(&items);

    let ts_item = find_by_fqn(&cls, "App\\WxHasTimestamps");
    let iface_item = find_by_fqn(&cls, "App\\WxUserInterface");
    let abs_item = find_by_fqn(&cls, "App\\AbstractWxModel");

    assert!(ts_item.is_some(), "WxHasTimestamps should be present");
    assert!(
        iface_item.is_some(),
        "WxUserInterface should be present (unloaded)"
    );
    assert!(
        abs_item.is_some(),
        "AbstractWxModel should be present (unloaded)"
    );

    let ts_sort = ts_item.unwrap().sort_text.as_deref().unwrap_or("");
    let iface_sort = iface_item.unwrap().sort_text.as_deref().unwrap_or("");
    let abs_sort = abs_item.unwrap().sort_text.as_deref().unwrap_or("");

    assert!(
        ts_sort < iface_sort,
        "WxHasTimestamps ({ts_sort}) should sort before WxUserInterface ({iface_sort}) in trait-use context"
    );
    assert!(
        ts_sort < abs_sort,
        "WxHasTimestamps ({ts_sort}) should sort before AbstractWxModel ({abs_sort}) in trait-use context"
    );
}

/// In `instanceof` context, no heuristic demotion should be applied —
/// classes, interfaces, and enums are all equally valid.
#[tokio::test]
async fn test_instanceof_no_heuristic_demotion() {
    let backend = create_test_backend();

    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "App\\UserInterface".to_string(),
            "file:///vendor/a.php".to_string(),
        );
        idx.insert(
            "App\\UserRepository".to_string(),
            "file:///vendor/b.php".to_string(),
        );
    }

    let uri = Url::parse("file:///test_instanceof_sort.php").unwrap();
    let text = "<?php\n$x instanceof User";

    let items = complete_at(&backend, &uri, text, 1, 21).await;
    let cls = class_items(&items);

    let repo_item = find_by_fqn(&cls, "App\\UserRepository");
    let iface_item = find_by_fqn(&cls, "App\\UserInterface");

    assert!(repo_item.is_some(), "UserRepository should be present");
    assert!(iface_item.is_some(), "UserInterface should be present");

    // sort_text format: {quality}{origin_tier}{source_tier}{affinity:4}{demote}{gap:3}_{name}
    // Demote flag is at position 7.  Neither should be demoted in
    // instanceof context.
    let demote_flag = |item: &CompletionItem| -> char {
        item.sort_text
            .as_deref()
            .and_then(|s| s.chars().nth(7))
            .unwrap_or('?')
    };

    assert_eq!(
        demote_flag(repo_item.unwrap()),
        '0',
        "UserRepository should not be demoted in instanceof context"
    );
    assert_eq!(
        demote_flag(iface_item.unwrap()),
        '0',
        "UserInterface should not be demoted in instanceof context"
    );
}

/// In `extends` (interface) context, names that look like interfaces
/// should NOT be demoted (they're the desired result).
#[tokio::test]
async fn test_extends_interface_does_not_demote_interface_names() {
    let backend = create_test_backend();

    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "App\\LoggerInterface".to_string(),
            "file:///vendor/a.php".to_string(),
        );
        idx.insert(
            "App\\Loggable".to_string(),
            "file:///vendor/b.php".to_string(),
        );
    }
    // Insert into fqn_class_index so check_context_match can verify their kind.
    {
        let mut idx = backend.fqn_class_index().write();
        idx.insert(
            "App\\LoggerInterface".to_string(),
            Arc::new(ClassInfo {
                kind: ClassLikeKind::Interface,
                name: atom("LoggerInterface"),
                file_namespace: Some(atom("App")),
                ..Default::default()
            }),
        );
        idx.insert(
            "App\\Loggable".to_string(),
            Arc::new(ClassInfo {
                kind: ClassLikeKind::Interface,
                name: atom("Loggable"),
                file_namespace: Some(atom("App")),
                ..Default::default()
            }),
        );
    }

    let uri = Url::parse("file:///test_ext_iface_sort.php").unwrap();
    let text = "<?php\ninterface Foo extends Log";

    let items = complete_at(&backend, &uri, text, 1, 28).await;
    let cls = class_items(&items);

    let logger_item = find_by_fqn(&cls, "App\\LoggerInterface");
    let loggable_item = find_by_fqn(&cls, "App\\Loggable");

    assert!(logger_item.is_some(), "LoggerInterface should be present");
    assert!(loggable_item.is_some(), "Loggable should be present");

    // sort_text format: {quality}{origin_tier}{source_tier}{affinity:4}{demote}{gap:3}_{name}
    // Demote flag is at position 7.  Neither should be demoted in
    // extends-interface context.
    let demote_flag = |item: &CompletionItem| -> char {
        item.sort_text
            .as_deref()
            .and_then(|s| s.chars().nth(7))
            .unwrap_or('?')
    };

    assert_eq!(
        demote_flag(logger_item.unwrap()),
        '0',
        "LoggerInterface should not be demoted in interface extends context"
    );
    assert_eq!(
        demote_flag(loggable_item.unwrap()),
        '0',
        "Loggable should not be demoted in interface extends context"
    );
}

// ─── Anonymous classes excluded ─────────────────────────────────────────────

/// Load a file containing an anonymous class into the backend for the
/// anonymous-class exclusion tests below.
async fn load_anon_scaffolding(backend: &Backend) {
    let uri = Url::parse("file:///anon_scaffolding.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri,
                language_id: "php".to_string(),
                version: 1,
                text: concat!(
                    "<?php\n",
                    "namespace AnonTest;\n",
                    "class AnonContainer {\n",
                    "    public function make() {\n",
                    "        return new class extends AnonContainer {};\n",
                    "    }\n",
                    "}\n",
                )
                .to_string(),
            },
        })
        .await;
}

/// In a plain (unfiltered) context, the named class should appear but
/// `__anonymous@*` entries should not.
#[tokio::test]
async fn test_anonymous_class_excluded_plain_context() {
    let backend = create_test_backend();
    load_anon_scaffolding(&backend).await;

    let uri = Url::parse("file:///anon_plain.php").unwrap();
    let text = "<?php\nnamespace AnonTest;\nAnon";
    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    let fqns = fqn_labels(&cls);
    assert!(
        fqns.contains(&"AnonTest\\AnonContainer"),
        "plain context should offer AnonContainer, got: {fqns:?}"
    );
    assert!(
        !lbls.iter().any(|l| l.starts_with("__anonymous")),
        "plain context should not offer anonymous classes, got: {lbls:?}"
    );
}

/// `extends` (class) should not offer `__anonymous@*`.
#[tokio::test]
async fn test_anonymous_class_excluded_extends() {
    let backend = create_test_backend();
    load_anon_scaffolding(&backend).await;

    let uri = Url::parse("file:///anon_ext.php").unwrap();
    let text = "<?php\nnamespace AnonTest;\nclass Foo extends Anon";
    let items = complete_at(&backend, &uri, text, 2, 25).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.iter().any(|l| l.starts_with("__anonymous")),
        "extends (class) should not offer anonymous classes, got: {lbls:?}"
    );
}

/// `implements` should not offer `__anonymous@*`.
#[tokio::test]
async fn test_anonymous_class_excluded_implements() {
    let backend = create_test_backend();
    load_anon_scaffolding(&backend).await;

    let uri = Url::parse("file:///anon_impl.php").unwrap();
    let text = "<?php\nnamespace AnonTest;\nclass Foo implements Anon";
    let items = complete_at(&backend, &uri, text, 2, 28).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.iter().any(|l| l.starts_with("__anonymous")),
        "implements should not offer anonymous classes, got: {lbls:?}"
    );
}

/// `instanceof` should not offer `__anonymous@*`.
#[tokio::test]
async fn test_anonymous_class_excluded_instanceof() {
    let backend = create_test_backend();
    load_anon_scaffolding(&backend).await;

    let uri = Url::parse("file:///anon_instanceof.php").unwrap();
    let text = "<?php\nnamespace AnonTest;\n$x instanceof Anon";
    let items = complete_at(&backend, &uri, text, 2, 18).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.iter().any(|l| l.starts_with("__anonymous")),
        "instanceof should not offer anonymous classes, got: {lbls:?}"
    );
}

/// `new` should not offer `__anonymous@*`.
#[tokio::test]
async fn test_anonymous_class_excluded_new() {
    let backend = create_test_backend();
    load_anon_scaffolding(&backend).await;

    let uri = Url::parse("file:///anon_new.php").unwrap();
    let text = "<?php\nnamespace AnonTest;\nnew Anon";
    let items = complete_at(&backend, &uri, text, 2, 8).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.iter().any(|l| l.starts_with("__anonymous")),
        "new should not offer anonymous classes, got: {lbls:?}"
    );
}

/// `use` (trait) inside a class body should not offer `__anonymous@*`.
#[tokio::test]
async fn test_anonymous_class_excluded_trait_use() {
    let backend = create_test_backend();
    load_anon_scaffolding(&backend).await;

    let uri = Url::parse("file:///anon_trait_use.php").unwrap();
    let text = "<?php\nnamespace AnonTest;\nclass Bar {\n    use Anon\n}\n";
    let items = complete_at(&backend, &uri, text, 3, 12).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.iter().any(|l| l.starts_with("__anonymous")),
        "trait use should not offer anonymous classes, got: {lbls:?}"
    );
}

// ─── Loaded stub filtered across all contexts ───────────────────────────────

/// After a final stub class is loaded (parsed into ast_map), it should be
/// filtered out of every context except `instanceof` and `Any`.
/// This simulates the user's real-world scenario: use a stub class once
/// (e.g. `new V8JsScriptException()`), then trigger completion in
/// extends/implements/use contexts where it does not belong.
#[tokio::test]
async fn test_loaded_final_stub_class_filtered_in_all_contexts() {
    let mut stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    stubs.insert(
        "V8JsScriptException",
        "<?php\nfinal class V8JsScriptException extends \\Exception {}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);

    // Force the stub to be loaded by resolving it through a member access.
    let load_uri = Url::parse("file:///load_stub.php").unwrap();
    let load_text = concat!(
        "<?php\n",
        "class Preload extends V8JsScriptException {}\n",
        "$x = new Preload(); $x->",
    );
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: load_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: load_text.to_string(),
            },
        })
        .await;
    let _ = complete_at(&backend, &load_uri, load_text, 2, 24).await;

    // ── extends (class) — final class must be excluded ──
    let uri1 = Url::parse("file:///test_final_ext_class.php").unwrap();
    let text1 = "<?php\nclass A extends V8Js";
    let items1 = complete_at(&backend, &uri1, text1, 1, 22).await;
    let cls1 = class_items(&items1);
    let lbls1: Vec<&str> = cls1.iter().map(|i| i.label.as_str()).collect();
    assert!(
        !lbls1.contains(&"V8JsScriptException"),
        "loaded final class should be excluded from class extends, got: {lbls1:?}"
    );

    // ── extends (interface) — class must be excluded ──
    let uri2 = Url::parse("file:///test_final_ext_iface.php").unwrap();
    let text2 = "<?php\ninterface B extends V8Js";
    let items2 = complete_at(&backend, &uri2, text2, 1, 26).await;
    let cls2 = class_items(&items2);
    let lbls2: Vec<&str> = cls2.iter().map(|i| i.label.as_str()).collect();
    assert!(
        !lbls2.contains(&"V8JsScriptException"),
        "loaded final class should be excluded from interface extends, got: {lbls2:?}"
    );

    // ── implements — class must be excluded ──
    let uri3 = Url::parse("file:///test_final_impl.php").unwrap();
    let text3 = "<?php\nclass C implements V8Js";
    let items3 = complete_at(&backend, &uri3, text3, 1, 25).await;
    let cls3 = class_items(&items3);
    let lbls3: Vec<&str> = cls3.iter().map(|i| i.label.as_str()).collect();
    assert!(
        !lbls3.contains(&"V8JsScriptException"),
        "loaded final class should be excluded from implements, got: {lbls3:?}"
    );

    // ── use (trait) — class must be excluded ──
    let uri4 = Url::parse("file:///test_final_trait.php").unwrap();
    let text4 = "<?php\nclass D {\n    use V8Js\n}\n";
    let items4 = complete_at(&backend, &uri4, text4, 2, 11).await;
    let cls4 = class_items(&items4);
    let lbls4: Vec<&str> = cls4.iter().map(|i| i.label.as_str()).collect();
    assert!(
        !lbls4.contains(&"V8JsScriptException"),
        "loaded final class should be excluded from trait use, got: {lbls4:?}"
    );

    // ── new — final class IS instantiable, so it SHOULD appear ──
    let uri5 = Url::parse("file:///test_final_new.php").unwrap();
    let text5 = "<?php\nnew V8Js";
    let items5 = complete_at(&backend, &uri5, text5, 1, 8).await;
    let cls5 = class_items(&items5);
    let lbls5: Vec<&str> = cls5.iter().map(|i| i.label.as_str()).collect();
    assert!(
        lbls5.contains(&"V8JsScriptException"),
        "loaded final class should be included in new context, got: {lbls5:?}"
    );

    // ── instanceof — class SHOULD appear ──
    let uri6 = Url::parse("file:///test_final_instanceof.php").unwrap();
    let text6 = "<?php\n$x instanceof V8Js";
    let items6 = complete_at(&backend, &uri6, text6, 1, 21).await;
    let cls6 = class_items(&items6);
    let lbls6: Vec<&str> = cls6.iter().map(|i| i.label.as_str()).collect();
    assert!(
        lbls6.contains(&"V8JsScriptException"),
        "loaded final class should be included in instanceof context, got: {lbls6:?}"
    );
}

// ─── Real multi-class stub source ───────────────────────────────────────────

/// Simulate the real phpstorm-stubs structure: a single source file
/// containing multiple classes (V8Js, V8JsScriptException, etc.).
/// All stub_index keys point to the same source.  Verify that the
/// lightweight scanner and the loaded-class path both filter correctly.
static V8JS_REAL_STUB: &str = "\
<?php

class V8Js
{
    public const V8_VERSION = '';

    public function __construct($object_name = 'PHP', array $variables = []) {}

    /**
     * @return V8JsScriptException|null
     */
    public function getPendingException() {}

    public function executeString($script, $identifier = '') {}
}

final class V8JsScriptException extends Exception
{
    final public function getJsFileName() {}
    final public function getJsLineNumber() {}
}

final class V8JsTimeLimitException extends Exception {}

final class V8JsMemoryLimitException extends Exception {}
";

#[tokio::test]
async fn test_real_multi_class_stub_unloaded_filtering() {
    // All four classes share the same source, just like real stubs.
    let mut stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    stubs.insert("V8Js", V8JS_REAL_STUB);
    stubs.insert("V8JsScriptException", V8JS_REAL_STUB);
    stubs.insert("V8JsTimeLimitException", V8JS_REAL_STUB);
    stubs.insert("V8JsMemoryLimitException", V8JS_REAL_STUB);
    let backend = Backend::new_test_with_stubs(stubs);

    // Do NOT load any stubs — test the lightweight scanner path.

    // extends (class): V8Js is a plain class → included.
    // V8JsScriptException is final → excluded.
    let uri1 = Url::parse("file:///test_v8_ext_class.php").unwrap();
    let text1 = "<?php\nclass A extends V8Js";
    let items1 = complete_at(&backend, &uri1, text1, 1, 22).await;
    let cls1 = class_items(&items1);
    let lbls1: Vec<&str> = cls1.iter().map(|i| i.label.as_str()).collect();
    assert!(
        lbls1.contains(&"V8Js"),
        "non-final class V8Js should be included in class extends, got: {lbls1:?}"
    );
    assert!(
        !lbls1.contains(&"V8JsScriptException"),
        "final class V8JsScriptException should be excluded from class extends, got: {lbls1:?}"
    );
    assert!(
        !lbls1.contains(&"V8JsTimeLimitException"),
        "final class V8JsTimeLimitException should be excluded from class extends, got: {lbls1:?}"
    );

    // extends (interface): all are classes → none should appear.
    let uri2 = Url::parse("file:///test_v8_ext_iface.php").unwrap();
    let text2 = "<?php\ninterface B extends V8Js";
    let items2 = complete_at(&backend, &uri2, text2, 1, 26).await;
    let cls2 = class_items(&items2);
    let lbls2: Vec<&str> = cls2.iter().map(|i| i.label.as_str()).collect();
    assert!(
        !lbls2.contains(&"V8Js"),
        "class V8Js should be excluded from interface extends, got: {lbls2:?}"
    );
    assert!(
        !lbls2.contains(&"V8JsScriptException"),
        "class V8JsScriptException should be excluded from interface extends, got: {lbls2:?}"
    );

    // implements: all are classes → none should appear.
    let uri3 = Url::parse("file:///test_v8_impl.php").unwrap();
    let text3 = "<?php\nclass C implements V8Js";
    let items3 = complete_at(&backend, &uri3, text3, 1, 25).await;
    let cls3 = class_items(&items3);
    let lbls3: Vec<&str> = cls3.iter().map(|i| i.label.as_str()).collect();
    assert!(
        !lbls3.contains(&"V8JsScriptException"),
        "class should be excluded from implements, got: {lbls3:?}"
    );

    // trait use: all are classes → none should appear.
    let uri4 = Url::parse("file:///test_v8_trait.php").unwrap();
    let text4 = "<?php\nclass D {\n    use V8Js\n}\n";
    let items4 = complete_at(&backend, &uri4, text4, 2, 11).await;
    let cls4 = class_items(&items4);
    let lbls4: Vec<&str> = cls4.iter().map(|i| i.label.as_str()).collect();
    assert!(
        !lbls4.contains(&"V8JsScriptException"),
        "class should be excluded from trait use, got: {lbls4:?}"
    );

    // new: V8Js (non-final, non-abstract) → included.
    // V8JsScriptException (final but concrete) → included.
    let uri5 = Url::parse("file:///test_v8_new.php").unwrap();
    let text5 = "<?php\nnew V8Js";
    let items5 = complete_at(&backend, &uri5, text5, 1, 8).await;
    let cls5 = class_items(&items5);
    let lbls5: Vec<&str> = cls5.iter().map(|i| i.label.as_str()).collect();
    assert!(
        lbls5.contains(&"V8Js"),
        "concrete class should be included in new, got: {lbls5:?}"
    );
    assert!(
        lbls5.contains(&"V8JsScriptException"),
        "final concrete class should be included in new, got: {lbls5:?}"
    );

    // instanceof: all classes should appear.
    let uri6 = Url::parse("file:///test_v8_instanceof.php").unwrap();
    let text6 = "<?php\n$x instanceof V8Js";
    let items6 = complete_at(&backend, &uri6, text6, 1, 21).await;
    let cls6 = class_items(&items6);
    let lbls6: Vec<&str> = cls6.iter().map(|i| i.label.as_str()).collect();
    assert!(
        lbls6.contains(&"V8Js"),
        "class should be included in instanceof, got: {lbls6:?}"
    );
    assert!(
        lbls6.contains(&"V8JsScriptException"),
        "class should be included in instanceof, got: {lbls6:?}"
    );
}

#[tokio::test]
async fn test_real_multi_class_stub_loaded_filtering() {
    let mut stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    stubs.insert("V8Js", V8JS_REAL_STUB);
    stubs.insert("V8JsScriptException", V8JS_REAL_STUB);
    stubs.insert("V8JsTimeLimitException", V8JS_REAL_STUB);
    stubs.insert("V8JsMemoryLimitException", V8JS_REAL_STUB);
    let backend = Backend::new_test_with_stubs(stubs);

    // Load the stub by resolving V8JsScriptException through member access.
    let load_uri = Url::parse("file:///load_v8.php").unwrap();
    let load_text = concat!(
        "<?php\n",
        "class Preload extends V8JsScriptException {}\n",
        "$x = new Preload(); $x->",
    );
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: load_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: load_text.to_string(),
            },
        })
        .await;
    let _ = complete_at(&backend, &load_uri, load_text, 2, 24).await;

    // After loading, the same filtering should apply via the ast_map fast path.

    // extends (class): final classes excluded, V8Js included.
    let uri1 = Url::parse("file:///test_v8_loaded_ext.php").unwrap();
    let text1 = "<?php\nclass A extends V8Js";
    let items1 = complete_at(&backend, &uri1, text1, 1, 22).await;
    let cls1 = class_items(&items1);
    let lbls1: Vec<&str> = cls1.iter().map(|i| i.label.as_str()).collect();
    assert!(
        lbls1.contains(&"V8Js"),
        "loaded non-final class V8Js should be included in class extends, got: {lbls1:?}"
    );
    assert!(
        !lbls1.contains(&"V8JsScriptException"),
        "loaded final class should be excluded from class extends, got: {lbls1:?}"
    );

    // implements: all are classes → excluded.
    let uri2 = Url::parse("file:///test_v8_loaded_impl.php").unwrap();
    let text2 = "<?php\nclass C implements V8Js";
    let items2 = complete_at(&backend, &uri2, text2, 1, 25).await;
    let cls2 = class_items(&items2);
    let lbls2: Vec<&str> = cls2.iter().map(|i| i.label.as_str()).collect();
    assert!(
        !lbls2.contains(&"V8JsScriptException"),
        "loaded class should be excluded from implements, got: {lbls2:?}"
    );

    // new: final concrete classes are instantiable.
    let uri3 = Url::parse("file:///test_v8_loaded_new.php").unwrap();
    let text3 = "<?php\nnew V8Js";
    let items3 = complete_at(&backend, &uri3, text3, 1, 8).await;
    let cls3 = class_items(&items3);
    let lbls3: Vec<&str> = cls3.iter().map(|i| i.label.as_str()).collect();
    assert!(
        lbls3.contains(&"V8JsScriptException"),
        "loaded final concrete class should be included in new, got: {lbls3:?}"
    );
}

// ─── use import context ─────────────────────────────────────────────────────

/// Top-level `use` should only offer class-like names, not constants or
/// functions (unlike the plain/Any context).
#[tokio::test]
async fn test_use_import_excludes_constants_and_functions() {
    let backend = create_test_backend_with_stubs();

    let scaffolding_uri = Url::parse("file:///use_scaffold.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: scaffolding_uri,
                language_id: "php".to_string(),
                version: 1,
                text: concat!("<?php\n", "namespace Scaffold;\n", "class SomeWidget {}\n",)
                    .to_string(),
            },
        })
        .await;

    // Register a global function so we can verify it's excluded.
    {
        let mut fmap = backend.global_functions().write();
        fmap.insert(
            "some_widget_func".to_string(),
            (
                "file:///funcs.php".to_string(),
                phpantom_lsp::types::FunctionInfo {
                    name: atom("some_widget_func"),
                    name_offset: 0,
                    parameters: vec![],
                    return_type: None,
                    native_return_type: None,
                    description: None,
                    return_description: None,
                    links: vec![],
                    see_refs: vec![],
                    namespace: None,
                    conditional_return: None,
                    type_assertions: vec![],
                    deprecation_message: None,
                    deprecated_replacement: None,
                    template_params: vec![],
                    template_bindings: vec![],
                    template_param_bounds: Default::default(),
                    throws: vec![],
                    is_polyfill: false,
                    overloads: vec![],
                },
            ),
        );
    }

    let uri = Url::parse("file:///use_test.php").unwrap();
    // `use Some` — top-level use import context.
    let text = concat!("<?php\n", "use Some\n",);

    let items = complete_at(&backend, &uri, text, 1, 8).await;

    let has_class = items
        .iter()
        .any(|i| i.kind == Some(CompletionItemKind::CLASS));
    let has_function = items
        .iter()
        .any(|i| i.kind == Some(CompletionItemKind::FUNCTION));

    assert!(has_class, "use import should offer classes, got: {items:?}");
    assert!(
        !has_function,
        "use import should NOT offer functions, got labels: {:?}",
        labels(&items)
    );
}

/// Typing `use f` should inject "function" as a keyword suggestion.
#[tokio::test]
async fn test_use_import_suggests_function_keyword() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///use_f.php").unwrap();
    let text = concat!("<?php\n", "use f\n",);

    let items = complete_at(&backend, &uri, text, 1, 5).await;

    let kw_items: Vec<&CompletionItem> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
        .collect();
    let kw_labels: Vec<&str> = kw_items.iter().map(|i| i.label.as_str()).collect();

    assert!(
        kw_labels.contains(&"function"),
        "`use f` should suggest 'function' keyword, got: {kw_labels:?}"
    );

    // The keyword insert_text should include a trailing space.
    let func_kw = kw_items.iter().find(|i| i.label == "function").unwrap();
    assert_eq!(
        func_kw.insert_text.as_deref(),
        Some("function "),
        "function keyword insert_text should have trailing space"
    );
}

/// Typing `use c` should inject "const" as a keyword suggestion.
#[tokio::test]
async fn test_use_import_suggests_const_keyword() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///use_c.php").unwrap();
    let text = concat!("<?php\n", "use c\n",);

    let items = complete_at(&backend, &uri, text, 1, 5).await;

    let kw_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        kw_labels.contains(&"const"),
        "`use c` should suggest 'const' keyword, got: {kw_labels:?}"
    );
}

/// Typing `use F` (uppercase) should NOT suggest the "function" keyword
/// because PHP keywords are case-sensitive lowercase.
#[tokio::test]
async fn test_use_import_no_keyword_for_uppercase() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///use_upper.php").unwrap();
    let text = concat!("<?php\n", "use F\n",);

    let items = complete_at(&backend, &uri, text, 1, 5).await;

    let kw_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        !kw_labels.contains(&"function"),
        "`use F` (uppercase) should NOT suggest 'function', got: {kw_labels:?}"
    );
}

/// `use function` context should only show functions, not classes.
#[tokio::test]
async fn test_use_function_shows_only_functions() {
    let backend = create_test_backend_with_stubs();

    // Register a function that will match our partial.
    {
        let mut fmap = backend.global_functions().write();
        fmap.insert(
            "array_merge".to_string(),
            (
                "file:///funcs.php".to_string(),
                phpantom_lsp::types::FunctionInfo {
                    name: atom("array_merge"),
                    name_offset: 0,
                    parameters: vec![],
                    return_type: Some(PhpType::parse("array")),
                    native_return_type: None,
                    description: None,
                    return_description: None,
                    links: vec![],
                    see_refs: vec![],
                    namespace: None,
                    conditional_return: None,
                    type_assertions: vec![],
                    deprecation_message: None,
                    deprecated_replacement: None,
                    template_params: vec![],
                    template_bindings: vec![],
                    template_param_bounds: Default::default(),
                    throws: vec![],
                    is_polyfill: false,
                    overloads: vec![],
                },
            ),
        );
    }

    let uri = Url::parse("file:///use_func.php").unwrap();
    let text = concat!("<?php\n", "use function array_m\n",);

    let items = complete_at(&backend, &uri, text, 1, 20).await;

    let has_function = items
        .iter()
        .any(|i| i.kind == Some(CompletionItemKind::FUNCTION));
    let has_class = items
        .iter()
        .any(|i| i.kind == Some(CompletionItemKind::CLASS));

    assert!(
        has_function,
        "`use function` should offer functions, got: {:?}",
        labels(&items)
    );
    assert!(
        !has_class,
        "`use function` should NOT offer classes, got: {:?}",
        labels(&items)
    );
}

/// `use const` context should only show constants, not classes or functions.
#[tokio::test]
async fn test_use_const_shows_only_constants() {
    let backend = create_test_backend_with_stubs();

    // Register a constant that will match our partial.
    {
        let mut dmap = backend.global_defines().write();
        dmap.insert(
            "APP_VERSION".to_string(),
            phpantom_lsp::DefineInfo {
                file_uri: "file:///config.php".to_string(),
                name_offset: 0,
                value: Some("'1.0.0'".to_string()),
            },
        );
    }

    let uri = Url::parse("file:///use_const.php").unwrap();
    let text = concat!("<?php\n", "use const APP_V\n",);

    let items = complete_at(&backend, &uri, text, 1, 15).await;

    let has_constant = items
        .iter()
        .any(|i| i.kind == Some(CompletionItemKind::CONSTANT));
    let has_class = items
        .iter()
        .any(|i| i.kind == Some(CompletionItemKind::CLASS));
    let has_function = items
        .iter()
        .any(|i| i.kind == Some(CompletionItemKind::FUNCTION));

    assert!(
        has_constant,
        "`use const` should offer constants, got: {:?}",
        labels(&items)
    );
    assert!(
        !has_class,
        "`use const` should NOT offer classes, got: {:?}",
        labels(&items)
    );
    assert!(
        !has_function,
        "`use const` should NOT offer functions, got: {:?}",
        labels(&items)
    );
}

/// Trait `use` inside a class body should NOT be affected by the new
/// When typing `use De` and picking `Decimal\Decimal`, the result should be
/// `use Decimal\Decimal;` — NOT `use Decimal;` with a redundant additional
/// `use Decimal\Decimal;` text edit.
#[tokio::test]
async fn test_use_import_inserts_fqn_no_redundant_text_edit() {
    let backend = create_test_backend();

    // Load a namespaced class so it appears in completions.
    let scaffolding_uri = Url::parse("file:///decimal_scaffolding.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: scaffolding_uri,
                language_id: "php".to_string(),
                version: 1,
                text: "<?php\nnamespace Decimal;\nclass Decimal {}\n".to_string(),
            },
        })
        .await;

    let uri = Url::parse("file:///test_use_fqn.php").unwrap();
    let text = "<?php\nuse De";

    let items = complete_at(&backend, &uri, text, 1, 6).await;
    let cls = class_items(&items);
    let decimal = cls
        .iter()
        .find(|i| i.detail.as_deref() == Some("Decimal\\Decimal"))
        .expect("should find Decimal\\Decimal in completions");

    // The insert text must be the FQN (not just the short name).
    let insert = decimal.insert_text.as_deref().unwrap_or("");
    assert!(
        insert.contains("Decimal\\Decimal"),
        "insert_text should be the FQN, got: {insert:?}"
    );

    // There must be NO additional text edit (no redundant `use` statement).
    assert!(
        decimal.additional_text_edits.is_none()
            || decimal.additional_text_edits.as_ref().unwrap().is_empty(),
        "should not generate a redundant use text edit, got: {:?}",
        decimal.additional_text_edits
    );
}

/// Same-namespace classes should still show the FQN in UseImport context,
/// not a simplified relative name (which would be invalid in a `use` statement).
#[tokio::test]
async fn test_use_import_same_namespace_still_uses_fqn() {
    let backend = create_test_backend();

    // Load a class in namespace App\Models.
    let scaffolding_uri = Url::parse("file:///models_scaffolding.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: scaffolding_uri,
                language_id: "php".to_string(),
                version: 1,
                text: "<?php\nnamespace App\\Models;\nclass Order {}\n".to_string(),
            },
        })
        .await;

    // The test file is in the SAME namespace.
    let uri = Url::parse("file:///test_same_ns_use.php").unwrap();
    let text = "<?php\nnamespace App\\Models;\nuse App\\Models\\Or";

    let items = complete_at(&backend, &uri, text, 2, 19).await;
    let cls = class_items(&items);
    let order = cls
        .iter()
        .find(|i| i.detail.as_deref() == Some("App\\Models\\Order"))
        .expect("should find App\\Models\\Order in completions");

    // The insert text must be the full FQN, not just "Order".
    let insert = order.insert_text.as_deref().unwrap_or("");
    assert!(
        insert.contains("App\\Models\\Order"),
        "insert_text should be the full FQN even in same namespace, got: {insert:?}"
    );
}

/// UseImport logic — it should still only offer traits.
#[tokio::test]
async fn test_trait_use_not_affected_by_use_import() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///trait_use_unchanged.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Scaffold;\n",
        "class Foo {\n",
        "    use Some\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 12).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"SomeTrait"),
        "trait use should still offer traits, got: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"ConcreteClass"),
        "trait use should still exclude classes, got: {lbls:?}"
    );
}

/// Use-map entries that resolve to stub interfaces or classes must be
/// filtered out in `TraitUse` context.  Previously, unloaded stubs in
/// the use-map bypassed the context filter because
/// `matches_context_or_unloaded` returned `true` for anything not in
/// the ast_map.
#[tokio::test]
async fn test_trait_use_filters_stub_use_map_entries() {
    use std::collections::HashMap;

    let mut stubs: HashMap<&'static str, &'static str> = HashMap::new();
    // Stringable is an interface — must NOT appear in trait-use.
    stubs.insert(
        "Stringable",
        "<?php\ninterface Stringable {\n    public function __toString(): string;\n}\n",
    );
    // Exception is a class — must NOT appear in trait-use.
    stubs.insert(
        "Cassandra\\Exception",
        "<?php\nnamespace Cassandra;\nclass Exception extends \\Exception {}\n",
    );
    // A stub trait — MUST appear in trait-use.
    stubs.insert(
        "Cassandra\\RetryPolicy",
        "<?php\nnamespace Cassandra;\ntrait RetryPolicy {}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///test_trait_use_stub_usemap.php").unwrap();
    // Test with partial "S" — matches Stringable (interface, should be
    // excluded) but not RetryPolicy or Exception.
    let text_s = concat!(
        "<?php\n",
        "namespace Demo;\n",
        "use Stringable;\n",
        "use Cassandra\\Exception;\n",
        "use Cassandra\\RetryPolicy;\n",
        "class A {\n",
        "    use S\n",
        "}\n",
    );
    let items_s = complete_at(&backend, &uri, text_s, 6, 9).await;
    let cls_s = class_items(&items_s);
    let lbls_s: Vec<&str> = cls_s.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls_s.contains(&"Stringable"),
        "stub interface Stringable should be excluded from trait use, got: {lbls_s:?}"
    );

    // Test with partial "E" — matches Exception (class, should be
    // excluded) but not RetryPolicy or Stringable.
    let uri_e = Url::parse("file:///test_trait_use_stub_usemap_e.php").unwrap();
    let text_e = concat!(
        "<?php\n",
        "namespace Demo;\n",
        "use Stringable;\n",
        "use Cassandra\\Exception;\n",
        "use Cassandra\\RetryPolicy;\n",
        "class A {\n",
        "    use E\n",
        "}\n",
    );
    let items_e = complete_at(&backend, &uri_e, text_e, 6, 9).await;
    let cls_e = class_items(&items_e);
    let lbls_e: Vec<&str> = cls_e.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls_e.contains(&"Cassandra\\Exception"),
        "stub class Cassandra\\Exception should be excluded from trait use, got: {lbls_e:?}"
    );

    // Test with partial "R" — matches RetryPolicy (trait, should be
    // included).
    let uri_r = Url::parse("file:///test_trait_use_stub_usemap_r.php").unwrap();
    let text_r = concat!(
        "<?php\n",
        "namespace Demo;\n",
        "use Stringable;\n",
        "use Cassandra\\Exception;\n",
        "use Cassandra\\RetryPolicy;\n",
        "class A {\n",
        "    use R\n",
        "}\n",
    );
    let items_r = complete_at(&backend, &uri_r, text_r, 6, 9).await;
    let cls_r = class_items(&items_r);
    let fqns_r = fqn_labels(&cls_r);

    assert!(
        fqns_r.contains(&"Cassandra\\RetryPolicy"),
        "stub trait RetryPolicy should be included in trait use, got: {fqns_r:?}"
    );
}

/// Use-map entries whose FQN is not found in any class source (ast_map,
/// class_index, classmap, stub_index) must be rejected in narrow
/// contexts like `TraitUse`.  A non-existent import like
/// `use Cassandra\ExceptionInterface;` should not pollute trait-use
/// completions.
#[tokio::test]
async fn test_trait_use_rejects_unknown_use_map_entries() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_trait_use_unknown.php").unwrap();
    // `Cassandra\ExceptionInterface` does not exist in any index.
    let text = concat!(
        "<?php\n",
        "namespace Demo;\n",
        "use Cassandra\\ExceptionInterface;\n",
        "class A {\n",
        "    use E\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 9).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.contains(&"Cassandra\\ExceptionInterface"),
        "unknown use-map entry should be excluded from trait use, got: {lbls:?}"
    );
}
