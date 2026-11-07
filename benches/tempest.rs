use criterion::{Criterion, criterion_group, criterion_main};
use phpantom_lsp::Backend;
use std::{fs, hint::black_box};
use tower_lsp_server::{LanguageServer, gen_lsp_types::*};

const BOOK: &str = include_str!("../examples/tempest/app/Book.php");
const VIEW: &str = include_str!("../examples/tempest/app/books.view.php");
const COMPONENT: &str = include_str!("../examples/tempest/app/components/x-card.view.php");
const CONTROLLER: &str = include_str!("../examples/tempest/app/BookController.php");

/// A catalogue large enough to exercise discovery alongside PHP completion.
const VIEWS: usize = 200;

fn position_after(content: &str, needle: &str) -> Position {
    let offset = content.rfind(needle).unwrap() + needle.len();
    let before = &content[..offset];
    Position {
        line: before.matches('\n').count() as u32,
        character: before.rsplit('\n').next().unwrap().encode_utf16().count() as u32,
    }
}

fn bench_tempest(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    fs::create_dir_all(root.join("app/components")).unwrap();
    fs::write(
        root.join("composer.json"),
        r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
    )
    .unwrap();
    for n in 0..VIEWS {
        fs::write(root.join(format!("app/page{n}.view.php")), VIEW).unwrap();
        fs::write(
            root.join(format!("app/components/x-panel-{n}.view.php")),
            COMPONENT,
        )
        .unwrap();
    }
    let (mappings, _) = phpantom_lsp::composer::parse_composer_json(&root);
    let backend = Backend::new_test_with_workspace(root.clone(), mappings);
    for (path, content) in [
        ("app/Book.php", BOOK),
        ("app/BookController.php", CONTROLLER),
        ("app/books.view.php", VIEW),
        ("app/components/x-card.view.php", COMPONENT),
    ] {
        fs::write(root.join(path), content).unwrap();
        runtime.block_on(backend.did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: Uri::from_file_path(root.join(path)).unwrap(),
                language_id: "php".to_string().into(),
                version: 1,
                text: content.to_string(),
            },
        }));
    }
    let view_uri = Uri::from_file_path(root.join("app/books.view.php")).unwrap();
    let controller_uri = Uri::from_file_path(root.join("app/BookController.php")).unwrap();
    let mut group = c.benchmark_group("tempest");
    for (name, uri, content, needle, expected) in [
        ("member_completion", &view_uri, VIEW, "$book->", "summary"),
        ("component_completion", &view_uri, VIEW, "<x-", "x-card"),
        (
            "attribute_completion",
            &view_uri,
            VIEW,
            "<x-card :",
            ":title",
        ),
        (
            "view_path_completion",
            &controller_uri,
            CONTROLLER,
            "__DIR__ . '/",
            "/books.view.php",
        ),
    ] {
        let params = CompletionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: position_after(content, needle),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        };
        let request = || {
            runtime
                .block_on(backend.completion(params.clone()))
                .unwrap()
        };
        let items = match request().expect("completion must return items") {
            CompletionResponse::CompletionItemList(items) => items,
            CompletionResponse::CompletionList(list) => list.items,
        };
        assert!(
            items
                .iter()
                .any(|item| item.label.split('(').next() == Some(expected)),
            "{name}: missing {expected}: {items:?}"
        );
        group.bench_function(name, |b| b.iter(|| black_box(request())));
    }

    let params = DefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: view_uri.clone(),
            },
            position: position_after(VIEW, "<x-ca"),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let request = || {
        runtime
            .block_on(backend.goto_definition(params.clone()))
            .unwrap()
    };
    let expected = Uri::from_file_path(root.join("app/components/x-card.view.php")).unwrap();
    let found = match request() {
        Some(DefinitionResponse::Definition(Definition::Location(location))) => vec![location],
        Some(DefinitionResponse::Definition(Definition::LocationList(locations))) => locations,
        other => panic!("component definition: {other:?}"),
    };
    assert!(found.iter().any(|location| location.uri == expected));
    group.bench_function("component_definition", |b| b.iter(|| black_box(request())));

    // Keep the source changing so the benchmark measures lowering and parsing,
    // rather than the unchanged-document cache.
    for count in [10, 100] {
        let source = format!(
            "<?php use App\\Book; /** @var list<Book> $books */ ?>\n{}",
            "<p :foreach=\"$books as $book\" :title=\"$book->title\">{{ $book->summary() }}</p>\n"
                .repeat(count)
        );
        let changed = format!("{source}\n");
        let uri = Uri::from_file_path(root.join(format!("app/edit{count}.view.php"))).unwrap();
        backend.update_ast(uri.as_str(), &source);
        assert!(
            backend
                .blade_virtual_php(uri.as_str())
                .unwrap()
                .contains("foreach")
        );
        let mut alternate = false;
        group.bench_function(format!("edit_{count}_loops"), |b| {
            b.iter(|| {
                alternate = !alternate;
                black_box(backend.update_ast(
                    uri.as_str(),
                    black_box(if alternate { &changed } else { &source }),
                ))
            })
        });
    }
    let changed_controller = CONTROLLER.replace("books: [new Book('The PHP Book')]", "books: []");
    assert_ne!(changed_controller, CONTROLLER);
    let mut alternate = false;
    group.bench_function("caller_edit_context", |b| {
        b.iter(|| {
            alternate = !alternate;
            runtime.block_on(backend.did_change(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    text_document_identifier: TextDocumentIdentifier {
                        uri: controller_uri.clone(),
                    },
                    version: 2,
                },
                content_changes: vec![
                    TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                        TextDocumentContentChangeWholeDocument {
                            text: if alternate {
                                changed_controller.clone()
                            } else {
                                CONTROLLER.to_owned()
                            },
                        },
                    ),
                ],
            }));
            black_box(backend.blade_virtual_php(view_uri.as_str()));
        })
    });
    group.finish();
}

criterion_group!(benches, bench_tempest);
criterion_main!(benches);
