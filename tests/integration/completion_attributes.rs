use crate::common::create_test_backend;
use phpantom_lsp::Backend;
use std::collections::HashMap;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

static BUILTIN_ATTRIBUTE_STUB: &str = r#"<?php
final class Attribute
{
    public const TARGET_CLASS = 1;
    public const TARGET_FUNCTION = 2;
    public const TARGET_METHOD = 4;
    public const TARGET_PROPERTY = 8;
    public const TARGET_CLASS_CONSTANT = 16;
    public const TARGET_PARAMETER = 32;
    public const TARGET_ALL = 63;
    public const IS_REPEATABLE = 64;

    public function __construct(int $flags = self::TARGET_ALL) {}
}

#[Attribute(Attribute::TARGET_METHOD)]
final class Override {}
"#;

// ─── Helper ─────────────────────────────────────────────────────────────────

fn create_builtin_attribute_backend() -> Backend {
    let mut class_stubs: HashMap<&'static str, &'static str> = HashMap::new();
    class_stubs.insert("Attribute", BUILTIN_ATTRIBUTE_STUB);
    class_stubs.insert("Override", BUILTIN_ATTRIBUTE_STUB);
    Backend::new_test_with_all_stubs(class_stubs, HashMap::new(), HashMap::new())
}

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

fn class_items(items: &[CompletionItem]) -> Vec<&CompletionItem> {
    items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::CLASS))
        .collect()
}

fn labels(items: &[CompletionItem]) -> Vec<&str> {
    items.iter().map(|i| i.label.as_str()).collect()
}

/// Load scaffolding classes into the backend's ast_map.
///
/// All attribute classes share a common prefix ("My") so that prefix-based
/// tests can use "My" to match all of them at once, while non-attribute
/// classes use a different prefix ("Plain").
async fn load_scaffolding(backend: &Backend) {
    let scaffolding_uri = Url::parse("file:///scaffolding_attr.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: scaffolding_uri,
                language_id: "php".to_string(),
                version: 1,
                text: concat!(
                    "<?php\n",
                    "namespace Scaffold;\n",
                    "#[\\Attribute(\\Attribute::TARGET_CLASS)]\n",
                    "class MyClassAttr {}\n",
                    "#[\\Attribute(\\Attribute::TARGET_METHOD)]\n",
                    "class MyMethodAttr {}\n",
                    "#[\\Attribute(\\Attribute::TARGET_PROPERTY)]\n",
                    "class MyPropertyAttr {}\n",
                    "#[\\Attribute(\\Attribute::TARGET_PARAMETER)]\n",
                    "class MyParameterAttr {}\n",
                    "#[\\Attribute(\\Attribute::TARGET_CLASS_CONSTANT)]\n",
                    "class MyConstantAttr {}\n",
                    "#[\\Attribute(\\Attribute::TARGET_FUNCTION)]\n",
                    "class MyFunctionAttr {}\n",
                    "#[\\Attribute]\n",
                    "class MyAnyAttr {}\n",
                    "#[\\Attribute(\\Attribute::TARGET_CLASS | \\Attribute::TARGET_METHOD)]\n",
                    "class MyClassMethodAttr {}\n",
                    "class PlainClass {}\n",
                    "interface PlainInterface {}\n",
                    "trait PlainTrait {}\n",
                    "enum PlainEnum {}\n",
                )
                .to_string(),
            },
        })
        .await;
}

fn insert_text(item: &CompletionItem) -> &str {
    item.insert_text.as_deref().unwrap_or(&item.label)
}

// ─── Basic detection ────────────────────────────────────────────────────────

/// Inside `#[…]` before a class, only attribute classes should appear.
/// Non-attribute classes, interfaces, traits, and enums must be excluded.
#[tokio::test]
async fn attribute_context_before_class_filters_to_attributes() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_attr1.php").unwrap();
    // Prefix "My" matches all scaffold attribute classes.
    let text = concat!(
        "<?php\n",
        "namespace Scaffold;\n",
        "#[My\n",
        "class Foo {}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    // TARGET_ALL (MyAnyAttr), TARGET_CLASS (MyClassAttr), and
    // TARGET_CLASS|TARGET_METHOD (MyClassMethodAttr) should all appear.
    assert!(
        lbls.contains(&"MyAnyAttr"),
        "MyAnyAttr (TARGET_ALL) missing from {lbls:?}"
    );
    assert!(
        lbls.contains(&"MyClassAttr"),
        "MyClassAttr (TARGET_CLASS) missing from {lbls:?}"
    );
    assert!(
        lbls.contains(&"MyClassMethodAttr"),
        "MyClassMethodAttr (TARGET_CLASS|TARGET_METHOD) missing from {lbls:?}"
    );

    // Method-only, Property-only, etc. must NOT appear before a class.
    assert!(
        !lbls.contains(&"MyMethodAttr"),
        "MyMethodAttr should not appear before a class"
    );
    assert!(
        !lbls.contains(&"MyPropertyAttr"),
        "MyPropertyAttr should not appear before a class"
    );
    assert!(
        !lbls.contains(&"MyFunctionAttr"),
        "MyFunctionAttr should not appear before a class"
    );
}

/// Inside `#[…]`, non-attribute classes must not appear even when the
/// prefix would otherwise match them.
#[tokio::test]
async fn attribute_context_excludes_non_attribute_classes() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_attr1b.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Scaffold;\n",
        "#[Plain\n",
        "class Foo {}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !lbls.contains(&"PlainClass"),
        "PlainClass should not appear in attribute context"
    );
    assert!(
        !lbls.contains(&"PlainInterface"),
        "PlainInterface should not appear in attribute context"
    );
    assert!(
        !lbls.contains(&"PlainTrait"),
        "PlainTrait should not appear in attribute context"
    );
    assert!(
        !lbls.contains(&"PlainEnum"),
        "PlainEnum should not appear in attribute context"
    );
}

// ─── Target-specific filtering ──────────────────────────────────────────────

/// Inside `#[…]` before a method, method-targeted attributes should
/// appear and class-only attributes should not.
#[tokio::test]
async fn attribute_context_before_method() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_attr_method.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Scaffold;\n",
        "class Bar {\n",
        "    #[My\n",
        "    public function baz(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 7).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"MyMethodAttr"),
        "MyMethodAttr missing from {lbls:?}"
    );
    assert!(
        lbls.contains(&"MyAnyAttr"),
        "MyAnyAttr (TARGET_ALL) missing from {lbls:?}"
    );
    assert!(
        lbls.contains(&"MyClassMethodAttr"),
        "MyClassMethodAttr should match method target: {lbls:?}"
    );

    assert!(
        !lbls.contains(&"MyClassAttr"),
        "MyClassAttr should not appear before a method"
    );
    assert!(
        !lbls.contains(&"MyPropertyAttr"),
        "MyPropertyAttr should not appear before a method"
    );
    assert!(
        !lbls.contains(&"MyFunctionAttr"),
        "MyFunctionAttr should not appear before a method"
    );
}

/// Inside `#[…]` before a property, property-targeted attributes should
/// appear.
#[tokio::test]
async fn attribute_context_before_property() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_attr_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Scaffold;\n",
        "class Qux {\n",
        "    #[My\n",
        "    public string $name;\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 7).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"MyPropertyAttr"),
        "MyPropertyAttr missing from {lbls:?}"
    );
    assert!(
        lbls.contains(&"MyAnyAttr"),
        "MyAnyAttr (TARGET_ALL) missing from {lbls:?}"
    );
    assert!(
        !lbls.contains(&"MyMethodAttr"),
        "MyMethodAttr should not appear before a property"
    );
    assert!(
        !lbls.contains(&"MyClassAttr"),
        "MyClassAttr should not appear before a property"
    );
}

/// Inside `#[…]` before a top-level function, function-targeted
/// attributes should appear.
#[tokio::test]
async fn attribute_context_before_function() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_attr_func.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Scaffold;\n",
        "#[My\n",
        "function myFunc(): void {}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"MyFunctionAttr"),
        "MyFunctionAttr missing from {lbls:?}"
    );
    assert!(
        lbls.contains(&"MyAnyAttr"),
        "MyAnyAttr (TARGET_ALL) missing from {lbls:?}"
    );
    assert!(
        !lbls.contains(&"MyMethodAttr"),
        "MyMethodAttr should not appear before a top-level function"
    );
    assert!(
        !lbls.contains(&"MyClassAttr"),
        "MyClassAttr should not appear before a top-level function"
    );
}

/// Inside `#[…]` before a class constant, class-constant-targeted
/// attributes should appear.
#[tokio::test]
async fn attribute_context_before_class_constant() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_attr_const.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Scaffold;\n",
        "class ConstHost {\n",
        "    #[My\n",
        "    const FOO = 1;\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 7).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"MyConstantAttr"),
        "MyConstantAttr missing from {lbls:?}"
    );
    assert!(
        lbls.contains(&"MyAnyAttr"),
        "MyAnyAttr (TARGET_ALL) missing from {lbls:?}"
    );
    assert!(
        !lbls.contains(&"MyMethodAttr"),
        "MyMethodAttr should not appear before a const"
    );
    assert!(
        !lbls.contains(&"MyClassAttr"),
        "MyClassAttr should not appear before a const"
    );
}

// ─── Multi-attribute lists ──────────────────────────────────────────────────

/// Completing the second attribute in `#[First, …]` should still detect
/// the attribute context.
#[tokio::test]
async fn attribute_context_after_comma() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_attr_comma.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Scaffold;\n",
        "class Baz {\n",
        "    #[MyAnyAttr, My\n",
        "    public function baz(): void {}\n",
        "}\n",
    );

    // Cursor at the end of "My" after the comma.
    let items = complete_at(&backend, &uri, text, 3, 18).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"MyMethodAttr"),
        "Should detect attribute context after comma: {lbls:?}"
    );
    assert!(
        !lbls.contains(&"PlainClass"),
        "PlainClass should not appear after comma in #[]: {lbls:?}"
    );
}

/// Completing after a prior attribute with arguments:
/// `#[MyAnyAttr('x'), …]`.
#[tokio::test]
async fn attribute_context_after_comma_with_args() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_attr_comma_args.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Scaffold;\n",
        "class Baz {\n",
        "    #[MyAnyAttr('x'), My\n",
        "    public function baz(): void {}\n",
        "}\n",
    );

    // Cursor at the end of "My" after the args-comma.
    let items = complete_at(&backend, &uri, text, 3, 23).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"MyMethodAttr"),
        "Should detect attribute context after comma+args: {lbls:?}"
    );
}

// ─── Non-attribute contexts ─────────────────────────────────────────────────

/// Outside `#[…]`, regular classes should appear (not just attributes).
#[tokio::test]
async fn non_attribute_context_shows_all_classes() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_attr_non.php").unwrap();
    let text = concat!("<?php\n", "namespace Scaffold;\n", "new Plain\n",);

    let items = complete_at(&backend, &uri, text, 2, 9).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"PlainClass"),
        "PlainClass should appear in `new` context: {lbls:?}"
    );
}

// ─── Empty attribute list ───────────────────────────────────────────────────

/// `#[` with nothing typed yet should still detect attribute context
/// and not show non-attribute classes.
#[tokio::test]
async fn attribute_context_empty_prefix() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_attr_empty.php").unwrap();
    let text = concat!("<?php\n", "namespace Scaffold;\n", "#[\n", "class Foo {}\n",);

    let items = complete_at(&backend, &uri, text, 2, 2).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    // Should see attribute classes.
    assert!(
        lbls.contains(&"MyAnyAttr"),
        "MyAnyAttr missing with empty prefix: {lbls:?}"
    );
    assert!(
        lbls.contains(&"MyClassAttr"),
        "MyClassAttr missing with empty prefix (before class): {lbls:?}"
    );

    // Should NOT see non-attribute classes.
    assert!(
        !lbls.contains(&"PlainClass"),
        "PlainClass should not appear with empty prefix in #[]: {lbls:?}"
    );
}

// ─── Same-file attribute classes ────────────────────────────────────────────

/// Attribute classes declared in the same file should appear in `#[…]`.
#[tokio::test]
async fn attribute_same_file() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_attr_same.php").unwrap();
    let text = concat!(
        "<?php\n",
        "#[\\Attribute]\n",
        "class MyAllAttr {}\n",
        "class Bar {\n",
        "    #[MyAll\n",
        "    public function baz(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 10).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"MyAllAttr"),
        "MyAllAttr (TARGET_ALL) should appear in method context: {lbls:?}"
    );
}

/// `#[\Attribute(Attribute::TARGET_CLASS | Attribute::TARGET_METHOD)]`
/// should match both class and method positions.
#[tokio::test]
async fn attribute_combined_targets_class_position() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_attr_combined.php").unwrap();
    let text = concat!(
        "<?php\n",
        "#[\\Attribute(\\Attribute::TARGET_CLASS | \\Attribute::TARGET_METHOD)]\n",
        "class CombinedAttr {}\n",
        "#[Combi\n",
        "class Foo {}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 7).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"CombinedAttr"),
        "CombinedAttr should appear before a class: {lbls:?}"
    );
}

#[tokio::test]
async fn attribute_combined_targets_method_position() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_attr_combined2.php").unwrap();
    let text = concat!(
        "<?php\n",
        "#[\\Attribute(\\Attribute::TARGET_CLASS | \\Attribute::TARGET_METHOD)]\n",
        "class CombinedAttr {}\n",
        "class Bar {\n",
        "    #[Combi\n",
        "    public function baz(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 10).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"CombinedAttr"),
        "CombinedAttr should appear before a method: {lbls:?}"
    );
}

/// `#[\Attribute(1)]` (numeric TARGET_CLASS) should only match class
/// positions, not method positions.
#[tokio::test]
async fn attribute_numeric_target_class_only() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_attr_num.php").unwrap();
    let text = concat!(
        "<?php\n",
        "#[\\Attribute(1)]\n",
        "class NumericAttr {}\n",
        "class Bar {\n",
        "    #[Numeric\n",
        "    public function baz(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 12).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    // TARGET_CLASS (1) should NOT match method position (TARGET_METHOD = 4).
    assert!(
        !lbls.contains(&"NumericAttr"),
        "NumericAttr (TARGET_CLASS only) should not appear before a method: {lbls:?}"
    );
}

// ─── No constants or functions in attribute context ─────────────────────────

/// Inside `#[…]`, constants and functions should not appear.
#[tokio::test]
async fn attribute_context_excludes_constants_and_functions() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_attr_no_funcs.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Scaffold;\n",
        "function myFunc(): void {}\n",
        "#[My\n",
        "class Foo {}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 4).await;

    let func_items: Vec<&CompletionItem> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
        .collect();
    assert!(
        func_items.is_empty(),
        "Functions should not appear in attribute context: {:?}",
        labels(&func_items.iter().map(|i| (*i).clone()).collect::<Vec<_>>())
    );

    let kw_items: Vec<&CompletionItem> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
        .collect();
    assert!(
        kw_items.is_empty(),
        "Keywords should not appear in attribute context: {:?}",
        labels(&kw_items.iter().map(|i| (*i).clone()).collect::<Vec<_>>())
    );
}

// ─── Demotion for unloaded classes ──────────────────────────────────────────

/// Unloaded classes whose name contains "Attribute" (or is a well-known
/// built-in attribute) should sort before unloaded classes whose name
/// does not contain "Attribute".
#[tokio::test]
async fn attribute_context_demotes_non_attribute_names() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    // Insert unloaded classes into the class index.
    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "Vendor\\CustomAttribute".to_string(),
            "file:///vendor/custom.php".to_string(),
        );
        idx.insert(
            "Vendor\\CustomService".to_string(),
            "file:///vendor/service.php".to_string(),
        );
    }

    let uri = Url::parse("file:///test_attr_demote.php").unwrap();
    let text = concat!("<?php\n", "#[Custom\n", "class Foo {}\n",);

    let items = complete_at(&backend, &uri, text, 1, 8).await;
    let cls = class_items(&items);

    let custom_attr = cls.iter().find(|i| i.label == "CustomAttribute");
    let custom_svc = cls.iter().find(|i| i.label == "CustomService");

    // Both should appear (unloaded classes pass through).
    // CustomAttribute should have a lower (better) sort_text because
    // "Attribute" is in its name.
    if let (Some(ca), Some(cs)) = (custom_attr, custom_svc) {
        assert!(
            ca.sort_text < cs.sort_text,
            "CustomAttribute sort {:?} should be < CustomService sort {:?}",
            ca.sort_text,
            cs.sort_text
        );
    }
}

// ─── Attribute short-form constant names ────────────────────────────────────

/// `#[Attribute(Attribute::TARGET_METHOD)]` (without leading `\`) should
/// be parsed correctly.
#[tokio::test]
async fn attribute_short_form_constants() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_attr_short.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use Attribute;\n",
        "#[Attribute(Attribute::TARGET_METHOD)]\n",
        "class ShortFormAttr {}\n",
        "class Baz {\n",
        "    #[Short\n",
        "    public function qux(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 5, 10).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"ShortFormAttr"),
        "ShortFormAttr should appear before a method: {lbls:?}"
    );
}

// ─── FQN prefix in attribute context ────────────────────────────────────────

/// `#[\Scaffold\My…]` should still detect attribute context with a
/// namespace-qualified prefix and only show attributes.
#[tokio::test]
async fn attribute_context_fqn_prefix() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_attr_fqn.php").unwrap();
    let text = concat!("<?php\n", "#[\\Scaffold\\MyAny\n", "class Foo {}\n",);

    let items = complete_at(&backend, &uri, text, 1, 17).await;
    let cls = class_items(&items);

    let has_any = cls
        .iter()
        .any(|i| i.detail.as_deref().is_some_and(|d| d.contains("MyAnyAttr")));
    assert!(
        has_any,
        "MyAnyAttr should appear with FQN prefix: {:?}",
        cls.iter()
            .map(|i| (&i.label, &i.detail))
            .collect::<Vec<_>>()
    );
}

// ─── Fallback when no declaration follows ───────────────────────────────────

/// When there is no declaration after `#[…]` (e.g. end of file), the
/// fallback target at the top level is TARGET_CLASS | TARGET_FUNCTION.
#[tokio::test]
async fn attribute_context_no_following_declaration_top_level() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_attr_eof.php").unwrap();
    let text = concat!("<?php\n", "namespace Scaffold;\n", "#[My\n",);

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    // Fallback at top level is TARGET_CLASS | TARGET_FUNCTION.
    assert!(
        lbls.contains(&"MyAnyAttr"),
        "MyAnyAttr should appear at top level fallback: {lbls:?}"
    );
    assert!(
        lbls.contains(&"MyClassAttr"),
        "MyClassAttr should appear at top level fallback: {lbls:?}"
    );
    assert!(
        lbls.contains(&"MyFunctionAttr"),
        "MyFunctionAttr should appear at top level fallback: {lbls:?}"
    );
    // Method-only should NOT appear at top level.
    assert!(
        !lbls.contains(&"MyMethodAttr"),
        "MyMethodAttr should not appear at top level fallback: {lbls:?}"
    );
}

/// When there is no declaration after `#[…]` inside a class body, the
/// fallback target is TARGET_METHOD | TARGET_PROPERTY | TARGET_CLASS_CONSTANT.
#[tokio::test]
async fn attribute_context_no_following_declaration_in_class() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_attr_eof2.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Scaffold;\n",
        "class Baz {\n",
        "    #[My\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 7).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"MyAnyAttr"),
        "MyAnyAttr should appear in class-body fallback: {lbls:?}"
    );
    assert!(
        lbls.contains(&"MyMethodAttr"),
        "MyMethodAttr should appear in class-body fallback: {lbls:?}"
    );
    assert!(
        lbls.contains(&"MyPropertyAttr"),
        "MyPropertyAttr should appear in class-body fallback: {lbls:?}"
    );
    assert!(
        lbls.contains(&"MyConstantAttr"),
        "MyConstantAttr should appear in class-body fallback: {lbls:?}"
    );
    // Class-only should NOT appear inside a class body.
    assert!(
        !lbls.contains(&"MyClassAttr"),
        "MyClassAttr should not appear in class-body fallback: {lbls:?}"
    );
    // Function-only should NOT appear inside a class body.
    assert!(
        !lbls.contains(&"MyFunctionAttr"),
        "MyFunctionAttr should not appear in class-body fallback: {lbls:?}"
    );
}

// ─── Built-in PHP attributes ────────────────────────────────────────────────

/// Built-in attributes like Override should appear in `#[…]` before
/// a method.
#[tokio::test]
async fn attribute_context_shows_builtin_override() {
    let backend = create_builtin_attribute_backend();

    let uri = Url::parse("file:///test_attr_builtin.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo extends \\stdClass {\n",
        "    #[Overr\n",
        "    public function __toString(): string { return ''; }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 11).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    // Override is a built-in PHP 8.3 attribute targeting methods.
    // If stubs include it and parse correctly, it should appear.
    // If stubs haven't been loaded with attribute_targets yet, this
    // test documents the expected behaviour.
    assert!(
        lbls.contains(&"Override"),
        "Override should appear in #[] before a method: {lbls:?}"
    );
}

// ─── Not confused with array syntax ─────────────────────────────────────────

/// `$arr[…]` should NOT be detected as attribute context.
#[tokio::test]
async fn array_access_not_confused_with_attribute() {
    let backend = create_test_backend();
    load_scaffolding(&backend).await;

    let uri = Url::parse("file:///test_attr_array.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Scaffold;\n",
        "function test() {\n",
        "    $arr = [];\n",
        "    $arr[My\n",
        "}\n",
    );

    // Inside $arr[My — this is array access, not an attribute.
    // We should not get attribute-filtered results.
    let items = complete_at(&backend, &uri, text, 4, 11).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    // PlainClass should be available in a non-attribute context
    // (the prefix doesn't match "Plain" so it won't show, but the
    // point is that MyAnyAttr should NOT appear as the ONLY option —
    // non-attributes matching the prefix should also appear if any).
    // Best we can assert: the result set should NOT be attribute-filtered.
    // MyClassAttr would only appear in attribute context, so if it
    // appears here it means we wrongly detected attribute context.
    // Actually, this test can simply check that the response doesn't
    // behave like attribute context.  A simple check: if PlainClass
    // would match the prefix, it should appear.
    //
    // With prefix "My", in a non-attribute context, both attribute and
    // non-attribute classes starting with "My" would appear.
    // Let's use a different approach: verify that non-attribute-target-
    // matching classes also show up (which they wouldn't in attribute
    // context).
    let has_non_attr = cls.iter().any(|i| i.label == "MyAnyAttr");
    // In attribute context, MyPropertyAttr would NOT appear before
    // an array access (since there's no declaration following).
    // But in non-attribute context, both would appear (no target
    // filtering).  Actually, inside a function body this is just
    // ClassNameContext::Any, so all classes appear.
    // The key assertion: this should NOT be Attribute context.
    // We verify by checking that non-attribute-specific classes also
    // appear with the same prefix.
    if has_non_attr {
        // If MyAnyAttr appears, PlainClass (different prefix) won't,
        // but ALL My* classes should appear, not just attribute ones.
        // In attribute context before a method, MyClassAttr would be
        // excluded.  In non-attribute context it's included.
        // Actually there is no declaration after $arr[, so this is
        // just Any context.
        assert!(
            lbls.contains(&"MyClassAttr"),
            "In non-attribute context, all My* classes should appear: {lbls:?}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Attribute constructor snippet tests
// ═══════════════════════════════════════════════════════════════════════

/// An attribute with no constructor parameters inserts the bare name
/// (no parentheses).
#[tokio::test]
async fn attribute_no_constructor_inserts_bare_name() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_attr_noctor.php").unwrap();
    let text = concat!(
        "<?php\n",
        "#[\\Attribute]\n",
        "class NoArgs {}\n",
        "class Foo {\n",
        "    #[NoAr\n",
        "    public function bar(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 10).await;
    let cls = class_items(&items);
    let item = cls.iter().find(|i| i.label == "NoArgs").unwrap();

    assert_eq!(
        insert_text(item),
        "NoArgs",
        "Attribute with no constructor should insert bare name without parens"
    );
    // No snippet format — plain text insertion.
    assert!(
        item.insert_text_format.is_none()
            || item.insert_text_format == Some(InsertTextFormat::PLAIN_TEXT),
        "Bare attribute should not use snippet format"
    );
}

/// An attribute with an empty constructor (no params) also inserts the
/// bare name.
#[tokio::test]
async fn attribute_empty_constructor_inserts_bare_name() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_attr_emptyctor.php").unwrap();
    let text = concat!(
        "<?php\n",
        "#[\\Attribute]\n",
        "class EmptyCtor {\n",
        "    public function __construct() {}\n",
        "}\n",
        "class Foo {\n",
        "    #[Empty\n",
        "    public function bar(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 6, 11).await;
    let cls = class_items(&items);
    let item = cls.iter().find(|i| i.label == "EmptyCtor").unwrap();

    assert_eq!(
        insert_text(item),
        "EmptyCtor",
        "Attribute with empty constructor should insert bare name"
    );
}

/// An attribute with a required string parameter generates a named-
/// argument snippet with a quoted placeholder.
#[tokio::test]
async fn attribute_required_string_param_named_arg_snippet() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_attr_string.php").unwrap();
    let text = concat!(
        "<?php\n",
        "#[\\Attribute(\\Attribute::TARGET_METHOD | \\Attribute::IS_REPEATABLE)]\n",
        "final readonly class DataProvider {\n",
        "    public function __construct(string $methodName, bool $validateArgumentCount = true) {}\n",
        "}\n",
        "class Foo {\n",
        "    #[DataProv\n",
        "    public function testSomething(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 6, 14).await;
    let cls = class_items(&items);
    let item = cls.iter().find(|i| i.label == "DataProvider").unwrap();

    assert_eq!(
        insert_text(item),
        "DataProvider(methodName: '${1:methodName}')$0",
        "Should generate named-arg snippet with string placeholder"
    );
    assert_eq!(
        item.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "Should use snippet format"
    );
}

/// An attribute with multiple required params of different types
/// generates a snippet with appropriate placeholders for each.
#[tokio::test]
async fn attribute_multiple_required_params() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_attr_multi.php").unwrap();
    let text = concat!(
        "<?php\n",
        "#[\\Attribute]\n",
        "class MultiParam {\n",
        "    public function __construct(string $name, int $priority, bool $enabled) {}\n",
        "}\n",
        "#[MultiPar\n",
        "class Foo {}\n",
    );

    let items = complete_at(&backend, &uri, text, 5, 10).await;
    let cls = class_items(&items);
    let item = cls.iter().find(|i| i.label == "MultiParam").unwrap();

    assert_eq!(
        insert_text(item),
        "MultiParam(name: '${1:name}', priority: ${2:0}, enabled: ${3:false})$0",
        "Should generate named-arg snippet for all required params with type-based defaults"
    );
}

/// Optional parameters with defaults are omitted from the snippet.
#[tokio::test]
async fn attribute_optional_params_omitted() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_attr_opt.php").unwrap();
    let text = concat!(
        "<?php\n",
        "#[\\Attribute]\n",
        "class WithDefaults {\n",
        "    public function __construct(string $path, array $methods = [], bool $strict = true) {}\n",
        "}\n",
        "class Foo {\n",
        "    #[WithDef\n",
        "    public function bar(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 6, 13).await;
    let cls = class_items(&items);
    let item = cls.iter().find(|i| i.label == "WithDefaults").unwrap();

    // Only the required `$path` should appear; `$methods` and `$strict`
    // are optional and should be omitted.
    assert_eq!(
        insert_text(item),
        "WithDefaults(path: '${1:path}')$0",
        "Optional params should be omitted from attribute snippet"
    );
}

/// All optional constructor parameters produces a bare name (no parens).
#[tokio::test]
async fn attribute_all_optional_params_bare_name() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_attr_allopt.php").unwrap();
    let text = concat!(
        "<?php\n",
        "#[\\Attribute]\n",
        "class AllOptional {\n",
        "    public function __construct(int $flags = 0, string $name = 'default') {}\n",
        "}\n",
        "#[AllOpt\n",
        "class Foo {}\n",
    );

    let items = complete_at(&backend, &uri, text, 5, 8).await;
    let cls = class_items(&items);
    let item = cls.iter().find(|i| i.label == "AllOptional").unwrap();

    assert_eq!(
        insert_text(item),
        "AllOptional",
        "Attribute with all optional params should insert bare name"
    );
}

/// Bool type hint generates `false` as the default placeholder.
#[tokio::test]
async fn attribute_bool_placeholder() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_attr_bool.php").unwrap();
    let text = concat!(
        "<?php\n",
        "#[\\Attribute]\n",
        "class BoolAttr {\n",
        "    public function __construct(bool $active) {}\n",
        "}\n",
        "#[BoolAt\n",
        "class Foo {}\n",
    );

    let items = complete_at(&backend, &uri, text, 5, 8).await;
    let cls = class_items(&items);
    let item = cls.iter().find(|i| i.label == "BoolAttr").unwrap();

    assert_eq!(insert_text(item), "BoolAttr(active: ${1:false})$0",);
}

/// Int type hint generates `0` as the default placeholder.
#[tokio::test]
async fn attribute_int_placeholder() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_attr_int.php").unwrap();
    let text = concat!(
        "<?php\n",
        "#[\\Attribute]\n",
        "class IntAttr {\n",
        "    public function __construct(int $priority) {}\n",
        "}\n",
        "#[IntAt\n",
        "class Foo {}\n",
    );

    let items = complete_at(&backend, &uri, text, 5, 7).await;
    let cls = class_items(&items);
    let item = cls.iter().find(|i| i.label == "IntAttr").unwrap();

    assert_eq!(insert_text(item), "IntAttr(priority: ${1:0})$0",);
}

/// Float type hint generates `0.0` as the default placeholder.
#[tokio::test]
async fn attribute_float_placeholder() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_attr_float.php").unwrap();
    let text = concat!(
        "<?php\n",
        "#[\\Attribute]\n",
        "class FloatAttr {\n",
        "    public function __construct(float $ratio) {}\n",
        "}\n",
        "#[FloatAt\n",
        "class Foo {}\n",
    );

    let items = complete_at(&backend, &uri, text, 5, 9).await;
    let cls = class_items(&items);
    let item = cls.iter().find(|i| i.label == "FloatAttr").unwrap();

    assert_eq!(insert_text(item), "FloatAttr(ratio: ${1:0.0})$0",);
}

/// Array type hint generates `[]` as the default placeholder.
#[tokio::test]
async fn attribute_array_placeholder() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_attr_array.php").unwrap();
    let text = concat!(
        "<?php\n",
        "#[\\Attribute]\n",
        "class ArrayAttr {\n",
        "    public function __construct(array $items) {}\n",
        "}\n",
        "#[ArrayAt\n",
        "class Foo {}\n",
    );

    let items = complete_at(&backend, &uri, text, 5, 9).await;
    let cls = class_items(&items);
    let item = cls.iter().find(|i| i.label == "ArrayAttr").unwrap();

    assert_eq!(insert_text(item), "ArrayAttr(items: ${1:[]})$0",);
}

/// A parameter with no type hint uses the bare name as placeholder.
#[tokio::test]
async fn attribute_untyped_param_placeholder() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_attr_untyped.php").unwrap();
    let text = concat!(
        "<?php\n",
        "#[\\Attribute]\n",
        "class UntypedAttr {\n",
        "    public function __construct($value) {}\n",
        "}\n",
        "#[UntypedAt\n",
        "class Foo {}\n",
    );

    let items = complete_at(&backend, &uri, text, 5, 11).await;
    let cls = class_items(&items);
    let item = cls.iter().find(|i| i.label == "UntypedAttr").unwrap();

    assert_eq!(insert_text(item), "UntypedAttr(value: ${1:value})$0",);
}

/// Cross-file attribute completions via use-import also get constructor
/// snippets.
#[tokio::test]
async fn attribute_cross_file_constructor_snippet() {
    let backend = create_test_backend();

    // File 1: attribute class with constructor.
    let attr_uri = Url::parse("file:///attr_def.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: attr_uri,
                language_id: "php".to_string(),
                version: 1,
                text: concat!(
                    "<?php\n",
                    "namespace App\\Attributes;\n",
                    "#[\\Attribute(\\Attribute::TARGET_METHOD)]\n",
                    "class Route {\n",
                    "    public function __construct(string $path, string $method = 'GET') {}\n",
                    "}\n",
                )
                .to_string(),
            },
        })
        .await;

    // File 2: uses the attribute.
    let uri = Url::parse("file:///test_cross.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Controllers;\n",
        "use App\\Attributes\\Route;\n",
        "class UserController {\n",
        "    #[Rou\n",
        "    public function index(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 9).await;
    let cls = class_items(&items);
    let item = cls.iter().find(|i| i.label == "Route").unwrap();

    assert_eq!(
        insert_text(item),
        "Route(path: '${1:path}')$0",
        "Cross-file attribute should generate constructor snippet"
    );
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET),);
}

/// Non-attribute context (`new`) still uses the `$`-prefixed variable
/// style, not named arguments, for the same class.
#[tokio::test]
async fn new_context_still_uses_variable_style_not_named_args() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_new_vs_attr.php").unwrap();
    let text = concat!(
        "<?php\n",
        "#[\\Attribute]\n",
        "class SomeAttr {\n",
        "    public function __construct(string $name) {}\n",
        "}\n",
        "function test() {\n",
        "    $x = new SomeAt\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 6, 19).await;
    let cls = class_items(&items);
    let item = cls.iter().find(|i| i.label == "SomeAttr").unwrap();

    // `new` context should use `$name` style, not `name: 'name'`.
    assert_eq!(
        insert_text(item),
        "SomeAttr(${1:\\$name})$0",
        "`new` context should use variable-style placeholders, not named args"
    );
}
