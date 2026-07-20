//! Integration tests for Laravel route controller method resolution.
//!
//! Tests go-to-definition, completion, and references for method-name
//! strings inside `Route::controller(X::class)->group(fn(){…})`.

use crate::common::create_test_backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

async fn open_file(backend: &phpantom_lsp::Backend, uri: &Url, text: &str) {
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;
}

async fn goto_def(
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

async fn complete_at(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    text: &str,
    line: u32,
    character: u32,
) -> Vec<CompletionItem> {
    open_file(backend, uri, text).await;
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };
    match backend.completion(params).await.unwrap() {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    }
}

fn method_labels(items: &[CompletionItem]) -> Vec<String> {
    items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
        .filter_map(|i| i.filter_text.clone())
        .collect()
}

// ─── Go-to-definition ───────────────────────────────────────────────────────

#[tokio::test]
async fn goto_definition_controller_method_in_group() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///routes.php").unwrap();
    let text = concat!(
        "<?php\n",                                                             // 0
        "class WorkItemController {\n",                                        // 1
        "    public function cancel() {}\n",                                   // 2
        "    public function complete() {}\n",                                 // 3
        "}\n",                                                                 // 4
        "\n",                                                                  // 5
        "Route::controller(WorkItemController::class)->group(function () {\n", // 6
        "    Route::patch('cancel', 'cancel');\n",                             // 7
        "});\n",                                                               // 8
    );
    open_file(&backend, &uri, text).await;

    // Click on 'cancel' (the method name) on line 7.
    let line_text = text.lines().nth(7).unwrap();
    let cancel_pos = line_text.rfind("cancel')").unwrap() as u32;
    let result = goto_def(&backend, &uri, 7, cancel_pos + 1).await;
    assert!(
        result.is_some(),
        "Should resolve 'cancel' to WorkItemController::cancel()"
    );
    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "cancel() is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn goto_definition_chained_route_with_name() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///routes_chained.php").unwrap();
    let text = concat!(
        "<?php\n",                                                       // 0
        "class MyController {\n",                                        // 1
        "    public function store() {}\n",                              // 2
        "}\n",                                                           // 3
        "\n",                                                            // 4
        "Route::controller(MyController::class)->group(function () {\n", // 5
        "    Route::post('store', 'store')->name('store');\n",           // 6
        "});\n",                                                         // 7
    );
    open_file(&backend, &uri, text).await;

    let line_text = text.lines().nth(6).unwrap();
    let store_pos = line_text.find("'store')->name").unwrap() as u32 + 1;
    let result = goto_def(&backend, &uri, 6, store_pos).await;
    assert!(
        result.is_some(),
        "Should resolve 'store' even when chained with ->name()"
    );
    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.range.start.line, 2);
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn goto_definition_controller_after_prefix() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///routes_prefix.php").unwrap();
    let text = concat!(
        "<?php\n",                                                                          // 0
        "class ItemController {\n",                                                         // 1
        "    public function show() {}\n",                                                  // 2
        "}\n",                                                                              // 3
        "\n",                                                                               // 4
        "Route::prefix('items')->controller(ItemController::class)->group(function () {\n", // 5
        "    Route::get('/{id}', 'show');\n",                                               // 6
        "});\n",                                                                            // 7
    );
    open_file(&backend, &uri, text).await;

    let line_text = text.lines().nth(6).unwrap();
    let show_pos = line_text.find("show").unwrap() as u32;
    let result = goto_def(&backend, &uri, 6, show_pos + 1).await;
    assert!(
        result.is_some(),
        "Should resolve 'show' when ->controller() follows ->prefix()"
    );
    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.range.start.line, 2);
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Completion ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn completes_controller_methods_in_group() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///routes_complete.php").unwrap();
    let text = concat!(
        "<?php\n",                                                         // 0
        "class TaskController {\n",                                        // 1
        "    public function index() {}\n",                                // 2
        "    public function store() {}\n",                                // 3
        "    public function destroy() {}\n",                              // 4
        "}\n",                                                             // 5
        "\n",                                                              // 6
        "Route::controller(TaskController::class)->group(function () {\n", // 7
        "    Route::get('/', '');\n",                                      // 8
        "});\n",                                                           // 9
    );
    let line = 8u32;
    let line_text = text.lines().nth(line as usize).unwrap();
    let col = line_text.find("'');").unwrap() as u32 + 1; // inside empty ''

    let items = complete_at(&backend, &uri, text, line, col).await;
    let labels = method_labels(&items);
    assert!(
        labels.contains(&"index".to_string()),
        "Should offer 'index'; got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"store".to_string()),
        "Should offer 'store'; got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"destroy".to_string()),
        "Should offer 'destroy'; got: {:?}",
        labels
    );
}

#[tokio::test]
async fn completion_filters_by_prefix() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///routes_filter.php").unwrap();
    let text = concat!(
        "<?php\n",                                                        // 0
        "class ApiController {\n",                                        // 1
        "    public function start() {}\n",                               // 2
        "    public function stop() {}\n",                                // 3
        "    public function index() {}\n",                               // 4
        "}\n",                                                            // 5
        "\n",                                                             // 6
        "Route::controller(ApiController::class)->group(function () {\n", // 7
        "    Route::patch('start', 'st');\n",                             // 8
        "});\n",                                                          // 9
    );
    let line = 8u32;
    let line_text = text.lines().nth(line as usize).unwrap();
    let col = line_text.find("st')").unwrap() as u32 + 2; // after "st"

    let items = complete_at(&backend, &uri, text, line, col).await;
    let labels = method_labels(&items);
    assert!(
        labels.contains(&"start".to_string()),
        "Should offer 'start'; got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"stop".to_string()),
        "Should offer 'stop'; got: {:?}",
        labels
    );
    assert!(
        !labels.contains(&"index".to_string()),
        "'index' should be filtered out by prefix 'st'; got: {:?}",
        labels
    );
}

#[tokio::test]
async fn completion_inserts_plain_method_name() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///routes_insert.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Ctrl {\n",
        "    public function doWork() {}\n",
        "}\n",
        "\n",
        "Route::controller(Ctrl::class)->group(function () {\n",
        "    Route::post('work', '');\n",
        "});\n",
    );
    let line = 6u32;
    let line_text = text.lines().nth(line as usize).unwrap();
    let col = line_text.find("'');").unwrap() as u32 + 1;

    let items = complete_at(&backend, &uri, text, line, col).await;
    let item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("doWork"));
    assert!(item.is_some(), "Should offer doWork");
    let item = item.unwrap();
    // insert_text should be the plain name, not a snippet.
    assert_eq!(
        item.insert_text.as_deref(),
        Some("doWork"),
        "Should insert plain method name, not a snippet"
    );
    assert!(
        item.insert_text_format.is_none(),
        "Should not have snippet format"
    );
}

#[tokio::test]
async fn no_completion_outside_controller_group() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///routes_no_ctrl.php").unwrap();
    let text = concat!("<?php\n", "Route::get('/', '');\n",);
    let line = 1u32;
    let line_text = text.lines().nth(line as usize).unwrap();
    let col = line_text.find("'');").unwrap() as u32 + 1;

    let items = complete_at(&backend, &uri, text, line, col).await;
    assert!(
        items.is_empty(),
        "Should not offer completions outside a controller group; got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}
