//! Go-to-definition on PHPUnit code-coverage metadata.
//!
//! Covers both the current attribute form (`#[CoversClass]`,
//! `#[CoversMethod]`, `#[CoversFunction]`, `#[CoversTrait]` and their
//! `Uses*` counterparts) and the deprecated annotation form (`@covers`,
//! `@coversDefaultClass`, `@uses`), including the bare `::functionName`
//! shape that names a global function.

use crate::common::{create_psr4_workspace, create_test_backend, open_php};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

async fn goto_definition(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    line: u32,
    character: u32,
) -> Option<GotoDefinitionResponse> {
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    backend.goto_definition(params).await.unwrap()
}

fn assert_line(response: Option<GotoDefinitionResponse>, expected_line: u32) {
    match response {
        Some(GotoDefinitionResponse::Scalar(location)) => {
            assert_eq!(
                location.range.start.line, expected_line,
                "expected line {}, got {}",
                expected_line, location.range.start.line
            );
        }
        other => panic!("expected a single location, got: {:?}", other),
    }
}

const COMPOSER: &str =
    r#"{"autoload":{"psr-4":{"App\\":"src/"}},"autoload-dev":{"psr-4":{"Tests\\":"tests/"}}}"#;

/// `src/Calculator.php` — line 4 is the class, line 6 the method.
const SUBJECT: &str = r#"<?php
namespace App;

class Calculator
{
    public function add(int $a, int $b): int
    {
        return $a + $b;
    }
}
"#;

#[tokio::test]
async fn covers_class_attribute_jumps_to_class() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[("src/Calculator.php", SUBJECT)]);
    let test_path = dir.path().join("tests/CalculatorTest.php");
    let uri = Url::from_file_path(&test_path).unwrap();
    let source = r#"<?php
namespace Tests;

use App\Calculator;
use PHPUnit\Framework\Attributes\CoversClass;

#[CoversClass(Calculator::class)]
class CalculatorTest
{
}
"#;
    open_php(&backend, &uri, source).await;

    // Cursor on `Calculator` inside `#[CoversClass(Calculator::class)]`.
    let response = goto_definition(&backend, &uri, 6, 18).await;
    assert_line(response, 3);
}

#[tokio::test]
async fn covers_annotation_jumps_to_class() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[("src/Calculator.php", SUBJECT)]);
    let test_path = dir.path().join("tests/CalculatorTest.php");
    let uri = Url::from_file_path(&test_path).unwrap();
    let source = r#"<?php
namespace Tests;

use App\Calculator;

/**
 * @covers Calculator
 */
class CalculatorTest
{
}
"#;
    open_php(&backend, &uri, source).await;

    let response = goto_definition(&backend, &uri, 6, 13).await;
    assert_line(response, 3);
}

#[tokio::test]
async fn covers_annotation_with_method_jumps_to_method() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[("src/Calculator.php", SUBJECT)]);
    let test_path = dir.path().join("tests/CalculatorTest.php");
    let uri = Url::from_file_path(&test_path).unwrap();
    let source = r#"<?php
namespace Tests;

use App\Calculator;

/**
 * @covers Calculator::add
 */
class CalculatorTest
{
}
"#;
    open_php(&backend, &uri, source).await;

    let response = goto_definition(&backend, &uri, 6, 25).await;
    assert_line(response, 5);
}

#[tokio::test]
async fn covers_fqn_annotation_jumps_to_class() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[("src/Calculator.php", SUBJECT)]);
    let test_path = dir.path().join("tests/CalculatorTest.php");
    let uri = Url::from_file_path(&test_path).unwrap();
    let source = r#"<?php
namespace Tests;

/**
 * @covers \App\Calculator::add()
 */
class CalculatorTest
{
}
"#;
    open_php(&backend, &uri, source).await;

    // The class portion of the reference.
    assert_line(goto_definition(&backend, &uri, 4, 16).await, 3);
    // The method portion, with the trailing `()` PHPUnit allows.
    assert_line(goto_definition(&backend, &uri, 4, 29).await, 5);
}

#[tokio::test]
async fn uses_annotation_jumps_to_class() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[("src/Calculator.php", SUBJECT)]);
    let test_path = dir.path().join("tests/CalculatorTest.php");
    let uri = Url::from_file_path(&test_path).unwrap();
    let source = r#"<?php
namespace Tests;

use App\Calculator;

/**
 * @uses Calculator
 */
class CalculatorTest
{
}
"#;
    open_php(&backend, &uri, source).await;

    assert_line(goto_definition(&backend, &uri, 6, 11).await, 3);
}

#[tokio::test]
async fn covers_bare_function_annotation_jumps_to_function() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let source = r#"<?php

function helper(): void {}

/**
 * @covers ::helper
 */
class HelperTest
{
}
"#;
    open_php(&backend, &uri, source).await;

    let response = goto_definition(&backend, &uri, 5, 14).await;
    assert_line(response, 2);
}

#[tokio::test]
async fn covers_default_class_makes_bare_member_a_method() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[("src/Calculator.php", SUBJECT)]);
    let test_path = dir.path().join("tests/CalculatorTest.php");
    let uri = Url::from_file_path(&test_path).unwrap();
    let source = r#"<?php
namespace Tests;

use App\Calculator;

/**
 * @coversDefaultClass Calculator
 * @covers ::add
 */
class CalculatorTest
{
}
"#;
    open_php(&backend, &uri, source).await;

    // `@coversDefaultClass` names the class itself.
    assert_line(goto_definition(&backend, &uri, 6, 24).await, 3);
    // `::add` resolves against that default, not as a global function.
    assert_line(goto_definition(&backend, &uri, 7, 13).await, 5);
}

#[tokio::test]
async fn covers_default_class_reaches_method_docblocks() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[("src/Calculator.php", SUBJECT)]);
    let test_path = dir.path().join("tests/CalculatorTest.php");
    let uri = Url::from_file_path(&test_path).unwrap();
    let source = r#"<?php
namespace Tests;

/**
 * @coversDefaultClass \App\Calculator
 */
class CalculatorTest
{
    /**
     * @covers ::add
     */
    public function testAdd(): void
    {
    }
}
"#;
    open_php(&backend, &uri, source).await;

    assert_line(goto_definition(&backend, &uri, 9, 17).await, 5);
}

#[tokio::test]
async fn covers_method_attribute_jumps_to_method() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[("src/Calculator.php", SUBJECT)]);
    let test_path = dir.path().join("tests/CalculatorTest.php");
    let uri = Url::from_file_path(&test_path).unwrap();
    let source = r#"<?php
namespace Tests;

use App\Calculator;
use PHPUnit\Framework\Attributes\CoversMethod;

#[CoversMethod(Calculator::class, 'add')]
class CalculatorTest
{
}
"#;
    open_php(&backend, &uri, source).await;

    // Cursor inside the `'add'` literal.
    assert_line(goto_definition(&backend, &uri, 6, 36).await, 5);
}

#[tokio::test]
async fn covers_function_attribute_jumps_to_function() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let source = r#"<?php

use PHPUnit\Framework\Attributes\CoversFunction;

function helper(): void {}

#[CoversFunction('helper')]
class HelperTest
{
}
"#;
    open_php(&backend, &uri, source).await;

    assert_line(goto_definition(&backend, &uri, 6, 20).await, 4);
}

#[tokio::test]
async fn covers_class_attribute_accepts_a_string_target() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[("src/Calculator.php", SUBJECT)]);
    let test_path = dir.path().join("tests/CalculatorTest.php");
    let uri = Url::from_file_path(&test_path).unwrap();
    let source = r#"<?php
namespace Tests;

#[\PHPUnit\Framework\Attributes\CoversClass('App\Calculator')]
class CalculatorTest
{
}
"#;
    open_php(&backend, &uri, source).await;

    assert_line(goto_definition(&backend, &uri, 3, 55).await, 3);
}

#[tokio::test]
async fn a_projects_own_covers_method_attribute_is_left_alone() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[("src/Calculator.php", SUBJECT)]);
    let test_path = dir.path().join("tests/CalculatorTest.php");
    let uri = Url::from_file_path(&test_path).unwrap();
    // No `use PHPUnit\Framework\Attributes\…` import, so the short name is
    // some other project's attribute and its string argument is not a
    // coverage target.
    let source = r#"<?php
namespace Tests;

use App\Calculator;

#[CoversMethod(Calculator::class, 'add')]
class CalculatorTest
{
}
"#;
    open_php(&backend, &uri, source).await;

    assert_eq!(goto_definition(&backend, &uri, 5, 36).await, None);
}

#[tokio::test]
async fn a_visibility_selector_leaves_the_class_navigable() {
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[("src/Calculator.php", SUBJECT)]);
    let test_path = dir.path().join("tests/CalculatorTest.php");
    let uri = Url::from_file_path(&test_path).unwrap();
    let source = r#"<?php
namespace Tests;

use App\Calculator;

/**
 * @covers Calculator::<public>
 */
class CalculatorTest
{
}
"#;
    open_php(&backend, &uri, source).await;

    // The class still navigates.
    assert_line(goto_definition(&backend, &uri, 6, 13).await, 3);
    // PHPUnit 4's visibility selector is not a member name.
    assert_eq!(goto_definition(&backend, &uri, 6, 27).await, None);
}

#[tokio::test]
async fn renaming_a_method_updates_its_coverage_metadata() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///rename.php").unwrap();
    let source = r#"<?php

use PHPUnit\Framework\Attributes\CoversMethod;

class Calculator
{
    public function add(int $a, int $b): int
    {
        return $a + $b;
    }
}

/**
 * @covers Calculator::add
 */
#[CoversMethod(Calculator::class, 'add')]
class CalculatorTest
{
}
"#;
    open_php(&backend, &uri, source).await;

    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 6,
                character: 22,
            },
        },
        new_name: "sum".to_string(),
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    let edit = backend
        .rename(params)
        .await
        .unwrap()
        .expect("a rename edit");
    let edits = &edit.changes.expect("changes")[&uri];
    let lines: Vec<u32> = edits.iter().map(|e| e.range.start.line).collect();
    assert!(
        lines.contains(&13) && lines.contains(&15),
        "expected the annotation (line 13) and the attribute (line 15) to be renamed, got {:?}",
        lines
    );
}
