//! Completion inside the method-name string of array callables:
//! `[Class::class, 'method']` and `[$obj, 'method']`.

use crate::common::{create_psr4_workspace, create_test_backend};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

async fn complete_at(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    text: &str,
    line: u32,
    character: u32,
) -> Vec<CompletionItem> {
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    match backend.completion(completion_params).await.unwrap() {
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

fn find_method<'a>(items: &'a [CompletionItem], name: &str) -> Option<&'a CompletionItem> {
    items.iter().find(|i| {
        i.kind == Some(CompletionItemKind::METHOD) && i.filter_text.as_deref() == Some(name)
    })
}

/// `[$this, '|']` offers the enclosing class's instance methods.
#[tokio::test]
async fn completes_this_callable_methods() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///this_callable.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Controller {\n",
        "    public function indexPage(): void {}\n",
        "    public function showPage(): void {}\n",
        "    public function register(): void {\n",
        "        $route = [$this, ''];\n",
        "    }\n",
        "}\n",
    );
    // Cursor inside the empty string on line 5 (the `''`).
    let line = text.lines().position(|l| l.contains("$route")).unwrap() as u32;
    let col = text
        .lines()
        .nth(line as usize)
        .unwrap()
        .find("'']")
        .unwrap() as u32
        + 1;

    let items = complete_at(&backend, &uri, text, line, col).await;
    let labels = method_labels(&items);
    assert!(labels.contains(&"indexPage".to_string()), "got {labels:?}");
    assert!(labels.contains(&"showPage".to_string()), "got {labels:?}");
    assert!(labels.contains(&"register".to_string()), "got {labels:?}");
}

/// The insert text is the plain method name, not a snippet with parens.
#[tokio::test]
async fn inserts_plain_method_name() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///plain_insert.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Controller {\n",
        "    public function handle(string $request): void {}\n",
        "    public function register(): void {\n",
        "        $route = [$this, ''];\n",
        "    }\n",
        "}\n",
    );
    let line = text.lines().position(|l| l.contains("$route")).unwrap() as u32;
    let col = text
        .lines()
        .nth(line as usize)
        .unwrap()
        .find("'']")
        .unwrap() as u32
        + 1;

    let items = complete_at(&backend, &uri, text, line, col).await;
    let handle = find_method(&items, "handle").expect("handle method");
    assert_eq!(handle.insert_text.as_deref(), Some("handle"));
    assert_ne!(handle.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

/// A partial prefix filters the offered methods.
#[tokio::test]
async fn filters_by_prefix() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///prefix_filter.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Controller {\n",
        "    public function indexPage(): void {}\n",
        "    public function showPage(): void {}\n",
        "    public function register(): void {\n",
        "        $route = [$this, 'ind'];\n",
        "    }\n",
        "}\n",
    );
    let line = text.lines().position(|l| l.contains("$route")).unwrap() as u32;
    // Cursor after `ind`.
    let col = text
        .lines()
        .nth(line as usize)
        .unwrap()
        .find("ind']")
        .unwrap() as u32
        + 3;

    let items = complete_at(&backend, &uri, text, line, col).await;
    let labels = method_labels(&items);
    assert!(labels.contains(&"indexPage".to_string()), "got {labels:?}");
    assert!(!labels.contains(&"showPage".to_string()), "got {labels:?}");
}

/// A plain data array (`['foo', 'bar']`) must not trigger method completion.
#[tokio::test]
async fn rejects_plain_string_array() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///plain_array.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Controller {\n",
        "    public function register(): void {\n",
        "        $data = ['foo', ''];\n",
        "    }\n",
        "}\n",
    );
    let line = text.lines().position(|l| l.contains("$data")).unwrap() as u32;
    let col = text
        .lines()
        .nth(line as usize)
        .unwrap()
        .find("'']")
        .unwrap() as u32
        + 1;

    let items = complete_at(&backend, &uri, text, line, col).await;
    assert!(
        method_labels(&items).is_empty(),
        "plain string array should not offer methods, got {:?}",
        method_labels(&items)
    );
}

/// `[Class::class, '|']` resolves the class across files (PSR-4) and
/// offers its methods, including inherited ones.
#[tokio::test]
async fn completes_class_const_callable_cross_file() {
    let composer = r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#;
    let controller = concat!(
        "<?php\n",
        "namespace App;\n",
        "class BaseController {\n",
        "    public function middleware(): void {}\n",
        "}\n",
    );
    let index = concat!(
        "<?php\n",
        "namespace App;\n",
        "class IndexController extends BaseController {\n",
        "    public function indexPage(): void {}\n",
        "}\n",
    );
    let routes = concat!(
        "<?php\n",
        "namespace App;\n",
        "$route = [IndexController::class, ''];\n",
    );
    let (backend, _dir) = create_psr4_workspace(
        composer,
        &[
            ("src/BaseController.php", controller),
            ("src/IndexController.php", index),
            ("src/routes.php", routes),
        ],
    );
    let uri = Url::parse("file:///routes.php").unwrap();

    let line = routes.lines().position(|l| l.contains("$route")).unwrap() as u32;
    let col = routes
        .lines()
        .nth(line as usize)
        .unwrap()
        .find("'']")
        .unwrap() as u32
        + 1;

    let items = complete_at(&backend, &uri, routes, line, col).await;
    let labels = method_labels(&items);
    assert!(labels.contains(&"indexPage".to_string()), "got {labels:?}");
    assert!(
        labels.contains(&"middleware".to_string()),
        "inherited method should be offered, got {labels:?}"
    );
}
