use crate::common::{create_psr4_workspace, create_test_backend, open_php};
use phpantom_lsp::Backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

async fn prepare_at(
    backend: &Backend,
    uri: &Url,
    line: u32,
    character: u32,
) -> Vec<CallHierarchyItem> {
    let params = CallHierarchyPrepareParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    };

    backend
        .prepare_call_hierarchy(params)
        .await
        .unwrap()
        .unwrap_or_default()
}

async fn incoming(backend: &Backend, item: &CallHierarchyItem) -> Vec<CallHierarchyIncomingCall> {
    backend
        .incoming_calls(CallHierarchyIncomingCallsParams {
            item: item.clone(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .unwrap_or_default()
}

async fn outgoing(backend: &Backend, item: &CallHierarchyItem) -> Vec<CallHierarchyOutgoingCall> {
    backend
        .outgoing_calls(CallHierarchyOutgoingCallsParams {
            item: item.clone(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .unwrap_or_default()
}

fn sorted(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names
}

// ─── Prepare ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn prepare_answers_on_a_method_declaration() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///prepare.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Worker {\n",
        "    public function run(): void {}\n",
        "}\n",
    );
    open_php(&backend, &uri, text).await;

    let items = prepare_at(&backend, &uri, 2, 22).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "run");
    assert_eq!(items[0].kind, SymbolKind::METHOD);
    assert_eq!(items[0].detail.as_deref(), Some("Worker"));
    assert_eq!(items[0].uri, uri);
}

#[tokio::test]
async fn prepare_answers_from_a_call_site() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///from_call_site.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Worker {\n",
        "    public function leaf(): void {}\n",
        "    public function run(): void { $this->leaf(); }\n",
        "}\n",
    );
    open_php(&backend, &uri, text).await;

    // Cursor on `leaf` inside `$this->leaf()`; the enclosing method wins
    // because the cursor is inside `run`'s body.
    let items = prepare_at(&backend, &uri, 3, 43).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "run");
}

#[tokio::test]
async fn prepare_declines_outside_any_callable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///outside.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Worker {\n",
        "    public string $name = 'x';\n",
        "}\n",
    );
    open_php(&backend, &uri, text).await;

    assert!(prepare_at(&backend, &uri, 2, 20).await.is_empty());
}

// ─── Cross-file calls ───────────────────────────────────────────────────────

#[tokio::test]
async fn outgoing_calls_cross_a_psr4_file_boundary() {
    let composer = r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#;
    let files = &[(
        "src/Mailer.php",
        "<?php\nnamespace App;\nclass Mailer {\n    public function send(string $to): void {}\n}\n",
    )];
    let (backend, _dir) = create_psr4_workspace(composer, files);

    let uri = Url::parse("file:///main.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Mailer;\n",
        "class Service {\n",
        "    public function notify(Mailer $mailer): void {\n",
        "        $mailer->send('a@b.c');\n",
        "    }\n",
        "}\n",
    );
    open_php(&backend, &uri, text).await;

    let items = prepare_at(&backend, &uri, 3, 22).await;
    assert_eq!(items.len(), 1, "should prepare on notify()");

    let calls = outgoing(&backend, &items[0]).await;
    let names = sorted(calls.iter().map(|call| call.to.name.clone()).collect());
    assert_eq!(
        names,
        vec!["send".to_string()],
        "notify() should call Mailer::send in another file"
    );
    assert!(
        calls[0].to.uri.as_str().ends_with("src/Mailer.php"),
        "callee should point at the declaring file, got {}",
        calls[0].to.uri
    );
    // The call site range belongs to the caller's file, not the callee's.
    assert_eq!(calls[0].from_ranges.len(), 1);
    assert_eq!(calls[0].from_ranges[0].start.line, 4);
}

#[tokio::test]
async fn incoming_calls_cross_a_file_boundary() {
    let backend = create_test_backend();
    let mailer_uri = Url::parse("file:///Mailer.php").unwrap();
    let service_uri = Url::parse("file:///Service.php").unwrap();

    open_php(
        &backend,
        &mailer_uri,
        concat!(
            "<?php\n",
            "class Mailer {\n",
            "    public function send(string $to): void {}\n",
            "}\n",
        ),
    )
    .await;
    open_php(
        &backend,
        &service_uri,
        concat!(
            "<?php\n",
            "class Service {\n",
            "    public function notify(Mailer $mailer): void {\n",
            "        $mailer->send('a@b.c');\n",
            "    }\n",
            "}\n",
        ),
    )
    .await;

    let items = prepare_at(&backend, &mailer_uri, 2, 22).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "send");

    let calls = incoming(&backend, &items[0]).await;
    let names: Vec<String> = calls.iter().map(|call| call.from.name.clone()).collect();
    assert_eq!(names, vec!["notify".to_string()]);
    assert_eq!(calls[0].from.uri, service_uri);
    assert_eq!(calls[0].from_ranges.len(), 1);
    assert_eq!(calls[0].from_ranges[0].start.line, 3);
}

// ─── Item data roundtrip ────────────────────────────────────────────────────

#[tokio::test]
async fn an_item_from_a_previous_answer_can_be_expanded_again() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///roundtrip.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Worker {\n",
        "    public function leaf(): void {}\n",
        "    public function middle(): void { $this->leaf(); }\n",
        "    public function top(): void { $this->middle(); }\n",
        "}\n",
    );
    open_php(&backend, &uri, text).await;

    let top = prepare_at(&backend, &uri, 4, 22).await.remove(0);
    let from_top = outgoing(&backend, &top).await;
    assert_eq!(from_top.len(), 1);
    assert_eq!(from_top[0].to.name, "middle");

    // Expanding the item the previous answer returned must work without
    // another prepare, which is how a client walks the tree.
    let from_middle = outgoing(&backend, &from_top[0].to).await;
    assert_eq!(from_middle.len(), 1);
    assert_eq!(from_middle[0].to.name, "leaf");

    let into_middle = incoming(&backend, &from_top[0].to).await;
    assert_eq!(into_middle.len(), 1);
    assert_eq!(into_middle[0].from.name, "top");
}

#[tokio::test]
async fn a_foreign_item_is_declined_rather_than_guessed_at() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///foreign.php").unwrap();
    open_php(
        &backend,
        &uri,
        "<?php\nclass Worker {\n    public function run(): void {}\n}\n",
    )
    .await;

    let foreign = CallHierarchyItem {
        name: "run".to_string(),
        kind: SymbolKind::METHOD,
        tags: None,
        detail: None,
        uri: uri.clone(),
        range: Range::new(Position::new(2, 20), Position::new(2, 23)),
        selection_range: Range::new(Position::new(2, 20), Position::new(2, 23)),
        data: None,
    };

    assert!(incoming(&backend, &foreign).await.is_empty());
    assert!(outgoing(&backend, &foreign).await.is_empty());
}

// ─── Recursion ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn a_recursive_function_is_its_own_caller() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///recursive.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function countdown(int $n): void {\n",
        "    if ($n > 0) { countdown($n - 1); }\n",
        "}\n",
    );
    open_php(&backend, &uri, text).await;

    let item = prepare_at(&backend, &uri, 1, 12).await.remove(0);
    assert_eq!(item.kind, SymbolKind::FUNCTION);

    let calls = incoming(&backend, &item).await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].from.name, "countdown");

    let out = outgoing(&backend, &item).await;
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].to.name, "countdown");
}
