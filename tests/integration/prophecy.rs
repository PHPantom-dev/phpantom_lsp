//! Prophecy test doubles: `$this->prophesize(Foo::class)` gives an
//! `ObjectProphecy<Foo>` whose `reveal()` is the double and whose calls
//! named after `Foo`'s methods set up a `MethodProphecy`.

use crate::common::{
    complete_labels_at_opened, create_psr4_workspace, definition_uri, goto_definition_at, hover_on,
    line_char_of, message_text, open_php,
};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::Diagnostic;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "": "stubs/"
        }
    }
}"#;

// Abridged from phpspec/prophecy 1.x.
const PROPHECY_INTERFACE_PHP: &str = r#"<?php
namespace Prophecy\Prophecy;
/**
 * @template-covariant T of object
 */
interface ProphecyInterface
{
    /**
     * @return T
     */
    public function reveal();
}
"#;

const OBJECT_PROPHECY_PHP: &str = r#"<?php
namespace Prophecy\Prophecy;
/**
 * @template-covariant T of object
 * @template-implements ProphecyInterface<T>
 */
class ObjectProphecy implements ProphecyInterface
{
    /**
     * @param class-string|null $class
     * @return $this
     */
    public function willExtend($class) { return $this; }

    /**
     * @return T
     */
    public function reveal() {}

    /**
     * @param string $methodName
     * @param array $arguments
     * @return MethodProphecy
     */
    public function __call($methodName, array $arguments) {}

    /**
     * @param string $name
     * @return static
     */
    public function __get($name) { return $this; }
}
"#;

const METHOD_PROPHECY_PHP: &str = r#"<?php
namespace Prophecy\Prophecy;
class MethodProphecy
{
    /**
     * @param mixed ...$returns
     * @return $this
     */
    public function willReturn(...$returns) { return $this; }

    /**
     * @return $this
     */
    public function shouldBeCalled() { return $this; }

    /**
     * @return ObjectProphecy
     */
    public function getObjectProphecy() {}
}
"#;

const ARGUMENT_PHP: &str = r#"<?php
namespace Prophecy;
class Argument
{
    /**
     * @return Argument\Token\AnyValueToken
     */
    public static function any() {}
}
"#;

const TOKEN_INTERFACE_PHP: &str = r#"<?php
namespace Prophecy\Argument\Token;
interface TokenInterface {}
"#;

const ANY_VALUE_TOKEN_PHP: &str = r#"<?php
namespace Prophecy\Argument\Token;
class AnyValueToken implements TokenInterface {}
"#;

// Abridged from phpspec/prophecy-phpunit 2.x.
const PROPHECY_TRAIT_PHP: &str = r#"<?php
namespace Prophecy\PhpUnit;
use Prophecy\Prophecy\ObjectProphecy;
trait ProphecyTrait
{
    /**
     * @template T of object
     * @phpstan-param class-string<T>|null $classOrInterface
     * @phpstan-return ($classOrInterface is null ? ObjectProphecy<object> : ObjectProphecy<T>)
     */
    protected function prophesize(?string $classOrInterface = null): ObjectProphecy {}
}
"#;

const TEST_CASE_PHP: &str = r#"<?php
namespace PHPUnit\Framework;
abstract class TestCase {}
"#;

const ORDER_REPOSITORY_PHP: &str = r#"<?php
namespace App;
class OrderRepository
{
    public function findOpen(): array { return []; }
    public function find(int $id): ?object { return null; }
    public static function make(): self { return new self(); }
}
"#;

const BILLING_PHP: &str = r#"<?php
namespace App;
class Billing
{
    public function __construct(OrderRepository $orders) {}
}
"#;

fn workspace() -> (Backend, tempfile::TempDir) {
    create_psr4_workspace(
        COMPOSER_JSON,
        &[
            (
                "stubs/Prophecy/Prophecy/ProphecyInterface.php",
                PROPHECY_INTERFACE_PHP,
            ),
            (
                "stubs/Prophecy/Prophecy/ObjectProphecy.php",
                OBJECT_PROPHECY_PHP,
            ),
            (
                "stubs/Prophecy/Prophecy/MethodProphecy.php",
                METHOD_PROPHECY_PHP,
            ),
            ("stubs/Prophecy/Argument.php", ARGUMENT_PHP),
            (
                "stubs/Prophecy/Argument/Token/TokenInterface.php",
                TOKEN_INTERFACE_PHP,
            ),
            (
                "stubs/Prophecy/Argument/Token/AnyValueToken.php",
                ANY_VALUE_TOKEN_PHP,
            ),
            (
                "stubs/Prophecy/PhpUnit/ProphecyTrait.php",
                PROPHECY_TRAIT_PHP,
            ),
            ("stubs/PHPUnit/Framework/TestCase.php", TEST_CASE_PHP),
            ("src/OrderRepository.php", ORDER_REPOSITORY_PHP),
            ("src/Billing.php", BILLING_PHP),
        ],
    )
}

fn test_source(body: &str) -> String {
    format!(
        r#"<?php
use App\Billing;
use App\OrderRepository;
use PHPUnit\Framework\TestCase;
use Prophecy\Argument;
use Prophecy\PhpUnit\ProphecyTrait;

final class BillingTest extends TestCase
{{
    use ProphecyTrait;

    public function testIt(): void
    {{
        $repo = $this->prophesize(OrderRepository::class);
{body}
    }}

    /** @var \Prophecy\Prophecy\ObjectProphecy<OrderRepository> */
    private $orders;

    public function testProperty(): void
    {{
        $this->orders->
    }}
}}
"#
    )
}

async fn open_test(body: &str) -> (Backend, tempfile::TempDir, Uri, String) {
    let (backend, dir) = workspace();
    let source = test_source(body);
    let uri = Uri::from_file_path(dir.path().join("tests/BillingTest.php")).unwrap();
    open_php(&backend, &uri, &source).await;
    (backend, dir, uri, source)
}

/// The hover on the variable `$name` where `needle` assigns it.
async fn hover_type(body: &str, needle: &str) -> String {
    let (backend, _dir, uri, source) = open_test(body).await;
    hover_on(&backend, &uri, &source, needle).await
}

#[tokio::test]
async fn reveal_gives_the_mocked_class() {
    let hover = hover_type("        $double = $repo->reveal();", "$double").await;
    assert!(hover.contains("OrderRepository"), "{hover}");
}

#[tokio::test]
async fn a_mocked_method_call_gives_a_method_prophecy() {
    let hover = hover_type("        $expectation = $repo->findOpen();", "$expectation").await;
    assert!(hover.contains("MethodProphecy"), "{hover}");
}

#[tokio::test]
async fn an_expectation_chain_stays_a_method_prophecy() {
    let hover = hover_type(
        "        $expectation = $repo->find(Argument::any())->willReturn(null)->shouldBeCalled();",
        "$expectation",
    )
    .await;
    assert!(hover.contains("MethodProphecy"), "{hover}");
}

#[tokio::test]
async fn completion_offers_the_mocked_methods() {
    let (backend, _dir, uri, source) = open_test("        $repo->").await;
    let (line, character) = line_char_of(&source, "        $repo->");
    let labels = complete_labels_at_opened(&backend, &uri, line, character + 15).await;
    assert!(
        labels.iter().any(|l| l.starts_with("findOpen")),
        "{labels:?}"
    );
    assert!(labels.iter().any(|l| l.starts_with("find(")), "{labels:?}");
    assert!(labels.iter().any(|l| l.starts_with("reveal")), "{labels:?}");
    assert!(!labels.iter().any(|l| l.starts_with("make")), "{labels:?}");
}

#[tokio::test]
async fn a_mocked_method_call_navigates_to_the_mocked_method() {
    let (backend, dir, uri, source) = open_test("        $repo->findOpen();").await;
    let (line, character) = line_char_of(&source, "findOpen()");
    let response = goto_definition_at(&backend, &uri, line, character + 2)
        .await
        .expect("a definition");
    assert_eq!(
        definition_uri(&response),
        &Uri::from_file_path(dir.path().join("src/OrderRepository.php")).unwrap()
    );
}

#[tokio::test]
async fn a_prophecy_property_completes_the_mocked_methods() {
    let (backend, _dir, uri, source) = open_test("").await;
    let (line, character) = line_char_of(&source, "$this->orders->");
    let labels = complete_labels_at_opened(&backend, &uri, line, character + 15).await;
    assert!(
        labels.iter().any(|l| l.starts_with("findOpen")),
        "{labels:?}"
    );
}

/// The argument-type and argument-count diagnostics on the test file.
async fn argument_diagnostics(body: &str) -> Vec<String> {
    let (backend, _dir, uri, source) = open_test(body).await;
    let mut out: Vec<Diagnostic> = Vec::new();
    backend.collect_argument_type_diagnostics(uri.as_str(), &source, &mut out);
    backend.collect_argument_count_diagnostics(uri.as_str(), &source, &mut out);
    out.iter()
        .map(|d| message_text(&d.message).to_string())
        .collect()
}

#[tokio::test]
async fn an_argument_token_stands_in_for_a_value() {
    let diagnostics = argument_diagnostics(
        "        $repo->find(Argument::any())->willReturn(null);\n        $repo->find(5);\n        new Billing($repo->reveal());",
    )
    .await;
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[tokio::test]
async fn a_value_of_the_wrong_type_is_still_reported() {
    let diagnostics = argument_diagnostics("        $repo->find('five');").await;
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
}
