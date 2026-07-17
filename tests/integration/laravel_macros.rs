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
            assert_eq!(location.range.start.character, 26);
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
