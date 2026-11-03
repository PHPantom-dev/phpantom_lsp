use crate::common::{collect_diagnostics_with, message_text};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

fn collect(php: &str) -> Vec<(DiagnosticSeverity, String)> {
    collect_diagnostics_with(
        &Backend::new_test(),
        php,
        Backend::collect_data_provider_diagnostics,
    )
    .iter()
    .map(|d| {
        assert_eq!(
            d.code,
            Some(Code::String("invalid_data_provider".to_string()))
        );
        (d.severity.unwrap(), message_text(&d.message).to_string())
    })
    .collect()
}

#[test]
fn a_provider_must_be_public() {
    let php = r#"<?php
namespace Tests;

final class PriceTest
{
    /**
     * @dataProvider prices
     */
    public function testPrice(int $n): void {}

    protected function prices(): array { return [[1]]; }
}
"#;
    assert_eq!(
        collect(php),
        vec![(
            DiagnosticSeverity::Error,
            "Data provider method Tests\\PriceTest::prices() must be public".to_string()
        )]
    );
}

/// Only PHPUnit 10 and later read the attribute, and they want the
/// provider static.
#[test]
fn an_attribute_provider_must_be_public_and_static() {
    let php = r#"<?php
use PHPUnit\Framework\Attributes\DataProvider;

final class PriceTest
{
    #[DataProvider('prices')]
    public function testPrice(int $n): void {}

    private function prices(): array { return [[1]]; }
}
"#;
    assert_eq!(
        collect(php),
        vec![
            (
                DiagnosticSeverity::Error,
                "Data provider method PriceTest::prices() must be public".to_string()
            ),
            (
                DiagnosticSeverity::Warning,
                "Data provider method PriceTest::prices() must be static".to_string()
            ),
        ]
    );
}

#[test]
fn an_external_provider_is_checked_on_its_own_class() {
    let php = r#"<?php
use PHPUnit\Framework\Attributes\DataProviderExternal;

final class Prices
{
    public function all(): array { return [[1]]; }
}

final class PriceTest
{
    #[DataProviderExternal(Prices::class, 'all')]
    public function testPrice(int $n): void {}
}
"#;
    assert_eq!(
        collect(php),
        vec![(
            DiagnosticSeverity::Warning,
            "Data provider method Prices::all() must be static".to_string()
        )]
    );
}

/// An inherited provider and a dependency are judged by what they are:
/// the first by the provider rules, the second not at all.
#[test]
fn valid_providers_and_dependencies_are_not_reported() {
    let php = r#"<?php
use PHPUnit\Framework\Attributes\DataProvider;
use PHPUnit\Framework\Attributes\Depends;

abstract class BaseTest
{
    public static function shared(): iterable { yield [1]; }
}

final class PriceTest extends BaseTest
{
    #[DataProvider('prices')]
    #[DataProvider('shared')]
    public function testPrice(int $n): void {}

    #[Depends('testPrice')]
    public function testLater(): void {}

    /**
     * @depends testPrice
     */
    public function testLegacyLater(): void {}

    public static function prices(): array { return [[1]]; }
}
"#;
    assert_eq!(collect(php), vec![]);
}

/// A provider that does not exist is the unknown-member check's finding.
#[test]
fn a_missing_provider_is_not_reported_here() {
    let php = r#"<?php
use PHPUnit\Framework\Attributes\DataProvider;

final class PriceTest
{
    #[DataProvider('nothing')]
    public function testPrice(int $n): void {}
}
"#;
    assert_eq!(collect(php), vec![]);
}
