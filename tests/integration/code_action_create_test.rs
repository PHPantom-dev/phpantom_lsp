//! Integration tests for the "Create test" code action.

use crate::common::{create_psr4_workspace, get_code_actions_at};
use tower_lsp_server::gen_lsp_types::*;

const LARAVEL_COMPOSER: &str = r#"{ "autoload": { "psr-4": { "App\\": "app/" } }, "autoload-dev": { "psr-4": { "Tests\\": "tests/" } } }"#;

const INVOICE_PATH: &str = "app/Billing/Invoice.php";

const INVOICE: &str = r#"<?php

declare(strict_types=1);

namespace App\Billing;

class Invoice
{
    public function __construct(private int $total) {}

    public function total(): int
    {
        return $this->total;
    }

    public static function empty(): self
    {
        return new self(0);
    }

    private function secret(): void {}
}
"#;

const BASE_TEST_CASE: (&str, &str) = (
    "tests/TestCase.php",
    "<?php\nnamespace Tests;\n\nabstract class TestCase extends \\PHPUnit\\Framework\\TestCase {}\n",
);

const UNIT_EXAMPLE: (&str, &str) = (
    "tests/Unit/ExampleTest.php",
    "<?php\nnamespace Tests\\Unit;\n\nclass ExampleTest extends \\Tests\\TestCase {}\n",
);

/// A "Create test" action, as `(title, created file relative to the
/// workspace, its content, where the editor opens it)`.
type CreatedTest = (String, String, String, Option<serde_json::Value>);

/// The "Create test" actions offered at `line:character` of `file`.
fn create_test_actions(
    composer: &str,
    files: &[(&str, &str)],
    file: &str,
    line: u32,
    character: u32,
) -> Vec<CreatedTest> {
    let (backend, dir) = create_psr4_workspace(composer, files);
    let uri = format!("file://{}", dir.path().join(file).display());
    let content = std::fs::read_to_string(dir.path().join(file)).unwrap();
    backend.update_ast(&uri, &content);
    let root = format!("file://{}/", dir.path().display());

    get_code_actions_at(&backend, &uri, &content, line, character)
        .into_iter()
        .filter_map(|action| match action {
            CodeActionResponse::CodeAction(action) if action.title.starts_with("Create test") => {
                Some(action)
            }
            _ => None,
        })
        .map(|action| {
            assert_eq!(action.kind, Some(CodeActionKind::Refactor));
            let ops = action
                .edit
                .and_then(|edit| edit.document_changes)
                .expect("the action should carry document changes");
            let path = match &ops[0] {
                DocumentChange::CreateFile(create) => create
                    .uri
                    .as_str()
                    .strip_prefix(&root)
                    .expect("the test should be inside the workspace")
                    .to_string(),
                other => panic!("expected a create operation, got {other:?}"),
            };
            let text = match &ops[1] {
                DocumentChange::TextDocumentEdit(edit) => match &edit.edits[0] {
                    Edit::TextEdit(edit) => edit.new_text.clone(),
                    other => panic!("expected a plain edit, got {other:?}"),
                },
                other => panic!("expected the file's content, got {other:?}"),
            };
            let position = action
                .command
                .and_then(|command| command.arguments)
                .and_then(|arguments| arguments.last().cloned());
            (action.title, path, text, position)
        })
        .collect()
}

/// The first `line` in the open command's target, whichever shape the
/// client's capabilities give it.
fn first_line(value: &serde_json::Value) -> Option<u64> {
    match value {
        serde_json::Value::Object(map) => map
            .get("line")
            .and_then(serde_json::Value::as_u64)
            .or_else(|| map.values().find_map(first_line)),
        serde_json::Value::Array(items) => items.iter().find_map(first_line),
        _ => None,
    }
}

#[test]
fn creates_a_phpunit_test_mirroring_the_class_path() {
    let actions = create_test_actions(
        LARAVEL_COMPOSER,
        &[(INVOICE_PATH, INVOICE), BASE_TEST_CASE, UNIT_EXAMPLE],
        INVOICE_PATH,
        6,
        8,
    );
    assert_eq!(actions.len(), 1, "{actions:?}");
    let (title, path, text, position) = &actions[0];
    assert_eq!(title, "Create test for Invoice");
    assert_eq!(path, "tests/Unit/Billing/InvoiceTest.php");
    assert_eq!(position.as_ref().and_then(first_line), Some(15));
    assert_eq!(
        text,
        r#"<?php

declare(strict_types=1);

namespace Tests\Unit\Billing;

use App\Billing\Invoice;
use PHPUnit\Framework\Attributes\CoversClass;
use Tests\TestCase;

#[CoversClass(Invoice::class)]
class InvoiceTest extends TestCase
{
    public function testTotal(): void
    {
        self::markTestIncomplete();
    }

    public function testEmpty(): void
    {
        self::markTestIncomplete();
    }
}
"#
    );
}

#[test]
fn on_a_public_method_tests_only_that_method() {
    let actions = create_test_actions(
        LARAVEL_COMPOSER,
        &[(INVOICE_PATH, INVOICE), BASE_TEST_CASE],
        INVOICE_PATH,
        15,
        30,
    );
    assert_eq!(actions.len(), 1, "{actions:?}");
    let (title, path, text, _) = &actions[0];
    assert_eq!(title, "Create test for Invoice::empty()");
    // Without a `Unit/` directory the test goes straight under the root.
    assert_eq!(path, "tests/Billing/InvoiceTest.php");
    assert!(text.contains("namespace Tests\\Billing;\n"), "{text}");
    assert!(text.contains("public function testEmpty(): void"), "{text}");
    assert!(!text.contains("testTotal"), "{text}");
}

#[test]
fn on_a_private_method_tests_every_public_method() {
    let actions = create_test_actions(
        LARAVEL_COMPOSER,
        &[(INVOICE_PATH, INVOICE)],
        INVOICE_PATH,
        20,
        22,
    );
    assert_eq!(actions.len(), 1, "{actions:?}");
    let (title, _, text, _) = &actions[0];
    assert_eq!(title, "Create test for Invoice");
    assert!(
        text.contains("testTotal") && text.contains("testEmpty"),
        "{text}"
    );
    assert!(
        !text.contains("testSecret") && !text.contains("Construct"),
        "{text}"
    );
    // No project test case: PHPUnit's own.
    assert!(
        text.contains("use PHPUnit\\Framework\\TestCase;\n"),
        "{text}"
    );
}

#[test]
fn not_offered_inside_a_method_body() {
    let actions = create_test_actions(
        LARAVEL_COMPOSER,
        &[(INVOICE_PATH, INVOICE)],
        INVOICE_PATH,
        12,
        12,
    );
    assert!(actions.is_empty(), "{actions:?}");
}

#[test]
fn not_offered_once_the_class_has_a_test() {
    let existing = (
        "tests/Feature/Billing/InvoiceTest.php",
        "<?php\nnamespace Tests\\Feature\\Billing;\n\nclass InvoiceTest {}\n",
    );
    let actions = create_test_actions(
        LARAVEL_COMPOSER,
        &[(INVOICE_PATH, INVOICE), existing],
        INVOICE_PATH,
        6,
        8,
    );
    assert!(actions.is_empty(), "{actions:?}");
}

#[test]
fn the_created_test_is_the_one_the_class_pairs_with() {
    let actions = create_test_actions(
        LARAVEL_COMPOSER,
        &[(INVOICE_PATH, INVOICE), UNIT_EXAMPLE],
        INVOICE_PATH,
        6,
        8,
    );
    let (_, path, text, _) = &actions[0];
    let again = create_test_actions(
        LARAVEL_COMPOSER,
        &[(INVOICE_PATH, INVOICE), UNIT_EXAMPLE, (path, text)],
        INVOICE_PATH,
        6,
        8,
    );
    assert!(again.is_empty(), "{again:?}");
}

#[test]
fn not_offered_without_a_test_root() {
    let actions = create_test_actions(
        r#"{ "autoload": { "psr-4": { "App\\": "app/" } } }"#,
        &[(INVOICE_PATH, INVOICE)],
        INVOICE_PATH,
        6,
        8,
    );
    assert!(actions.is_empty(), "{actions:?}");
}

#[test]
fn not_offered_on_test_code() {
    let actions = create_test_actions(
        LARAVEL_COMPOSER,
        &[(INVOICE_PATH, INVOICE), BASE_TEST_CASE],
        BASE_TEST_CASE.0,
        3,
        16,
    );
    assert!(actions.is_empty(), "{actions:?}");
}

#[test]
fn a_pest_project_gets_a_pest_file() {
    let pest_config = (
        "tests/Pest.php",
        "<?php\n\npest()->extend(Tests\\TestCase::class)->in('Feature');\n",
    );
    let actions = create_test_actions(
        LARAVEL_COMPOSER,
        &[
            (INVOICE_PATH, INVOICE),
            BASE_TEST_CASE,
            UNIT_EXAMPLE,
            pest_config,
        ],
        INVOICE_PATH,
        6,
        8,
    );
    assert_eq!(actions.len(), 1, "{actions:?}");
    let (_, path, text, position) = &actions[0];
    assert_eq!(path, "tests/Unit/Billing/InvoiceTest.php");
    assert_eq!(
        text,
        r#"<?php

declare(strict_types=1);

namespace Tests\Unit\Billing;

use App\Billing\Invoice;

covers(Invoice::class);

test('total')->todo();

test('empty')->todo();
"#
    );
    assert_eq!(position.as_ref().and_then(first_line), Some(10));
}

#[test]
fn a_package_class_gets_the_packages_own_test_root() {
    let composer = r#"{
        "autoload": { "psr-4": { "App\\": "app/", "Acme\\Billing\\": "packages/billing/src/" } },
        "autoload-dev": { "psr-4": { "Tests\\": "tests/", "Acme\\Billing\\Tests\\": "packages/billing/tests/" } }
    }"#;
    let ledger = (
        "packages/billing/src/Ledger.php",
        "<?php\nnamespace Acme\\Billing;\n\nclass Ledger\n{\n    public function post(): void {}\n}\n",
    );
    let actions = create_test_actions(composer, &[ledger], ledger.0, 3, 8);
    assert_eq!(actions.len(), 1, "{actions:?}");
    let (_, path, text, _) = &actions[0];
    assert_eq!(path, "packages/billing/tests/LedgerTest.php");
    assert!(text.contains("namespace Acme\\Billing\\Tests;\n"), "{text}");
    assert!(text.contains("use Acme\\Billing\\Ledger;\n"), "{text}");
    assert!(!text.contains("declare"), "{text}");
}
