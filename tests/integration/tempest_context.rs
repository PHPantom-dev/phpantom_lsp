//! Inferred Tempest data follows the caller's types and the renderer's scope.

use crate::common::{
    completion_response, create_psr4_workspace, definition_locations, goto_definition_at, hover_on,
    open_php, position_after, position_of, template_diagnostics, workspace_uri,
};
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#;
const BOOK: &str = "<?php namespace App; class Book { public string $title; public function summary(): string {} }";

#[tokio::test]
async fn tempest_context_named_data_types_loops_and_this_without_annotations() {
    let source =
        "<p :foreach=\"$books as $book\">{{ $book->summary() }}</p>\n{{ $this->selected->title }}";
    let caller = r"<?php
namespace App;
use function Tempest\View\view as render;
function page(Book $selected) {
    return render(__DIR__ . '/books.view.php', books: [$selected], selected: $selected);
}";
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Book.php", BOOK),
            ("app/Controller.php", caller),
            ("app/books.view.php", source),
        ],
    );
    let book_uri = workspace_uri(&backend, "app/Book.php");
    open_php(&backend, &book_uri, BOOK).await;
    open_php(
        &backend,
        &workspace_uri(&backend, "app/Controller.php"),
        caller,
    )
    .await;
    let uri = workspace_uri(&backend, "app/books.view.php");
    open_php(&backend, &uri, source).await;
    assert_eq!(template_diagnostics(&backend, &uri), Vec::<String>::new());
    assert!(
        hover_on(&backend, &uri, source, "summary()")
            .await
            .contains("string")
    );
    let pos = position_of(source, "summary");
    let definitions =
        definition_locations(goto_definition_at(&backend, &uri, pos.line, pos.character + 1).await);
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].uri, book_uri);
    let pos = position_after(source, "$this->selected->");
    let response = completion_response(&backend, &uri, pos.line, pos.character, None)
        .await
        .unwrap();
    let items = match response {
        CompletionResponse::CompletionItemList(items) => items,
        CompletionResponse::CompletionList(list) => list.items,
    };
    assert!(items.iter().any(|item| item.label == "title"), "{items:?}");
}

#[tokio::test]
async fn tempest_context_caller_edits_and_removed_calls_refresh_open_views() {
    let source = "{{ $entry->title }}";
    let caller = "<?php use function Tempest\\View\\view; view(__DIR__ . '/page.view.php', entry: new \\App\\Book());";
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Book.php", BOOK),
            ("app/Controller.php", caller),
            ("app/page.view.php", source),
        ],
    );
    let uri = workspace_uri(&backend, "app/page.view.php");
    let caller_uri = workspace_uri(&backend, "app/Controller.php");
    open_php(&backend, &uri, source).await;
    open_php(&backend, &caller_uri, caller).await;
    assert!(
        hover_on(&backend, &uri, source, "title")
            .await
            .contains("string")
    );
    backend
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                text_document_identifier: TextDocumentIdentifier { uri: caller_uri },
                version: 2,
            },
            content_changes: vec![
                TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                    TextDocumentContentChangeWholeDocument {
                        text: "<?php // Removed render".into(),
                    },
                ),
            ],
        })
        .await;
    assert!(
        !backend
            .blade_virtual_php(uri.as_str())
            .unwrap()
            .contains("@var \\App\\Book $entry")
    );
}

#[tokio::test]
async fn tempest_context_spreads_chained_overrides_and_explicit_annotations() {
    let source = "{{ $entry->title }}\n{{ $this->entry->title }}\n<?php /** @var \\App\\Book $override */ ?>{{ $override->summary() }}";
    let caller = r"<?php
use function Tempest\View\view;
$data = ['entry' => 7, 'override' => false, 'bad-key' => 1];
view(__DIR__ . '/page.view.php', ...$data)->data(entry: new \App\Book());
";
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Book.php", BOOK),
            ("app/Controller.php", caller),
            ("app/page.view.php", source),
        ],
    );
    open_php(
        &backend,
        &workspace_uri(&backend, "app/Controller.php"),
        caller,
    )
    .await;
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, source).await;
    assert_eq!(template_diagnostics(&backend, &uri), Vec::<String>::new());
    assert!(
        hover_on(&backend, &uri, source, "title")
            .await
            .contains("string")
    );
    assert!(
        hover_on(&backend, &uri, source, "summary()")
            .await
            .contains("string")
    );
}

#[tokio::test]
async fn tempest_context_custom_view_binds_this_and_only_extracts_explicit_data() {
    let source = "{{ $this->book->title }}\n{{ $this->heading() }}\n{{ $entry->summary() }}";
    let custom = r"<?php
namespace App;
use Tempest\View\View;
use Tempest\View\IsView;
final class BookView implements View {
    use IsView;
    public function __construct(public Book $book) {
        $this->path = __DIR__ . '/page.view.php';
        $this->data = ['entry' => 0];
        $this->data(entry: $book);
        $this->relativeRootPath = __DIR__;
        $this->heading();
        $book->summary();
        $nested = new class { public function __construct() { $this->path = 'other.view.php'; } };
    }
    public function heading(): string { return $this->book->title; }
}";
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Book.php", BOOK),
            ("app/BookView.php", custom),
            ("app/page.view.php", source),
        ],
    );
    open_php(
        &backend,
        &workspace_uri(&backend, "app/BookView.php"),
        custom,
    )
    .await;
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, source).await;
    assert_eq!(template_diagnostics(&backend, &uri), Vec::<String>::new());
    for needle in ["title", "heading()", "summary()"] {
        assert!(
            hover_on(&backend, &uri, source, needle)
                .await
                .contains("string")
        );
    }
    let php = backend.blade_virtual_php(uri.as_str()).unwrap();
    assert!(
        !php.contains("$book = null"),
        "public view properties are not extracted as locals: {php}"
    );
}

#[tokio::test]
async fn tempest_context_component_arguments_and_slots_use_the_component_scope() {
    let source = r#"<?php $outside = new \App\Book(); ?>
<x-card :entry="$selected" label="Selected">
    {{ $local->summary() }}
    <x-slot name="header">{{ $entry->title }} {{ $label }}</x-slot>
</x-card>"#;
    let component = "<h2><x-slot name=\"header\" /></h2>\n<?php $local = $entry; ?>\n<x-slot />\n{{ $entry->title }}";
    let caller = "<?php \\Tempest\\View\\view(__DIR__ . '/page.view.php', selected: new \\App\\Book(), entry: 42);";
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Book.php", BOOK),
            ("app/Controller.php", caller),
            ("app/page.view.php", source),
            ("app/x-card.view.php", component),
        ],
    );
    open_php(
        &backend,
        &workspace_uri(&backend, "app/Controller.php"),
        caller,
    )
    .await;
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, source).await;
    assert_eq!(template_diagnostics(&backend, &uri), Vec::<String>::new());
    for needle in ["summary()", "title"] {
        assert!(
            hover_on(&backend, &uri, source, needle)
                .await
                .contains("string")
        );
    }
    let component_uri = workspace_uri(&backend, "app/x-card.view.php");
    open_php(&backend, &component_uri, component).await;
    assert_eq!(
        template_diagnostics(&backend, &component_uri),
        Vec::<String>::new()
    );
    assert!(
        hover_on(&backend, &component_uri, component, "title")
            .await
            .contains("string")
    );
}

#[tokio::test]
async fn tempest_context_slots_do_not_capture_the_callers_locals() {
    let source = "<?php $outside = new \\App\\Book(); ?>\n<x-card>{{ $outside->title }}</x-card>";
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Book.php", BOOK),
            ("app/page.view.php", source),
            ("app/x-card.view.php", "<x-slot />"),
        ],
    );
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, source).await;
    let diagnostics = template_diagnostics(&backend, &uri);
    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("Undefined variable $outside")),
        "{diagnostics:?}"
    );
}

#[tokio::test]
async fn tempest_context_names_stay_in_the_declaring_namespace() {
    let classes = r"<?php
namespace { class Entry { public function globalOnly(): string {} } }
namespace App { class Entry { public function localOnly(): int {} } }
namespace Other { class Entry { public function otherOnly(): bool {} } }
";
    let caller = r"<?php
namespace App {
    use function Tempest\View\view as render;
    function local(Entry $entry) { return render(__DIR__.'/local.view.php', entry: $entry); }
}
namespace Different {
    use Entry as Imported;
    function globalPage(Imported $entry) { return \Tempest\View\view(__DIR__.'/global.view.php', entry: $entry); }
}
";
    let local = "<?php use Other\\Entry; ?>{{ $entry->localOnly() }}";
    let global = "<?php use Other\\Entry; ?>{{ $entry->globalOnly() }}";
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Classes.php", classes),
            ("app/Controller.php", caller),
            ("app/local.view.php", local),
            ("app/global.view.php", global),
        ],
    );
    open_php(
        &backend,
        &workspace_uri(&backend, "app/Classes.php"),
        classes,
    )
    .await;
    open_php(
        &backend,
        &workspace_uri(&backend, "app/Controller.php"),
        caller,
    )
    .await;
    for (path, source, method, expected) in [
        ("app/local.view.php", local, "localOnly()", "int"),
        ("app/global.view.php", global, "globalOnly()", "string"),
    ] {
        let uri = workspace_uri(&backend, path);
        open_php(&backend, &uri, source).await;
        assert_eq!(template_diagnostics(&backend, &uri), Vec::<String>::new());
        assert!(
            hover_on(&backend, &uri, source, method)
                .await
                .contains(expected)
        );
    }
}

#[tokio::test]
async fn tempest_context_generic_objects_and_custom_data_overrides() {
    let source = "{{ $entry->summary() }}\n{{ $this->entry->summary() }}";
    let custom = r"<?php namespace App;
class Page implements \Tempest\View\View {
    use \Tempest\View\IsView;
    public string $path = 'app/custom.view.php';
    public array $data = ['entry' => 5];
}";
    let caller = r"<?php
use Tempest\View\GenericView as Page;
new Page(__DIR__.'/generic.view.php', data: ['entry' => new \App\Book()]);
(new \App\Page())->data(entry: new \App\Book());
";
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Book.php", BOOK),
            ("app/Page.php", custom),
            ("app/Controller.php", caller),
            ("app/generic.view.php", source),
            ("app/custom.view.php", source),
        ],
    );
    open_php(&backend, &workspace_uri(&backend, "app/Page.php"), custom).await;
    open_php(
        &backend,
        &workspace_uri(&backend, "app/Controller.php"),
        caller,
    )
    .await;
    for path in ["app/generic.view.php", "app/custom.view.php"] {
        let uri = workspace_uri(&backend, path);
        open_php(&backend, &uri, source).await;
        assert_eq!(template_diagnostics(&backend, &uri), Vec::<String>::new());
        assert!(
            hover_on(&backend, &uri, source, "summary()")
                .await
                .contains("string")
        );
    }
}

#[tokio::test]
async fn tempest_context_foreach_component_globals_and_slot_imports() {
    let source = r#"<x-card :foreach="$books as $book" :selected="$book" flag :enabled data-label="Read">
        <?php use App\Book as Imported; /** @var Imported $explicit */ ?>
        {{ $book->title }} {{ $selected->summary() }} {{ $explicit->title }}
        <x-child :entry="$selected" />
    </x-card>"#;
    let card =
        "{{ $book->title }} {{ $selected->summary() }} {{ $dataLabel }} {{ $enabled }}<x-slot />";
    let child = "{{ $entry->title }} {{ $book->title }}";
    let caller =
        "<?php \\Tempest\\View\\view(__DIR__.'/page.view.php', books: [new \\App\\Book()]);";
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Book.php", BOOK),
            ("app/Controller.php", caller),
            ("app/page.view.php", source),
            ("app/x-card.view.php", card),
            ("app/x-child.view.php", child),
        ],
    );
    open_php(
        &backend,
        &workspace_uri(&backend, "app/Controller.php"),
        caller,
    )
    .await;
    for (path, source) in [
        ("app/page.view.php", source),
        ("app/x-card.view.php", card),
        ("app/x-child.view.php", child),
    ] {
        let uri = workspace_uri(&backend, path);
        open_php(&backend, &uri, source).await;
        assert_eq!(
            template_diagnostics(&backend, &uri),
            Vec::<String>::new(),
            "{path}"
        );
        assert!(
            hover_on(&backend, &uri, source, "title")
                .await
                .contains("string")
        );
    }
}

#[tokio::test]
async fn tempest_context_project_component_override_and_invocation_specific_slots() {
    let source = r#"<x-card :entry="new \App\Book()">{{ $local->summary() }}</x-card>
<x-card :entry="new \App\Count()">{{ $local->total() }}</x-card>"#;
    let project = "<?php $local = $entry; ?><x-slot define=\"default\" />";
    let vendor = "<?php $local = false; ?><x-slot />";
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Book.php", BOOK),
            (
                "app/Count.php",
                "<?php namespace App; class Count { public function total(): int {} }",
            ),
            ("app/page.view.php", source),
            ("app/x-card.view.php", project),
            (".gitignore", "/vendor/\n"),
            (
                "vendor/composer/installed.json",
                r#"{"packages":[{"name":"acme/views","autoload":{"psr-4":{"Acme\\":"src/"}},"extra":{"tempest":{"can-discover":true}},"install-path":"../acme/views"}]}"#,
            ),
            (
                "vendor/acme/views/composer.json",
                r#"{"autoload":{"psr-4":{"Acme\\":"src/"}}}"#,
            ),
            ("vendor/acme/views/src/x-card.view.php", vendor),
        ],
    );
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, source).await;
    assert_eq!(template_diagnostics(&backend, &uri), Vec::<String>::new());
    assert!(
        hover_on(&backend, &uri, source, "summary()")
            .await
            .contains("string")
    );
    assert!(
        hover_on(&backend, &uri, source, "total()")
            .await
            .contains("int")
    );
}

#[tokio::test]
async fn tempest_context_inherited_custom_view_and_property_precedence() {
    let declarations = r"<?php namespace App;
interface PageContract extends \Tempest\View\View {}
class BasePage implements PageContract {
    use \Tempest\View\IsView;
    public string $path = 'app/page.view.php';
    public Book $entry;
    public array $data = ['entry' => 123];
}

class Page extends BasePage { public function heading(): string {} }
class Unrelated { public string $path = 'app/page.view.php'; }
";
    let source = "{{ $this->entry->title }}\n{{ $entry + 1 }}";
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Book.php", BOOK),
            ("app/Pages.php", declarations),
            ("app/page.view.php", source),
        ],
    );
    open_php(
        &backend,
        &workspace_uri(&backend, "app/Pages.php"),
        declarations,
    )
    .await;
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, source).await;
    assert_eq!(template_diagnostics(&backend, &uri), Vec::<String>::new());
    assert!(
        hover_on(&backend, &uri, source, "title")
            .await
            .contains("string")
    );
}

#[tokio::test]
async fn tempest_context_real_client_edits_saves_and_closes_refresh_data() {
    use crate::common::lsp_transport::{TransportClient, serve_backend};
    use serde_json::json;
    use std::time::Duration;
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let caller = "<?php class Entry { public string $title; } \\Tempest\\View\\view(__DIR__.'/page.view.php', entry: new Entry());";
    let source = "{{ $entry->title }}";
    std::fs::write(root.join("caller.php"), caller).unwrap();
    std::fs::write(root.join("page.view.php"), source).unwrap();
    let caller_uri = Uri::from_file_path(root.join("caller.php")).unwrap();
    let uri = Uri::from_file_path(root.join("page.view.php")).unwrap();
    let mut client = serve_backend();
    async fn inferred(
        client: &mut TransportClient,
        uri: &Uri,
        id: &mut i64,
        expected: &str,
        present: bool,
    ) {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                *id += 1;
                client
                    .send(
                        "template hover",
                        json!({"jsonrpc":"2.0", "id":*id, "method":"textDocument/hover", "params":{
                            "textDocument":{"uri":uri}, "position":{"line":0,"character":5}
                        }}),
                    )
                    .await;
                let response = client
                    .read_until("hover response", |message| message["id"] == *id)
                    .await;
                if response["result"].to_string().contains(expected) == present {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the template should reflect the latest caller");
    }
    tokio::time::timeout(Duration::from_secs(30), async {
        client.send("initialize", json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "capabilities":{},"rootUri":Uri::from_file_path(&root).unwrap()
        }})).await;
        client.wait_for_id(1).await;
        client.send("initialized", json!({"jsonrpc":"2.0","method":"initialized","params":{}})).await;
        for (uri, text) in [(&caller_uri, caller), (&uri, source)] {
            client.send("didOpen", json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{
                "textDocument":{"uri":uri,"languageId":"php","version":1,"text":text}
            }})).await;
        }
        let mut id = 1;
        inferred(&mut client, &uri, &mut id, "Entry", true).await;
        client.send("didChange", json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{
            "textDocument":{"uri":caller_uri,"version":2},"contentChanges":[{"text":caller.replace("entry: new Entry()", "entry: 42")}]
        }})).await;
        inferred(&mut client, &uri, &mut id, "42", true).await;
        client.send("didSave", json!({"jsonrpc":"2.0","method":"textDocument/didSave","params":{
            "textDocument":{"uri":caller_uri},"text":caller.replace("entry: new Entry()", "entry: false")
        }})).await;
        inferred(&mut client, &uri, &mut id, "false", true).await;
        client.send("didClose", json!({"jsonrpc":"2.0","method":"textDocument/didClose","params":{"textDocument":{"uri":caller_uri}}})).await;
        inferred(&mut client, &uri, &mut id, "Entry", true).await;
        client.send("didOpen", json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{
            "textDocument":{"uri":caller_uri,"languageId":"php","version":3,"text":caller}
        }})).await;
        std::fs::remove_file(root.join("caller.php")).unwrap();
        client.send("didClose", json!({"jsonrpc":"2.0","method":"textDocument/didClose","params":{"textDocument":{"uri":caller_uri}}})).await;
        inferred(&mut client, &uri, &mut id, "Entry", false).await;
        client.send("shutdown", json!({"jsonrpc":"2.0","id":id + 1,"method":"shutdown","params":null})).await;
        client.wait_for_id(id + 1).await;
        client.send("exit", json!({"jsonrpc":"2.0","method":"exit","params":null})).await;
    }).await.unwrap();
}

#[tokio::test]
async fn tempest_context_literal_values_and_shape_keys_cannot_end_generated_annotations() {
    let source = "{{ $data['*/']->title }}\r\n{{ $message }}";
    let caller = r#"<?php \Tempest\View\view(__DIR__.'/page.view.php', data: ['*/' => new \App\Book(), 'safe' => 'ordinary'], message: "*/\r\n\$literal");"#;
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Book.php", BOOK),
            ("app/Controller.php", caller),
            ("app/page.view.php", source),
        ],
    );
    open_php(
        &backend,
        &workspace_uri(&backend, "app/Controller.php"),
        caller,
    )
    .await;
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, source).await;
    assert_eq!(template_diagnostics(&backend, &uri), Vec::<String>::new());
    assert!(
        hover_on(&backend, &uri, source, "title")
            .await
            .contains("string")
    );
    assert!(
        backend
            .blade_virtual_php(uri.as_str())
            .unwrap()
            .contains("\\x2f")
    );
}
