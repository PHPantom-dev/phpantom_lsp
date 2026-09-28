//! Tests for smart `@throws` PHPDoc tag completion.
//!
//! These tests verify that when the user types `@` inside a docblock
//! preceding a function or method, the LSP suggests `@throws ExceptionType`
//! items for each exception type that is thrown but not caught inside the
//! function body.  Already-documented `@throws` tags are filtered out,
//! and `additional_text_edits` are emitted to auto-import exception classes
//! that are not yet in the file's `use` list.

use crate::common::{complete_at, create_psr4_workspace, create_test_backend};
use tower_lsp::lsp_types::*;

/// Helper: extract `@throws` completion items from a list.
fn throws_items(items: &[CompletionItem]) -> Vec<&CompletionItem> {
    items
        .iter()
        .filter(|i| {
            i.filter_text.as_deref() == Some("@throws")
                && i.label.starts_with("@throws ")
                && i.label != "@throws ExceptionType"
        })
        .collect()
}

/// Helper: extract the generic `@throws` fallback item.
fn generic_throws_item(items: &[CompletionItem]) -> Option<&CompletionItem> {
    items.iter().find(|i| i.label == "@throws ExceptionType")
}

// ─── Basic throw detection ──────────────────────────────────────────────────

/// A single `throw new` should produce a smart @throws suggestion.
#[tokio::test]
async fn test_throws_single_throw() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_single.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function doWork(): void {\n",
        "    throw new RuntimeException('boom');\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        1,
        "Should suggest one @throws. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(smart[0].label, "@throws RuntimeException");
    assert_eq!(
        smart[0].insert_text.as_deref(),
        Some("throws RuntimeException")
    );
}

/// Multiple different throw types produce multiple suggestions.
#[tokio::test]
async fn test_throws_multiple_types() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_multi.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function validate(mixed $value): void {\n",
        "    if ($value === null) {\n",
        "        throw new InvalidArgumentException('null');\n",
        "    }\n",
        "    if (!is_string($value)) {\n",
        "        throw new UnexpectedValueException('not string');\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        2,
        "Should suggest two @throws. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    let labels: Vec<&str> = smart.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"@throws InvalidArgumentException"));
    assert!(labels.contains(&"@throws UnexpectedValueException"));
}

/// Duplicate throw types should be deduplicated.
#[tokio::test]
async fn test_throws_deduplication() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_dedup.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function parse(string $json): array {\n",
        "    if (empty($json)) {\n",
        "        throw new InvalidArgumentException('empty');\n",
        "    }\n",
        "    $data = json_decode($json, true);\n",
        "    if ($data === null) {\n",
        "        throw new InvalidArgumentException('invalid json');\n",
        "    }\n",
        "    return $data;\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        1,
        "Duplicate throws should be merged. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(smart[0].label, "@throws InvalidArgumentException");
}

// ─── Try/catch filtering ────────────────────────────────────────────────────

/// Exceptions caught by a matching catch block should NOT be suggested.
#[tokio::test]
async fn test_throws_caught_exception_excluded() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_caught.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function safeWork(): void {\n",
        "    try {\n",
        "        throw new RuntimeException('boom');\n",
        "    } catch (RuntimeException $e) {\n",
        "        // handled\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert!(
        smart.is_empty(),
        "Caught exception should not be suggested. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// When only some exceptions are caught, only uncaught ones should be suggested.
#[tokio::test]
async fn test_throws_partial_catch() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_partial.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function riskyWork(): void {\n",
        "    try {\n",
        "        throw new InvalidArgumentException('bad arg');\n",
        "        throw new RuntimeException('runtime');\n",
        "    } catch (InvalidArgumentException $e) {\n",
        "        // only IAE is caught\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        1,
        "Only uncaught exception should be suggested. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(smart[0].label, "@throws RuntimeException");
}

/// Multi-catch `catch (TypeA | TypeB $e)` should exclude all listed types.
#[tokio::test]
async fn test_throws_multi_catch() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_multicatch.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function process(): void {\n",
        "    try {\n",
        "        throw new InvalidArgumentException('a');\n",
        "        throw new RuntimeException('b');\n",
        "        throw new LogicException('c');\n",
        "    } catch (InvalidArgumentException | RuntimeException $e) {\n",
        "        // both caught\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        1,
        "Only LogicException should remain. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(smart[0].label, "@throws LogicException");
}

/// `catch (Throwable $e)` catches everything.
#[tokio::test]
async fn test_throws_catch_throwable_catches_all() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_throwable.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function safe(): void {\n",
        "    try {\n",
        "        throw new RuntimeException('a');\n",
        "        throw new InvalidArgumentException('b');\n",
        "    } catch (\\Throwable $e) {\n",
        "        // catches everything\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert!(
        smart.is_empty(),
        "catch(Throwable) should catch all. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// `catch (Exception $e)` catches all Exception subclasses.
#[tokio::test]
async fn test_throws_catch_exception_catches_all() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_exception.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function safe(): void {\n",
        "    try {\n",
        "        throw new RuntimeException('a');\n",
        "    } catch (Exception $e) {\n",
        "        // catches Exception and subclasses\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert!(
        smart.is_empty(),
        "catch(Exception) should catch all Exceptions. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Throw outside try block should always be uncaught.
#[tokio::test]
async fn test_throws_outside_try() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_outside.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function mixed(): void {\n",
        "    throw new LogicException('always uncaught');\n",
        "    try {\n",
        "        throw new RuntimeException('caught');\n",
        "    } catch (RuntimeException $e) {\n",
        "        // handled\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        1,
        "Only LogicException (outside try) should be suggested. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(smart[0].label, "@throws LogicException");
}

// ─── Already-documented filtering ───────────────────────────────────────────

/// Exceptions already documented with @throws should be excluded.
#[tokio::test]
async fn test_throws_skips_already_documented() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_skip.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @throws RuntimeException\n",
        " * @\n",
        " */\n",
        "function doWork(): void {\n",
        "    throw new RuntimeException('boom');\n",
        "    throw new InvalidArgumentException('bad');\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 4).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        1,
        "RuntimeException is already documented, only IAE should appear. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(smart[0].label, "@throws InvalidArgumentException");
}

/// When all thrown exceptions are already documented, fall back to generic.
#[tokio::test]
async fn test_throws_all_documented_falls_back() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_all_doc.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @throws RuntimeException\n",
        " * @\n",
        " */\n",
        "function doWork(): void {\n",
        "    throw new RuntimeException('boom');\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 4).await;
    let smart = throws_items(&items);

    // All are documented, so no smart items
    assert!(
        smart.is_empty(),
        "All throws documented, should have no smart items. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );

    // Generic @throws fallback is always shown — the user may want to
    // manually document additional exceptions the detection missed.
    let generic = generic_throws_item(&items);
    assert!(
        generic.is_some(),
        "Generic @throws should always appear so users can manually add exceptions"
    );
}

/// @throws with FQN prefix (\RuntimeException) in docblock should match short name.
#[tokio::test]
async fn test_throws_documented_with_backslash_prefix() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_fqn_doc.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @throws \\RuntimeException\n",
        " * @\n",
        " */\n",
        "function doWork(): void {\n",
        "    throw new RuntimeException('boom');\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 4).await;
    let smart = throws_items(&items);

    assert!(
        smart.is_empty(),
        "FQN @throws should match short name in throw. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

// ─── Method context ─────────────────────────────────────────────────────────

/// @throws completion should work inside a class method docblock.
#[tokio::test]
async fn test_throws_in_method() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_method.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class UserService {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function createUser(string $email): void {\n",
        "        if (empty($email)) {\n",
        "            throw new InvalidArgumentException('Email required');\n",
        "        }\n",
        "        throw new RuntimeException('DB error');\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        2,
        "Should suggest two @throws. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    let labels: Vec<&str> = smart.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"@throws InvalidArgumentException"));
    assert!(labels.contains(&"@throws RuntimeException"));
}

/// Static methods should also get @throws completion.
#[tokio::test]
async fn test_throws_in_static_method() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_static.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Factory {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public static function create(string $type): self {\n",
        "        throw new InvalidArgumentException('Unknown type');\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;
    let smart = throws_items(&items);

    assert_eq!(smart.len(), 1);
    assert_eq!(smart[0].label, "@throws InvalidArgumentException");
}

// ─── FQN throw statements ───────────────────────────────────────────────────

/// `throw new \Namespace\ExceptionType()` should extract the short name.
#[tokio::test]
async fn test_throws_fqn_throw() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_fqn.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function doWork(): void {\n",
        "    throw new \\App\\Exceptions\\CustomException('error');\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        1,
        "Should extract short name from FQN. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(smart[0].label, "@throws CustomException");
}

// ─── Auto-import edits ──────────────────────────────────────────────────────

/// When an exception type is already imported, no additional edit is needed.
#[tokio::test]
async fn test_throws_no_import_when_already_imported() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_imported.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Exceptions\\CustomException;\n",
        "\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function doWork(): void {\n",
        "    throw new CustomException('error');\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 4).await;
    let smart = throws_items(&items);

    assert_eq!(smart.len(), 1);
    assert_eq!(smart[0].label, "@throws CustomException");
    // The type is already imported, so no additional edit needed
    assert!(
        smart[0].additional_text_edits.is_none(),
        "Should not add import for already-imported type"
    );
}

/// When in a namespace and the exception is resolved via the use map,
/// an import edit should be added if the use statement doesn't exist.
#[tokio::test]
async fn test_throws_auto_import_in_namespace() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_autoimport.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Services;\n",
        "\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function doWork(): void {\n",
        "    throw new CustomException('error');\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 4).await;
    let smart = throws_items(&items);

    assert_eq!(smart.len(), 1);
    assert_eq!(smart[0].label, "@throws CustomException");

    // Since we're in a namespace and CustomException isn't imported,
    // an auto-import edit should be present (it will resolve to
    // App\Services\CustomException since that's the current namespace).
    let edits = &smart[0].additional_text_edits;
    assert!(
        edits.is_some(),
        "Should add import edit for unimported exception in namespace"
    );
    let edits = edits.as_ref().unwrap();
    assert_eq!(edits.len(), 1);
    assert!(
        edits[0]
            .new_text
            .contains("use App\\Services\\CustomException;"),
        "Import should use current namespace. Got: {}",
        edits[0].new_text
    );
}

// ─── No function body (abstract / interface) ────────────────────────────────

/// Abstract methods have no body, so no @throws should be suggested.
#[tokio::test]
async fn test_throws_abstract_method_no_suggestions() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_abstract.php").unwrap();
    let text = concat!(
        "<?php\n",
        "abstract class Base {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    abstract public function doWork(): void;\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;
    let smart = throws_items(&items);

    assert!(
        smart.is_empty(),
        "Abstract methods have no body, no smart @throws. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );

    // Generic @throws IS shown for abstract methods — they commonly
    // declare @throws to document what exceptions implementations may throw.
    let generic = generic_throws_item(&items);
    assert!(
        generic.is_some(),
        "Generic @throws should appear for abstract methods (contract documentation)"
    );
}

/// Interface methods have no body, so no @throws should be suggested.
#[tokio::test]
async fn test_throws_interface_method_no_suggestions() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_interface.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface Repository {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function find(int $id): object;\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;
    let smart = throws_items(&items);

    assert!(
        smart.is_empty(),
        "Interface methods have no body. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

// ─── Non-function context ───────────────────────────────────────────────────

/// @throws should not produce smart items in a class-level docblock.
#[tokio::test]
async fn test_throws_not_in_class_context() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_class.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "class Foo {\n",
        "    public function bar(): void {\n",
        "        throw new RuntimeException('boom');\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    // Class-level docblock should not include @throws at all
    // (it's not in FUNCTION_TAGS context)
    assert!(
        smart.is_empty(),
        "Class context should not have smart @throws. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

// ─── Sort text ordering ────────────────────────────────────────────────────

/// Smart @throws items should sort at the top (sort_text starts with "0_").
#[tokio::test]
async fn test_throws_sort_text() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_sort.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function doWork(): void {\n",
        "    throw new RuntimeException('boom');\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert!(!smart.is_empty());
    for item in &smart {
        assert!(
            item.sort_text.as_deref().unwrap_or("").starts_with("0a_"),
            "Smart @throws should sort at top. sort_text: {:?}",
            item.sort_text
        );
    }
}

/// Smart @throws items should have filter_text = "@throws".
#[tokio::test]
async fn test_throws_filter_text() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_filter.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function doWork(): void {\n",
        "    throw new RuntimeException('boom');\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert!(!smart.is_empty());
    for item in &smart {
        assert_eq!(
            item.filter_text.as_deref(),
            Some("@throws"),
            "Smart @throws should filter on @throws"
        );
    }
}

// ─── Edge cases ─────────────────────────────────────────────────────────────

/// Throw in a string literal should not be detected.
#[tokio::test]
async fn test_throws_not_in_string() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_string.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function describe(): string {\n",
        "    return 'throw new RuntimeException is not real code';\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert!(
        smart.is_empty(),
        "Throw inside string should not be detected. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Throw in a comment should not be detected.
#[tokio::test]
async fn test_throws_not_in_comment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_comment.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function doNothing(): void {\n",
        "    // throw new RuntimeException('not real');\n",
        "    /* throw new InvalidArgumentException('also not real'); */\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert!(
        smart.is_empty(),
        "Throw inside comments should not be detected. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Namespaced exception with relative name: `throw new Exceptions\Custom()`
#[tokio::test]
async fn test_throws_relative_namespace() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_relative.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function doWork(): void {\n",
        "    throw new Exceptions\\CustomException('error');\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        1,
        "Should extract short name from relative namespace. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(smart[0].label, "@throws CustomException");
}

/// Prefix filtering: typing `@thr` should still show smart @throws items.
#[tokio::test]
async fn test_throws_prefix_filtering() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_prefix.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @thr\n",
        " */\n",
        "function doWork(): void {\n",
        "    throw new RuntimeException('boom');\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        1,
        "Partial prefix should still match @throws. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(smart[0].label, "@throws RuntimeException");
}

/// Empty function body should produce no smart @throws items.
#[tokio::test]
async fn test_throws_empty_body() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_empty.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function doNothing(): void {}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert!(
        smart.is_empty(),
        "Empty body should produce no smart @throws. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Method with @throws already documented below cursor should still filter.
#[tokio::test]
async fn test_throws_documented_below_cursor() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_below.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " * @throws RuntimeException\n",
        " */\n",
        "function doWork(): void {\n",
        "    throw new RuntimeException('boom');\n",
        "    throw new InvalidArgumentException('bad');\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        1,
        "RuntimeException documented below cursor should be excluded. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(smart[0].label, "@throws InvalidArgumentException");
}

/// Nested try-catch: throw in inner try that's caught by inner catch.
#[tokio::test]
async fn test_throws_nested_try_catch() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_nested.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function nested(): void {\n",
        "    try {\n",
        "        try {\n",
        "            throw new InvalidArgumentException('inner');\n",
        "        } catch (InvalidArgumentException $e) {\n",
        "            // caught in inner\n",
        "        }\n",
        "        throw new RuntimeException('outer');\n",
        "    } catch (RuntimeException $e) {\n",
        "        // caught in outer\n",
        "    }\n",
        "    throw new LogicException('not caught at all');\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        1,
        "Only LogicException should be uncaught. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(smart[0].label, "@throws LogicException");
}

/// PSR-4 cross-file: exception thrown is a known class with an import needed.
#[tokio::test]
async fn test_throws_psr4_auto_import() {
    let composer_json = r#"{
        "autoload": {
            "psr-4": {
                "App\\": "src/"
            }
        }
    }"#;

    let exception_file = concat!(
        "<?php\n",
        "namespace App\\Exceptions;\n",
        "\n",
        "class NotFoundException extends \\RuntimeException {}\n",
    );

    let service_file = concat!(
        "<?php\n",
        "namespace App\\Services;\n",
        "\n",
        "use App\\Exceptions\\NotFoundException;\n",
        "\n",
        "class UserService {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function find(int $id): object {\n",
        "        throw new NotFoundException('User not found');\n",
        "        throw new \\RuntimeException('DB error');\n",
        "    }\n",
        "}\n",
    );

    let (backend, _dir) = create_psr4_workspace(
        composer_json,
        &[
            ("src/Exceptions/NotFoundException.php", exception_file),
            ("src/Services/UserService.php", service_file),
        ],
    );

    let uri = Url::parse("file:///service.php").unwrap();
    let items = complete_at(&backend, &uri, service_file, 7, 8).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        2,
        "Should suggest NotFoundException and RuntimeException. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );

    // NotFoundException is already imported — no additional edit
    let nfe = smart
        .iter()
        .find(|i| i.label == "@throws NotFoundException");
    assert!(nfe.is_some(), "Should suggest NotFoundException");
    assert!(
        nfe.unwrap().additional_text_edits.is_none(),
        "NotFoundException already imported, no edit needed"
    );

    // RuntimeException is NOT imported (used with FQN in code) — should get import edit
    let rte = smart.iter().find(|i| i.label == "@throws RuntimeException");
    assert!(rte.is_some(), "Should suggest RuntimeException");
    // RuntimeException in namespace App\Services needs an import if used as short name
    // But it was thrown with \RuntimeException (FQN), the short name extraction gives "RuntimeException"
    // In namespace context, an import edit will be generated for App\Services\RuntimeException
    // which may not be ideal but is the expected behavior for unresolved types
}

/// When a function has no throws, only the generic @throws fallback appears.
#[tokio::test]
async fn test_throws_no_throws_generic_fallback() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_none.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function simple(): string {\n",
        "    return 'hello';\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert!(
        smart.is_empty(),
        "No throws, no smart items. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );

    // Generic fallback IS shown when no throws are detected — the user
    // may want to manually document exceptions the detection missed
    // (e.g. from external calls or library methods).
    let generic = generic_throws_item(&items);
    assert!(
        generic.is_some(),
        "Generic @throws should appear when no throw statements are detected (manual documentation)"
    );
}

/// throw $e where $e is a caught exception should detect the type.
#[tokio::test]
async fn test_throws_rethrow_variable_detected_from_catch() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_rethrow.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function wrapper(): void {\n",
        "    try {\n",
        "        doSomething();\n",
        "    } catch (RuntimeException $e) {\n",
        "        throw $e;\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    // `throw $e` re-throws the caught variable — the LSP resolves $e
    // to RuntimeException from the catch clause.
    assert_eq!(
        smart.len(),
        1,
        "expected one @throws suggestion, got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(smart[0].label, "@throws RuntimeException");
}

/// No global namespace, no use — should not add import edits.
#[tokio::test]
async fn test_throws_global_namespace_no_import() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_global.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function doWork(): void {\n",
        "    throw new RuntimeException('boom');\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert_eq!(smart.len(), 1);
    assert_eq!(smart[0].label, "@throws RuntimeException");
    // In global namespace, RuntimeException is already accessible
    assert!(
        smart[0].additional_text_edits.is_none(),
        "No import needed in global namespace"
    );
}

/// Completion item kind should be KEYWORD.
#[tokio::test]
async fn test_throws_item_kind() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_kind.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function doWork(): void {\n",
        "    throw new RuntimeException('boom');\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert!(!smart.is_empty());
    for item in &smart {
        assert_eq!(
            item.kind,
            Some(CompletionItemKind::KEYWORD),
            "Smart @throws items should have KEYWORD kind"
        );
    }
}

/// Throw with underscored exception name.
#[tokio::test]
async fn test_throws_underscored_name() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_underscore.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function doWork(): void {\n",
        "    throw new My_Custom_Exception('error');\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert_eq!(smart.len(), 1);
    assert_eq!(smart[0].label, "@throws My_Custom_Exception");
}

/// Throw inside double-quoted string should be ignored.
#[tokio::test]
async fn test_throws_not_in_double_quoted_string() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_dblquote.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function describe(): string {\n",
        "    return \"throw new RuntimeException is not real code\";\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert!(
        smart.is_empty(),
        "Throw inside double-quoted string should not be detected. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Throw with escaped quote in string should still not leak.
#[tokio::test]
async fn test_throws_escaped_quote_in_string() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_escape.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function doWork(): void {\n",
        "    $s = 'it\\'s a throw new RuntimeException test';\n",
        "    throw new LogicException('real');\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        1,
        "Only LogicException (real throw) should appear. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(smart[0].label, "@throws LogicException");
}

/// Catch with FQN should still filter the throw.
#[tokio::test]
async fn test_throws_fqn_catch() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_fqn_catch.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function safe(): void {\n",
        "    try {\n",
        "        throw new RuntimeException('boom');\n",
        "    } catch (\\RuntimeException $e) {\n",
        "        // caught via FQN\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert!(
        smart.is_empty(),
        "FQN catch should match short name throw. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Multiple independent try-catch blocks.
#[tokio::test]
async fn test_throws_multiple_try_catch_blocks() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_multitry.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function complex(): void {\n",
        "    try {\n",
        "        throw new InvalidArgumentException('first');\n",
        "    } catch (InvalidArgumentException $e) {}\n",
        "\n",
        "    try {\n",
        "        throw new RuntimeException('second');\n",
        "    } catch (RuntimeException $e) {}\n",
        "\n",
        "    throw new LogicException('uncaught');\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        1,
        "Only LogicException should be uncaught. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(smart[0].label, "@throws LogicException");
}

// ─── Propagated @throws from called methods ─────────────────────────────────

/// Calling a method that declares @throws should propagate to the caller.
#[tokio::test]
async fn test_throws_propagated_from_called_method() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_propagated.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Service {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function findOrFail(): void {\n",
        "        $this->safeOperation();\n",
        "    }\n",
        "\n",
        "    /**\n",
        "     * @throws AuthorizationException\n",
        "     */\n",
        "    public function safeOperation(): void {\n",
        "        throw new AuthorizationException('forbidden');\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        1,
        "Should propagate @throws from called method. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(smart[0].label, "@throws AuthorizationException");
}

/// Propagated throws should not duplicate direct throws.
#[tokio::test]
async fn test_throws_propagated_dedup_with_direct() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_prop_dedup.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Service {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function doWork(): void {\n",
        "        throw new RuntimeException('direct');\n",
        "        $this->riskyCall();\n",
        "    }\n",
        "\n",
        "    /**\n",
        "     * @throws RuntimeException\n",
        "     */\n",
        "    public function riskyCall(): void {\n",
        "        throw new RuntimeException('indirect');\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        1,
        "Direct and propagated same type should dedup. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(smart[0].label, "@throws RuntimeException");
}

/// Multiple called methods with different @throws should all propagate.
#[tokio::test]
async fn test_throws_propagated_from_multiple_methods() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_prop_multi.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Service {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function process(): void {\n",
        "        $this->validate();\n",
        "        $this->execute();\n",
        "    }\n",
        "\n",
        "    /**\n",
        "     * @throws InvalidArgumentException\n",
        "     */\n",
        "    public function validate(): void {}\n",
        "\n",
        "    /**\n",
        "     * @throws RuntimeException\n",
        "     */\n",
        "    public function execute(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        2,
        "Should propagate from both methods. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    let labels: Vec<&str> = smart.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"@throws InvalidArgumentException"));
    assert!(labels.contains(&"@throws RuntimeException"));
}

/// Already-documented propagated throws should be filtered.
#[tokio::test]
async fn test_throws_propagated_filtered_when_documented() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_prop_filter.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Service {\n",
        "    /**\n",
        "     * @throws InvalidArgumentException\n",
        "     * @\n",
        "     */\n",
        "    public function process(): void {\n",
        "        $this->validate();\n",
        "        $this->execute();\n",
        "    }\n",
        "\n",
        "    /**\n",
        "     * @throws InvalidArgumentException\n",
        "     */\n",
        "    public function validate(): void {}\n",
        "\n",
        "    /**\n",
        "     * @throws RuntimeException\n",
        "     */\n",
        "    public function execute(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 8).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        1,
        "IAE is documented, only RTE should appear. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(smart[0].label, "@throws RuntimeException");
}

/// self:: static calls should also propagate @throws.
#[tokio::test]
async fn test_throws_propagated_from_static_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_prop_static.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Service {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function run(): void {\n",
        "        self::validate();\n",
        "    }\n",
        "\n",
        "    /**\n",
        "     * @throws ValidationException\n",
        "     */\n",
        "    public static function validate(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        1,
        "Should propagate from self:: call. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(smart[0].label, "@throws ValidationException");
}

// ─── throw $this->method() — return type detection ──────────────────────────

/// `throw $this->createException()` should detect the return type.
#[tokio::test]
async fn test_throws_method_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_rettype.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Service {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function doWork(): void {\n",
        "        throw $this->makeException();\n",
        "    }\n",
        "\n",
        "    private function makeException(): RuntimeException {\n",
        "        return new RuntimeException('boom');\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        1,
        "Should detect return type of thrown method call. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(smart[0].label, "@throws RuntimeException");
}

/// `throw $this->createException()` with @return docblock type.
#[tokio::test]
async fn test_throws_method_return_type_from_docblock() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_rettype_doc.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Service {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function doWork(): void {\n",
        "        throw $this->makeException();\n",
        "    }\n",
        "\n",
        "    /**\n",
        "     * @return \\App\\Exceptions\\CustomException\n",
        "     */\n",
        "    private function makeException() {\n",
        "        return new \\App\\Exceptions\\CustomException('boom');\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        1,
        "Should detect @return docblock type. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(smart[0].label, "@throws CustomException");
}

/// `throw self::createException()` should also work.
#[tokio::test]
async fn test_throws_static_method_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_static_ret.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Service {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function doWork(): void {\n",
        "        throw self::createError();\n",
        "    }\n",
        "\n",
        "    private static function createError(): LogicException {\n",
        "        return new LogicException('oops');\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        1,
        "Should detect return type from self:: call. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(smart[0].label, "@throws LogicException");
}

/// Combined: direct throw + propagated + throw-expression all together.
#[tokio::test]
async fn test_throws_combined_all_sources() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_combined.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Service {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function process(): void {\n",
        "        throw new InvalidArgumentException('direct');\n",
        "        throw $this->makeError();\n",
        "        $this->riskyCall();\n",
        "    }\n",
        "\n",
        "    private function makeError(): LogicException {\n",
        "        return new LogicException('factory');\n",
        "    }\n",
        "\n",
        "    /**\n",
        "     * @throws RuntimeException\n",
        "     */\n",
        "    public function riskyCall(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        3,
        "Should have all three sources. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    let labels: Vec<&str> = smart.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"@throws InvalidArgumentException"));
    assert!(labels.contains(&"@throws LogicException"));
    assert!(labels.contains(&"@throws RuntimeException"));
}

// ─── Propagated throws inside try/catch ─────────────────────────────────────

/// When a called method's `@throws` exception is NOT caught by the surrounding
/// catch clause, the propagated throw should still appear as uncaught.
///
/// In this scenario `riskyOperation()` declares `@throws Exception`, but the
/// catch only handles `RuntimeException` — which is a *subclass* of Exception,
/// so it does NOT catch the broader `Exception`.
#[tokio::test]
async fn test_throws_propagated_not_caught_by_narrower_catch() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_prop_uncaught.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Demo;\n",
        "\n",
        "use RuntimeException;\n",
        "use Exception;\n",
        "\n",
        "class CatchVariableDemo\n",
        "{\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function singleCatch(): void\n",
        "    {\n",
        "        try {\n",
        "            $this->riskyOperation();\n",
        "            return;\n",
        "        } catch (RuntimeException $e) {\n",
        "        }\n",
        "    }\n",
        "\n",
        "    /** @throws Exception */\n",
        "    public function riskyOperation(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 9, 8).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        1,
        "Propagated Exception should not be considered caught by RuntimeException. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(smart[0].label, "@throws Exception");
}

/// When a called method's `@throws` exception IS caught by the surrounding
/// catch clause (exact match), the propagated throw should NOT appear.
#[tokio::test]
async fn test_throws_propagated_caught_by_exact_catch() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_prop_caught.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Service\n",
        "{\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function caller(): void\n",
        "    {\n",
        "        try {\n",
        "            $this->riskyOperation();\n",
        "        } catch (RuntimeException $e) {\n",
        "        }\n",
        "    }\n",
        "\n",
        "    /** @throws RuntimeException */\n",
        "    public function riskyOperation(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 8).await;
    let smart = throws_items(&items);

    assert!(
        smart.is_empty(),
        "Propagated RuntimeException should be caught by catch(RuntimeException). Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// When a called method's `@throws` exception IS caught by a broader catch
/// (e.g. `catch (Exception ...)` catches `RuntimeException`), the propagated
/// throw should NOT appear.
#[tokio::test]
async fn test_throws_propagated_caught_by_broader_catch() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_prop_broad.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Service\n",
        "{\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function caller(): void\n",
        "    {\n",
        "        try {\n",
        "            $this->riskyOperation();\n",
        "        } catch (Exception $e) {\n",
        "        }\n",
        "    }\n",
        "\n",
        "    /** @throws RuntimeException */\n",
        "    public function riskyOperation(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 8).await;
    let smart = throws_items(&items);

    assert!(
        smart.is_empty(),
        "Propagated RuntimeException should be caught by catch(Exception). Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// When a called method's `@throws` exception IS caught by `catch (Throwable)`,
/// the propagated throw should NOT appear.
#[tokio::test]
async fn test_throws_propagated_caught_by_throwable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_prop_throwable.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Service\n",
        "{\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function caller(): void\n",
        "    {\n",
        "        try {\n",
        "            $this->riskyOperation();\n",
        "        } catch (\\Throwable $e) {\n",
        "        }\n",
        "    }\n",
        "\n",
        "    /** @throws RuntimeException */\n",
        "    public function riskyOperation(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 8).await;
    let smart = throws_items(&items);

    assert!(
        smart.is_empty(),
        "Propagated RuntimeException should be caught by catch(Throwable). Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Mixed: one propagated throw caught, another not caught.
#[tokio::test]
async fn test_throws_propagated_partial_catch() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_prop_partial.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Service\n",
        "{\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function caller(): void\n",
        "    {\n",
        "        try {\n",
        "            $this->riskyA();\n",
        "            $this->riskyB();\n",
        "        } catch (RuntimeException $e) {\n",
        "        }\n",
        "    }\n",
        "\n",
        "    /** @throws RuntimeException */\n",
        "    public function riskyA(): void {}\n",
        "\n",
        "    /** @throws InvalidArgumentException */\n",
        "    public function riskyB(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 8).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        1,
        "Only InvalidArgumentException should remain uncaught. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(smart[0].label, "@throws InvalidArgumentException");
}

/// Propagated throws from a method called OUTSIDE a try block should always
/// appear as uncaught, regardless of any other try/catch in the function.
#[tokio::test]
async fn test_throws_propagated_outside_try_block() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throws_prop_outside.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Service\n",
        "{\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function caller(): void\n",
        "    {\n",
        "        $this->riskyA();\n",
        "        try {\n",
        "            $this->riskyB();\n",
        "        } catch (RuntimeException $e) {\n",
        "        }\n",
        "    }\n",
        "\n",
        "    /** @throws Exception */\n",
        "    public function riskyA(): void {}\n",
        "\n",
        "    /** @throws RuntimeException */\n",
        "    public function riskyB(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 8).await;
    let smart = throws_items(&items);

    assert_eq!(
        smart.len(),
        1,
        "riskyA() is outside try — Exception should be uncaught. riskyB() is caught. Got: {:?}",
        smart.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(smart[0].label, "@throws Exception");
}
