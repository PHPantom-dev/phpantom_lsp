//! Tests for route parameter name completion.
//!
//! The keys of the parameters array of `route('users.show', [...])` are the
//! `{parameters}` of the named route's URI, recovered from the registration
//! the name was declared on.  The same applies to `to_route()`,
//! `signedRoute()`, and `temporarySignedRoute()`.

use crate::common::{create_psr4_workspace, open_php};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

const ROUTES: &str = "\
<?php
use Illuminate\\Support\\Facades\\Route;

Route::get('/', 'home')->name('home');
Route::get('/users/{user}/posts/{post}', 'show')->name('users.posts.show');

Route::prefix('admin')->name('admin.')->group(function () {
    Route::patch('/bakeries/{bakery}/cancel', 'cancel')->name('bakeries.cancel');
});

Route::resource('photos', PhotoController::class);
Route::resource('photos.comments', CommentController::class);
Route::apiResource('categories', CategoryController::class)
    ->parameters(['categories' => 'slug']);
";

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

fn completion_labels(response: Option<CompletionResponse>) -> Vec<String> {
    match response {
        Some(CompletionResponse::Array(items)) => items.into_iter().map(|i| i.label).collect(),
        Some(CompletionResponse::List(list)) => list.items.into_iter().map(|i| i.label).collect(),
        None => Vec::new(),
    }
}

/// Open a workspace holding `ROUTES` plus a consumer file, and complete at the
/// first position after `needle` in the consumer.
async fn labels_after(consumer: &str, needle: &str) -> Vec<String> {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[("routes/web.php", ROUTES), ("src/Runner.php", consumer)],
    );
    backend.initialized(InitializedParams {}).await;

    let routes_uri = Url::from_file_path(dir.path().join("routes/web.php")).unwrap();
    open_php(&backend, &routes_uri, ROUTES).await;

    let uri = Url::from_file_path(dir.path().join("src/Runner.php")).unwrap();
    open_php(&backend, &uri, consumer).await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: position_after(consumer, needle),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();
    completion_labels(result)
}

fn consumer(body: &str) -> String {
    format!(
        "\
<?php
namespace App;
class Runner {{
    public function go(): void
    {{
        {body}
    }}
}}
"
    )
}

#[tokio::test]
async fn parameters_complete_from_the_route_uri() {
    let source = consumer("route('users.posts.show', ['']);");
    let labels = labels_after(&source, "['").await;
    assert_eq!(labels, vec!["user".to_string(), "post".to_string()]);
}

#[tokio::test]
async fn subsequent_parameter_completes() {
    let source = consumer("route('users.posts.show', ['user' => 1, '']);");
    let labels = labels_after(&source, "'user' => 1, '").await;
    assert_eq!(labels, vec!["user".to_string(), "post".to_string()]);
}

/// A comment between the call and the key may hold brackets, parentheses, and
/// apostrophes; none of them belong to the expression.
#[tokio::test]
async fn parameters_complete_below_a_comment() {
    let source = consumer(
        "route('users.posts.show', [   // TODO: check (parameters), don't\n            '',\n        ]);",
    );
    let labels = labels_after(&source, "            '").await;
    assert_eq!(labels, vec!["user".to_string(), "post".to_string()]);
}

#[tokio::test]
async fn group_prefixed_route_offers_its_parameter() {
    let source = consumer("to_route('admin.bakeries.cancel', ['']);");
    let labels = labels_after(&source, "['").await;
    assert_eq!(labels, vec!["bakery".to_string()]);
}

#[tokio::test]
async fn temporary_signed_route_offers_parameters_as_its_third_argument() {
    let source = consumer("URL::temporarySignedRoute('users.posts.show', $expiration, ['']);");
    let labels = labels_after(&source, "$expiration, ['").await;
    assert_eq!(labels, vec!["user".to_string(), "post".to_string()]);
}

#[tokio::test]
async fn parameterless_route_offers_no_parameters() {
    let source = consumer("route('home', ['']);");
    assert!(labels_after(&source, "['").await.is_empty());
}

#[tokio::test]
async fn the_route_name_argument_still_completes_names() {
    let source = consumer("route('');");
    let labels = labels_after(&source, "route('").await;
    assert!(
        labels.contains(&"users.posts.show".to_string()),
        "expected the route names, got {labels:?}"
    );
}

#[tokio::test]
async fn resource_route_offers_its_derived_parameter() {
    let source = consumer("route('photos.show', ['']);");
    let labels = labels_after(&source, "['").await;
    assert_eq!(labels, vec!["photo".to_string()]);
}

#[tokio::test]
async fn nested_resource_route_offers_every_derived_parameter() {
    let source = consumer("route('photos.comments.update', ['']);");
    let labels = labels_after(&source, "['").await;
    assert_eq!(labels, vec!["photo".to_string(), "comment".to_string()]);
}

#[tokio::test]
async fn resource_parameters_override_the_derived_name() {
    let source = consumer("route('categories.show', ['']);");
    let labels = labels_after(&source, "['").await;
    assert_eq!(labels, vec!["slug".to_string()]);
}

#[tokio::test]
async fn resource_index_route_offers_no_parameters() {
    let source = consumer("route('photos.index', ['']);");
    assert!(labels_after(&source, "['").await.is_empty());
}
