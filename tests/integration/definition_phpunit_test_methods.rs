//! Go-to-definition on the methods PHPUnit test metadata names by string.
//!
//! Covers data providers (`#[DataProvider]`, `#[DataProviderExternal]`,
//! `@dataProvider`) and test dependencies (`#[Depends]` and its variants,
//! `@depends`).

use crate::common::{
    create_psr4_workspace, create_test_backend, goto_definition_at, line_char_of, open_php,
};
use tower_lsp::lsp_types::*;

fn assert_location(response: Option<GotoDefinitionResponse>, uri: &Url, expected_line: u32) {
    match response {
        Some(GotoDefinitionResponse::Scalar(location)) => {
            assert_eq!(&location.uri, uri, "jumped to the wrong file");
            assert_eq!(
                location.range.start.line, expected_line,
                "expected line {}, got {}",
                expected_line, location.range.start.line
            );
        }
        other => panic!("expected a single location, got: {:?}", other),
    }
}

/// Go to definition on the first occurrence of `needle` in `source`,
/// `offset` characters into it.
async fn definition_of(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    source: &str,
    needle: &str,
    offset: u32,
) -> Option<GotoDefinitionResponse> {
    let (line, character) = line_char_of(source, needle);
    goto_definition_at(backend, uri, line, character + offset).await
}

#[tokio::test]
async fn data_provider_attribute_jumps_to_provider() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///PlaceholdersTest.php").unwrap();
    let source = r#"<?php
namespace Tests;

use PHPUnit\Framework\Attributes\DataProvider;

final class PlaceholdersTest
{
    #[DataProvider('fitting')]
    public function testFits(string $english): void {}

    public static function fitting(): iterable
    {
        yield ['Save'];
    }
}
"#;
    open_php(&backend, &uri, source).await;

    let response = definition_of(&backend, &uri, source, "'fitting'", 3).await;
    assert_location(response, &uri, 10);
}

#[tokio::test]
async fn fully_qualified_data_provider_attribute_jumps_to_provider() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///FqnTest.php").unwrap();
    let source = r#"<?php
final class FqnTest
{
    #[\PHPUnit\Framework\Attributes\DataProvider('cases')]
    public function testIt(int $n): void {}

    public static function cases(): array { return [[1]]; }
}
"#;
    open_php(&backend, &uri, source).await;

    let response = definition_of(&backend, &uri, source, "'cases'", 2).await;
    assert_location(response, &uri, 6);
}

/// A same-named attribute of the project's own is not PHPUnit's.
#[tokio::test]
async fn unrelated_data_provider_attribute_is_left_alone() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///OwnAttributeTest.php").unwrap();
    let source = r#"<?php
use App\Attributes\DataProvider;

final class OwnAttributeTest
{
    #[DataProvider('cases')]
    public function testIt(int $n): void {}

    public static function cases(): array { return [[1]]; }
}
"#;
    open_php(&backend, &uri, source).await;

    let response = definition_of(&backend, &uri, source, "'cases'", 2).await;
    assert!(
        response.is_none(),
        "expected no definition, got {response:?}"
    );
}

#[tokio::test]
async fn data_provider_inherited_from_a_base_test_case() {
    let composer = r#"{"autoload-dev":{"psr-4":{"Tests\\":"tests/"}}}"#;
    let base = r#"<?php
namespace Tests;

abstract class TestCase
{
    public static function sharedCases(): iterable
    {
        yield [1];
    }
}
"#;
    let (backend, dir) = create_psr4_workspace(composer, &[("tests/TestCase.php", base)]);
    let base_uri = Url::from_file_path(dir.path().join("tests/TestCase.php")).unwrap();
    let uri = Url::from_file_path(dir.path().join("tests/ChildTest.php")).unwrap();
    let source = r#"<?php
namespace Tests;

use PHPUnit\Framework\Attributes\DataProvider;

final class ChildTest extends TestCase
{
    #[DataProvider('sharedCases')]
    public function testIt(int $n): void {}
}
"#;
    open_php(&backend, &uri, source).await;

    let response = definition_of(&backend, &uri, source, "'sharedCases'", 3).await;
    assert_location(response, &base_uri, 5);
}

#[tokio::test]
async fn data_provider_external_attribute_jumps_to_other_class() {
    let composer = r#"{"autoload-dev":{"psr-4":{"Tests\\":"tests/"}}}"#;
    let provider = r#"<?php
namespace Tests\Providers;

final class Amounts
{
    public static function positive(): iterable
    {
        yield [1];
    }
}
"#;
    let (backend, dir) =
        create_psr4_workspace(composer, &[("tests/Providers/Amounts.php", provider)]);
    let provider_uri = Url::from_file_path(dir.path().join("tests/Providers/Amounts.php")).unwrap();
    let uri = Url::from_file_path(dir.path().join("tests/AmountTest.php")).unwrap();
    let source = r#"<?php
namespace Tests;

use PHPUnit\Framework\Attributes\DataProviderExternal;
use Tests\Providers\Amounts;

final class AmountTest
{
    #[DataProviderExternal(Amounts::class, 'positive')]
    public function testIt(int $n): void {}

    #[DataProviderExternal('Tests\Providers\Amounts', 'positive')]
    public function testItAgain(int $n): void {}
}
"#;
    open_php(&backend, &uri, source).await;

    let response = definition_of(&backend, &uri, source, "'positive'", 3).await;
    assert_location(response, &provider_uri, 5);

    // The string spelling names the class by its fully-qualified name, which
    // must not be read relative to the test's own namespace.
    let (line, _) = line_char_of(source, "'Tests\\Providers\\Amounts', 'positive'");
    let character = source
        .lines()
        .nth(line as usize)
        .unwrap()
        .rfind("positive")
        .unwrap();
    let response = goto_definition_at(&backend, &uri, line, character as u32 + 1).await;
    assert_location(response, &provider_uri, 5);
}

#[tokio::test]
async fn depends_attributes_jump_to_the_test_depended_on() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///StackTest.php").unwrap();
    let source = r#"<?php
use PHPUnit\Framework\Attributes\Depends;
use PHPUnit\Framework\Attributes\DependsUsingShallowClone;

final class StackTest
{
    public function testEmpty(): array { return []; }

    #[Depends('testEmpty')]
    public function testPush(array $stack): array { return $stack; }

    #[DependsUsingShallowClone('testPush')]
    public function testPop(array $stack): void {}
}
"#;
    open_php(&backend, &uri, source).await;

    let response = definition_of(&backend, &uri, source, "'testEmpty'", 3).await;
    assert_location(response, &uri, 6);

    let response = definition_of(&backend, &uri, source, "'testPush'", 3).await;
    assert_location(response, &uri, 9);
}

#[tokio::test]
async fn data_provider_annotation_jumps_to_provider() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///AnnotatedTest.php").unwrap();
    let source = r#"<?php
final class Amounts
{
    public static function negative(): array { return [[-1]]; }
}

final class AnnotatedTest
{
    /**
     * @dataProvider additions
     */
    public function testAdd(int $a): void {}

    /**
     * @dataProvider Amounts::negative
     */
    public function testNegative(int $a): void {}

    public static function additions(): array { return [[1]]; }
}
"#;
    open_php(&backend, &uri, source).await;

    let response = definition_of(&backend, &uri, source, "@dataProvider additions", 16).await;
    assert_location(response, &uri, 18);

    let response = definition_of(&backend, &uri, source, "Amounts::negative", 11).await;
    assert_location(response, &uri, 3);
}

#[tokio::test]
async fn depends_annotation_with_a_clone_modifier_jumps_to_test() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///DependsTest.php").unwrap();
    let source = r#"<?php
final class DependsTest
{
    public function testProducer(): array { return []; }

    /**
     * @depends clone testProducer
     */
    public function testConsumer(array $value): void {}
}
"#;
    open_php(&backend, &uri, source).await;

    let response = definition_of(&backend, &uri, source, "clone testProducer", 8).await;
    assert_location(response, &uri, 3);
}
