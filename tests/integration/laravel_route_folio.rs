//! Tests for Laravel Folio page-derived routes.
//!
//! Folio (https://laravel.com/docs/folio) registers a route for every Blade
//! page under a mounted directory, with no `Route::` call anywhere — a page
//! names itself via `Laravel\Folio\name()`.  These routes must resolve for
//! `route()` the same way a conventional `->name()` declaration does:
//! completion, hover, diagnostics, and go-to-definition.

use crate::common::{create_psr4_workspace, open_php};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

/// The framework's own `withRouting(pages: ...)` forwards to
/// `Folio::route()` internally, which is why this file — not a service
/// provider — is where a Folio mount most commonly gets registered.
const BOOTSTRAP_APP_PHP: &str = "\
<?php
use Illuminate\\Foundation\\Application;

return Application::configure(basePath: dirname(__DIR__))
    ->withRouting(
        web: __DIR__.'/../routes/web.php',
        pages: __DIR__.'/../resources/views/pages',
        commands: __DIR__.'/../routes/console.php',
    )->create();
";

const EXPLORE_PAGE: &str = "\
<?php
use function Laravel\\Folio\\name;

name('explore');
?>
<div>Explore</div>
";

const SERVICE_PHP: &str = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        route('explore');
        route('nope');
    }
}
";

fn workspace() -> (phpantom_lsp::Backend, tempfile::TempDir) {
    create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("bootstrap/app.php", BOOTSTRAP_APP_PHP),
            ("resources/views/pages/explore.blade.php", EXPLORE_PAGE),
            ("src/Services/Service.php", SERVICE_PHP),
        ],
    )
}

/// Position of the cursor immediately after the first occurrence of `needle`.
fn position_after(content: &str, needle: &str) -> Position {
    let idx = content.find(needle).expect("needle not found") + needle.len();
    let mut line = 0u32;
    let mut character = 0u32;
    for (i, ch) in content.char_indices() {
        if i == idx {
            break;
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += 1;
        }
    }
    Position { line, character }
}

fn definition_uri(response: &GotoDefinitionResponse) -> &Url {
    match response {
        GotoDefinitionResponse::Scalar(location) => &location.uri,
        GotoDefinitionResponse::Array(locations) => &locations[0].uri,
        GotoDefinitionResponse::Link(links) => &links[0].target_uri,
    }
}

fn completion_labels(response: Option<CompletionResponse>) -> Vec<String> {
    match response {
        Some(CompletionResponse::Array(items)) => items.into_iter().map(|i| i.label).collect(),
        Some(CompletionResponse::List(list)) => list.items.into_iter().map(|i| i.label).collect(),
        None => Vec::new(),
    }
}

#[tokio::test]
async fn a_named_folio_page_is_not_reported_as_an_unknown_route() {
    let (backend, dir) = workspace();
    backend.initialized(InitializedParams {}).await;

    let uri = Url::from_file_path(dir.path().join("src/Services/Service.php")).unwrap();
    open_php(&backend, &uri, SERVICE_PHP).await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), SERVICE_PHP, &mut diags);

    let messages: Vec<&String> = diags
        .iter()
        .filter(
            |d| matches!(&d.code, Some(NumberOrString::String(s)) if s == "invalid_laravel_route"),
        )
        .map(|d| &d.message)
        .collect();

    assert_eq!(
        messages.len(),
        1,
        "only the genuinely missing route should be flagged, got: {messages:?}"
    );
    assert!(
        messages[0].contains("nope"),
        "the flagged route should be the missing one, got: {}",
        messages[0]
    );
}

#[tokio::test]
async fn goto_definition_on_a_folio_route_name_lands_on_the_page() {
    let (backend, dir) = workspace();
    backend.initialized(InitializedParams {}).await;

    let service_uri = Url::from_file_path(dir.path().join("src/Services/Service.php")).unwrap();
    open_php(&backend, &service_uri, SERVICE_PHP).await;

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: service_uri.clone(),
            },
            position: position_after(SERVICE_PHP, "route('expl"),
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend
        .goto_definition(params)
        .await
        .unwrap()
        .expect("route('explore') should resolve to the Folio page that names it");
    let target_uri = definition_uri(&result);
    assert!(
        target_uri
            .as_str()
            .ends_with("resources/views/pages/explore.blade.php"),
        "should jump to the Folio page, got: {}",
        target_uri
    );
}

#[tokio::test]
async fn hover_on_a_folio_route_name_names_the_page() {
    let (backend, dir) = workspace();
    backend.initialized(InitializedParams {}).await;

    let uri = Url::from_file_path(dir.path().join("src/Services/Service.php")).unwrap();
    open_php(&backend, &uri, SERVICE_PHP).await;

    let hover = backend
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: position_after(SERVICE_PHP, "route('expl"),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()
        .expect("route('explore') should hover");
    let text = match hover.contents {
        HoverContents::Markup(markup) => markup.value,
        other => panic!("expected markup hover, got {other:?}"),
    };

    assert!(
        text.contains("resources/views/pages/explore.blade.php"),
        "hover should name the Folio page, not fall back to the bare label or a mangled path, got: {text}"
    );
}

#[tokio::test]
async fn completion_inside_route_offers_the_folio_page_name() {
    let (backend, dir) = workspace();
    backend.initialized(InitializedParams {}).await;

    let uri = Url::from_file_path(dir.path().join("src/Services/Service.php")).unwrap();
    open_php(&backend, &uri, SERVICE_PHP).await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: position_after(SERVICE_PHP, "route('"),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    let labels = completion_labels(result);
    assert!(
        labels.iter().any(|l| l == "explore"),
        "expected 'explore' among route() completions, got: {labels:?}"
    );
}
