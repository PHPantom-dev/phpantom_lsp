//! Integration tests for smart `catch (…)` clause completion.
//!
//! These tests verify two key behaviours:
//!
//! 1. When the try block contains discoverable thrown types (`throw new`,
//!    `@throws` annotations, propagated `@throws` from called methods),
//!    the catch clause suggests those types **plus `Throwable`** as a
//!    catch-all safety net.
//!
//! 2. When no specific thrown types are discovered, the fallback suggests
//!    only classes that are confirmed or potentially Throwable descendants
//!    — already-parsed classes whose parent chain does NOT reach
//!    `\Throwable` / `\Exception` / `\Error` are filtered out.

use crate::common::{complete_at, create_test_backend};
use tower_lsp::lsp_types::*;

/// Extract catch-suggestion items (detail = "Exception thrown in try block").
fn catch_items(items: &[CompletionItem]) -> Vec<&CompletionItem> {
    items
        .iter()
        .filter(|i| i.detail.as_deref() == Some("Exception thrown in try block"))
        .collect()
}

/// Find an item by label.
fn find_item<'a>(items: &'a [CompletionItem], label: &str) -> Option<&'a CompletionItem> {
    items.iter().find(|i| i.label == label)
}

// ─── Throwable injection with specific throws ───────────────────────────────

/// When a single `throw new` is in the try block, both that type and
/// `Throwable` should be suggested.
#[tokio::test]
async fn test_catch_specific_throw_includes_throwable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///catch_throwable.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class ValidationException extends \\RuntimeException {}\n",
        "class CatchDemo {\n",
        "    public function singleCatch(): void {\n",
        "        try {\n",
        "            throw new ValidationException('bad');\n",
        "        } catch (\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    // cursor at line 6 (0-based), character 18 (inside the catch paren)
    let items = complete_at(&backend, &uri, text, 6, 18).await;
    let smart = catch_items(&items);

    let labels: Vec<&str> = smart.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.contains(&"ValidationException"),
        "Should suggest ValidationException, got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"Throwable"),
        "Should always include Throwable when specific types found, got: {:?}",
        labels
    );
}

/// When `@throws` is propagated from a called method, both the declared
/// type and `Throwable` should be suggested.
#[tokio::test]
async fn test_catch_propagated_throws_includes_throwable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///catch_propagated.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class ApiException extends \\RuntimeException {}\n",
        "class ApiClient {\n",
        "    /**\n",
        "     * @throws ApiException\n",
        "     */\n",
        "    private function callApi(): void {}\n",
        "    public function run(): void {\n",
        "        try {\n",
        "            $this->callApi();\n",
        "        } catch (\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 10, 18).await;
    let smart = catch_items(&items);
    let labels: Vec<&str> = smart.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.contains(&"ApiException"),
        "Should suggest ApiException from @throws, got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"Throwable"),
        "Should always include Throwable, got: {:?}",
        labels
    );
}

/// With an inline `/** @throws */` annotation, the annotated type and
/// `Throwable` should both be suggested.
#[tokio::test]
async fn test_catch_inline_throws_includes_throwable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///catch_inline.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class CatchInlineDemo {\n",
        "    public function demo(): void {\n",
        "        try {\n",
        "            /** @throws \\InvalidArgumentException */\n",
        "            doSomething();\n",
        "        } catch (\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 6, 18).await;
    let smart = catch_items(&items);
    let labels: Vec<&str> = smart.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.contains(&"InvalidArgumentException"),
        "Should suggest InvalidArgumentException from inline @throws, got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"Throwable"),
        "Should always include Throwable, got: {:?}",
        labels
    );
}

/// With multiple thrown types, all of them plus `Throwable` should be suggested.
#[tokio::test]
async fn test_catch_multiple_throws_all_plus_throwable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///catch_multi.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class CatchMultiDemo {\n",
        "    public function demo(): void {\n",
        "        try {\n",
        "            throw new \\RuntimeException('rt');\n",
        "            throw new \\InvalidArgumentException('ia');\n",
        "        } catch (\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 6, 18).await;
    let smart = catch_items(&items);
    let labels: Vec<&str> = smart.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.contains(&"RuntimeException"),
        "Should suggest RuntimeException, got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"InvalidArgumentException"),
        "Should suggest InvalidArgumentException, got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"Throwable"),
        "Should always include Throwable, got: {:?}",
        labels
    );
}

/// `Throwable` should sort AFTER specific exception types.
#[tokio::test]
async fn test_catch_throwable_sorts_after_specific() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///catch_sort.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class CatchSortDemo {\n",
        "    public function demo(): void {\n",
        "        try {\n",
        "            throw new \\RuntimeException('rt');\n",
        "        } catch (\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 5, 18).await;
    let smart = catch_items(&items);

    let rt = smart.iter().find(|i| i.label == "RuntimeException");
    let th = smart.iter().find(|i| i.label == "Throwable");

    assert!(rt.is_some(), "Should have RuntimeException");
    assert!(th.is_some(), "Should have Throwable");

    // Throwable's sort_text starts with "1_" while specific types start with "0_"
    let rt_sort = rt.unwrap().sort_text.as_deref().unwrap_or("");
    let th_sort = th.unwrap().sort_text.as_deref().unwrap_or("");
    assert!(
        th_sort > rt_sort,
        "Throwable should sort after specific types: Throwable='{}', RuntimeException='{}'",
        th_sort,
        rt_sort
    );
}

// ─── Throwable-filtered fallback when no throws are found ───────────────────

/// When no specific throws are found, non-exception parsed classes
/// should be filtered out. Only Throwable descendants remain from
/// the parsed sources.
#[tokio::test]
async fn test_catch_fallback_filters_non_exception_classes() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///catch_filter.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class ValidationException extends \\RuntimeException {}\n",
        "class UserService {}\n",
        "class OrderRepository {}\n",
        "class CatchFilterDemo {\n",
        "    public function demo(): void {\n",
        "        try {\n",
        "            $this->doWork();\n",
        "        } catch (\n",
        "        }\n",
        "    }\n",
        "    private function doWork(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 8, 18).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    // ValidationException extends RuntimeException which should be detected
    // as a Throwable descendant (if RuntimeException is loaded as a stub, the
    // parent chain check may return None — include by benefit of the doubt).
    // UserService and OrderRepository have no parent → confirmed NOT Throwable.
    assert!(
        !labels.contains(&"UserService"),
        "Non-exception class UserService should be filtered out, got: {:?}",
        labels
    );
    assert!(
        !labels.contains(&"OrderRepository"),
        "Non-exception class OrderRepository should be filtered out, got: {:?}",
        labels
    );
    assert!(
        !labels.contains(&"CatchFilterDemo"),
        "Non-exception class CatchFilterDemo should be filtered out, got: {:?}",
        labels
    );
}

/// Throwable itself should still appear in the fallback suggestions.
#[tokio::test]
async fn test_catch_fallback_includes_throwable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///catch_fallback_th.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class FallbackDemo {\n",
        "    public function demo(): void {\n",
        "        try {\n",
        "            $this->doWork();\n",
        "        } catch (\n",
        "        }\n",
        "    }\n",
        "    private function doWork(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 5, 18).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    // Throwable should appear either from the catch_completions (as a
    // suggestion) or from the class completion fallback
    assert!(
        labels.contains(&"Throwable"),
        "Throwable should be in fallback suggestions, got: {:?}",
        labels
    );
}

/// An exception class defined in the same file with a known parent chain
/// should appear in fallback completion.
#[tokio::test]
async fn test_catch_fallback_includes_known_exception() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///catch_known_exc.php").unwrap();
    // Define a class that clearly extends Exception — the parent chain
    // terminates at Exception which is a known Throwable root.
    let text = concat!(
        "<?php\n",
        "class AppException extends \\Exception {}\n",
        "class PlainHelper {}\n",
        "class ExcDemo {\n",
        "    public function demo(): void {\n",
        "        try {\n",
        "            $this->work();\n",
        "        } catch (\n",
        "        }\n",
        "    }\n",
        "    private function work(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 7, 18).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.contains(&"AppException"),
        "Exception subclass should be included, got: {:?}",
        labels
    );
    assert!(
        !labels.contains(&"PlainHelper"),
        "Non-exception PlainHelper should be filtered out, got: {:?}",
        labels
    );
    assert!(
        !labels.contains(&"ExcDemo"),
        "Non-exception ExcDemo should be filtered out, got: {:?}",
        labels
    );
}

/// Classes extending \\Error should also be included as Throwable descendants.
#[tokio::test]
async fn test_catch_fallback_includes_error_descendants() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///catch_error_desc.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class CustomError extends \\Error {}\n",
        "class RegularClass {}\n",
        "class ErrorDemo {\n",
        "    public function demo(): void {\n",
        "        try {\n",
        "            $this->work();\n",
        "        } catch (\n",
        "        }\n",
        "    }\n",
        "    private function work(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 7, 18).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.contains(&"CustomError"),
        "Error subclass should be included, got: {:?}",
        labels
    );
    assert!(
        !labels.contains(&"RegularClass"),
        "Non-exception RegularClass should be filtered out, got: {:?}",
        labels
    );
}

/// Partial typing should filter fallback suggestions by prefix.
#[tokio::test]
async fn test_catch_fallback_partial_filtering() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///catch_partial.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class AppException extends \\Exception {}\n",
        "class ApiError extends \\Error {}\n",
        "class UserModel {}\n",
        "class Demo {\n",
        "    public function demo(): void {\n",
        "        try {\n",
        "            $this->work();\n",
        "        } catch (App\n",
        "        }\n",
        "    }\n",
        "    private function work(): void {}\n",
        "}\n",
    );

    // Cursor after "App" — should filter to AppException
    let items = complete_at(&backend, &uri, text, 8, 21).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.contains(&"AppException"),
        "AppException should match prefix 'App', got: {:?}",
        labels
    );
    // ApiError shouldn't match because it doesn't contain "App"
    assert!(
        !labels.contains(&"ApiError"),
        "ApiError should not match prefix 'App', got: {:?}",
        labels
    );
    assert!(
        !labels.contains(&"UserModel"),
        "Non-exception UserModel should be filtered out, got: {:?}",
        labels
    );
}

/// When a class's parent isn't loaded (e.g. extends a class from an
/// unloaded library), `must_extend` cannot confirm Throwable ancestry
/// so the class is excluded from the loaded section.  Since it IS loaded,
/// it's also excluded from classmap/stubs via `filter_out_loaded`.
/// Neither the unknown-parent class nor PlainClass should appear.
#[tokio::test]
async fn test_catch_fallback_excludes_unknown_parent() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///catch_unknown.php").unwrap();
    let text = concat!(
        "<?php\n",
        // This extends a class we don't have in our ast_map
        "class StripeException extends \\Stripe\\ApiException {}\n",
        "class PlainClass {}\n",
        "class Demo {\n",
        "    public function demo(): void {\n",
        "        try {\n",
        "            $this->work();\n",
        "        } catch (\n",
        "        }\n",
        "    }\n",
        "    private function work(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 7, 18).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    // StripeException's parent (Stripe\ApiException) isn't loaded, so
    // is_throwable_descendant returns false — excluded from loaded section.
    // Since it IS loaded, it's also filtered out of classmap/stubs.
    assert!(
        !labels.contains(&"StripeException"),
        "Class with unresolvable parent chain should be excluded, got: {:?}",
        labels
    );
    assert!(
        !labels.contains(&"PlainClass"),
        "Plain class with no parent should be filtered, got: {:?}",
        labels
    );
}

/// Chained exception hierarchy: A extends B extends \\Exception should
/// be detected as a Throwable descendant.
#[tokio::test]
async fn test_catch_fallback_chained_inheritance() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///catch_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class DomainException extends \\Exception {}\n",
        "class OrderException extends DomainException {}\n",
        "class NotAnException {}\n",
        "class Demo {\n",
        "    public function demo(): void {\n",
        "        try {\n",
        "            $this->work();\n",
        "        } catch (\n",
        "        }\n",
        "    }\n",
        "    private function work(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 8, 18).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.contains(&"DomainException"),
        "DomainException (extends Exception) should be included, got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"OrderException"),
        "OrderException (extends DomainException extends Exception) should be included, got: {:?}",
        labels
    );
    assert!(
        !labels.contains(&"NotAnException"),
        "NotAnException should be filtered out, got: {:?}",
        labels
    );
}

/// When specific throws are found, only the smart suggestions should be
/// returned (not the full class list).
#[tokio::test]
async fn test_catch_with_throws_returns_only_smart_items() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///catch_smart_only.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class SomeService {}\n",
        "class SomeException extends \\Exception {}\n",
        "class Demo {\n",
        "    public function demo(): void {\n",
        "        try {\n",
        "            throw new SomeException('oops');\n",
        "        } catch (\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 7, 18).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    // Should have smart items only: SomeException + Throwable
    assert!(
        labels.contains(&"SomeException"),
        "Should suggest SomeException, got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"Throwable"),
        "Should include Throwable, got: {:?}",
        labels
    );
    // SomeService should NOT appear — smart mode doesn't include the
    // full class list
    assert!(
        !labels.contains(&"SomeService"),
        "Non-exception class should not appear in smart mode, got: {:?}",
        labels
    );
}

/// Verify that the `Throwable` item is NOT injected somewhere OTHER than
/// catch clause completion — e.g. normal class name completion should NOT
/// get an extra "Exception thrown in try block" Throwable item.
#[tokio::test]
async fn test_throwable_not_injected_outside_catch() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///no_catch.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class MyClass {\n",
        "    public function demo(): void {\n",
        "        $x = new Thr\n",
        "    }\n",
        "}\n",
    );

    // cursor after "Thr" in `$x = new Thr`
    let items = complete_at(&backend, &uri, text, 3, 20).await;
    let catch_special = catch_items(&items);

    assert!(
        catch_special.is_empty(),
        "No 'Exception thrown in try block' items should appear outside catch, got: {:?}",
        catch_special.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Test catch with `@throws` on a method with visibility modifier.
/// This is a regression test — visibility modifiers used to prevent
/// the docblock from being found.
#[tokio::test]
async fn test_catch_throws_on_private_method() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///catch_private.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class ServiceException extends \\Exception {}\n",
        "class Service {\n",
        "    /**\n",
        "     * @throws ServiceException\n",
        "     */\n",
        "    private function riskyOp(): void {}\n",
        "    public function run(): void {\n",
        "        try {\n",
        "            $this->riskyOp();\n",
        "        } catch (\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 10, 18).await;
    let smart = catch_items(&items);
    let labels: Vec<&str> = smart.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.contains(&"ServiceException"),
        "Should find @throws on private method, got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"Throwable"),
        "Should include Throwable, got: {:?}",
        labels
    );
}

/// In the fallback path, the catch-specific `Throwable` item (from
/// detect_catch_context) should appear alongside class completion items.
#[tokio::test]
async fn test_catch_fallback_throwable_present_with_class_items() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///catch_fallback_mix.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class AppException extends \\Exception {}\n",
        "class NormalClass {}\n",
        "class Demo {\n",
        "    public function demo(): void {\n",
        "        try {\n",
        "            $this->work();\n",
        "        } catch (\n",
        "        }\n",
        "    }\n",
        "    private function work(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 7, 18).await;

    // The Throwable item from detect_catch_context should be present
    let throwable = find_item(&items, "Throwable");
    assert!(
        throwable.is_some(),
        "Throwable should be in fallback results, got labels: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );

    // AppException (extends Exception) should be present
    let app_exc = find_item(&items, "AppException");
    assert!(
        app_exc.is_some(),
        "AppException should be in fallback results"
    );

    // NormalClass (no parent) should be filtered out
    let normal = find_item(&items, "NormalClass");
    assert!(
        normal.is_none(),
        "NormalClass should be filtered out of catch fallback"
    );
}

/// When second catch clause is used, already-caught types should be
/// excluded but Throwable should still be offered.
#[tokio::test]
async fn test_catch_second_clause_still_has_throwable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///catch_second.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class ExcA extends \\Exception {}\n",
        "class ExcB extends \\Exception {}\n",
        "class Demo {\n",
        "    public function demo(): void {\n",
        "        try {\n",
        "            throw new ExcA('a');\n",
        "            throw new ExcB('b');\n",
        "        } catch (ExcA $e) {\n",
        "            // handled\n",
        "        } catch (\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 10, 18).await;
    let smart = catch_items(&items);
    let labels: Vec<&str> = smart.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.contains(&"ExcB"),
        "ExcB should be suggested in second catch, got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"Throwable"),
        "Throwable should be in second catch, got: {:?}",
        labels
    );
}

/// When `use Throwable;` is imported, the catch fallback should NOT
/// produce two Throwable entries — the catch-context Throwable (with
/// detail "Exception thrown in try block") takes priority and the
/// use-imported duplicate is suppressed.
#[tokio::test]
async fn test_catch_fallback_no_duplicate_throwable_with_use_import() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///catch_dedup.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Demo;\n",
        "use Throwable;\n",
        "class CatchDedup {\n",
        "    public function demo(): void {\n",
        "        try {\n",
        "            $this->work();\n",
        "        } catch (T\n",
        "        }\n",
        "    }\n",
        "    private function work(): void {}\n",
        "}\n",
    );

    // Cursor after "T" in `catch (T`
    let items = complete_at(&backend, &uri, text, 7, 19).await;
    let throwable_items: Vec<&CompletionItem> =
        items.iter().filter(|i| i.label == "Throwable").collect();

    assert_eq!(
        throwable_items.len(),
        1,
        "Throwable should appear exactly once, got {} entries: {:?}",
        throwable_items.len(),
        throwable_items
            .iter()
            .map(|i| (&i.label, &i.detail))
            .collect::<Vec<_>>()
    );

    // The surviving entry should be the catch-context one
    assert_eq!(
        throwable_items[0].detail.as_deref(),
        Some("Exception thrown in try block"),
        "The Throwable entry should have the catch-context detail"
    );
}

// ─── `throw new` context completion ─────────────────────────────────────────

/// `throw new` should only suggest Throwable descendants and filter out
/// non-exception classes, constants, and functions.
#[tokio::test]
async fn test_throw_new_filters_non_exception_classes() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throw_new_filter.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class NormalService {}\n",
        "class AppException extends \\Exception {}\n",
        "class AppError extends \\Error {}\n",
        "class Demo {\n",
        "    public function demo(): void {\n",
        "        throw new App\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 6, 22).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.contains(&"AppException"),
        "Should suggest AppException (extends Exception), got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"AppError"),
        "Should suggest AppError (extends Error), got: {:?}",
        labels
    );
    assert!(
        !labels.contains(&"NormalService"),
        "Should NOT suggest NormalService (not a Throwable), got: {:?}",
        labels
    );
}

/// `throw new` should not suggest constants or functions.
#[tokio::test]
async fn test_throw_new_no_constants_or_functions() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throw_new_no_const.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class AppException extends \\Exception {}\n",
        "class Demo {\n",
        "    public function demo(): void {\n",
        "        throw new App\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 22).await;

    // Should only have class items (kind = CLASS), no constants or functions
    for item in &items {
        assert_eq!(
            item.kind,
            Some(CompletionItemKind::CLASS),
            "throw new should only suggest classes, got {:?} for '{}'",
            item.kind,
            item.label
        );
    }

    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"AppException"),
        "Should suggest AppException, got: {:?}",
        labels
    );
}

/// `$x = new` should still show all classes (not just Throwable descendants).
/// This verifies the `throw new` detection doesn't trigger for plain `new`.
#[tokio::test]
async fn test_regular_new_still_shows_all_classes() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///regular_new.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class NormalService {}\n",
        "class AppException extends \\Exception {}\n",
        "class Demo {\n",
        "    public function demo(): void {\n",
        "        $x = new Nor\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 5, 20).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.contains(&"NormalService"),
        "`$x = new` should still suggest non-exception classes, got: {:?}",
        labels
    );
}

/// `throw new` with use-imported exception should suggest it.
#[tokio::test]
async fn test_throw_new_with_use_import() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throw_new_use.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class MyException extends \\Exception {}\n",
        "class NotAnException {}\n",
        "\n",
        "class Demo {\n",
        "    public function demo(): void {\n",
        "        throw new My\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 8, 21).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.contains(&"MyException"),
        "Should suggest MyException, got: {:?}",
        labels
    );
    assert!(
        !labels.contains(&"NotAnException"),
        "Should NOT suggest NotAnException, got: {:?}",
        labels
    );
}

/// `throw new` with extra whitespace between `throw` and `new` should work.
#[tokio::test]
async fn test_throw_new_extra_whitespace() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throw_new_ws.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class CustomError extends \\Error {}\n",
        "class PlainClass {}\n",
        "class Demo {\n",
        "    public function demo(): void {\n",
        "        throw  new  Cus\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 5, 24).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.contains(&"CustomError"),
        "Should handle extra whitespace between throw/new, got: {:?}",
        labels
    );
    assert!(
        !labels.contains(&"PlainClass"),
        "Should filter out non-Throwable, got: {:?}",
        labels
    );
}

// ─── Catch allows Throwable interfaces ──────────────────────────────────────

/// `catch (\Throwable $e)` is idiomatic PHP — the Throwable interface
/// itself should appear in catch fallback completions.
#[tokio::test]
async fn test_catch_fallback_includes_throwable_interface() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///catch_iface.php").unwrap();
    // Define a custom interface that extends \Throwable and a class
    // that implements it.  Both should appear in catch completions.
    let text = concat!(
        "<?php\n",
        "interface AppException extends \\Throwable {}\n",
        "class ConcreteAppException extends \\Exception implements AppException {}\n",
        "class NotAnException {}\n",
        "class CatchIfaceDemo {\n",
        "    public function demo(): void {\n",
        "        try {\n",
        "            $this->doWork();\n",
        "        } catch (\n",
        "        }\n",
        "    }\n",
        "    private function doWork(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 8, 18).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    // The Throwable interface itself should be offered.
    assert!(
        labels.contains(&"AppException"),
        "Throwable-extending interface should appear in catch, got: {:?}",
        labels
    );
    // Concrete exception class should also be offered.
    assert!(
        labels.contains(&"ConcreteAppException"),
        "Concrete exception class should appear in catch, got: {:?}",
        labels
    );
    // Non-exception class should be filtered out.
    assert!(
        !labels.contains(&"NotAnException"),
        "Non-Throwable class should be filtered out of catch, got: {:?}",
        labels
    );
}

/// Abstract exception classes should appear in catch completions.
/// `catch (AbstractBaseException $e)` is valid PHP.
#[tokio::test]
async fn test_catch_fallback_includes_abstract_exception() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///catch_abstract.php").unwrap();
    let text = concat!(
        "<?php\n",
        "abstract class AbstractBaseException extends \\Exception {}\n",
        "class ConcreteChildException extends AbstractBaseException {}\n",
        "class CatchAbstractDemo {\n",
        "    public function demo(): void {\n",
        "        try {\n",
        "            $this->doWork();\n",
        "        } catch (\n",
        "        }\n",
        "    }\n",
        "    private function doWork(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 7, 18).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.contains(&"AbstractBaseException"),
        "Abstract exception class should appear in catch, got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"ConcreteChildException"),
        "Concrete child exception should appear in catch, got: {:?}",
        labels
    );
}

/// An interface that extends another interface that extends \Throwable
/// should still be recognized as a Throwable descendant through the
/// interface chain.
#[tokio::test]
async fn test_catch_fallback_interface_chain() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///catch_iface_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface BaseAppException extends \\Throwable {}\n",
        "interface SpecificAppException extends BaseAppException {}\n",
        "class IfaceChainDemo {\n",
        "    public function demo(): void {\n",
        "        try {\n",
        "            $this->doWork();\n",
        "        } catch (\n",
        "        }\n",
        "    }\n",
        "    private function doWork(): void {}\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 7, 18).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.contains(&"BaseAppException"),
        "Direct Throwable-extending interface should appear, got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"SpecificAppException"),
        "Transitive Throwable-extending interface should appear, got: {:?}",
        labels
    );
}
