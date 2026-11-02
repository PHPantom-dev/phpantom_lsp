use crate::common::{
    apply_edits, create_test_backend, extract_edits, find_action_titled, get_code_actions_at,
    line_char_of,
};

fn fix(content: &str, at: &str, title: &str) -> String {
    let backend = create_test_backend();
    let uri = "file:///PriceTest.php";
    backend.update_ast(uri, content);
    let (line, character) = line_char_of(content, at);
    let actions = get_code_actions_at(&backend, uri, content, line, character);
    let action = find_action_titled(&actions, title).unwrap_or_else(|| {
        panic!(
            "expected `{title}`, got: {:?}",
            actions
                .iter()
                .filter_map(|a| match a {
                    tower_lsp_server::gen_lsp_types::CodeActionResponse::CodeAction(ca) =>
                        Some(&ca.title),
                    _ => None,
                })
                .collect::<Vec<_>>()
        )
    });
    apply_edits(content, &extract_edits(action))
}

#[test]
fn makes_a_provider_public_and_static() {
    let content = r#"<?php
use PHPUnit\Framework\Attributes\DataProvider;

final class PriceTest
{
    #[DataProvider('prices')]
    public function testPrice(int $n): void {}

    private function prices(): array { return [[1]]; }
}
"#;
    let public = fix(content, "prices'", "Make data provider 'prices' public");
    assert!(
        public.contains("    public function prices(): array"),
        "got:\n{public}"
    );
    let r#static = fix(content, "prices'", "Make data provider 'prices' static");
    assert!(
        r#static.contains("    private static function prices(): array"),
        "got:\n{static}"
    );
    let both = fix(&public, "prices'", "Make data provider 'prices' static");
    assert!(
        both.contains("    public static function prices(): array"),
        "got:\n{both}"
    );
}

#[test]
fn makes_a_provider_without_modifiers_static() {
    let content = r#"<?php
use PHPUnit\Framework\Attributes\DataProvider;

final class PriceTest
{
    #[DataProvider('prices')]
    public function testPrice(int $n): void {}

    function prices(): array { return [[1]]; }
}
"#;
    let fixed = fix(content, "prices'", "Make data provider 'prices' static");
    assert!(
        fixed.contains("    static function prices(): array"),
        "got:\n{fixed}"
    );
}

#[test]
fn creates_a_missing_provider() {
    let content = r#"<?php
namespace Tests;

final class PriceTest
{
    /**
     * @dataProvider prices
     */
    public function testPrice(int $n): void {}
}
"#;
    let fixed = fix(content, "prices", "Create data provider 'prices'");
    assert_eq!(
        fixed,
        r#"<?php
namespace Tests;

final class PriceTest
{
    /**
     * @dataProvider prices
     */
    public function testPrice(int $n): void {}

    public static function prices(): iterable
    {
        return [];
    }
}
"#
    );
}
