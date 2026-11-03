//! The method names mock expectations spell as strings: PHPUnit's
//! `$mock->method('name')`, Mockery's `shouldReceive('name')` and its
//! siblings, and Laravel's `Facade::shouldReceive('name')`.

use crate::common::{
    complete_labels_at_opened, create_psr4_workspace, definition_uri, find_references,
    goto_definition_at, hover_on, hover_text_at, line_char_of, message_text, open_php, position_of,
    rename,
};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "": "stubs/"
        }
    }
}"#;

const STUB_PHP: &str = r#"<?php
namespace PHPUnit\Framework\MockObject;
interface Stub
{
    public function method(string $constraint): InvocationStubber;
}
"#;

const MOCK_OBJECT_PHP: &str = r#"<?php
namespace PHPUnit\Framework\MockObject;
interface MockObject extends Stub
{
    public function expects(object $invocationRule): InvocationMocker;
}
"#;

const INVOCATION_STUBBER_PHP: &str = r#"<?php
namespace PHPUnit\Framework\MockObject;
interface InvocationStubber
{
    public function willReturn(mixed $value): static;
}
"#;

const INVOCATION_MOCKER_PHP: &str = r#"<?php
namespace PHPUnit\Framework\MockObject;
final class InvocationMocker implements InvocationStubber
{
    public function method(string $constraint): static { return $this; }
    public function willReturn(mixed $value): static { return $this; }
}
"#;

const MOCK_BUILDER_PHP: &str = r#"<?php
namespace PHPUnit\Framework\MockObject;
/**
 * @template MockedType of object
 */
final class MockBuilder
{
    /**
     * @return MockedType&MockObject
     */
    public function getMock(): MockObject {}
}
"#;

const TEST_CASE_PHP: &str = r#"<?php
namespace PHPUnit\Framework;
use PHPUnit\Framework\MockObject\MockBuilder;
use PHPUnit\Framework\MockObject\MockObject;
use PHPUnit\Framework\MockObject\Stub;
abstract class TestCase
{
    /**
     * @template RealInstanceType of object
     * @param class-string<RealInstanceType> $type
     * @return MockObject&RealInstanceType
     */
    final protected function createMock(string $type): MockObject {}

    /**
     * @template RealInstanceType of object
     * @param class-string<RealInstanceType> $type
     * @return RealInstanceType&Stub
     */
    final protected static function createStub(string $type): Stub {}

    /**
     * @template RealInstanceType of object
     * @param class-string<RealInstanceType> $className
     * @return MockBuilder<RealInstanceType>
     */
    final protected function getMockBuilder(string $className): MockBuilder {}

    final public static function once(): object {}
}
"#;

const LEGACY_MOCK_INTERFACE_PHP: &str = r#"<?php
namespace Mockery;
interface LegacyMockInterface
{
    public function shouldReceive(...$methodNames);
    public function shouldNotReceive(...$methodNames);
    public function shouldHaveReceived($method, $args = null);
}
"#;

const MOCK_INTERFACE_PHP: &str = r#"<?php
namespace Mockery;
interface MockInterface extends LegacyMockInterface
{
    public function allows($something = []);
    public function expects($something = null);
}
"#;

const MOCKERY_PHP: &str = r#"<?php
class Mockery
{
    /**
     * @template TMock of object
     * @param array<class-string<TMock>|TMock> $args
     * @return \Mockery\LegacyMockInterface&\Mockery\MockInterface&TMock
     */
    public static function mock(...$args) {}
}
"#;

const FACADE_PHP: &str = r#"<?php
namespace Illuminate\Support\Facades;
abstract class Facade
{
    public static function shouldReceive() {}
    public static function expects() {}
    public static function __callStatic($method, $args) {}
}
"#;

const ORDER_REPOSITORY_PHP: &str = r#"<?php
namespace App;
class OrderRepository
{
    public function findOpen(): array { return []; }
    public function save(object $order): void {}
}
"#;

const MAILER_PHP: &str = r#"<?php
namespace App;
interface Mailer
{
    public function send(string $to): bool;
    public function queue(string $to): void;
}
"#;

const MAGIC_PHP: &str = r#"<?php
namespace App;
class Magic
{
    public function __call(string $name, array $arguments): mixed { return null; }
}
"#;

const GATE_PHP: &str = r#"<?php
namespace App;
class Gate
{
    public function allows(string $ability): bool { return true; }
    public function update(): void {}
}
"#;

const CACHE_MANAGER_PHP: &str = r#"<?php
namespace App;
class CacheManager
{
    public function remember(string $key): mixed { return null; }
}
"#;

const CACHE_FACADE_PHP: &str = r#"<?php
namespace App;
use Illuminate\Support\Facades\Facade;
class Cache extends Facade
{
    protected static function getFacadeAccessor(): string
    {
        return CacheManager::class;
    }
}
"#;

fn workspace() -> (Backend, tempfile::TempDir) {
    create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("stubs/PHPUnit/Framework/MockObject/Stub.php", STUB_PHP),
            (
                "stubs/PHPUnit/Framework/MockObject/MockObject.php",
                MOCK_OBJECT_PHP,
            ),
            (
                "stubs/PHPUnit/Framework/MockObject/InvocationStubber.php",
                INVOCATION_STUBBER_PHP,
            ),
            (
                "stubs/PHPUnit/Framework/MockObject/InvocationMocker.php",
                INVOCATION_MOCKER_PHP,
            ),
            (
                "stubs/PHPUnit/Framework/MockObject/MockBuilder.php",
                MOCK_BUILDER_PHP,
            ),
            ("stubs/PHPUnit/Framework/TestCase.php", TEST_CASE_PHP),
            (
                "stubs/Mockery/LegacyMockInterface.php",
                LEGACY_MOCK_INTERFACE_PHP,
            ),
            ("stubs/Mockery/MockInterface.php", MOCK_INTERFACE_PHP),
            ("stubs/Mockery.php", MOCKERY_PHP),
            ("stubs/Illuminate/Support/Facades/Facade.php", FACADE_PHP),
            ("src/OrderRepository.php", ORDER_REPOSITORY_PHP),
            ("src/Mailer.php", MAILER_PHP),
            ("src/Magic.php", MAGIC_PHP),
            ("src/Gate.php", GATE_PHP),
            ("src/CacheManager.php", CACHE_MANAGER_PHP),
            ("src/Cache.php", CACHE_FACADE_PHP),
        ],
    )
}

/// A test class whose body is `body`, with the usual imports.
fn test_source(body: &str) -> String {
    format!(
        r#"<?php
use App\Cache;
use App\Gate;
use App\Magic;
use App\Mailer;
use App\OrderRepository;
use PHPUnit\Framework\TestCase;

final class OrderTest extends TestCase
{{
    public function testIt(Gate $gate): void
    {{
{body}
    }}
}}
"#
    )
}

async fn open_test(backend: &Backend, dir: &tempfile::TempDir, source: &str) -> Uri {
    let uri = Uri::from_file_path(dir.path().join("tests/OrderTest.php")).unwrap();
    open_php(backend, &uri, source).await;
    uri
}

fn file_uri(dir: &tempfile::TempDir, relative: &str) -> Uri {
    Uri::from_file_path(dir.path().join(relative)).unwrap()
}

/// The line and file go-to-definition on `needle` (plus `offset`
/// characters) lands on, if any.
async fn definition_target(
    backend: &Backend,
    uri: &Uri,
    source: &str,
    needle: &str,
    offset: u32,
) -> Option<(Uri, u32)> {
    let (line, character) = line_char_of(source, needle);
    let response = goto_definition_at(backend, uri, line, character + offset).await?;
    let uri = definition_uri(&response).clone();
    let line = match response {
        DefinitionResponse::Definition(Definition::Location(location)) => location.range.start.line,
        DefinitionResponse::Definition(Definition::LocationList(locations)) => {
            locations[0].range.start.line
        }
        DefinitionResponse::DefinitionLinkList(links) => links[0].target_selection_range.start.line,
    };
    Some((uri, line))
}

async fn assert_jumps_to(body: &str, needle: &str, file: &str, line: u32) {
    let (backend, dir) = workspace();
    let source = test_source(body);
    let uri = open_test(&backend, &dir, &source).await;
    let target = definition_target(&backend, &uri, &source, needle, 2).await;
    assert_eq!(
        target,
        Some((file_uri(&dir, file), line)),
        "go-to-definition on {needle} in:\n{source}"
    );
}

// ─── Go to definition ───────────────────────────────────────────────────────

#[tokio::test]
async fn phpunit_create_mock_method() {
    assert_jumps_to(
        "        $repo = $this->createMock(OrderRepository::class);\n        $repo->method('findOpen')->willReturn([]);",
        "'findOpen'",
        "src/OrderRepository.php",
        4,
    )
    .await;
}

#[tokio::test]
async fn phpunit_method_after_expects() {
    assert_jumps_to(
        "        $repo = $this->createMock(OrderRepository::class);\n        $repo->expects($this->once())->method('save');",
        "'save'",
        "src/OrderRepository.php",
        5,
    )
    .await;
}

#[tokio::test]
async fn phpunit_create_stub_method() {
    assert_jumps_to(
        "        $repo = self::createStub(OrderRepository::class);\n        $repo->method('findOpen');",
        "'findOpen'",
        "src/OrderRepository.php",
        4,
    )
    .await;
}

#[tokio::test]
async fn phpunit_mock_builder_method() {
    assert_jumps_to(
        "        $repo = $this->getMockBuilder(OrderRepository::class)->getMock();\n        $repo->method('findOpen');",
        "'findOpen'",
        "src/OrderRepository.php",
        4,
    )
    .await;
}

#[tokio::test]
async fn mockery_should_receive() {
    assert_jumps_to(
        "        $mailer = \\Mockery::mock(Mailer::class);\n        $mailer->shouldReceive('send')->once();",
        "'send'",
        "src/Mailer.php",
        4,
    )
    .await;
}

#[tokio::test]
async fn mockery_should_receive_every_name() {
    assert_jumps_to(
        "        $mailer = \\Mockery::mock(Mailer::class);\n        $mailer->shouldReceive('send', 'queue');",
        "'queue'",
        "src/Mailer.php",
        5,
    )
    .await;
}

#[tokio::test]
async fn mockery_allows_map() {
    assert_jumps_to(
        "        $mailer = \\Mockery::mock(Mailer::class);\n        $mailer->allows(['send' => true]);",
        "'send'",
        "src/Mailer.php",
        4,
    )
    .await;
}

#[tokio::test]
async fn mockery_expects_on_a_direct_mock() {
    assert_jumps_to(
        "        \\Mockery::mock(Mailer::class)->expects('queue');",
        "'queue'",
        "src/Mailer.php",
        5,
    )
    .await;
}

#[tokio::test]
async fn mockery_should_have_received() {
    assert_jumps_to(
        "        $mailer = \\Mockery::mock(Mailer::class);\n        $mailer->shouldHaveReceived('send');",
        "'send'",
        "src/Mailer.php",
        4,
    )
    .await;
}

#[tokio::test]
async fn facade_should_receive_names_the_target() {
    assert_jumps_to(
        "        Cache::shouldReceive('remember')->andReturn(1);",
        "'remember'",
        "src/CacheManager.php",
        4,
    )
    .await;
}

/// `allows('update')` on an object that is not a mock is an ordinary call
/// with a string argument.
#[tokio::test]
async fn a_non_mock_is_left_alone() {
    let (backend, dir) = workspace();
    let source = test_source("        $gate->allows('update');");
    let uri = open_test(&backend, &dir, &source).await;
    let target = definition_target(&backend, &uri, &source, "'update'", 2).await;
    assert_eq!(target, None);
}

// ─── Hover ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn hover_shows_the_mocked_method() {
    let (backend, dir) = workspace();
    let source = test_source(
        "        $repo = $this->createMock(OrderRepository::class);\n        $repo->method('findOpen');",
    );
    let uri = open_test(&backend, &dir, &source).await;
    let text = hover_on(&backend, &uri, &source, "findOpen'").await;
    assert!(text.contains("function findOpen(): array"), "{text}");
}

#[tokio::test]
async fn no_hover_on_a_non_mock() {
    let (backend, dir) = workspace();
    let source = test_source("        $gate->allows('update');");
    let uri = open_test(&backend, &dir, &source).await;
    let position = position_of(&source, "update'");
    let text = hover_text_at(&backend, &uri, position.line, position.character + 2)
        .await
        .unwrap_or_default();
    assert!(!text.contains("function update"), "{text}");
}

// ─── Find references and rename ─────────────────────────────────────────────

#[tokio::test]
async fn references_include_mock_expectations() {
    let (backend, dir) = workspace();
    let source = test_source(
        "        $repo = $this->createMock(OrderRepository::class);\n        $repo->method('findOpen');\n        $gate->allows('findOpen');",
    );
    let uri = open_test(&backend, &dir, &source).await;
    let repo_uri = file_uri(&dir, "src/OrderRepository.php");
    open_php(&backend, &repo_uri, ORDER_REPOSITORY_PHP).await;

    let (line, character) = line_char_of(ORDER_REPOSITORY_PHP, "findOpen");
    let locations = find_references(
        &backend,
        &repo_uri,
        Position::new(line, character + 1),
        false,
    )
    .await;
    let in_test: Vec<u32> = locations
        .iter()
        .filter(|location| location.uri == uri)
        .map(|location| location.range.start.line)
        .collect();
    let (expectation_line, _) = line_char_of(&source, "'findOpen'");
    assert_eq!(in_test, vec![expectation_line], "{locations:?}");
}

#[tokio::test]
async fn references_from_the_expectation() {
    let (backend, dir) = workspace();
    let source = test_source(
        "        $repo = $this->createMock(OrderRepository::class);\n        $repo->method('findOpen');",
    );
    let uri = open_test(&backend, &dir, &source).await;
    let repo_uri = file_uri(&dir, "src/OrderRepository.php");
    open_php(&backend, &repo_uri, ORDER_REPOSITORY_PHP).await;

    let (line, character) = line_char_of(&source, "'findOpen'");
    let locations = find_references(&backend, &uri, Position::new(line, character + 2), true).await;
    assert!(
        locations
            .iter()
            .any(|location| location.uri == repo_uri && location.range.start.line == 4),
        "{locations:?}"
    );
}

#[tokio::test]
async fn rename_updates_mock_expectations() {
    let (backend, dir) = workspace();
    let source = test_source(
        "        $repo = $this->createMock(OrderRepository::class);\n        $repo->method('findOpen');\n        $gate->allows('findOpen');",
    );
    let uri = open_test(&backend, &dir, &source).await;
    let repo_uri = file_uri(&dir, "src/OrderRepository.php");
    open_php(&backend, &repo_uri, ORDER_REPOSITORY_PHP).await;

    let (line, character) = line_char_of(ORDER_REPOSITORY_PHP, "findOpen");
    let edit = rename(&backend, &repo_uri, line, character + 1, "findPending")
        .await
        .expect("expected an edit");
    let changes = edit.changes.expect("expected changes");
    let test_edits = changes.get(&uri).expect("expected edits in the test");
    let (expectation_line, expectation_char) = line_char_of(&source, "'findOpen'");
    assert_eq!(test_edits.len(), 1, "{test_edits:?}");
    assert_eq!(
        test_edits[0].range.start,
        Position::new(expectation_line, expectation_char + 1)
    );
    assert_eq!(test_edits[0].new_text, "findPending");
}

// ─── Unknown-method diagnostic ──────────────────────────────────────────────

async fn unknown_member_messages(body: &str) -> Vec<String> {
    let (backend, dir) = workspace();
    let source = test_source(body);
    let uri = open_test(&backend, &dir, &source).await;
    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri.as_str(), &source, &mut out);
    out.into_iter()
        .map(|d| message_text(&d.message).to_string())
        .collect()
}

#[tokio::test]
async fn an_unknown_mocked_method_is_reported() {
    let messages = unknown_member_messages(
        "        $repo = $this->createMock(OrderRepository::class);\n        $repo->method('findClosed');\n        $repo->expects($this->once())->method('delete');\n        \\Mockery::mock(Mailer::class)->shouldReceive('sendLater');",
    )
    .await;
    assert_eq!(messages.len(), 3, "{messages:?}");
    assert!(messages[0].contains("findClosed"), "{messages:?}");
    assert!(messages[1].contains("delete"), "{messages:?}");
    assert!(messages[2].contains("sendLater"), "{messages:?}");
}

#[tokio::test]
async fn a_mock_framework_method_is_not_a_mocked_method() {
    let messages = unknown_member_messages(
        "        $repo = $this->createMock(OrderRepository::class);\n        $repo->method('expects');",
    )
    .await;
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("expects"), "{messages:?}");
}

#[tokio::test]
async fn known_mocked_methods_are_not_reported() {
    let messages = unknown_member_messages(
        "        $repo = $this->createMock(OrderRepository::class);\n        $repo->method('findOpen');\n        \\Mockery::mock(Mailer::class)->allows(['send' => true, 'queue' => null]);\n        Cache::shouldReceive('remember');",
    )
    .await;
    assert!(messages.is_empty(), "{messages:?}");
}

#[tokio::test]
async fn a_mocked_class_with_call_takes_any_name() {
    let messages = unknown_member_messages(
        "        $magic = $this->createMock(Magic::class);\n        $magic->method('anything');",
    )
    .await;
    assert!(messages.is_empty(), "{messages:?}");
}

#[tokio::test]
async fn a_non_mock_is_not_checked() {
    let messages = unknown_member_messages("        $gate->allows('publish');").await;
    assert!(messages.is_empty(), "{messages:?}");
}

// ─── Completion ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn completion_offers_the_mocked_methods() {
    let (backend, dir) = workspace();
    let source = test_source(
        "        $repo = $this->createMock(OrderRepository::class);\n        $repo->method('');",
    );
    let uri = open_test(&backend, &dir, &source).await;
    let (line, character) = line_char_of(&source, "method('')");
    let labels = complete_labels_at_opened(&backend, &uri, line, character + 8).await;
    assert_eq!(labels, ["findOpen()", "save($order)"]);
}

#[tokio::test]
async fn completion_offers_the_facade_targets_methods() {
    let (backend, dir) = workspace();
    let source = test_source("        Cache::shouldReceive('rem');");
    let uri = open_test(&backend, &dir, &source).await;
    let (line, character) = line_char_of(&source, "'rem'");
    let labels = complete_labels_at_opened(&backend, &uri, line, character + 4).await;
    assert_eq!(labels, ["remember($key)"]);
}

#[tokio::test]
async fn no_completion_on_a_non_mock() {
    let (backend, dir) = workspace();
    let source = test_source("        $gate->allows('');");
    let uri = open_test(&backend, &dir, &source).await;
    let (line, character) = line_char_of(&source, "allows('')");
    let labels = complete_labels_at_opened(&backend, &uri, line, character + 8).await;
    assert!(
        !labels.iter().any(|l| l.starts_with("update(")),
        "{labels:?}"
    );
}

#[tokio::test]
async fn completion_after_expects() {
    let (backend, dir) = workspace();
    let source = test_source(
        "        $repo = $this->createMock(OrderRepository::class);\n        $repo->expects($this->once())->method('sa');",
    );
    let uri = open_test(&backend, &dir, &source).await;
    let (line, character) = line_char_of(&source, "'sa'");
    let labels = complete_labels_at_opened(&backend, &uri, line, character + 3).await;
    assert_eq!(labels, ["save($order)"]);
}

#[tokio::test]
async fn completion_in_a_mockery_map_key() {
    let (backend, dir) = workspace();
    let source = test_source(
        "        $mailer = \\Mockery::mock(Mailer::class);\n        $mailer->allows(['q' => null]);",
    );
    let uri = open_test(&backend, &dir, &source).await;
    let (line, character) = line_char_of(&source, "'q'");
    let labels = complete_labels_at_opened(&backend, &uri, line, character + 2).await;
    assert_eq!(labels, ["queue($to)"]);
}

#[tokio::test]
async fn completion_in_an_unfinished_expectation() {
    let (backend, dir) = workspace();
    let source = test_source(
        "        $mailer = \\Mockery::mock(Mailer::class);\n        $mailer->shouldReceive('se",
    );
    let uri = open_test(&backend, &dir, &source).await;
    let (line, character) = line_char_of(&source, "'se");
    let labels = complete_labels_at_opened(&backend, &uri, line, character + 3).await;
    assert_eq!(labels, ["send($to)"]);
}

/// A Mockery demeter chain names a method of whatever the first call
/// returns, not only of the mocked class.
#[tokio::test]
async fn a_demeter_chain_is_not_checked() {
    let messages = unknown_member_messages(
        "        \\Mockery::mock(Mailer::class)->shouldReceive('transport->flush');",
    )
    .await;
    assert!(messages.is_empty(), "{messages:?}");
}
