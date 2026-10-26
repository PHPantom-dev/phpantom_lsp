use crate::common::message_text;
use crate::common::{
    complete_at, create_psr4_workspace, method_names, unknown_member_diagnostics_with_scope_cache,
    workspace_uri,
};
use phpantom_lsp::Backend;

const COMPOSER_JSON: &str = r#"{"autoload-dev": {"psr-4": {"Tests\\": "tests/"}}}"#;

/// Pest's own functions, as newer releases declare them: the closure's
/// `$this` is documented as the `TestCall` the function returns.
const PEST_FUNCTIONS: &str = r#"<?php
use Pest\PendingCalls\TestCall;

/** @param-closure-this TestCall $closure */
function it(string $description, ?Closure $closure = null): TestCall { return new TestCall(); }

/** @param-closure-this TestCall $closure */
function test(?string $description = null, ?Closure $closure = null): TestCall { return new TestCall(); }
"#;

const TEST_CALL: &str = r#"<?php
namespace Pest\PendingCalls;

class TestCall {
    public function __call(string $name, array $arguments): self { return $this; }
}
"#;

const PHPUNIT_TEST_CASE: &str = r#"<?php
namespace PHPUnit\Framework;

abstract class TestCase {
    public function assertTrue(bool $condition): void {}
}
"#;

const BROWSER_TEST_CASE: &str = r#"<?php
namespace Tests;

use PHPUnit\Framework\TestCase;

abstract class BrowserTestCase extends TestCase {
    public function visitPage(string $url): Page { return new Page(); }
}
"#;

const FEATURE_TEST_CASE: &str = r#"<?php
namespace Tests;

use PHPUnit\Framework\TestCase;

abstract class FeatureTestCase extends TestCase {
    public function featureOnly(): void {}
}
"#;

const CREATES_PRODUCTS: &str = r#"<?php
namespace Tests\Concerns;

trait CreatesProducts {
    public function createProducts(int $count): \Tests\Page { return new \Tests\Page(); }
}
"#;

const PAGE: &str = r#"<?php
namespace Tests;

class Page {
    public function click(string $selector): void {}
}
"#;

const PEST_CONFIG: &str = r#"<?php

use Tests\BrowserTestCase;
use Tests\Concerns\CreatesProducts;
use Tests\FeatureTestCase;

pest()->extends(BrowserTestCase::class)
    ->in('Browser', 'Modules/*/Browser');

pest()->extends(FeatureTestCase::class)->use(CreatesProducts::class)
    ->in('Shop');

uses(FeatureTestCase::class)->in(__DIR__ . '/Feature');
"#;

/// A workspace with the Pest functions, the test cases, and a
/// `tests/Pest.php` binding them, all parsed.
fn pest_workspace() -> (Backend, tempfile::TempDir) {
    let files = [
        ("vendor/pest/Functions.php", PEST_FUNCTIONS),
        ("vendor/pest/TestCall.php", TEST_CALL),
        ("vendor/phpunit/TestCase.php", PHPUNIT_TEST_CASE),
        ("tests/BrowserTestCase.php", BROWSER_TEST_CASE),
        ("tests/FeatureTestCase.php", FEATURE_TEST_CASE),
        ("tests/Page.php", PAGE),
        ("tests/Concerns/CreatesProducts.php", CREATES_PRODUCTS),
        ("tests/Pest.php", PEST_CONFIG),
    ];
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, &files);
    for (path, content) in files {
        backend.update_ast(workspace_uri(&backend, path).as_str(), content);
    }
    (backend, dir)
}

fn unknown_members_in(backend: &Backend, path: &str, text: &str) -> Vec<String> {
    let uri = workspace_uri(backend, path);
    unknown_member_diagnostics_with_scope_cache(backend, uri.as_str(), text)
        .into_iter()
        .map(|d| message_text(&d.message).to_string())
        .collect()
}

/// `$this` in a test closure is the test case `tests/Pest.php` binds to
/// the file's directory, not the `TestCall` Pest's `it()` declares, so
/// the helpers it adds resolve and their return types flow on.
#[test]
fn this_is_the_test_case_bound_to_the_directory() {
    let (backend, _dir) = pest_workspace();
    let text = r#"<?php
it('opens the basket', function (): void {
    $page = $this->visitPage('/basket');
    $page->click('#checkout');
    $page->missing();
    $this->assertTrue(true);
});
"#;

    let messages = unknown_members_in(&backend, "tests/Browser/BasketTest.php", text);
    assert_eq!(
        messages.len(),
        1,
        "Only `missing()` on the page should be unknown, got: {messages:?}"
    );
    assert!(
        messages[0].contains("missing") && messages[0].contains("Tests\\Page"),
        "Expected `missing()` reported on Tests\\Page, got: {messages:?}"
    );
}

/// The older `uses(…)->in(…)` form binds the same way, and a target
/// written as `__DIR__ . '/…'` is the same directory as the relative one.
#[tokio::test]
async fn uses_with_a_dir_relative_target_binds_the_test_case() {
    let (backend, _dir) = pest_workspace();
    let uri = workspace_uri(&backend, "tests/Feature/CheckoutTest.php");
    let text = concat!(
        "<?php\n",
        "test('checks out', function () {\n",
        "    $this->\n",
        "});\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 11).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"featureOnly"),
        "Expected 'featureOnly' from the bound FeatureTestCase, got: {names:?}"
    );
    assert!(
        !names.contains(&"visitPage"),
        "BrowserTestCase is bound to another directory, got: {names:?}"
    );
}

/// A glob target binds every directory it matches.
#[test]
fn a_glob_target_binds_the_directories_it_matches() {
    let (backend, _dir) = pest_workspace();
    let text = r#"<?php
it('opens the shop', function (): void {
    $this->visitPage('/shop')->click('#buy');
    $this->visitPage('/shop')->missing();
});
"#;

    let messages = unknown_members_in(&backend, "tests/Modules/Shop/Browser/ShopTest.php", text);
    assert_eq!(
        messages.len(),
        1,
        "Only `missing()` on the page should be unknown, got: {messages:?}"
    );
    assert!(
        messages[0].contains("Tests\\Page"),
        "Expected the glob to bind BrowserTestCase, got: {messages:?}"
    );
}

/// A test file no binding covers runs as PHPUnit's base test case, which
/// is what Pest falls back to, rather than the `TestCall` its functions
/// declare.
#[test]
fn an_unbound_test_falls_back_to_the_phpunit_test_case() {
    let (backend, _dir) = pest_workspace();
    let text = r#"<?php
it('adds up', function (): void {
    $this->assertTrue(true);
    $this->visitPage('/');
});
"#;

    let messages = unknown_members_in(&backend, "tests/Unit/MathTest.php", text);
    assert_eq!(
        messages.len(),
        1,
        "Only `visitPage()` should be unknown, got: {messages:?}"
    );
    assert!(
        messages[0].contains("visitPage") && messages[0].contains("PHPUnit\\Framework\\TestCase"),
        "Expected `visitPage()` reported on the PHPUnit test case, got: {messages:?}"
    );
}

/// A trait bound alongside the test case is part of `$this` too, since
/// Pest's generated test class uses it.
#[test]
fn a_bound_trait_is_part_of_this() {
    let (backend, _dir) = pest_workspace();
    let text = r#"<?php
it('lists products', function (): void {
    $this->createProducts(5)->click('#first');
    $this->featureOnly();
    $this->missing();
});
"#;

    let messages = unknown_members_in(&backend, "tests/Shop/ListTest.php", text);
    assert_eq!(
        messages.len(),
        1,
        "Only `missing()` should be unknown, got: {messages:?}"
    );
    assert!(
        messages[0].contains("missing"),
        "Expected `missing()` to be reported, got: {messages:?}"
    );
}

/// A `uses()` in the test file itself, without `->in()`, binds that file.
#[test]
fn uses_in_the_test_file_binds_the_file() {
    let (backend, _dir) = pest_workspace();
    let text = r#"<?php
use Tests\Concerns\CreatesProducts;
use Tests\FeatureTestCase;

uses(FeatureTestCase::class, CreatesProducts::class);

it('gives a gift', function (): void {
    $this->createProducts(5)->click('#gift');
    $this->featureOnly();
    $this->visitPage('/');
});
"#;

    let uri = workspace_uri(&backend, "tests/Unit/GiftTest.php");
    backend.update_ast(uri.as_str(), text);
    let messages = unknown_members_in(&backend, "tests/Unit/GiftTest.php", text);
    assert_eq!(
        messages.len(),
        1,
        "Only `visitPage()` should be unknown, got: {messages:?}"
    );
    assert!(
        messages[0].contains("visitPage"),
        "Expected `visitPage()` to be reported, got: {messages:?}"
    );
}

/// `pest()->extends()->use()` in the test file binds it the same way.
#[tokio::test]
async fn pest_extends_in_the_test_file_binds_the_file() {
    let (backend, _dir) = pest_workspace();
    let uri = workspace_uri(&backend, "tests/Unit/BasketTest.php");
    let text = concat!(
        "<?php\n",
        "pest()->extends(Tests\\BrowserTestCase::class)->use(Tests\\Concerns\\CreatesProducts::class);\n",
        "test('fills the basket', function () {\n",
        "    $this->\n",
        "});\n",
    );
    backend.update_ast(uri.as_str(), text);

    let items = complete_at(&backend, &uri, text, 3, 11).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"visitPage") && names.contains(&"createProducts"),
        "Expected members of BrowserTestCase and CreatesProducts, got: {names:?}"
    );
}

/// The `Pest.php` of a test directory other than `tests/` binds the tests
/// under it.
#[test]
fn a_test_directory_other_than_tests_is_read() {
    let files = [
        ("vendor/pest/Functions.php", PEST_FUNCTIONS),
        ("vendor/pest/TestCall.php", TEST_CALL),
        ("vendor/phpunit/TestCase.php", PHPUNIT_TEST_CASE),
        ("tests/BrowserTestCase.php", BROWSER_TEST_CASE),
        ("tests/Page.php", PAGE),
        (
            "spec/Pest.php",
            "<?php\npest()->extends(Tests\\BrowserTestCase::class)->in('Browser');\n",
        ),
    ];
    let (backend, _dir) = create_psr4_workspace(COMPOSER_JSON, &files);
    for (path, content) in files {
        backend.update_ast(workspace_uri(&backend, path).as_str(), content);
    }
    let text = r#"<?php
it('opens the basket', function (): void {
    $this->visitPage('/basket')->missing();
});
"#;

    let messages = unknown_members_in(&backend, "spec/Browser/BasketTest.php", text);
    assert_eq!(
        messages.len(),
        1,
        "Only `missing()` on the page should be unknown, got: {messages:?}"
    );
    assert!(
        messages[0].contains("Tests\\Page"),
        "Expected spec/Pest.php to bind BrowserTestCase, got: {messages:?}"
    );
}

/// A `Pest.php` written outside the editor, as `pest --init` or a branch
/// switch does, binds the tests under it from the next lookup on, even
/// after a lookup found the directory without one.
#[tokio::test]
async fn a_pest_php_created_on_disk_is_read() {
    use tower_lsp_server::LanguageServer;
    use tower_lsp_server::gen_lsp_types::{
        DidChangeWatchedFilesParams, FileChangeType, FileEvent, Uri,
    };

    let files = [
        ("vendor/pest/Functions.php", PEST_FUNCTIONS),
        ("vendor/pest/TestCall.php", TEST_CALL),
        ("vendor/phpunit/TestCase.php", PHPUNIT_TEST_CASE),
        ("tests/BrowserTestCase.php", BROWSER_TEST_CASE),
        ("tests/Page.php", PAGE),
    ];
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, &files);
    for (path, content) in files {
        backend.update_ast(workspace_uri(&backend, path).as_str(), content);
    }
    let text = r#"<?php
it('opens the basket', function (): void {
    $this->visitPage('/basket')->missing();
});
"#;

    let before = unknown_members_in(&backend, "tests/Browser/BasketTest.php", text);
    assert!(
        before.iter().any(|m| m.contains("visitPage")),
        "Without a Pest.php `visitPage()` should be unknown, got: {before:?}"
    );

    let pest_path = dir.path().join("tests/Pest.php");
    std::fs::write(
        &pest_path,
        "<?php\npest()->extends(Tests\\BrowserTestCase::class)->in('Browser');\n",
    )
    .unwrap();
    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: Uri::from_file_path(&pest_path).unwrap(),
                kind: FileChangeType::Created,
            }],
        })
        .await;

    let after = unknown_members_in(&backend, "tests/Browser/BasketTest.php", text);
    assert_eq!(
        after.len(),
        1,
        "Only `missing()` on the page should be unknown, got: {after:?}"
    );
    assert!(
        after[0].contains("Tests\\Page"),
        "Expected the new Pest.php to bind BrowserTestCase, got: {after:?}"
    );
}
