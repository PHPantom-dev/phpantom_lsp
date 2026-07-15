//! Tests for Laravel `Target::macro('name', closure)` recognition.
//!
//! A macro registered in a service provider is surfaced as a real method on
//! the target class: it appears in completion, resolves for member access,
//! and is not flagged as an unknown member.

use crate::common::create_psr4_workspace;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "Illuminate\\Support\\": "vendor/illuminate/Support/"
        }
    }
}"#;

const COLLECTION_PHP: &str = "\
<?php
namespace Illuminate\\Support;
class Collection {
    public function count(): int { return 0; }
}
";

const PROVIDER_PHP: &str = "\
<?php
namespace App\\Providers;
use Illuminate\\Support\\Collection;
class AppServiceProvider {
    public function boot(): void {
        Collection::macro('sumField', function (string $field): float {
            return 0.0;
        });
    }
}
";

fn workspace_files(consumer: &str) -> (phpantom_lsp::Backend, tempfile::TempDir) {
    create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("vendor/illuminate/Support/Collection.php", COLLECTION_PHP),
            ("src/Providers/AppServiceProvider.php", PROVIDER_PHP),
            ("src/Consumer.php", consumer),
        ],
    )
}

async fn open(backend: &phpantom_lsp::Backend, uri: &str, text: &str) {
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: Url::parse(uri).unwrap(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;
}

#[tokio::test]
async fn macro_appears_in_member_completion() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Collection;
class Consumer {
    public function go(Collection $c): void {
        $c->
    }
}
";
    let (backend, _dir) = workspace_files(consumer);
    // Opening the provider registers the macro in the index.
    open(
        &backend,
        "file:///src/Providers/AppServiceProvider.php",
        PROVIDER_PHP,
    )
    .await;
    open(&backend, "file:///src/Consumer.php", consumer).await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse("file:///src/Consumer.php").unwrap(),
                },
                position: Position {
                    line: 5,
                    character: 12,
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
        names.contains(&"sumField"),
        "macro method should appear in completion, got: {names:?}"
    );
    assert!(
        names.contains(&"count"),
        "real methods should still appear, got: {names:?}"
    );
}

#[tokio::test]
async fn macro_call_is_not_flagged_and_resolves() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Collection;
class Consumer {
    public function go(Collection $c): float {
        return $c->sumField('price');
    }
}
";
    let (backend, _dir) = workspace_files(consumer);
    open(
        &backend,
        "file:///src/Providers/AppServiceProvider.php",
        PROVIDER_PHP,
    )
    .await;

    let uri = "file:///src/Consumer.php";
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
        "macro method call should not be flagged as unknown, got: {members:?}"
    );
}

#[tokio::test]
async fn macro_recognized_statically_on_target() {
    // Macros are callable statically too (Macroable::__callStatic), so the
    // synthesized static variant must resolve `Collection::sumField(...)`.
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Collection;
class Consumer {
    public function go(): float {
        return Collection::sumField('price');
    }
}
";
    let (backend, _dir) = workspace_files(consumer);
    open(
        &backend,
        "file:///src/Providers/AppServiceProvider.php",
        PROVIDER_PHP,
    )
    .await;

    let uri = "file:///src/Consumer.php";
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
        "static macro call should resolve, got: {members:?}"
    );
}
