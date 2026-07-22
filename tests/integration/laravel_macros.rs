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

const PROVIDER_RETURNS_THIS_PHP: &str = "\
<?php
namespace App\\Providers;
use Illuminate\\Support\\Collection;
class AppServiceProvider {
    public function boot(): void {
        Collection::macro('asValueLabel', function () {
            return $this;
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
async fn macro_without_return_hint_can_chain_from_this_return() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Collection;
class Consumer {
    public function go(Collection $c): void {
        $c->asValueLabel()->
    }
}
";
    let (backend, _dir) = workspace_files(consumer);
    open(
        &backend,
        "file:///src/Providers/AppServiceProvider.php",
        PROVIDER_RETURNS_THIS_PHP,
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
                    character: 28,
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
        names.contains(&"count"),
        "macro returning $this should preserve chain completions, got: {names:?}"
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
async fn goto_definition_on_macro_call_jumps_to_registration() {
    // Go-to-definition on a macro call lands on the `::macro('name', ...)`
    // registration site, not the target class's own file (where the macro has
    // no declaration).
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
    let provider_uri = "file:///src/Providers/AppServiceProvider.php";
    open(&backend, provider_uri, PROVIDER_PHP).await;
    open(&backend, "file:///src/Consumer.php", consumer).await;

    let result = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse("file:///src/Consumer.php").unwrap(),
                },
                position: Position {
                    line: 5,
                    character: 22,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();

    match result.expect("go-to-definition should resolve a macro call") {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, Url::parse(provider_uri).unwrap());
            // The `Collection::macro('sumField', ...)` line in PROVIDER_PHP.
            assert_eq!(location.range.start.line, 5);
            assert_eq!(location.range.start.character, 27);
        }
        other => panic!("expected a scalar location, got: {other:?}"),
    }
}

#[tokio::test]
async fn vendor_registered_macro_is_surfaced() {
    // A macro registered in a vendor package's service provider (discovered via
    // `extra.laravel.providers` in installed.json) is surfaced as a real
    // method, without the provider file ever being opened.
    let composer_json = r#"{
        "require": { "laravel/framework": "^11.0" },
        "autoload": {
            "psr-4": {
                "App\\": "src/",
                "Illuminate\\Support\\": "vendor/illuminate/Support/"
            }
        }
    }"#;
    let installed_json = r#"{"packages": [{
        "name": "acme/pkg",
        "version": "1.0.0",
        "install-path": "../acme/pkg",
        "autoload": {"psr-4": {"Acme\\Pkg\\": ""}},
        "extra": {"laravel": {"providers": ["Acme\\Pkg\\PkgServiceProvider"]}}
    }]}"#;
    let vendor_provider = "\
<?php
namespace Acme\\Pkg;
use Illuminate\\Support\\Collection;
class PkgServiceProvider {
    public function boot(): void {
        Collection::macro('vendorSum', function (string $field): float {
            return 0.0;
        });
    }
}
";
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
    let (backend, _dir) = create_psr4_workspace(
        composer_json,
        &[
            ("vendor/illuminate/Support/Collection.php", COLLECTION_PHP),
            ("vendor/acme/pkg/PkgServiceProvider.php", vendor_provider),
            ("vendor/composer/installed.json", installed_json),
            ("src/Consumer.php", consumer),
        ],
    );

    // Full indexing pass: the vendor scan indexes the provider and the macro
    // index scans its registrations. The provider is never opened.
    backend.initialized(InitializedParams {}).await;
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
        names.contains(&"vendorSum"),
        "vendor-registered macro should appear in completion, got: {names:?}"
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

#[tokio::test]
async fn provider_same_namespace_helper_reference_is_scanned() {
    let composer_json = r#"{
        "require": { "laravel/framework": "^11.0" },
        "autoload": {
            "psr-4": {
                "App\\": "src/",
                "Illuminate\\Support\\": "vendor/illuminate/Support/"
            }
        }
    }"#;
    let provider = "\
<?php
namespace App\\Providers;
class AppServiceProvider {
    public function boot(): void {
        LocalCollectionMacros::boot();
    }
}
";
    let helper = "\
<?php
namespace App\\Providers;
use Illuminate\\Support\\Collection;
class LocalCollectionMacros {
    public static function boot(): void {
        Collection::macro('sameNamespaceSum', function (string $field): float {
            return 0.0;
        });
    }
}
";
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
    let (backend, _dir) = create_psr4_workspace(
        composer_json,
        &[
            ("vendor/illuminate/Support/Collection.php", COLLECTION_PHP),
            (
                "bootstrap/providers.php",
                "<?php\nreturn [\n    App\\Providers\\AppServiceProvider::class,\n];\n",
            ),
            ("src/Providers/AppServiceProvider.php", provider),
            ("src/Providers/LocalCollectionMacros.php", helper),
            ("src/Consumer.php", consumer),
        ],
    );

    backend.initialized(InitializedParams {}).await;
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
        names.contains(&"sameNamespaceSum"),
        "same-namespace helper reference should be scanned, got: {names:?}"
    );
}

#[tokio::test]
async fn vendor_provider_same_package_helper_reference_is_scanned() {
    let composer_json = r#"{
        "require": { "laravel/framework": "^11.0" },
        "autoload": {
            "psr-4": {
                "App\\": "src/",
                "Illuminate\\Support\\": "vendor/illuminate/Support/"
            }
        }
    }"#;
    let installed_json = r#"{"packages": [{
        "name": "acme/pkg",
        "version": "1.0.0",
        "install-path": "../acme/pkg",
        "autoload": {"psr-4": {"Acme\\Pkg\\": ""}},
        "extra": {"laravel": {"providers": ["Acme\\Pkg\\PkgServiceProvider"]}}
    }]}"#;
    let vendor_provider = "\
<?php
namespace Acme\\Pkg;
use Acme\\Pkg\\Macros\\CollectionMacros;
class PkgServiceProvider {
    public function boot(): void {
        $this->registerMacros();
    }

    protected function registerMacros(): void {
        CollectionMacros::boot();
    }
}
";
    let vendor_helper = "\
<?php
namespace Acme\\Pkg\\Macros;
use Illuminate\\Support\\Collection;
class CollectionMacros {
    public static function boot(): void {
        Collection::macro('vendorDelegatedSum', function (string $field): float {
            return 0.0;
        });
    }
}
";
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
    let (backend, _dir) = create_psr4_workspace(
        composer_json,
        &[
            ("vendor/illuminate/Support/Collection.php", COLLECTION_PHP),
            ("vendor/acme/pkg/PkgServiceProvider.php", vendor_provider),
            ("vendor/acme/pkg/Macros/CollectionMacros.php", vendor_helper),
            ("vendor/composer/installed.json", installed_json),
            ("src/Consumer.php", consumer),
        ],
    );

    backend.initialized(InitializedParams {}).await;
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
        names.contains(&"vendorDelegatedSum"),
        "same-package vendor helper reference should be scanned, got: {names:?}"
    );
}

#[tokio::test]
async fn typed_variable_macro_registration_is_surfaced() {
    let composer_json = r#"{
        "require": { "laravel/framework": "^11.0" },
        "autoload": {
            "psr-4": {
                "App\\": "src/",
                "Illuminate\\Database\\Eloquent\\": "vendor/illuminate/Database/Eloquent/"
            }
        }
    }"#;
    let builder = "\
<?php
namespace Illuminate\\Database\\Eloquent;
class Builder {}
";
    let scope = "\
<?php
namespace App;
use Illuminate\\Database\\Eloquent\\Builder;
class ConfidentialScope {
    public function extend(Builder $query): void {
        $query->macro('withConfidential', function (bool $withConfidential = true): Builder {
            return $this;
        });
    }
}
";
    let consumer = "\
<?php
namespace App;
use Illuminate\\Database\\Eloquent\\Builder;
class Consumer {
    public function go(Builder $query): void {
        $query->
    }
}
";
    let (backend, _dir) = create_psr4_workspace(
        composer_json,
        &[
            ("vendor/illuminate/Database/Eloquent/Builder.php", builder),
            ("src/ConfidentialScope.php", scope),
            ("src/Consumer.php", consumer),
        ],
    );

    open(&backend, "file:///src/ConfidentialScope.php", scope).await;
    open(&backend, "file:///src/Consumer.php", consumer).await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse("file:///src/Consumer.php").unwrap(),
                },
                position: Position {
                    line: 5,
                    character: 16,
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
        names.contains(&"withConfidential"),
        "typed-variable macro registration should be surfaced, got: {names:?}"
    );
}

#[tokio::test]
async fn provider_imported_macro_helper_is_scanned_without_opening_it() {
    let composer_json = r#"{
        "require": { "laravel/framework": "^11.0" },
        "autoload": {
            "psr-4": {
                "App\\": "src/",
                "Illuminate\\Support\\": "vendor/illuminate/Support/"
            }
        }
    }"#;
    let provider = "\
<?php
namespace App\\Providers;
use App\\Macros\\CollectionMacros;
class AppServiceProvider {
    public function boot(): void {
        CollectionMacros::boot();
    }
}
";
    let helper = "\
<?php
namespace App\\Macros;
use Illuminate\\Support\\Collection;
class CollectionMacros {
    public static function boot(): void {
        Collection::macro('delegatedSum', function (string $field): float {
            return 0.0;
        });
    }
}
";
    let unrelated = "\
<?php
namespace App;
use Illuminate\\Support\\Collection;
class Unrelated {
    public static function boot(): void {
        Collection::macro('ignoredMacro', function (): int {
            return 1;
        });
    }
}
";
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
    let (backend, _dir) = create_psr4_workspace(
        composer_json,
        &[
            ("vendor/illuminate/Support/Collection.php", COLLECTION_PHP),
            (
                "bootstrap/providers.php",
                "<?php\nreturn [\n    App\\Providers\\AppServiceProvider::class,\n];\n",
            ),
            ("src/Providers/AppServiceProvider.php", provider),
            ("src/Macros/CollectionMacros.php", helper),
            ("src/Unrelated.php", unrelated),
            ("src/Consumer.php", consumer),
        ],
    );

    backend.initialized(InitializedParams {}).await;
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
        names.contains(&"delegatedSum"),
        "provider-imported macro helper should be scanned, got: {names:?}"
    );
    assert!(
        !names.contains(&"ignoredMacro"),
        "unrelated project files should not seed macro discovery, got: {names:?}"
    );
}

#[tokio::test]
async fn editing_provider_to_reference_new_helper_rebuilds_the_index() {
    let composer_json = r#"{
        "require": { "laravel/framework": "^11.0" },
        "autoload": {
            "psr-4": {
                "App\\": "src/",
                "Illuminate\\Support\\": "vendor/illuminate/Support/"
            }
        }
    }"#;
    let provider_before = "\
<?php
namespace App\\Providers;
class AppServiceProvider {
    public function boot(): void {
    }
}
";
    let provider_after = "\
<?php
namespace App\\Providers;
use App\\Macros\\CollectionMacros;
class AppServiceProvider {
    public function boot(): void {
        CollectionMacros::boot();
    }
}
";
    let helper = "\
<?php
namespace App\\Macros;
use Illuminate\\Support\\Collection;
class CollectionMacros {
    public static function boot(): void {
        Collection::macro('lateSum', function (string $field): float {
            return 0.0;
        });
    }
}
";
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
    let (backend, dir) = create_psr4_workspace(
        composer_json,
        &[
            ("vendor/illuminate/Support/Collection.php", COLLECTION_PHP),
            (
                "bootstrap/providers.php",
                "<?php\nreturn [\n    App\\Providers\\AppServiceProvider::class,\n];\n",
            ),
            ("src/Providers/AppServiceProvider.php", provider_before),
            ("src/Macros/CollectionMacros.php", helper),
            ("src/Consumer.php", consumer),
        ],
    );

    // Initial build: the provider references no helper, so the macro is
    // not discovered.
    backend.initialized(InitializedParams {}).await;

    // Open the provider at its real workspace URI and add a helper
    // reference; the changed reference set must trigger an index rebuild
    // that scans the newly referenced helper.
    let provider_uri = Url::from_file_path(dir.path().join("src/Providers/AppServiceProvider.php"))
        .unwrap()
        .to_string();
    open(&backend, &provider_uri, provider_after).await;

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
        names.contains(&"lateSum"),
        "helper referenced by the edited provider should be scanned, got: {names:?}"
    );
}

#[tokio::test]
async fn hover_on_macro_call_shows_macro_origin() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Collection;
class Consumer {
    public function go(Collection $c): void {
        $c->sumField('price');
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
    open(&backend, "file:///src/Consumer.php", consumer).await;

    let result = backend
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse("file:///src/Consumer.php").unwrap(),
                },
                position: Position {
                    line: 5,
                    character: 14,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap();

    let hover = result.expect("hover should return a result");
    if let HoverContents::Markup(markup) = hover.contents {
        let value = &markup.value;
        assert!(
            value.contains("macro"),
            "hover should show macro origin indicator, got:\n{value}"
        );
        assert!(
            !value.contains("(inferred)"),
            "explicit return type should not show (inferred), got:\n{value}"
        );
    } else {
        panic!("expected HoverContents::Markup");
    }
}

#[tokio::test]
async fn hover_on_macro_with_inferred_return_shows_annotation() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Collection;
class Consumer {
    public function go(Collection $c): void {
        $c->asValueLabel();
    }
}
";
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("vendor/illuminate/Support/Collection.php", COLLECTION_PHP),
            (
                "src/Providers/AppServiceProvider.php",
                PROVIDER_RETURNS_THIS_PHP,
            ),
            ("src/Consumer.php", consumer),
        ],
    );
    open(
        &backend,
        "file:///src/Providers/AppServiceProvider.php",
        PROVIDER_RETURNS_THIS_PHP,
    )
    .await;
    open(&backend, "file:///src/Consumer.php", consumer).await;

    let result = backend
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse("file:///src/Consumer.php").unwrap(),
                },
                position: Position {
                    line: 5,
                    character: 14,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap();

    let hover = result.expect("hover should return a result");
    if let HoverContents::Markup(markup) = hover.contents {
        let value = &markup.value;
        assert!(
            value.contains("macro"),
            "hover should show macro origin indicator, got:\n{value}"
        );
        assert!(
            value.contains("(inferred)"),
            "inferred return type should show (inferred) annotation, got:\n{value}"
        );
    } else {
        panic!("expected HoverContents::Markup");
    }
}

// ─── Macroable::mixin() ─────────────────────────────────────────────────────

const MIXIN_COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "Illuminate\\Support\\": "vendor/illuminate/Support/"
        }
    }
}"#;

const MIXIN_PROVIDER_PHP: &str = "\
<?php
namespace App\\Providers;
use Illuminate\\Support\\Collection;
use App\\Mixins\\CollectionMixin;
class AppServiceProvider {
    public function boot(): void {
        Collection::mixin(new CollectionMixin());
    }
}
";

const COLLECTION_MIXIN_PHP: &str = "\
<?php
namespace App\\Mixins;
use Closure;
class CollectionMixin {
    public function sumField(): Closure {
        return function (string $field): float {
            return 0.0;
        };
    }
}
";

#[tokio::test]
async fn mixin_macro_is_surfaced_on_target() {
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
    let (backend, _dir) = create_psr4_workspace(
        MIXIN_COMPOSER_JSON,
        &[
            ("vendor/illuminate/Support/Collection.php", COLLECTION_PHP),
            (
                "bootstrap/providers.php",
                "<?php\nreturn [\n    App\\Providers\\AppServiceProvider::class,\n];\n",
            ),
            ("src/Providers/AppServiceProvider.php", MIXIN_PROVIDER_PHP),
            ("src/Mixins/CollectionMixin.php", COLLECTION_MIXIN_PHP),
            ("src/Consumer.php", consumer),
        ],
    );

    backend.initialized(InitializedParams {}).await;
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
        "mixin method should be surfaced as a macro, got: {names:?}"
    );
}

#[tokio::test]
async fn mixin_macro_call_is_not_flagged_and_resolves() {
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
    let (backend, _dir) = create_psr4_workspace(
        MIXIN_COMPOSER_JSON,
        &[
            ("vendor/illuminate/Support/Collection.php", COLLECTION_PHP),
            (
                "bootstrap/providers.php",
                "<?php\nreturn [\n    App\\Providers\\AppServiceProvider::class,\n];\n",
            ),
            ("src/Providers/AppServiceProvider.php", MIXIN_PROVIDER_PHP),
            ("src/Mixins/CollectionMixin.php", COLLECTION_MIXIN_PHP),
            ("src/Consumer.php", consumer),
        ],
    );

    backend.initialized(InitializedParams {}).await;

    let uri = "file:///src/Consumer.php";
    open(&backend, uri, consumer).await;
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
        "mixin macro method call should not be flagged as unknown, got: {members:?}"
    );
}

#[tokio::test]
async fn goto_definition_on_mixin_macro_jumps_to_mixin_method() {
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
    let (backend, dir) = create_psr4_workspace(
        MIXIN_COMPOSER_JSON,
        &[
            ("vendor/illuminate/Support/Collection.php", COLLECTION_PHP),
            (
                "bootstrap/providers.php",
                "<?php\nreturn [\n    App\\Providers\\AppServiceProvider::class,\n];\n",
            ),
            ("src/Providers/AppServiceProvider.php", MIXIN_PROVIDER_PHP),
            ("src/Mixins/CollectionMixin.php", COLLECTION_MIXIN_PHP),
            ("src/Consumer.php", consumer),
        ],
    );

    backend.initialized(InitializedParams {}).await;
    open(&backend, "file:///src/Consumer.php", consumer).await;

    let result = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse("file:///src/Consumer.php").unwrap(),
                },
                position: Position {
                    line: 5,
                    character: 22,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();

    let mixin_uri = Url::from_file_path(dir.path().join("src/Mixins/CollectionMixin.php")).unwrap();
    match result.expect("go-to-definition should resolve a mixin macro call") {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, mixin_uri);
            // The `public function sumField(): Closure` line in the mixin file.
            assert_eq!(location.range.start.line, 4);
        }
        other => panic!("expected a scalar location, got: {other:?}"),
    }
}

#[tokio::test]
async fn hover_on_mixin_macro_shows_signature() {
    // Regression guard: a mixin registered in a provider that is listed in
    // bootstrap/providers.php must be discovered, so hovering a mixed-in macro
    // shows its recovered signature (return type + macro origin).
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
    let (backend, _dir) = create_psr4_workspace(
        MIXIN_COMPOSER_JSON,
        &[
            ("vendor/illuminate/Support/Collection.php", COLLECTION_PHP),
            (
                "bootstrap/providers.php",
                "<?php\nreturn [\n    App\\Providers\\AppServiceProvider::class,\n];\n",
            ),
            ("src/Providers/AppServiceProvider.php", MIXIN_PROVIDER_PHP),
            ("src/Mixins/CollectionMixin.php", COLLECTION_MIXIN_PHP),
            ("src/Consumer.php", consumer),
        ],
    );

    backend.initialized(InitializedParams {}).await;
    open(&backend, "file:///src/Consumer.php", consumer).await;

    let result = backend
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse("file:///src/Consumer.php").unwrap(),
                },
                position: Position {
                    line: 5,
                    character: 20,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap();

    let hover = result.expect("hover should resolve a mixin macro call");
    if let HoverContents::Markup(markup) = hover.contents {
        let value = &markup.value;
        assert!(
            value.contains("macro"),
            "hover should show the macro origin indicator, got:\n{value}"
        );
        assert!(
            value.contains("float"),
            "hover should show the recovered return type, got:\n{value}"
        );
    } else {
        panic!("expected HoverContents::Markup");
    }
}

#[tokio::test]
async fn this_using_mixin_macro_resolves_in_completion_and_hover() {
    // A `$this`-using mixin (the closure body calls Collection methods on the
    // rebound `$this`) whose macro returns `array`: it must be surfaced in
    // completion and hover on the target, exactly like a signature-only mixin.
    let collection = "\
<?php
namespace Illuminate\\Support;
class Collection {
    public function count(): int { return 0; }
    public function mapWithKeys(callable $cb): static { return $this; }
    public function all(): array { return []; }
}
";
    let provider = "\
<?php
namespace App\\Providers;
use App\\Support\\CollectionMixin;
use Illuminate\\Support\\Collection;
class DemoServiceProvider {
    public function boot(): void {
        Collection::mixin(new CollectionMixin());
    }
}
";
    let mixin = "\
<?php
namespace App\\Support;
use Closure;
class CollectionMixin {
    public function toAssoc(): Closure {
        return function (string $keyField, string $valueField): array {
            return $this->mapWithKeys(fn (array $item) => [$item[$keyField] => $item[$valueField]])->all();
        };
    }
}
";
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Collection;
class Consumer {
    public function go(Collection $c): array {
        return $c->toAssoc('id', 'name');
    }
}
";
    let (backend, _dir) = create_psr4_workspace(
        MIXIN_COMPOSER_JSON,
        &[
            ("vendor/illuminate/Support/Collection.php", collection),
            (
                "bootstrap/providers.php",
                "<?php\nreturn [\n    App\\Providers\\DemoServiceProvider::class,\n];\n",
            ),
            ("src/Providers/DemoServiceProvider.php", provider),
            ("src/Support/CollectionMixin.php", mixin),
            ("src/Consumer.php", consumer),
        ],
    );

    backend.initialized(InitializedParams {}).await;
    open(&backend, "file:///src/Consumer.php", consumer).await;

    // Hover on `toAssoc` in `$c->toAssoc('id', 'name')` (name starts at col 19).
    let hover = backend
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse("file:///src/Consumer.php").unwrap(),
                },
                position: Position {
                    line: 5,
                    character: 21,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()
        .expect("hover should resolve the mixin macro call");
    let HoverContents::Markup(markup) = hover.contents else {
        panic!("expected markup hover");
    };
    assert!(
        markup.value.contains("macro"),
        "hover should mark the macro origin, got:\n{}",
        markup.value
    );
    assert!(
        markup
            .value
            .contains("toAssoc(string $keyField, string $valueField): array"),
        "hover should show the recovered signature, got:\n{}",
        markup.value
    );
}

#[tokio::test]
async fn mixin_macro_return_type_is_inferred_against_the_mixin_file() {
    // A mixin method whose returned closure has no explicit return type: the
    // type is inferred from the closure body, which must be resolved against
    // the mixin class file's imports, not the registration site's. Here the
    // closure builds a `Widget` imported only in the mixin file, so inference
    // only succeeds when it uses that file's use-map and namespace.
    let collection = "\
<?php
namespace Illuminate\\Support;
class Collection {}
";
    let widget = "\
<?php
namespace App\\Models;
class Widget {
    public function label(): string { return ''; }
}
";
    let provider = "\
<?php
namespace App\\Providers;
use App\\Support\\InferMixin;
use Illuminate\\Support\\Collection;
class DemoServiceProvider {
    public function boot(): void {
        Collection::mixin(new InferMixin());
    }
}
";
    let mixin = "\
<?php
namespace App\\Support;
use Closure;
use App\\Models\\Widget;
class InferMixin {
    public function makeWidget(): Closure {
        return function () {
            return new Widget();
        };
    }
}
";
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Collection;
class Consumer {
    public function go(Collection $c): void {
        $c->makeWidget();
    }
}
";
    let (backend, _dir) = create_psr4_workspace(
        MIXIN_COMPOSER_JSON,
        &[
            ("vendor/illuminate/Support/Collection.php", collection),
            (
                "bootstrap/providers.php",
                "<?php\nreturn [\n    App\\Providers\\DemoServiceProvider::class,\n];\n",
            ),
            ("src/Providers/DemoServiceProvider.php", provider),
            ("src/Support/InferMixin.php", mixin),
            ("src/Models/Widget.php", widget),
            ("src/Consumer.php", consumer),
        ],
    );

    backend.initialized(InitializedParams {}).await;
    open(&backend, "file:///src/Consumer.php", consumer).await;

    // Hover on `makeWidget` in `$c->makeWidget()` (name starts at col 12).
    let hover = backend
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse("file:///src/Consumer.php").unwrap(),
                },
                position: Position {
                    line: 5,
                    character: 14,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()
        .expect("hover should resolve the mixin macro call");
    let HoverContents::Markup(markup) = hover.contents else {
        panic!("expected markup hover");
    };
    assert!(
        markup.value.contains("Widget"),
        "return type should be inferred against the mixin file's imports, got:\n{}",
        markup.value
    );
}

// ─── Trait-based mixin() (Carbon pattern) ───────────────────────────────────

const TRAIT_MIXIN_COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "Carbon\\": "vendor/carbon/Carbon/"
        }
    }
}"#;

const CARBON_IMMUTABLE_PHP: &str = "\
<?php
namespace Carbon;
class CarbonImmutable {
    public function shiftTimezone(string $tz): CarbonImmutable { return $this; }
    public function timezone(string $tz): CarbonImmutable { return $this; }
    public function format(string $f): string { return ''; }
}
";

const CARBON_INTERFACE_PHP: &str = "\
<?php
namespace Carbon;
interface CarbonInterface {}
";

const TRAIT_MIXIN_PROVIDER_PHP: &str = "\
<?php
namespace App\\Providers;
use Carbon\\CarbonImmutable;
use App\\Mixins\\DateMixin;
class AppServiceProvider {
    public function boot(): void {
        CarbonImmutable::mixin(DateMixin::class);
    }
}
";

const DATE_TRAIT_MIXIN_PHP: &str = "\
<?php
namespace App\\Mixins;

use Carbon\\CarbonInterface;

trait DateMixin {
    public function toTz(string $tz, bool $shift = false): CarbonInterface
    {
        return $shift
            ? $this->shiftTimezone($tz)
            : $this->timezone($tz);
    }

    public function toAppTz(bool $shift = false): CarbonInterface
    {
        return $this->toTz(config('app.timezone'), $shift);
    }
}
";

#[tokio::test]
async fn trait_mixin_macro_appears_in_completion() {
    let consumer = "\
<?php
namespace App;
use Carbon\\CarbonImmutable;
class Consumer {
    public function go(CarbonImmutable $date): void {
        $date->
    }
}
";
    let (backend, _dir) = create_psr4_workspace(
        TRAIT_MIXIN_COMPOSER_JSON,
        &[
            (
                "vendor/carbon/Carbon/CarbonImmutable.php",
                CARBON_IMMUTABLE_PHP,
            ),
            (
                "vendor/carbon/Carbon/CarbonInterface.php",
                CARBON_INTERFACE_PHP,
            ),
            (
                "bootstrap/providers.php",
                "<?php\nreturn [\n    App\\Providers\\AppServiceProvider::class,\n];\n",
            ),
            (
                "src/Providers/AppServiceProvider.php",
                TRAIT_MIXIN_PROVIDER_PHP,
            ),
            ("src/Mixins/DateMixin.php", DATE_TRAIT_MIXIN_PHP),
            ("src/Consumer.php", consumer),
        ],
    );

    backend.initialized(InitializedParams {}).await;
    open(&backend, "file:///src/Consumer.php", consumer).await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse("file:///src/Consumer.php").unwrap(),
                },
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
        names.contains(&"toTz"),
        "trait mixin method should appear in completion, got: {names:?}"
    );
    assert!(
        names.contains(&"toAppTz"),
        "trait mixin method should appear in completion, got: {names:?}"
    );
    assert!(
        names.contains(&"format"),
        "real methods should still appear, got: {names:?}"
    );
}

#[tokio::test]
async fn trait_mixin_macro_call_is_not_flagged() {
    let consumer = "\
<?php
namespace App;
use Carbon\\CarbonImmutable;
use Carbon\\CarbonInterface;
class Consumer {
    public function go(CarbonImmutable $date): CarbonInterface {
        return $date->toTz('UTC');
    }
}
";
    let (backend, _dir) = create_psr4_workspace(
        TRAIT_MIXIN_COMPOSER_JSON,
        &[
            (
                "vendor/carbon/Carbon/CarbonImmutable.php",
                CARBON_IMMUTABLE_PHP,
            ),
            (
                "vendor/carbon/Carbon/CarbonInterface.php",
                CARBON_INTERFACE_PHP,
            ),
            (
                "bootstrap/providers.php",
                "<?php\nreturn [\n    App\\Providers\\AppServiceProvider::class,\n];\n",
            ),
            (
                "src/Providers/AppServiceProvider.php",
                TRAIT_MIXIN_PROVIDER_PHP,
            ),
            ("src/Mixins/DateMixin.php", DATE_TRAIT_MIXIN_PHP),
            ("src/Consumer.php", consumer),
        ],
    );

    backend.initialized(InitializedParams {}).await;

    let uri = "file:///src/Consumer.php";
    open(&backend, uri, consumer).await;
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
        "trait mixin method call should not be flagged as unknown, got: {members:?}"
    );
}

#[tokio::test]
async fn carbon_macro_registration_appears_in_completion() {
    let provider = "\
<?php
namespace App\\Providers;
use Carbon\\CarbonImmutable;
class AppServiceProvider {
    public function boot(): void {
        CarbonImmutable::macro('diffFromYear', function (int $year): string {
            return '';
        });
    }
}
";
    let consumer = "\
<?php
namespace App;
use Carbon\\CarbonImmutable;
class Consumer {
    public function go(CarbonImmutable $date): void {
        $date->
    }
}
";
    let (backend, _dir) = create_psr4_workspace(
        TRAIT_MIXIN_COMPOSER_JSON,
        &[
            (
                "vendor/carbon/Carbon/CarbonImmutable.php",
                CARBON_IMMUTABLE_PHP,
            ),
            (
                "bootstrap/providers.php",
                "<?php\nreturn [\n    App\\Providers\\AppServiceProvider::class,\n];\n",
            ),
            ("src/Providers/AppServiceProvider.php", provider),
            ("src/Consumer.php", consumer),
        ],
    );

    backend.initialized(InitializedParams {}).await;
    open(&backend, "file:///src/Consumer.php", consumer).await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse("file:///src/Consumer.php").unwrap(),
                },
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
        names.contains(&"diffFromYear"),
        "Carbon macro should appear in completion, got: {names:?}"
    );
}
