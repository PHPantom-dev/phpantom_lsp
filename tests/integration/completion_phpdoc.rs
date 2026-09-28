use crate::common::{complete_at, create_psr4_workspace, create_test_backend, filter_texts};
use tower_lsp::lsp_types::*;

// ─── Basic trigger ──────────────────────────────────────────────────────────

/// Typing `@` inside a docblock should produce PHPDoc tag completions.
#[tokio::test]
async fn test_phpdoc_bare_at_triggers_completion() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_bare.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function foo(): void {}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let tags = filter_texts(&items);

    // foo() has no params, void return, and no throws — smart tags are
    // filtered out.  General tags like @deprecated should still appear.
    assert!(
        tags.contains(&"@deprecated"),
        "Should suggest @deprecated. Got: {:?}",
        tags
    );
    assert!(
        tags.contains(&"@inheritdoc"),
        "Should suggest @inheritdoc. Got: {:?}",
        tags
    );
}

/// Typing `@par` should filter to tags matching the prefix.
#[tokio::test]
async fn test_phpdoc_partial_prefix_filters() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_filter.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @par\n",
        " */\n",
        "function greet(string $name): string {}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let tags = filter_texts(&items);

    assert!(
        tags.contains(&"@param"),
        "Should suggest @param for prefix @par. Got: {:?}",
        tags
    );
    assert!(
        !tags.contains(&"@return"),
        "Should NOT suggest @return for prefix @par. Got: {:?}",
        tags
    );
}

// ─── Not triggered outside docblocks ────────────────────────────────────────

/// `@` in regular PHP code (e.g. error suppression) should NOT trigger
/// PHPDoc completion.
#[tokio::test]
async fn test_phpdoc_not_triggered_outside_docblock() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_outside.php").unwrap();
    let text = concat!("<?php\n", "@mkdir('/tmp/test');\n",);

    let items = complete_at(&backend, &uri, text, 1, 1).await;

    let phpdoc_items: Vec<_> = items
        .iter()
        .filter(|i| {
            i.filter_text
                .as_deref()
                .is_some_and(|ft| ft.starts_with('@'))
        })
        .collect();
    assert!(
        phpdoc_items.is_empty(),
        "Should NOT suggest PHPDoc tags outside a docblock. Got: {:?}",
        phpdoc_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// `@` inside a regular `/* ... */` comment should NOT trigger PHPDoc completion.
#[tokio::test]
async fn test_phpdoc_not_triggered_in_regular_comment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_regular_comment.php").unwrap();
    let text = concat!("<?php\n", "/* @param string $x */\n",);

    let items = complete_at(&backend, &uri, text, 1, 4).await;

    let phpdoc_items: Vec<_> = items
        .iter()
        .filter(|i| {
            i.filter_text
                .as_deref()
                .is_some_and(|ft| ft.starts_with('@'))
        })
        .collect();
    assert!(
        phpdoc_items.is_empty(),
        "Should NOT suggest PHPDoc tags inside a regular comment. Got: {:?}",
        phpdoc_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

// ─── Context: function / method ─────────────────────────────────────────────

/// Docblock before a function should suggest function-related tags.
#[tokio::test]
async fn test_phpdoc_function_context_tags() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_func.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function greet(string $name): string {\n",
        "    throw new InvalidArgumentException('bad');\n",
        "    return 'Hello ' . $name;\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let tags = filter_texts(&items);

    // Function-specific smart tags (function has a param, non-void return, and a throw)
    assert!(tags.contains(&"@param"), "Should suggest @param");
    assert!(tags.contains(&"@return"), "Should suggest @return");
    assert!(tags.contains(&"@throws"), "Should suggest @throws");

    // General tags
    assert!(tags.contains(&"@deprecated"), "Should suggest @deprecated");
    assert!(tags.contains(&"@see"), "Should suggest @see");

    // PHPStan function tags
    assert!(
        tags.contains(&"@phpstan-assert"),
        "Should suggest @phpstan-assert"
    );

    // Should NOT include class-only tags
    assert!(
        !tags.contains(&"@property"),
        "Should NOT suggest @property in function context"
    );
    assert!(
        !tags.contains(&"@method"),
        "Should NOT suggest @method in function context"
    );
    assert!(
        !tags.contains(&"@mixin"),
        "Should NOT suggest @mixin in function context"
    );
}

/// Docblock before a method should also get function-related tags.
#[tokio::test]
async fn test_phpdoc_method_context_tags() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_method.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Service {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function handle(Request $request): Response {\n",
        "        throw new RuntimeException('fail');\n",
        "        return new Response();\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;
    let tags = filter_texts(&items);

    assert!(tags.contains(&"@param"), "Should suggest @param");
    assert!(tags.contains(&"@return"), "Should suggest @return");
    assert!(tags.contains(&"@throws"), "Should suggest @throws");
    assert!(tags.contains(&"@inheritdoc"), "Should suggest @inheritdoc");
}

/// Docblock before a static method should get function-related tags.
#[tokio::test]
async fn test_phpdoc_static_method_context_tags() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_static_method.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Factory {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public static function create(string $type): self {\n",
        "        return new self();\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;
    let tags = filter_texts(&items);

    assert!(tags.contains(&"@return"), "Should suggest @return");
    assert!(tags.contains(&"@param"), "Should suggest @param");
}

// ─── Context: class / interface / trait / enum ──────────────────────────────

/// Docblock before a class should suggest class-related tags.
#[tokio::test]
async fn test_phpdoc_class_context_tags() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_class.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "class UserRepository {\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let tags = filter_texts(&items);

    // Class-specific tags
    assert!(tags.contains(&"@property"), "Should suggest @property");
    assert!(tags.contains(&"@method"), "Should suggest @method");
    assert!(tags.contains(&"@mixin"), "Should suggest @mixin");
    assert!(tags.contains(&"@template"), "Should suggest @template");
    assert!(tags.contains(&"@extends"), "Should suggest @extends");
    assert!(tags.contains(&"@implements"), "Should suggest @implements");

    // General tags
    assert!(tags.contains(&"@deprecated"), "Should suggest @deprecated");

    // Should NOT include function-only tags
    assert!(
        !tags.contains(&"@param"),
        "Should NOT suggest @param in class context"
    );
    assert!(
        !tags.contains(&"@return"),
        "Should NOT suggest @return in class context"
    );
    assert!(
        !tags.contains(&"@throws"),
        "Should NOT suggest @throws in class context"
    );
}

/// Docblock before an abstract class.
#[tokio::test]
async fn test_phpdoc_abstract_class_context() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_abstract.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "abstract class BaseService {\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let tags = filter_texts(&items);

    assert!(tags.contains(&"@property"), "Should suggest @property");
    assert!(tags.contains(&"@method"), "Should suggest @method");
    assert!(
        !tags.contains(&"@param"),
        "Should NOT suggest @param in class context"
    );
}

/// Docblock before a final class.
#[tokio::test]
async fn test_phpdoc_final_class_context() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_final.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "final class Singleton {\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let tags = filter_texts(&items);

    assert!(tags.contains(&"@property"), "Should suggest @property");
    assert!(tags.contains(&"@method"), "Should suggest @method");
}

/// Docblock before an interface should suggest class-related tags.
#[tokio::test]
async fn test_phpdoc_interface_context_tags() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_iface.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "interface Cacheable {\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let tags = filter_texts(&items);

    assert!(tags.contains(&"@method"), "Should suggest @method");
    assert!(tags.contains(&"@template"), "Should suggest @template");
    assert!(tags.contains(&"@extends"), "Should suggest @extends");
    assert!(
        !tags.contains(&"@param"),
        "Should NOT suggest @param in interface context"
    );
}

/// Docblock before a trait should suggest class-related tags.
#[tokio::test]
async fn test_phpdoc_trait_context_tags() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_trait.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "trait Loggable {\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let tags = filter_texts(&items);

    assert!(tags.contains(&"@property"), "Should suggest @property");
    assert!(tags.contains(&"@method"), "Should suggest @method");
    assert!(
        !tags.contains(&"@return"),
        "Should NOT suggest @return in trait context"
    );
}

/// Docblock before an enum should suggest class-related tags.
#[tokio::test]
async fn test_phpdoc_enum_context_tags() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_enum.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "enum Status: string {\n",
        "    case Active = 'active';\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let tags = filter_texts(&items);

    assert!(tags.contains(&"@method"), "Should suggest @method");
    assert!(tags.contains(&"@deprecated"), "Should suggest @deprecated");
}

// ─── Context: property ──────────────────────────────────────────────────────

/// Docblock before a property should suggest property-related tags.
#[tokio::test]
async fn test_phpdoc_property_context_tags() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public string $name;\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;
    let tags = filter_texts(&items);

    assert!(tags.contains(&"@var"), "Should suggest @var");
    assert!(tags.contains(&"@deprecated"), "Should suggest @deprecated");
    // Should NOT include function or class tags
    assert!(
        !tags.contains(&"@param"),
        "Should NOT suggest @param in property context"
    );
    assert!(
        !tags.contains(&"@return"),
        "Should NOT suggest @return in property context"
    );
    assert!(
        !tags.contains(&"@method"),
        "Should NOT suggest @method in property context"
    );
    assert!(
        !tags.contains(&"@property"),
        "Should NOT suggest @property in property context"
    );
}

/// Docblock before a typed property with nullable type.
#[tokio::test]
async fn test_phpdoc_nullable_property_context() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_nullable_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Config {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    protected ?string $apiKey = null;\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;
    let tags = filter_texts(&items);

    assert!(tags.contains(&"@var"), "Should suggest @var");
    assert!(tags.contains(&"@deprecated"), "Should suggest @deprecated");
}

/// Docblock before a readonly property.
#[tokio::test]
async fn test_phpdoc_readonly_property_context() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_readonly_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Entity {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public readonly int $id;\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;
    let tags = filter_texts(&items);

    assert!(tags.contains(&"@var"), "Should suggest @var");
}

/// Docblock before a static property.
#[tokio::test]
async fn test_phpdoc_static_property_context() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_static_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Registry {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    private static array $instances = [];\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;
    let tags = filter_texts(&items);

    assert!(tags.contains(&"@var"), "Should suggest @var");
    assert!(tags.contains(&"@deprecated"), "Should suggest @deprecated");
}

// ─── Context: constant ──────────────────────────────────────────────────────

/// Docblock before a class constant should suggest constant-related tags.
#[tokio::test]
async fn test_phpdoc_constant_context_tags() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_const.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Config {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    const MAX_RETRIES = 3;\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;
    let tags = filter_texts(&items);

    assert!(tags.contains(&"@var"), "Should suggest @var");
    assert!(tags.contains(&"@deprecated"), "Should suggest @deprecated");

    // Should NOT include function or class tags
    assert!(
        !tags.contains(&"@param"),
        "Should NOT suggest @param in constant context"
    );
    assert!(
        !tags.contains(&"@return"),
        "Should NOT suggest @return in constant context"
    );
    assert!(
        !tags.contains(&"@method"),
        "Should NOT suggest @method in constant context"
    );
}

/// Docblock before a constant with visibility.
#[tokio::test]
async fn test_phpdoc_visibility_constant_context() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_vis_const.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class HttpStatus {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public const OK = 200;\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;
    let tags = filter_texts(&items);

    assert!(tags.contains(&"@var"), "Should suggest @var");
    assert!(tags.contains(&"@deprecated"), "Should suggest @deprecated");
}

// ─── PHPStan-specific tag filtering ─────────────────────────────────────────

/// Typing `@phpstan-` should suggest only PHPStan tags matching the prefix.
#[tokio::test]
async fn test_phpdoc_phpstan_prefix_filtering() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_phpstan.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @phpstan-\n",
        " */\n",
        "function check($value): void {}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 12).await;
    let tags = filter_texts(&items);

    assert!(
        tags.contains(&"@phpstan-assert"),
        "Should suggest @phpstan-assert"
    );
    assert!(
        tags.contains(&"@phpstan-assert-if-true"),
        "Should suggest @phpstan-assert-if-true"
    );
    assert!(
        tags.contains(&"@phpstan-assert-if-false"),
        "Should suggest @phpstan-assert-if-false"
    );

    // Regular tags should NOT match
    assert!(
        !tags.contains(&"@param"),
        "Should NOT suggest @param for prefix @phpstan-"
    );
    assert!(
        !tags.contains(&"@deprecated"),
        "Should NOT suggest @deprecated for prefix @phpstan-"
    );
}

/// PHPStan tags should be context-aware: function context should not include
/// class-only PHPStan tags.
#[tokio::test]
async fn test_phpdoc_phpstan_context_aware() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_phpstan_ctx.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @phpstan-\n",
        " */\n",
        "function transform(): void {}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 12).await;
    let tags = filter_texts(&items);

    // Function-context PHPStan tags
    assert!(
        tags.contains(&"@phpstan-assert"),
        "Should suggest @phpstan-assert in function context"
    );

    // Class-only PHPStan tags should NOT appear
    assert!(
        !tags.contains(&"@phpstan-require-extends"),
        "Should NOT suggest @phpstan-require-extends in function context"
    );
    assert!(
        !tags.contains(&"@phpstan-require-implements"),
        "Should NOT suggest @phpstan-require-implements in function context"
    );
}

/// PHPStan class tags in class context.
#[tokio::test]
async fn test_phpdoc_phpstan_class_context() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_phpstan_class.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @phpstan-\n",
        " */\n",
        "class GenericRepo {\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 12).await;
    let tags = filter_texts(&items);

    assert!(
        tags.contains(&"@phpstan-type"),
        "Should suggest @phpstan-type in class context"
    );
    assert!(
        tags.contains(&"@phpstan-import-type"),
        "Should suggest @phpstan-import-type in class context"
    );
    assert!(
        tags.contains(&"@phpstan-require-extends"),
        "Should suggest @phpstan-require-extends in class context"
    );
    assert!(
        tags.contains(&"@phpstan-require-implements"),
        "Should suggest @phpstan-require-implements in class context"
    );

    // Function-only PHPStan tags should NOT appear
    assert!(
        !tags.contains(&"@phpstan-assert"),
        "Should NOT suggest @phpstan-assert in class context"
    );
}

// ─── Unknown context ────────────────────────────────────────────────────────

/// When the symbol after the docblock can't be determined, suggest all tags.
#[tokio::test]
async fn test_phpdoc_unknown_context_suggests_all() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_unknown.php").unwrap();
    let text = concat!("<?php\n", "/**\n", " * @\n", " */\n",);

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let tags = filter_texts(&items);

    // Unknown context: class tags and general tags should appear.
    // @param, @return, @throws are filtered because no function body
    // can be detected (no params, no return, no throws).
    assert!(tags.contains(&"@property"), "Should suggest @property");
    assert!(tags.contains(&"@method"), "Should suggest @method");
    assert!(tags.contains(&"@var"), "Should suggest @var");
    assert!(tags.contains(&"@deprecated"), "Should suggest @deprecated");
    assert!(tags.contains(&"@inheritdoc"), "Should suggest @inheritdoc");
}

// ─── Completion item details ────────────────────────────────────────────────

/// Completion items should have the KEYWORD kind.
#[tokio::test]
async fn test_phpdoc_items_have_keyword_kind() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_kind.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function foo(): void {}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;

    for item in &items {
        assert_eq!(
            item.kind,
            Some(CompletionItemKind::KEYWORD),
            "PHPDoc tag {:?} should use KEYWORD kind",
            item.label
        );
    }
}

/// Completion items should have a detail description.
#[tokio::test]
async fn test_phpdoc_items_have_detail() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_detail.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function foo(): void {}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;

    for item in &items {
        assert!(
            item.detail.is_some(),
            "PHPDoc tag {:?} should have a detail description",
            item.label
        );
        assert!(
            !item.detail.as_ref().unwrap().is_empty(),
            "PHPDoc tag {:?} should have a non-empty detail",
            item.label
        );
    }
}

/// Completion items should not be duplicated.
#[tokio::test]
async fn test_phpdoc_no_duplicates() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_dedup.php").unwrap();
    let text = concat!("<?php\n", "/**\n", " * @\n", " */\n",);

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let tags = filter_texts(&items);
    let unique: std::collections::HashSet<&&str> = tags.iter().collect();

    assert_eq!(
        tags.len(),
        unique.len(),
        "Should not have duplicate PHPDoc tags. Got: {:?}",
        tags
    );
}

// ─── Open (unclosed) docblock ───────────────────────────────────────────────

/// PHPDoc completion should work even when the docblock is not yet closed.
#[tokio::test]
async fn test_phpdoc_open_docblock() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_open.php").unwrap();
    let text = concat!("<?php\n", "/**\n", " * @\n",);

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let tags = filter_texts(&items);

    // Should still produce completions even without closing */
    assert!(
        !tags.is_empty(),
        "Should suggest tags even in an unclosed docblock. Got: {:?}",
        tags
    );
    assert!(
        tags.contains(&"@deprecated"),
        "Should suggest @deprecated. Got: {:?}",
        tags
    );
}

// ─── Multiple docblocks ─────────────────────────────────────────────────────

/// When there are multiple docblocks, only trigger for the one containing
/// the cursor.
#[tokio::test]
async fn test_phpdoc_multiple_docblocks() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_multi.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @param string $x\n",
        " */\n",
        "function first(): void {}\n",
        "\n",
        "/**\n",
        " * @\n",
        " */\n",
        "class MyClass {}\n",
    );

    // Cursor in second docblock — should get class context
    let items = complete_at(&backend, &uri, text, 7, 4).await;
    let tags = filter_texts(&items);

    assert!(
        tags.contains(&"@property"),
        "Should suggest @property for class docblock"
    );
    assert!(
        tags.contains(&"@method"),
        "Should suggest @method for class docblock"
    );
    assert!(
        !tags.contains(&"@param"),
        "Should NOT suggest @param for class docblock"
    );
}

// ─── Case insensitivity ─────────────────────────────────────────────────────

/// Prefix matching should be case-insensitive.
#[tokio::test]
async fn test_phpdoc_case_insensitive_prefix() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_case.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @PAR\n",
        " */\n",
        "function greet(string $name): void {}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let tags = filter_texts(&items);

    assert!(
        tags.contains(&"@param"),
        "Should match @param case-insensitively. Got: {:?}",
        tags
    );
}

// ─── Second tag on another line ─────────────────────────────────────────────

/// Adding a second tag to an existing docblock should still work.
#[tokio::test]
async fn test_phpdoc_second_tag_in_docblock() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_second.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @param string $name\n",
        " * @\n",
        " */\n",
        "function greet(string $name): string {\n",
        "    throw new RuntimeException('fail');\n",
        "    return 'Hello ' . $name;\n",
        "}\n",
    );

    // Cursor on the second `@` (line 3)
    let items = complete_at(&backend, &uri, text, 3, 4).await;
    let tags = filter_texts(&items);

    assert!(tags.contains(&"@return"), "Should suggest @return");
    assert!(tags.contains(&"@throws"), "Should suggest @throws");
}

// ─── @deprecated with reason ────────────────────────────────────────────────

/// @deprecated should be available in all contexts.
#[tokio::test]
async fn test_phpdoc_deprecated_in_all_contexts() {
    let backend = create_test_backend();

    // Function context
    let uri1 = Url::parse("file:///phpdoc_dep_func.php").unwrap();
    let text1 = concat!(
        "<?php\n",
        "/**\n",
        " * @dep\n",
        " */\n",
        "function old(): void {}\n",
    );
    let items1 = complete_at(&backend, &uri1, text1, 2, 7).await;
    assert!(
        items1
            .iter()
            .any(|i| i.filter_text.as_deref() == Some("@deprecated")),
        "Should suggest @deprecated in function context"
    );

    // Class context
    let uri2 = Url::parse("file:///phpdoc_dep_class.php").unwrap();
    let text2 = concat!(
        "<?php\n",
        "/**\n",
        " * @dep\n",
        " */\n",
        "class OldClass {}\n",
    );
    let items2 = complete_at(&backend, &uri2, text2, 2, 7).await;
    assert!(
        items2
            .iter()
            .any(|i| i.filter_text.as_deref() == Some("@deprecated")),
        "Should suggest @deprecated in class context"
    );

    // Property context
    let uri3 = Url::parse("file:///phpdoc_dep_prop.php").unwrap();
    let text3 = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "     * @dep\n",
        "     */\n",
        "    public string $old;\n",
        "}\n",
    );
    let items3 = complete_at(&backend, &uri3, text3, 3, 11).await;
    assert!(
        items3
            .iter()
            .any(|i| i.filter_text.as_deref() == Some("@deprecated")),
        "Should suggest @deprecated in property context"
    );

    // Constant context
    let uri4 = Url::parse("file:///phpdoc_dep_const.php").unwrap();
    let text4 = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "     * @dep\n",
        "     */\n",
        "    const OLD = 1;\n",
        "}\n",
    );
    let items4 = complete_at(&backend, &uri4, text4, 3, 11).await;
    assert!(
        items4
            .iter()
            .any(|i| i.filter_text.as_deref() == Some("@deprecated")),
        "Should suggest @deprecated in constant context"
    );
}

// ─── @template in class vs function ─────────────────────────────────────────

/// @template should appear in class context but not in property or constant context.
#[tokio::test]
async fn test_phpdoc_template_context_awareness() {
    let backend = create_test_backend();

    // Class context — should have @template
    let uri_class = Url::parse("file:///phpdoc_tmpl_class.php").unwrap();
    let text_class = concat!(
        "<?php\n",
        "/**\n",
        " * @templ\n",
        " */\n",
        "class Container {}\n",
    );
    let items_class = complete_at(&backend, &uri_class, text_class, 2, 9).await;
    assert!(
        items_class
            .iter()
            .any(|i| i.filter_text.as_deref() == Some("@template")),
        "Should suggest @template in class context"
    );

    // Property context — should NOT have @template
    let uri_prop = Url::parse("file:///phpdoc_tmpl_prop.php").unwrap();
    let text_prop = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "     * @templ\n",
        "     */\n",
        "    public string $name;\n",
        "}\n",
    );
    let items_prop = complete_at(&backend, &uri_prop, text_prop, 3, 12).await;
    assert!(
        !items_prop
            .iter()
            .any(|i| i.filter_text.as_deref() == Some("@template")),
        "Should NOT suggest @template in property context"
    );
}

// ─── @var availability ──────────────────────────────────────────────────────

/// @var should be available in property and constant contexts
/// but not in function/method or class contexts.
#[tokio::test]
async fn test_phpdoc_var_context_awareness() {
    let backend = create_test_backend();

    // Function context — should NOT have @var (use @param / @return instead)
    let uri_func = Url::parse("file:///phpdoc_var_func.php").unwrap();
    let text_func = concat!(
        "<?php\n",
        "/**\n",
        " * @va\n",
        " */\n",
        "function foo(): void {}\n",
    );
    let items_func = complete_at(&backend, &uri_func, text_func, 2, 6).await;
    assert!(
        !items_func
            .iter()
            .any(|i| i.filter_text.as_deref() == Some("@var")),
        "Should NOT suggest @var in function/method context"
    );

    // Property context
    let uri_prop = Url::parse("file:///phpdoc_var_prop.php").unwrap();
    let text_prop = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "     * @va\n",
        "     */\n",
        "    public $name;\n",
        "}\n",
    );
    let items_prop = complete_at(&backend, &uri_prop, text_prop, 3, 10).await;
    assert!(
        items_prop
            .iter()
            .any(|i| i.filter_text.as_deref() == Some("@var")),
        "Should suggest @var in property context"
    );

    // Class context — should NOT have @var
    let uri_class = Url::parse("file:///phpdoc_var_class.php").unwrap();
    let text_class = concat!("<?php\n", "/**\n", " * @va\n", " */\n", "class Foo {}\n",);
    let items_class = complete_at(&backend, &uri_class, text_class, 2, 6).await;
    assert!(
        !items_class
            .iter()
            .any(|i| i.filter_text.as_deref() == Some("@var")),
        "Should NOT suggest @var in class context"
    );
}

// ─── @inheritdoc only in function / method ──────────────────────────────────

/// @inheritdoc should only appear in function/method context.
#[tokio::test]
async fn test_phpdoc_inheritdoc_context() {
    let backend = create_test_backend();

    // Method context — should have @inheritdoc
    let uri_method = Url::parse("file:///phpdoc_inherit_method.php").unwrap();
    let text_method = concat!(
        "<?php\n",
        "class Child extends Base {\n",
        "    /**\n",
        "     * @inherit\n",
        "     */\n",
        "    public function doWork(): void {}\n",
        "}\n",
    );
    let items_method = complete_at(&backend, &uri_method, text_method, 3, 15).await;
    assert!(
        items_method
            .iter()
            .any(|i| i.filter_text.as_deref() == Some("@inheritdoc")),
        "Should suggest @inheritdoc in method context"
    );

    // Class context — should NOT have @inheritdoc
    let uri_class = Url::parse("file:///phpdoc_inherit_class.php").unwrap();
    let text_class = concat!(
        "<?php\n",
        "/**\n",
        " * @inherit\n",
        " */\n",
        "class Child extends Base {}\n",
    );
    let items_class = complete_at(&backend, &uri_class, text_class, 2, 11).await;
    assert!(
        !items_class
            .iter()
            .any(|i| i.filter_text.as_deref() == Some("@inheritdoc")),
        "Should NOT suggest @inheritdoc in class context"
    );
}

// ─── Property-related tags only in class context ────────────────────────────

/// @property should only appear in class context, not function context.
#[tokio::test]
async fn test_phpdoc_magic_property_tags_context() {
    let backend = create_test_backend();

    // Class context — should have @property
    let uri_class = Url::parse("file:///phpdoc_magic_class.php").unwrap();
    let text_class = concat!(
        "<?php\n",
        "/**\n",
        " * @property\n",
        " */\n",
        "class Magic {}\n",
    );
    let items_class = complete_at(&backend, &uri_class, text_class, 2, 12).await;
    let tags = filter_texts(&items_class);
    assert!(tags.contains(&"@property"), "Should suggest @property");

    // Function context — should NOT have @property
    let uri_func = Url::parse("file:///phpdoc_magic_func.php").unwrap();
    let text_func = concat!(
        "<?php\n",
        "/**\n",
        " * @property\n",
        " */\n",
        "function notAClass(): void {}\n",
    );
    let items_func = complete_at(&backend, &uri_func, text_func, 2, 12).await;
    let func_tags = filter_texts(&items_func);
    assert!(
        !func_tags.contains(&"@property"),
        "Should NOT suggest @property in function context. Got: {:?}",
        func_tags
    );
}

// ─── Display labels ─────────────────────────────────────────────────────────

/// Tags with a specific format should show a display label indicating usage.
#[tokio::test]
async fn test_phpdoc_display_labels_show_usage_format() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_display.php").unwrap();
    let text = concat!("<?php\n", "/**\n", " * @\n", " */\n", "class Foo {}\n",);

    let items = complete_at(&backend, &uri, text, 2, 4).await;

    // Tags with formats should show the format in label
    let method_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@method"));
    assert!(method_item.is_some(), "Should have @method item");
    assert_eq!(
        method_item.unwrap().label,
        "@method ReturnType name()",
        "@method label should show usage pattern"
    );

    let template_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@template"));
    assert!(template_item.is_some(), "Should have @template item");
    assert_eq!(
        template_item.unwrap().label,
        "@template T",
        "@template label should show usage pattern"
    );

    // Tags without a special format should use the raw tag as label
    let deprecated_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@deprecated"));
    assert!(deprecated_item.is_some(), "Should have @deprecated item");
    assert_eq!(
        deprecated_item.unwrap().label,
        "@deprecated",
        "@deprecated should use tag as label"
    );
}

/// The insert_text for generic tags should be the raw tag name only,
/// not the display format.
#[tokio::test]
async fn test_phpdoc_insert_text_is_raw_tag() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_insert.php").unwrap();
    let text = concat!("<?php\n", "/**\n", " * @\n", " */\n", "class Foo {}\n",);

    let items = complete_at(&backend, &uri, text, 2, 4).await;

    let method_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@method"));
    assert!(method_item.is_some(), "Should have @method item");
    assert_eq!(
        method_item.unwrap().insert_text.as_deref(),
        Some("method"),
        "@method insert_text should be the raw tag without @"
    );

    let template_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@template"));
    assert!(template_item.is_some(), "Should have @template item");
    assert_eq!(
        template_item.unwrap().insert_text.as_deref(),
        Some("template"),
        "@template insert_text should be the raw tag without @"
    );
}

// ─── Smart pre-fill integration tests ───────────────────────────────────────

/// @param items should be pre-filled with parameter types and names
/// extracted from the function declaration.
#[tokio::test]
async fn test_phpdoc_smart_param_prefilled() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_smart_param.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function greet(string $name, int $age): string {}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;

    let param_items: Vec<_> = items
        .iter()
        .filter(|i| i.filter_text.as_deref() == Some("@param"))
        .collect();

    // Should have one item per parameter
    assert_eq!(
        param_items.len(),
        2,
        "Should have one @param per parameter. Got: {:?}",
        param_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );

    assert_eq!(param_items[0].label, "@param string $name");
    assert_eq!(
        param_items[0].insert_text.as_deref(),
        Some("param string $name")
    );
    assert_eq!(param_items[1].label, "@param int $age");
    assert_eq!(
        param_items[1].insert_text.as_deref(),
        Some("param int $age")
    );
}

/// @return should be pre-filled with the return type hint.
#[tokio::test]
async fn test_phpdoc_smart_return_prefilled() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_smart_return.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function getName(): string {}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;

    let return_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@return"));
    assert!(return_item.is_some(), "Should have @return item");
    let r = return_item.unwrap();
    assert_eq!(r.label, "@return string");
    assert_eq!(r.insert_text.as_deref(), Some("return string"));
}

/// When a function has an explicit `: void` type hint, `@return` should
/// not be suggested at all — the type hint speaks for itself.
#[tokio::test]
async fn test_phpdoc_smart_return_void_generic() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_smart_void.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function doStuff(): void {}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;

    let return_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@return"));
    // Explicit `: void` type hint → @return is not needed
    assert!(
        return_item.is_none(),
        "Should NOT suggest @return when `: void` type hint is present. Got: {:?}",
        return_item.map(|i| &i.label)
    );
}

/// When an explicit `: void` type hint is present, `@return` should not
/// be suggested even if the body contains return statements with values.
#[tokio::test]
async fn test_phpdoc_smart_return_void_with_return_value() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_void_returns.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function doStuff(): void {\n",
        "    return $this->something();\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;

    let return_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@return"));
    assert!(
        return_item.is_none(),
        "Should NOT suggest @return when `: void` type hint is present. Got: {:?}",
        return_item.map(|i| &i.label)
    );
}

/// When an explicit `: void` type hint is present, `@return` should not
/// be suggested even with bare `return;` statements.
#[tokio::test]
async fn test_phpdoc_smart_return_void_with_bare_return() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_void_bare.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function doStuff(): void {\n",
        "    if (true) {\n",
        "        return;\n",
        "    }\n",
        "    echo 'done';\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;

    let return_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@return"));
    assert!(
        return_item.is_none(),
        "Should NOT suggest @return when `: void` type hint is present. Got: {:?}",
        return_item.map(|i| &i.label)
    );
}

/// @var should be pre-filled with the property type hint.
#[tokio::test]
async fn test_phpdoc_smart_var_prefilled() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_smart_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public string $name;\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;

    let var_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@var"));
    assert!(var_item.is_some(), "Should have @var item");
    let v = var_item.unwrap();
    assert_eq!(v.label, "@var string");
    assert_eq!(v.insert_text.as_deref(), Some("var string"));
}

/// Smart @param should skip parameters that are already documented.
#[tokio::test]
async fn test_phpdoc_smart_param_skips_documented() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_smart_skip.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @param string $name\n",
        " * @\n",
        " */\n",
        "function greet(string $name, int $age): string {}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 4).await;

    let param_items: Vec<_> = items
        .iter()
        .filter(|i| i.filter_text.as_deref() == Some("@param"))
        .collect();

    // $name is already documented, only $age should appear
    assert_eq!(
        param_items.len(),
        1,
        "Should only suggest undocumented params. Got: {:?}",
        param_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(param_items[0].label, "@param int $age");
}

/// @return should not be suggested when already documented.
#[tokio::test]
async fn test_phpdoc_smart_return_skipped_when_documented() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_smart_ret_skip.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @return string\n",
        " * @\n",
        " */\n",
        "function getName(): string {}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 4).await;

    let return_items: Vec<_> = items
        .iter()
        .filter(|i| i.filter_text.as_deref() == Some("@return"))
        .collect();

    assert!(
        return_items.is_empty(),
        "Should NOT suggest @return when already documented. Got: {:?}",
        return_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// @var should be pre-filled with nullable type.
#[tokio::test]
async fn test_phpdoc_smart_var_nullable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_smart_nullable.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    protected ?int $count = 0;\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;

    let var_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@var"));
    assert!(var_item.is_some(), "Should have @var item");
    assert_eq!(var_item.unwrap().label, "@var ?int");
}

/// @return should be pre-filled with nullable return type.
#[tokio::test]
async fn test_phpdoc_smart_return_nullable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_smart_ret_null.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function find(): ?User {}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;

    let return_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@return"));
    assert!(return_item.is_some(), "Should have @return item");
    assert_eq!(return_item.unwrap().label, "@return ?User");
}

/// Smart @param for untyped parameters should still show parameter names.
#[tokio::test]
async fn test_phpdoc_smart_param_untyped() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_smart_untyped.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function process($data, $options) {}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;

    let param_items: Vec<_> = items
        .iter()
        .filter(|i| i.filter_text.as_deref() == Some("@param"))
        .collect();

    assert_eq!(param_items.len(), 2);
    assert_eq!(param_items[0].label, "@param $data");
    assert_eq!(param_items[1].label, "@param $options");
}

/// When all params are documented, fall back to generic @param.
#[tokio::test]
async fn test_phpdoc_smart_all_params_documented() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_smart_all_doc.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @param string $name\n",
        " * @\n",
        " */\n",
        "function greet(string $name): string {}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 4).await;

    let param_items: Vec<_> = items
        .iter()
        .filter(|i| i.filter_text.as_deref() == Some("@param"))
        .collect();

    // All params documented → @param is filtered out entirely
    assert!(
        param_items.is_empty(),
        "Should NOT suggest @param when all params are documented. Got: {:?}",
        param_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

// ─── Docblock @ prefix isolation ────────────────────────────────────────────
// When the cursor is inside a docblock on a word starting with `@`, ONLY
// PHPDoc tag suggestions should appear — never class names, constants, or
// functions that happen to match the text after `@`.

/// Typing `@potato` in a docblock should return an empty list, not class
/// names or constants that contain "potato".
#[tokio::test]
async fn test_phpdoc_no_class_completion_for_unknown_tag() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_potato.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class PotatoFactory {}\n",
        "define('WORLD_POTATO_CONSUMPTION', 42);\n",
        "/**\n",
        " * @potato\n",
        " */\n",
        "function cook(): void {}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 11).await;

    // No PHPDoc tags start with `@potato`, so the list should be empty.
    // Crucially, PotatoFactory and WORLD_POTATO_CONSUMPTION must NOT appear.
    assert!(
        items.is_empty(),
        "Typing @potato in a docblock should yield no completions, got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Typing `@throw` should suggest `@throws` (the matching PHPDoc tag),
/// not class names containing "throw".
#[tokio::test]
async fn test_phpdoc_partial_tag_suggests_matching_tags() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_throw.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @throw\n",
        " */\n",
        "function risky(): void {}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 10).await;

    // Should contain the generic @throws fallback
    let throws_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@throws"));
    assert!(
        throws_item.is_some(),
        "Typing @throw should suggest @throws tag, got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );

    // Should NOT contain class items
    let class_items: Vec<_> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::CLASS))
        .collect();
    assert!(
        class_items.is_empty(),
        "No class items should appear in docblock @tag context, got: {:?}",
        class_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Typing `@re` should suggest `@return` (and other matching tags) but
/// never class names or constants.
#[tokio::test]
async fn test_phpdoc_partial_re_suggests_return_not_classes() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_re.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Renderer {}\n",
        "/**\n",
        " * @re\n",
        " */\n",
        "function render(): string { return ''; }\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 7).await;

    // Should contain @return
    let return_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@return"));
    assert!(
        return_item.is_some(),
        "Typing @re should suggest @return, got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );

    // Should NOT contain Renderer or any class
    let class_items: Vec<_> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::CLASS))
        .collect();
    assert!(
        class_items.is_empty(),
        "Class items like Renderer must not appear in docblock, got: {:?}",
        class_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Typing just `@` should show all applicable PHPDoc tags, not classes.
#[tokio::test]
async fn test_phpdoc_at_sign_only_shows_tags() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_at.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class SomeClass {}\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function demo(): void {}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 4).await;

    // Should have PHPDoc tags
    assert!(
        !items.is_empty(),
        "Typing @ in docblock should suggest PHPDoc tags"
    );

    // Every item should be a KEYWORD (PHPDoc tag), not a CLASS or CONSTANT
    for item in &items {
        assert_eq!(
            item.kind,
            Some(CompletionItemKind::KEYWORD),
            "All items in docblock @ context should be KEYWORD, got {:?} for '{}'",
            item.kind,
            item.label
        );
    }
}

// ─── @return void with no type hint ─────────────────────────────────────────

/// When a function has no return type hint and an empty body, `@return void`
/// should be suggested (not the generic `@return Type`).
#[tokio::test]
async fn test_phpdoc_return_void_no_type_hint_empty_body() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_void_no_hint.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Demo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function singleCatch() { }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;

    let return_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@return"));
    assert!(
        return_item.is_some(),
        "Should suggest @return void when no type hint and empty body"
    );
    assert_eq!(
        return_item.unwrap().label,
        "@return void",
        "Label should be @return void, not the generic fallback"
    );
    assert_eq!(
        return_item.unwrap().insert_text.as_deref(),
        Some("return void"),
    );
}

/// When a function has no return type hint but DOES return a value,
/// the generic `@return Type` fallback should appear instead of `@return void`.
#[tokio::test]
async fn test_phpdoc_return_no_type_hint_with_return_value() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_no_hint_ret.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Demo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function getData() {\n",
        "        return ['key' => 'value'];\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;

    let return_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@return"));
    // Body has `return $value;` so it's not void — show generic fallback
    assert!(
        return_item.is_some(),
        "Should suggest @return when body has return with value"
    );
    assert_eq!(
        return_item.unwrap().label,
        "@return Type",
        "Should show generic @return Type, not @return void"
    );
}

/// When a function has no return type hint and only bare `return;` statements,
/// `@return void` should still be suggested.
#[tokio::test]
async fn test_phpdoc_return_void_no_hint_bare_return() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_no_hint_bare.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Demo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function process() {\n",
        "        if (true) {\n",
        "            return;\n",
        "        }\n",
        "        echo 'done';\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;

    let return_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@return"));
    assert!(
        return_item.is_some(),
        "Should suggest @return void when body only has bare return;"
    );
    assert_eq!(return_item.unwrap().label, "@return void");
}

// ─── Inline @var completion ─────────────────────────────────────────────────

/// Inline @var above a variable assignment should sort first (0a_ prefix),
/// ahead of general tags like @example, @todo, etc.
#[tokio::test]
async fn test_phpdoc_inline_var_sorts_first() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_inline_sort.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo() {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    $var = getUser();\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;

    let var_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@var"));
    assert!(
        var_item.is_some(),
        "Should have @var item in inline context"
    );
    let v = var_item.unwrap();
    assert!(
        v.sort_text.as_deref().unwrap_or("").starts_with("0a_"),
        "Inline @var should have sort text starting with 0a_, got: {:?}",
        v.sort_text
    );

    // General tags like @todo should sort later (prefix "1_")
    let todo_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@todo"));
    assert!(todo_item.is_some(), "Should have @todo item");
    let t = todo_item.unwrap();
    assert!(
        t.sort_text.as_deref().unwrap_or("") > v.sort_text.as_deref().unwrap_or(""),
        "@var should sort before @todo. var={:?}, todo={:?}",
        v.sort_text,
        t.sort_text
    );
}

/// Inline @var with an inferred type should pre-fill the type.
/// Here the RHS is a simple array literal whose type can be inferred.
#[tokio::test]
async fn test_phpdoc_inline_var_inferred_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_inline_inferred.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo() {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    $items = [1, 2, 3];\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;

    let var_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@var"));
    assert!(
        var_item.is_some(),
        "Should have @var item in inline context"
    );
    let v = var_item.unwrap();

    // The label should contain the inferred type (e.g. "list<int>")
    assert!(
        v.label.starts_with("@var ") && v.label != "@var Type",
        "Inline @var should have an inferred type, got label: {:?}",
        v.label
    );
    // Insert text should start with "var " followed by the type
    assert!(
        v.insert_text.as_deref().unwrap_or("").starts_with("var "),
        "Insert text should start with 'var ', got: {:?}",
        v.insert_text
    );
    // Sort text should have 0a_ prefix
    assert!(
        v.sort_text.as_deref().unwrap_or("").starts_with("0a_"),
        "Inferred inline @var should have sort text starting with 0a_, got: {:?}",
        v.sort_text
    );
}

/// Inline @var without an inferable type should offer "@var Type" with
/// a trailing space for the user to type the type manually.
#[tokio::test]
async fn test_phpdoc_inline_var_no_inferred_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_inline_no_infer.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo() {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    $result = someUnknownCall();\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;

    let var_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@var"));
    assert!(
        var_item.is_some(),
        "Should have @var item in inline context even without inferred type"
    );
    let v = var_item.unwrap();
    assert_eq!(
        v.label, "@var Type",
        "Label should be '@var Type' when type cannot be inferred"
    );
    // Insert text should be a snippet with a tab stop for the type
    assert_eq!(
        v.insert_text.as_deref(),
        Some("var ${1:Type}"),
        "Insert text should be a snippet with a Type placeholder"
    );
    assert_eq!(
        v.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "Inline @var without inferred type should use snippet format"
    );
    assert!(
        v.sort_text.as_deref().unwrap_or("").starts_with("0a_"),
        "Non-inferred inline @var should still sort first with 0a_ prefix"
    );
}

/// In unknown context (not above a variable, property, or function),
/// @var should offer a snippet with tab stops for Type and $var.
#[tokio::test]
async fn test_phpdoc_unknown_context_var_snippet() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_unknown_var.php").unwrap();
    // Docblock at file level, not above any declaration
    let text = concat!("<?php\n", "/**\n", " * @\n", " */\n",);

    let items = complete_at(&backend, &uri, text, 2, 4).await;

    let var_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@var"));
    assert!(
        var_item.is_some(),
        "Should have @var item in unknown context"
    );
    let v = var_item.unwrap();
    assert_eq!(
        v.label, "@var Type $var",
        "Unknown-context @var label should show full pattern"
    );
    assert_eq!(
        v.insert_text.as_deref(),
        Some("var ${1:Type} \\$${2:var}"),
        "Unknown-context @var should be a snippet with tab stops"
    );
    assert_eq!(
        v.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "Unknown-context @var should have SNIPPET insert format"
    );
}

// ─── Template enrichment on @param, @return, @var ───────────────────────────

/// @param completion for a function whose parameter is typed with a class
/// that has @template should offer the enriched type (e.g. Collection<T>).
#[tokio::test]
async fn test_phpdoc_template_enrichment_param() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        &[(
            "src/Collection.php",
            "<?php\nnamespace App;\n/**\n * @template T\n */\nclass Collection {}\n",
        )],
    );
    let uri = Url::parse("file:///phpdoc_enrich_param.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Collection;\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function process(Collection $items): void {}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 4).await;

    let param_items: Vec<_> = items
        .iter()
        .filter(|i| i.filter_text.as_deref() == Some("@param"))
        .collect();

    assert!(!param_items.is_empty(), "Should have @param items");

    let first = &param_items[0];
    assert_eq!(
        first.label, "@param Collection<T> $items",
        "Should enrich Collection with template param T"
    );
    assert_eq!(
        first.insert_text.as_deref(),
        Some("param Collection<${1:T}> \\$items"),
        "Insert text should contain tab stops on template params"
    );
    assert_eq!(
        first.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "Enriched @param should use snippet format"
    );
}

/// @return completion for a function returning a class with @template
/// should offer the enriched type.
#[tokio::test]
async fn test_phpdoc_template_enrichment_return() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        &[(
            "src/Collection.php",
            "<?php\nnamespace App;\n/**\n * @template T\n */\nclass Collection {}\n",
        )],
    );
    let uri = Url::parse("file:///phpdoc_enrich_return.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Collection;\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function getItems(): Collection {}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 4).await;

    let return_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@return"));
    assert!(return_item.is_some(), "Should have @return item");
    let r = return_item.unwrap();
    assert_eq!(
        r.label, "@return Collection<T>",
        "Should enrich Collection with template param T"
    );
    assert_eq!(
        r.insert_text.as_deref(),
        Some("return Collection<${1:T}>"),
        "Insert text should contain tab stops on template params"
    );
    assert_eq!(
        r.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "Enriched @return should use snippet format"
    );
}

/// @var completion on a property typed with a class that has @template
/// should offer the enriched type.
///
/// Note: the property type uses a `\`-prefixed FQN because the context
/// classifier lowercases tokens and only recognises unqualified class
/// names that start with `\`, `?`, or are built-in type keywords.
#[tokio::test]
async fn test_phpdoc_template_enrichment_var_property() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        &[(
            "src/Collection.php",
            "<?php\nnamespace App;\n/**\n * @template T\n */\nclass Collection {}\n",
        )],
    );
    let uri = Url::parse("file:///phpdoc_enrich_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Collection;\n",
        "class Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public \\App\\Collection $items;\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 8).await;

    let var_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@var"));
    assert!(var_item.is_some(), "Should have @var item for property");
    let v = var_item.unwrap();
    // The enrichment strips the leading `\` and appends template params.
    assert!(
        v.label.contains("Collection<T>"),
        "Should enrich Collection with template param T, got: {:?}",
        v.label
    );
    assert!(
        v.insert_text
            .as_deref()
            .unwrap_or("")
            .contains("Collection<${1:T}>"),
        "Insert text should contain enriched type with tab stop, got: {:?}",
        v.insert_text
    );
    assert_eq!(
        v.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "Enriched @var should use snippet format"
    );
}

/// Template enrichment with multiple template parameters should list all.
#[tokio::test]
async fn test_phpdoc_template_enrichment_multiple_params() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        &[(
            "src/Map.php",
            "<?php\nnamespace App;\n/**\n * @template TKey\n * @template TValue\n */\nclass Map {}\n",
        )],
    );
    let uri = Url::parse("file:///phpdoc_enrich_multi.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Map;\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function process(Map $lookup): Map {}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 4).await;

    let param_items: Vec<_> = items
        .iter()
        .filter(|i| i.filter_text.as_deref() == Some("@param"))
        .collect();
    assert!(!param_items.is_empty(), "Should have @param items");
    assert_eq!(
        param_items[0].label, "@param Map<TKey, TValue> $lookup",
        "Should enrich Map with both template params"
    );
    assert_eq!(
        param_items[0].insert_text.as_deref(),
        Some("param Map<${1:TKey}, ${2:TValue}> \\$lookup"),
        "Insert text should contain tab stops on all template params"
    );
    assert_eq!(
        param_items[0].insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "Enriched @param should use snippet format"
    );

    let return_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@return"));
    assert!(return_item.is_some(), "Should have @return item");
    let r = return_item.unwrap();
    assert_eq!(
        r.label, "@return Map<TKey, TValue>",
        "Should enrich Map with both template params"
    );
    assert_eq!(
        r.insert_text.as_deref(),
        Some("return Map<${1:TKey}, ${2:TValue}>"),
        "Insert text should contain tab stops on all template params"
    );
    assert_eq!(
        r.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "Enriched @return should use snippet format"
    );
}

// ─── Inline @var context boundary tests ─────────────────────────────────────

/// When the line after the docblock is a method call (`$names->add(…)`),
/// NOT a variable assignment, the context should be Unknown and @var
/// should offer the `@var Type $var` snippet, not a pre-filled type.
#[tokio::test]
async fn test_phpdoc_inline_var_method_call_not_assignment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_inline_method_call.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo() {\n",
        "    $names = new \\ArrayObject();\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    $names->add('foo');\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 8).await;

    let var_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@var"));
    assert!(
        var_item.is_some(),
        "Should still have a @var item. Got tags: {:?}",
        filter_texts(&items)
    );
    let v = var_item.unwrap();
    // Should be the Unknown-context snippet, NOT a pre-filled type
    assert_eq!(
        v.label, "@var Type $var",
        "Method call on next line should yield Unknown context, not Inline. Got: {:?}",
        v.label
    );
    assert_eq!(
        v.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "Should be a snippet with tab stops"
    );
}

/// When there is a blank line between the docblock and the assignment,
/// the context should be Unknown and @var should offer the snippet form.
#[tokio::test]
async fn test_phpdoc_inline_var_blank_line_gap() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_inline_blank_gap.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo() {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "\n",
        "    $names = new \\ArrayObject();\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;

    let var_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@var"));
    assert!(
        var_item.is_some(),
        "Should still have a @var item. Got tags: {:?}",
        filter_texts(&items)
    );
    let v = var_item.unwrap();
    assert_eq!(
        v.label, "@var Type $var",
        "Blank line gap should prevent Inline context. Got: {:?}",
        v.label
    );
    assert_eq!(
        v.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "Should be a snippet with tab stops"
    );
}

/// When the docblock is directly above an assignment (no blank line),
/// inline @var should be detected and the type should be pre-filled.
#[tokio::test]
async fn test_phpdoc_inline_var_immediate_assignment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_inline_immediate.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo() {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    $items = [1, 2, 3];\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;

    let var_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@var"));
    assert!(
        var_item.is_some(),
        "Should have @var item in inline context"
    );
    let v = var_item.unwrap();
    // Should be a pre-filled type, NOT the snippet form
    assert_ne!(
        v.label, "@var Type $var",
        "Immediate assignment should yield Inline context with inferred type"
    );
    assert!(
        v.label.starts_with("@var "),
        "Label should start with '@var '. Got: {:?}",
        v.label
    );
    assert_eq!(
        v.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "Inline @var with inferred type should be a snippet with tab stops"
    );
}

/// Inline @var with an inferred class type should enrich it with
/// template parameters from @template when a class loader is available,
/// and shorten the class name using the file's `use` import.
#[tokio::test]
async fn test_phpdoc_inline_var_enriched_with_templates() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        &[(
            "src/Collection.php",
            "<?php\nnamespace App;\n/**\n * @template TKey\n * @template TValue\n */\nclass Collection {}\n",
        )],
    );
    let uri = Url::parse("file:///phpdoc_inline_enrich.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Collection;\n",
        "function demo() {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    $names = new Collection();\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 8).await;

    let var_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@var"));
    assert!(
        var_item.is_some(),
        "Should have @var item in inline context"
    );
    let v = var_item.unwrap();
    assert_eq!(
        v.label, "@var Collection<TKey, TValue>",
        "Inferred Collection type should be enriched with template params and shortened via the use import"
    );
    assert_eq!(
        v.insert_text.as_deref(),
        Some("var Collection<${1:TKey}, ${2:TValue}>"),
        "Insert text should contain enriched type with tab stops on template params, shortened via the use import"
    );
    assert_eq!(
        v.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "Enriched inline @var should use snippet format"
    );
}

/// When a variable is used (not assigned) on the next line after the
/// docblock, the context should be Unknown even if the variable was
/// assigned earlier in the function.
#[tokio::test]
async fn test_phpdoc_inline_var_usage_not_assignment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_inline_usage.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo() {\n",
        "    $names = ['a', 'b'];\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    echo $names[0];\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 8).await;

    let var_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@var"));
    assert!(
        var_item.is_some(),
        "Should have a @var item. Got tags: {:?}",
        filter_texts(&items)
    );
    let v = var_item.unwrap();
    // `echo $names[0]` is not an assignment — Unknown context
    assert_eq!(
        v.label, "@var Type $var",
        "Variable usage (not assignment) should yield Unknown context. Got: {:?}",
        v.label
    );
}

/// Compound assignment operators like `+=`, `.=` should NOT be treated
/// as a variable assignment for inline @var purposes.
#[tokio::test]
async fn test_phpdoc_smart_var_property_with_v_prefix() {
    // When typing `@v` (not bare `@`), the smart pre-filled @var should
    // still appear for a typed property — not the generic `@var Type $var`.
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_var_v_prefix.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "     * @v\n",
        "     */\n",
        "    public string $name;\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 9).await;

    let var_items: Vec<_> = items
        .iter()
        .filter(|i| i.filter_text.as_deref() == Some("@var"))
        .collect();
    assert!(
        !var_items.is_empty(),
        "Should have @var item with @v prefix"
    );
    assert_eq!(
        var_items.len(),
        1,
        "Should have exactly one @var item (smart, not generic). Got: {:?}",
        var_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    let v = var_items[0];
    assert_eq!(
        v.label, "@var string",
        "Should be smart pre-filled @var, not generic snippet"
    );
    assert_eq!(v.insert_text.as_deref(), Some("var string"));
}

#[tokio::test]
async fn test_phpdoc_smart_var_promoted_property_bare_at() {
    // A docblock above a promoted constructor property should detect
    // Property context and offer smart @var with the type hint.
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_promoted_bare.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function __construct(\n",
        "        /**\n",
        "         * @\n",
        "         */\n",
        "        private string $service,\n",
        "    ) {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 12).await;

    let var_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@var"));
    assert!(
        var_item.is_some(),
        "Should have @var item for promoted property"
    );
    let v = var_item.unwrap();
    assert_eq!(
        v.label, "@var string",
        "Should be smart pre-filled @var for promoted property"
    );
    assert_eq!(v.insert_text.as_deref(), Some("var string"));
}

#[tokio::test]
async fn test_phpdoc_smart_var_promoted_property_with_v_prefix() {
    // Typing `@v` above a promoted constructor property should still
    // produce the smart pre-filled @var, not the generic snippet.
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_promoted_v.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function __construct(\n",
        "        /**\n",
        "         * @v\n",
        "         */\n",
        "        private string $service,\n",
        "    ) {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 13).await;

    let var_items: Vec<_> = items
        .iter()
        .filter(|i| i.filter_text.as_deref() == Some("@var"))
        .collect();
    assert!(
        !var_items.is_empty(),
        "Should have @var item for promoted property with @v prefix"
    );
    assert_eq!(
        var_items.len(),
        1,
        "Should have exactly one @var item (smart). Got: {:?}",
        var_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    let v = var_items[0];
    assert_eq!(
        v.label, "@var string",
        "Should be smart pre-filled @var for promoted property"
    );
    assert_eq!(v.insert_text.as_deref(), Some("var string"));
}

#[tokio::test]
async fn test_phpdoc_smart_var_promoted_property_enriched() {
    // Promoted property with a class type should get template enrichment.
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        &[(
            "src/Collection.php",
            "<?php\nnamespace App;\n/**\n * @template TKey\n * @template TModel\n */\nclass Collection {}\n",
        )],
    );
    let uri = Url::parse("file:///phpdoc_promoted_enriched.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Collection;\n",
        "class Foo {\n",
        "    public function __construct(\n",
        "        /**\n",
        "         * @\n",
        "         */\n",
        "        private \\App\\Collection $items,\n",
        "    ) {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 5, 12).await;

    let var_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@var"));
    assert!(
        var_item.is_some(),
        "Should have @var item for promoted property"
    );
    let v = var_item.unwrap();
    assert!(
        v.label.contains("Collection<TKey, TModel>"),
        "Should enrich Collection with template params TKey, TModel, got: {:?}",
        v.label
    );
    // Insert text should use snippet format with tab stops on template params.
    assert_eq!(
        v.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "Enriched property @var should use snippet format. insert_text: {:?}",
        v.insert_text
    );
    let insert = v.insert_text.as_deref().unwrap_or("");
    assert!(
        insert.contains("${1:TKey}") && insert.contains("${2:TModel}"),
        "Insert text should have tab stops for template params, got: {:?}",
        insert
    );
}

#[tokio::test]
async fn test_phpdoc_smart_var_no_generic_when_smart_available() {
    // When a smart @var is available, the generic `@var Type $var` snippet
    // must NOT appear alongside it.
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_no_generic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "     * @v\n",
        "     */\n",
        "    public int $count;\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 9).await;

    let var_items: Vec<_> = items
        .iter()
        .filter(|i| i.filter_text.as_deref() == Some("@var"))
        .collect();
    // Should never see the generic `@var Type $var` snippet
    let has_generic = var_items
        .iter()
        .any(|i| i.label.contains("$var") || i.label.contains("Type $"));
    assert!(
        !has_generic,
        "Generic `@var Type $var` should NOT appear when smart @var is available. Items: {:?}",
        var_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Regression test: exact editor scenario where typing `@v` in a property
/// docblock should produce the smart pre-filled `@var string`, not the
/// generic `@var Type $var` snippet. The content here mirrors what the
/// editor sends on a re-triggered completion request after the user types
/// the `v` character following `@`.
#[tokio::test]
async fn test_phpdoc_smart_var_property_retrigger_after_v() {
    // Simulate the exact sequence: user has a closed docblock, types `@`,
    // gets completions, then types `v` and the editor re-requests.
    // The file content at that point has `@v` in it.
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_retrigger_v.php").unwrap();

    // Step 1: bare `@` — should get smart @var
    let text_at = concat!(
        "<?php\n",
        "class MyService {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public string $name;\n",
        "}\n",
    );
    let items_at = complete_at(&backend, &uri, text_at, 3, 8).await;
    let var_at = items_at
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@var"));
    assert!(var_at.is_some(), "Bare @ should produce @var");
    assert_eq!(
        var_at.unwrap().label,
        "@var string",
        "Bare @ should produce smart @var string"
    );

    // Step 2: user types `v`, editor re-requests with `@v` in the content.
    // Use a different URI to avoid cached state from step 1.
    let uri2 = Url::parse("file:///phpdoc_retrigger_v2.php").unwrap();
    let text_v = concat!(
        "<?php\n",
        "class MyService {\n",
        "    /**\n",
        "     * @v\n",
        "     */\n",
        "    public string $name;\n",
        "}\n",
    );
    let items_v = complete_at(&backend, &uri2, text_v, 3, 9).await;
    let var_items_v: Vec<_> = items_v
        .iter()
        .filter(|i| i.filter_text.as_deref() == Some("@var"))
        .collect();
    assert!(
        !var_items_v.is_empty(),
        "@v prefix should still produce @var item"
    );
    assert_eq!(
        var_items_v.len(),
        1,
        "Should have exactly one @var, not both smart and generic. Got: {:?}",
        var_items_v.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(
        var_items_v[0].label, "@var string",
        "@v prefix should produce smart pre-filled @var, not generic snippet"
    );
    // The insert text should NOT contain `$var` (property @var omits the variable name)
    assert!(
        !var_items_v[0]
            .insert_text
            .as_deref()
            .unwrap_or("")
            .contains("$var"),
        "Property @var should not include $var in insert text. Got: {:?}",
        var_items_v[0].insert_text
    );
}

#[tokio::test]
async fn test_phpdoc_smart_var_property_open_docblock_v_prefix() {
    // When the docblock is not yet closed (user is still typing), the
    // context detection falls back to scanning text after the cursor.
    // Make sure the smart @var still works with an unclosed docblock.
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_open_v_prefix.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "     * @v\n",
        "    public string $name;\n",
        "}\n",
    );

    // Cursor after `@v` on line 3
    let items = complete_at(&backend, &uri, text, 3, 9).await;

    let var_items: Vec<_> = items
        .iter()
        .filter(|i| i.filter_text.as_deref() == Some("@var"))
        .collect();
    assert!(
        !var_items.is_empty(),
        "Should have @var item for open docblock with @v prefix. All items: {:?}",
        items
            .iter()
            .map(|i| (&i.label, &i.filter_text))
            .collect::<Vec<_>>()
    );
    // Should be the smart pre-filled version, not the generic snippet.
    let v = var_items[0];
    assert_eq!(
        v.label, "@var string",
        "Open docblock should still produce smart @var. Got: {:?}",
        v.label
    );
}

#[tokio::test]
async fn test_phpdoc_inline_var_compound_assignment_is_unknown() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpdoc_inline_compound.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo() {\n",
        "    $total = 0;\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    $total += 10;\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 8).await;

    let var_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@var"));
    assert!(var_item.is_some(), "Should have a @var item");
    let v = var_item.unwrap();
    // Inline @var should detect `$total = …` as assignment, and
    // `$total += …` has `=` after `+`, so our `is_variable_assignment`
    // should handle this — it checks for `=` not preceded by other ops.
    // Actually += starts with `+` not `=`, so `rest` after var name
    // is `+= 10;`. strip_prefix('=') fails → not assignment → Unknown.
    assert_eq!(
        v.label, "@var Type $var",
        "Compound assignment should yield Unknown context. Got: {:?}",
        v.label
    );
}

// ─── PHPDoc type references exclude traits ──────────────────────────────────

/// `@param` type completions should exclude traits (they are meaningless
/// as type hints in PHP).
#[tokio::test]
async fn test_phpdoc_param_type_excludes_traits() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/phpdoc_trait_filter.php").unwrap();

    let src = concat!(
        "<?php\n",
        "namespace DocTraitTest;\n",
        "class SomeClass {}\n",
        "interface SomeInterface {}\n",
        "trait SomeTrait {}\n",
        "enum SomeEnum {}\n",
        "class Demo {\n",
        "    /**\n",
        "     * @param Some\n",
        "     */\n",
        "    public function foo($x): void {}\n",
        "}\n",
    );
    // Line 8: `     * @param Some`
    // cursor after "Some" = col 17
    let items = complete_at(&backend, &uri, src, 8, 17).await;
    let class_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::CLASS))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        class_labels.contains(&"SomeClass"),
        "@param type should include classes, got: {:?}",
        class_labels
    );
    assert!(
        class_labels.contains(&"SomeInterface"),
        "@param type should include interfaces, got: {:?}",
        class_labels
    );
    assert!(
        class_labels.contains(&"SomeEnum"),
        "@param type should include enums, got: {:?}",
        class_labels
    );
    assert!(
        !class_labels.contains(&"SomeTrait"),
        "@param type should NOT include traits, got: {:?}",
        class_labels
    );
}

/// `@return` type completions should exclude traits.
#[tokio::test]
async fn test_phpdoc_return_type_excludes_traits() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/phpdoc_return_trait.php").unwrap();

    let src = concat!(
        "<?php\n",
        "namespace DocReturnTest;\n",
        "class SomeClass {}\n",
        "trait SomeTrait {}\n",
        "class Demo {\n",
        "    /**\n",
        "     * @return Some\n",
        "     */\n",
        "    public function foo() {}\n",
        "}\n",
    );
    // Line 6: `     * @return Some`
    // cursor after "Some" = col 18
    let items = complete_at(&backend, &uri, src, 6, 18).await;
    let class_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::CLASS))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        class_labels.contains(&"SomeClass"),
        "@return type should include classes, got: {:?}",
        class_labels
    );
    assert!(
        !class_labels.contains(&"SomeTrait"),
        "@return type should NOT include traits, got: {:?}",
        class_labels
    );
}

/// `@var` type completions should exclude traits.
#[tokio::test]
async fn test_phpdoc_var_type_excludes_traits() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/phpdoc_var_trait.php").unwrap();

    let src = concat!(
        "<?php\n",
        "namespace DocVarTest;\n",
        "class SomeClass {}\n",
        "trait SomeTrait {}\n",
        "class Demo {\n",
        "    /**\n",
        "     * @var Some\n",
        "     */\n",
        "    public $prop;\n",
        "}\n",
    );
    // Line 6: `     * @var Some`
    // cursor after "Some" = col 15
    let items = complete_at(&backend, &uri, src, 6, 15).await;
    let class_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::CLASS))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        class_labels.contains(&"SomeClass"),
        "@var type should include classes, got: {:?}",
        class_labels
    );
    assert!(
        !class_labels.contains(&"SomeTrait"),
        "@var type should NOT include traits, got: {:?}",
        class_labels
    );
}

/// `@throws` type completions should still use Throwable-filtered
/// completion (not the TypeHint filter), so this is unchanged.
#[tokio::test]
async fn test_phpdoc_param_variable_uses_text_edit_with_dollar() {
    // Regression test for issue #27: typing `$` (or `$va`) after the type
    // in a @param tag was doubling the dollar sign in editors like Helix and
    // Neovim that don't treat `$` as a word character.  The fix is to return
    // a `text_edit` with an explicit replacement range covering the typed
    // `$…` prefix instead of using `insert_text`.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/phpdoc_var_text_edit.php").unwrap();

    // Cursor is right after `$v` on the @param line — partial = "$v".
    let src = concat!(
        "<?php\n",
        "class Demo {\n",
        "    /**\n",
        "     * @param string $v\n",
        "     */\n",
        "    public function foo(string $value): void {}\n",
        "}\n",
    );
    // Line 3: `     * @param string $v`
    // character 24 = right after `$v`
    let items = complete_at(&backend, &uri, src, 3, 24).await;

    let value_item = items.iter().find(|i| i.label == "$value");
    assert!(
        value_item.is_some(),
        "Should suggest $value, got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    let item = value_item.unwrap();

    // Must use text_edit (not insert_text) so the editor replaces the
    // already-typed `$v` prefix rather than inserting after it.
    match &item.text_edit {
        Some(CompletionTextEdit::Edit(te)) => {
            assert_eq!(te.new_text, "$value", "text_edit new_text should be $value");
            // Range must start at the `$` (col 22) and end at cursor (col 24).
            assert_eq!(
                te.range.start,
                Position::new(3, 22),
                "replacement range should start at the `$`"
            );
            assert_eq!(
                te.range.end,
                Position::new(3, 24),
                "replacement range end should be at the cursor"
            );
        }
        other => panic!(
            "Expected text_edit with Edit variant to prevent double-dollar, got: {:?}",
            other
        ),
    }

    // insert_text must NOT be set — having both confuses editors.
    assert!(
        item.insert_text.is_none(),
        "insert_text should be None when text_edit is set"
    );
}

/// When the cursor is right after the bare `$` (no letters yet), the
/// replacement range should cover just that single character.
#[tokio::test]
async fn test_phpdoc_param_variable_bare_dollar_text_edit_range() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/phpdoc_bare_dollar.php").unwrap();

    // Cursor is right after `$` on the @param line — partial = "$".
    let src = concat!(
        "<?php\n",
        "class Demo {\n",
        "    /**\n",
        "     * @param string $\n",
        "     */\n",
        "    public function foo(string $name): void {}\n",
        "}\n",
    );
    // Line 3: `     * @param string $`
    // character 22 = right after `$`
    let items = complete_at(&backend, &uri, src, 3, 22).await;

    let name_item = items.iter().find(|i| i.label == "$name");
    assert!(
        name_item.is_some(),
        "Should suggest $name, got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    let item = name_item.unwrap();

    match &item.text_edit {
        Some(CompletionTextEdit::Edit(te)) => {
            assert_eq!(te.new_text, "$name", "new_text should be $name");
            // Range covers just the `$` at col 21..22.
            assert_eq!(
                te.range.start,
                Position::new(3, 21),
                "replacement should start at the `$`"
            );
            assert_eq!(
                te.range.end,
                Position::new(3, 22),
                "replacement end should be at the cursor"
            );
        }
        other => panic!("Expected text_edit with Edit variant, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_phpdoc_throws_uses_throwable_filter_not_type_hint() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/phpdoc_throws_filter.php").unwrap();

    let src = concat!(
        "<?php\n",
        "namespace DocThrowsTest;\n",
        "class SomeClass {}\n",
        "class SomeException extends \\Exception {}\n",
        "class Demo {\n",
        "    /**\n",
        "     * @throws Some\n",
        "     */\n",
        "    public function foo(): void {}\n",
        "}\n",
    );
    // Line 6: `     * @throws Some`
    // cursor after "Some" = col 18
    let items = complete_at(&backend, &uri, src, 6, 18).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    // SomeException extends RuntimeException → Throwable descendant → included
    assert!(
        labels.contains(&"SomeException"),
        "@throws should include Throwable descendants, got: {:?}",
        labels
    );
    // SomeClass has no parent → confirmed NOT Throwable → filtered out
    assert!(
        !labels.contains(&"SomeClass"),
        "@throws should filter out non-Throwable classes, got: {:?}",
        labels
    );
}
