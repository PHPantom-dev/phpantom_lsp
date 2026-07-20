//! Tests for the core Illuminate contract to concrete-class binding.
//!
//! Several Laravel contracts (`Illuminate\Contracts\*` interfaces) are
//! type-hinted throughout application code, but the object bound at runtime
//! is a concrete class that uses the `Macroable` trait (and therefore has a
//! `__call` magic method).  Because the contract declares no `__call`,
//! member access on a contract-typed value used to raise a false
//! "method not found" for anything the concrete resolves dynamically.
//!
//! Injecting the default concrete as a `@mixin` on the contract merges the
//! concrete's members (including `__call`) into the contract, so completion
//! surfaces them and unknown-member diagnostics are suppressed.

use crate::common::create_psr4_workspace;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "Illuminate\\Contracts\\View\\": "vendor/illuminate/contracts/View/",
            "Illuminate\\View\\": "vendor/illuminate/view/"
        }
    }
}"#;

/// The `View` contract: no `extends()`, no `__call`.
const CONTRACT_VIEW: &str = "\
<?php
namespace Illuminate\\Contracts\\View;
interface View {
    public function name(): string;
}
";

/// The default concrete `View`: has a `render()` method and the `__call`
/// magic method that `Macroable` provides at runtime.
const CONCRETE_VIEW: &str = "\
<?php
namespace Illuminate\\View;
class View {
    public function render(): string { return ''; }
    public function __call(string $method, array $parameters): mixed { return null; }
}
";

#[tokio::test]
async fn contract_concrete_mixin_merges_members_for_completion() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Contracts\\View\\View;
class Renderer {
    public function handle(View $view): void {
        $view->
    }
}
";
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("vendor/illuminate/contracts/View/View.php", CONTRACT_VIEW),
            ("vendor/illuminate/view/View.php", CONCRETE_VIEW),
            ("src/Renderer.php", consumer),
        ],
    );

    let uri = Url::parse("file:///src/Renderer.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: consumer.to_string(),
            },
        })
        .await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 5,
                    character: 15,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    let items = match result.expect("completion should return results") {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    let names: Vec<&str> = items
        .iter()
        .filter_map(|i| i.filter_text.as_deref())
        .collect();

    assert!(
        names.contains(&"name"),
        "contract's own method should be present, got: {names:?}"
    );
    assert!(
        names.contains(&"render"),
        "concrete's method should be merged via the contract mixin, got: {names:?}"
    );
}

#[tokio::test]
async fn contract_concrete_mixin_suppresses_unknown_method() {
    // `extends()` is registered as a macro on the concrete at runtime and is
    // dispatched through `__call`.  With the concrete bound as a mixin, the
    // contract inherits `__call`, so calling `extends()` on a contract-typed
    // value must not raise an unknown-member diagnostic.
    let consumer = "\
<?php
namespace App;
use Illuminate\\Contracts\\View\\View;
class Renderer {
    public function handle(View $view): string {
        $view->extends('layouts.default');
        return $view->render();
    }
}
";
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("vendor/illuminate/contracts/View/View.php", CONTRACT_VIEW),
            ("vendor/illuminate/view/View.php", CONCRETE_VIEW),
            ("src/Renderer.php", consumer),
        ],
    );

    let uri = "file:///src/Renderer.php";
    backend.update_ast(uri, consumer);
    let mut diagnostics = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, consumer, &mut diagnostics);

    let members: Vec<&str> = diagnostics
        .iter()
        .filter(|d| {
            d.code
                .as_ref()
                .is_some_and(|c| matches!(c, NumberOrString::String(s) if s == "unknown_member"))
        })
        .map(|d| d.message.as_str())
        .collect();

    assert!(
        members.is_empty(),
        "contract-typed value should not raise unknown-member diagnostics once the concrete mixin is bound, got: {members:?}"
    );
}
