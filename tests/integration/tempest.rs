//! Tempest's template expressions use the same PHP engine as source files.

use crate::common::{
    completion_response, create_test_backend, definition_locations, goto_definition_at, hover_on,
    open_php, position_after, position_of, template_diagnostics,
};
use tower_lsp_server::gen_lsp_types::*;

const MODELS: &str = r"<?php
namespace App;
class Book {
    public string $title;
    public function summary(): string { return $this->title; }
}
class BookView {
    /** @var list<Book> */
    public array $books;
    public ?Book $selected;
}
";

#[tokio::test]
async fn tempest_php_expressions_complete_and_navigate_with_imports_and_annotations() {
    let backend = create_test_backend();
    let model_uri = uri("src/Book.php");
    open_php(&backend, &model_uri, MODELS).await;
    let uri = uri("src/books.view.php");
    let source = r#"<?php
use App\Book;
/** @var Book $book */
?>
<article :title="$book->summary()">{{ $book->title }}</article>
<p>{!! $book->summary() !!}</p>
<p><?= $book->title ?></p>
"#;
    open_php(&backend, &uri, source).await;
    assert!(template_diagnostics(&backend, &uri).is_empty());
    for needle in ["$book->summary", "$book->title"] {
        let pos = position_of(source, needle);
        let response = goto_definition_at(&backend, &uri, pos.line, pos.character + 8).await;
        let locations = definition_locations(response);
        assert_eq!(locations.len(), 1, "{needle}");
        assert_eq!(locations[0].uri, model_uri);
    }
    assert!(
        hover_on(&backend, &uri, source, "title }}</article>")
            .await
            .contains("string")
    );
    let pos = position_after(source, "$book->");
    let response = completion_response(&backend, &uri, pos.line, pos.character, None).await;
    let items = match response.unwrap() {
        CompletionResponse::CompletionItemList(items) => items,
        CompletionResponse::CompletionList(list) => list.items,
    };
    assert!(items.iter().any(|item| item.label == "title"));
    assert!(items.iter().any(|item| item.label.starts_with("summary")));
}

#[tokio::test]
async fn tempest_foreach_and_conditionals_feed_the_shared_type_engine() {
    let backend = create_test_backend();
    let model_uri = uri("src/Book.php");
    open_php(&backend, &model_uri, MODELS).await;
    let uri = uri("src/books.view.php");
    let source = r#"<?php /** @var \App\BookView $this */ ?>
<ul>
    <li :foreach="$this->books as $book" :if="$book->title !== ''">
        {{ $book->summary() }}
    </li>
</ul>
<span :if="$this->selected !== null">{{ $this->selected->title }}</span>
<span :else>No selection</span>
"#;
    open_php(&backend, &uri, source).await;
    assert_eq!(template_diagnostics(&backend, &uri), Vec::<String>::new());
    assert!(
        hover_on(&backend, &uri, source, "summary()")
            .await
            .contains("string")
    );
    let position = position_of(source, "$book->summary");
    let locations = definition_locations(
        goto_definition_at(&backend, &uri, position.line, position.character + 8).await,
    );
    assert_eq!(locations.len(), 1);
    assert_eq!(locations[0].uri, model_uri);
}

#[tokio::test]
async fn tempest_comments_and_escaped_attributes_are_not_php() {
    let backend = create_test_backend();
    let uri = uri("src/comment.view.php");
    let source = r#"{{-- {{ unknown_function() }} <p :if="$missing"> --}}
<!-- {{ unknown_function() }} <p :if="$missing"> -->
<p ::class="frontend ? active : inactive">Hello</p>
"#;
    open_php(&backend, &uri, source).await;
    assert_eq!(template_diagnostics(&backend, &uri), Vec::<String>::new());
}

#[tokio::test]
async fn tempest_unicode_and_crlf_diagnostic_ranges_point_to_the_original_member() {
    let backend = create_test_backend();
    let model_uri = uri("src/Book.php");
    open_php(&backend, &model_uri, MODELS).await;
    let uri = uri("src/books.view.php");
    let source = "<?php /** @var \\App\\Book $book */ ?>\r\n<p>📚 {{ $book->missing() }}</p>\r\n";
    open_php(&backend, &uri, source).await;
    let php = backend.blade_virtual_php(uri.as_str()).unwrap();
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), &php, &mut diagnostics);
    let diagnostic = diagnostics.iter().find(|d| matches!(&d.code, Some(Code::String(code)) if code == "unknown_method" || code == "unknown_member"))
        .unwrap_or_else(|| panic!("expected a missing member: {diagnostics:?}"));
    assert_eq!(diagnostic.range.start, position_of(source, "missing"));
    assert_eq!(diagnostic.range.end, position_after(source, "missing"));
}

fn uri(relative: &str) -> Uri {
    Uri::from_file_path(
        std::env::temp_dir()
            .join("phpantom-tempest-test")
            .join(relative),
    )
    .unwrap()
}

#[tokio::test]
async fn tempest_view_paths_are_not_judged_as_laravel_view_names() {
    use crate::common::{LARAVEL_APP_COMPOSER, create_psr4_workspace, with_code, workspace_uri};
    let (backend, _dir) = create_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[("app/page.view.php", "<p>Tempest</p>")],
    );
    let uri = workspace_uri(&backend, "app/Controller.php");
    let source = "<?php\nuse function Tempest\\View\\view;\nview('page.view.php');\nview(__DIR__ . '/page.view.php');\n\\view('missing-laravel-view');\n";
    open_php(&backend, &uri, source).await;
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), source, &mut diagnostics);
    let invalid_views = with_code(diagnostics, "invalid_laravel_view");
    assert_eq!(invalid_views.len(), 1, "{invalid_views:?}");
    assert_eq!(
        invalid_views[0].range.start,
        position_of(source, "missing-laravel-view")
    );
    for needle in ["page.view.php'", "/page.view.php'"] {
        let position = position_of(source, needle);
        assert_eq!(
            definition_locations(
                goto_definition_at(&backend, &uri, position.line, position.character + 1).await
            )
            .len(),
            1
        );
    }
}

#[tokio::test]
async fn tempest_components_complete_navigate_and_refresh_after_creation() {
    use crate::common::{create_psr4_workspace, workspace_uri};
    use tower_lsp_server::LanguageServer;
    let component = "<?php /** @var string $bookTitle */ ?>\n<h1>{{ $bookTitle }}</h1><x-slot />";
    let (backend, dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        &[("app/Parts/x-card.view.php", component)],
    );
    let uri = workspace_uri(&backend, "app/index.view.php");
    let source = "<x-card :book-title=\"'Hello'\">Text</x-card>";
    open_php(&backend, &uri, source).await;
    for needle in ["<x-card", "</x-card"] {
        let pos = position_after(source, needle);
        let locations =
            definition_locations(goto_definition_at(&backend, &uri, pos.line, pos.character).await);
        assert_eq!(locations.len(), 1);
        let expected = dunce::canonicalize(dir.path().join("app/Parts/x-card.view.php")).unwrap();
        assert_eq!(locations[0].uri, Uri::from_file_path(expected).unwrap());
    }
    let pos = position_after(source, "<x-ca");
    let items = completion_response(&backend, &uri, pos.line, pos.character, None)
        .await
        .unwrap();
    assert!(labels(items).contains(&"x-card".to_string()));
    let pos = position_after(source, "<x-card :book-");
    let items = completion_response(&backend, &uri, pos.line, pos.character, None)
        .await
        .unwrap();
    assert!(labels(items).contains(&":book-title".to_string()));

    let created = dir.path().join("app/x-caption.view.php");
    std::fs::write(&created, "<span><x-slot /></span>").unwrap();
    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: Uri::from_file_path(dunce::canonicalize(&created).unwrap()).unwrap(),
                kind: FileChangeType::Created,
            }],
        })
        .await;
    let pos = position_after(source, "<x-ca");
    let items = completion_response(&backend, &uri, pos.line, pos.character, None)
        .await
        .unwrap();
    assert!(labels(items).contains(&"x-caption".to_string()));
}

#[tokio::test]
async fn tempest_view_paths_resolve_imported_helpers_and_dir_concatenations() {
    use crate::common::{create_psr4_workspace, workspace_uri};
    let (backend, dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/","Views\\":"views/"}}}"#,
        &[
            ("app/Pages/home.view.php", "<h1>Home</h1>"),
            ("views/shared.view.php", "<p>Shared</p>"),
        ],
    );
    let uri = workspace_uri(&backend, "app/Pages/Controller.php");
    let source = r#"<?php
namespace App\Pages;
use function Tempest\View\view as render;
render('home.view.php');
render(__DIR__ . '/home.view.php');
render(path: 'shared.view.php');
\Tempest\View\view('shared.view.php');
function view(string $path) {}
view('home.view.php');
"#;
    open_php(&backend, &uri, source).await;
    for (line, column, expected) in [
        (3, 13, "app/Pages/home.view.php"),
        (4, 25, "app/Pages/home.view.php"),
        (5, 20, "views/shared.view.php"),
        (6, 27, "views/shared.view.php"),
    ] {
        let locations =
            definition_locations(goto_definition_at(&backend, &uri, line, column).await);
        assert_eq!(locations.len(), 1, "line {line}");
        assert_eq!(
            locations[0].uri,
            Uri::from_file_path(dunce::canonicalize(dir.path().join(expected)).unwrap()).unwrap()
        );
    }
    assert!(definition_locations(goto_definition_at(&backend, &uri, 8, 12).await).is_empty());
    let pos = position_after(source, "render('ho");
    let items = completion_response(&backend, &uri, pos.line, pos.character, None)
        .await
        .unwrap();
    let labels = labels(items);
    assert!(labels.contains(&"home.view.php".to_string()), "{labels:?}");
}

#[tokio::test]
async fn tempest_forelse_isset_and_void_elements_preserve_php_control_flow() {
    let backend = create_test_backend();
    let model_uri = uri("src/Book.php");
    open_php(&backend, &model_uri, MODELS).await;
    let uri = uri("src/books.view.php");
    let source = r#"<?php
/** @var list<\App\Book> $books */
/** @var \App\Book|null $selected */
?>
<div :foreach="$books as $book">{{ $book->summary() }}</div>
<p :forelse>No books</p>
<input :if="$selected !== null" :value="$selected->title">
<p :isset="$selected">{{ $selected->summary() }}</p>
<p :isset="$first || $second">Present</p>
"#;
    open_php(&backend, &uri, source).await;
    assert_eq!(template_diagnostics(&backend, &uri), Vec::<String>::new());
}

fn labels(response: CompletionResponse) -> Vec<String> {
    match response {
        CompletionResponse::CompletionItemList(items) => items,
        CompletionResponse::CompletionList(list) => list.items,
    }
    .into_iter()
    .map(|item| item.label)
    .collect()
}

#[tokio::test]
async fn tempest_attributes_before_directives_keep_their_scope_and_edit_ranges() {
    let backend = create_test_backend();
    open_php(&backend, &uri("src/Book.php"), MODELS).await;
    let uri = uri("src/attributes.view.php");
    let source = "<?php /** @var list<\\App\\Book> $books */ ?>\r\n<article\r\n :title=\"$book->summary()\"\r\n :foreach=\"$books as $book\"\r\n>{{ $book->title }}</article>";
    open_php(&backend, &uri, source).await;
    assert_eq!(template_diagnostics(&backend, &uri), Vec::<String>::new());
    assert!(
        hover_on(&backend, &uri, source, "summary()")
            .await
            .contains("string")
    );
    let pos = position_after(source, "$book->su");
    let response = completion_response(&backend, &uri, pos.line, pos.character, None)
        .await
        .unwrap();
    let items = match response {
        CompletionResponse::CompletionItemList(items) => items,
        CompletionResponse::CompletionList(list) => list.items,
    };
    let item = items
        .iter()
        .find(|item| item.label.starts_with("summary"))
        .unwrap();
    if let Some(CompletionItemTextEdit::TextEdit(edit)) = &item.text_edit {
        assert_eq!(edit.range.start, position_of(source, "summary"));
    }
}

#[tokio::test]
async fn tempest_native_php_comments_strings_and_heredocs_keep_their_boundaries() {
    let backend = create_test_backend();
    open_php(&backend, &uri("src/Book.php"), MODELS).await;
    let uri = uri("src/php.view.php");
    let source = r#"<?xml version="1.0"?>
<?php
/** @var \App\Book $book */
$text = <<<'HTML'
?> {{ not_php() }} <x-fake>
HTML;
$label = '?>'; // ?>
<div <?= $book->title ?> class="<?= $book->summary() ?>" :title="$book->title">
    {{ '}}' . $book->title }}
    {!! '!!}' . $book->summary() !!}
    <?php echo $book->summary() // comment ?>
</div>
<?php if (true): ?><?= $book->title ?><?php endif ?>
"#;
    open_php(&backend, &uri, source).await;
    assert_eq!(template_diagnostics(&backend, &uri), Vec::<String>::new());
    assert!(
        hover_on(&backend, &uri, source, "summary()")
            .await
            .contains("string")
    );
}

#[tokio::test]
async fn tempest_incomplete_expressions_still_complete() {
    for tail in [
        "{{ $book->",
        "{!! $book->",
        "<div :title=\"$book->",
        "<?= $book->",
    ] {
        let backend = create_test_backend();
        open_php(&backend, &uri("src/Book.php"), MODELS).await;
        let uri = uri("src/edit.view.php");
        let source = format!("<?php /** @var \\App\\Book $book */ ?>\n{tail}");
        open_php(&backend, &uri, &source).await;
        let pos = position_after(&source, "$book->");
        let items = completion_response(&backend, &uri, pos.line, pos.character, None)
            .await
            .unwrap();
        assert!(labels(items).contains(&"title".to_string()), "{tail}");
    }
}

#[tokio::test]
async fn tempest_markup_completes_directives_without_php_symbols() {
    let backend = create_test_backend();
    let uri = uri("src/markup.view.php");
    for (source, needle, expected) in [
        ("<div ::class>", "::cl", None),
        ("<div></div >", "</div ", None),
        ("<div :for>", ":for", Some(":foreach")),
        ("<div >", "<div ", Some(":if")),
        ("<x-slot />", "<x-s", Some("x-slot")),
        ("<div ::class=\"client\">", "client", None),
        ("<div title=\"ordinary\">", "ordinary", None),
        ("<!-- {{ $unknown-> }} -->", "$unknown->", None),
        ("{{-- {{ $unknown-> }} --}}", "$unknown->", None),
        ("<div>Plain text</div>", "Plain", None),
        ("<div>Plain text</div>", "</di", None),
    ] {
        open_php(&backend, &uri, source).await;
        let pos = position_after(source, needle);
        let response = completion_response(&backend, &uri, pos.line, pos.character, None).await;
        let names = response.map(labels).unwrap_or_default();
        if let Some(expected) = expected {
            assert!(names.contains(&expected.to_string()), "{source}: {names:?}");
        } else {
            assert!(names.is_empty(), "{source}: {names:?}");
        }
    }
}

#[tokio::test]
async fn tempest_view_path_completions_replace_the_whole_literal_and_respect_arguments() {
    use crate::common::{create_psr4_workspace, workspace_uri};
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        &[("app/home.view.php", "<p>Home</p>")],
    );
    let uri = workspace_uri(&backend, "app/Controller.php");
    for (call, needle, expected) in [
        ("render('home.view.php')", "'ho", "home.view.php"),
        (
            "render(__DIR__ . '/home.view.php')",
            "'/ho",
            "/home.view.php",
        ),
        (
            "render(data: [], path: 'home.view.php')",
            "'ho",
            "home.view.php",
        ),
        (
            "render(path: __DIR__. '/home.view.php')",
            "'/ho",
            "/home.view.php",
        ),
        (
            "\\Tempest\\View\\view('./home.view.php')",
            "'./ho",
            "./home.view.php",
        ),
        ("render('ho", "'ho", "home.view.php"),
    ] {
        let source =
            format!("<?php\nnamespace App;\nuse function Tempest\\View\\view as render;\n{call};");
        open_php(&backend, &uri, &source).await;
        let pos = position_after(&source, needle);
        let response = completion_response(&backend, &uri, pos.line, pos.character, None)
            .await
            .unwrap();
        let items = match response {
            CompletionResponse::CompletionItemList(items) => items,
            CompletionResponse::CompletionList(list) => list.items,
        };
        let item = items
            .iter()
            .find(|item| item.label == expected)
            .unwrap_or_else(|| panic!("{call}: {items:?}"));
        let Some(CompletionItemTextEdit::TextEdit(edit)) = &item.text_edit else {
            panic!("missing path edit")
        };
        assert_eq!(edit.range.start, position_after(&source, "'"));
        if call.ends_with(')') {
            assert_eq!(edit.range.end.line, 3);
            assert_eq!(edit.range.end.character, call.len() as u32 - 2);
        }
    }
    for call in [
        "view('ho')",
        "$object->render('ho')",
        "render('first', 'ho')",
        "render(other: 'ho')",
        "render($prefix . 'ho')",
        "render(\"$ho\")",
    ] {
        let source = format!("<?php use function Tempest\\View\\view as render; {call};");
        open_php(&backend, &uri, &source).await;
        let pos = position_after(&source, "ho");
        let response = completion_response(&backend, &uri, pos.line, pos.character, None).await;
        assert!(
            !response
                .map(labels)
                .unwrap_or_default()
                .contains(&"home.view.php".to_string()),
            "{call}"
        );
    }
}

#[tokio::test]
async fn tempest_relative_view_references_are_resolved_from_each_caller() {
    use crate::common::{create_psr4_workspace, references_response, workspace_uri};
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        &[
            ("app/one/page.view.php", "One"),
            ("app/two/page.view.php", "Two"),
        ],
    );
    let first = workspace_uri(&backend, "app/one/Controller.php");
    let second = workspace_uri(&backend, "app/two/Controller.php");
    let source = "<?php use function Tempest\\View\\view; view('page.view.php'); view(__DIR__ . '/page.view.php');";
    open_php(&backend, &first, source).await;
    open_php(&backend, &second, source).await;
    let references = references_response(
        &backend,
        &first,
        position_of(source, "page.view.php"),
        false,
    )
    .await
    .unwrap();
    assert_eq!(references.len(), 2, "{references:?}");
    assert!(references.iter().all(|location| location.uri == first));
}

#[tokio::test]
async fn tempest_vendor_components_project_overrides_and_open_buffer_attributes() {
    use crate::common::{create_psr4_workspace, workspace_uri};
    let (backend, dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        &[
            (".gitignore", "/vendor/\n"),
            (
                "vendor/composer/installed.json",
                r#"{"packages":[{"name":"acme/views","autoload":{"psr-4":{"Acme\\":["src/","src/."]}},"extra":{"tempest":{"can-discover":true}},"install-path":"../acme/views"},{"name":"acme/missing"}]}"#,
            ),
            (
                "vendor/acme/views/composer.json",
                r#"{"autoload":{"psr-4":{"Acme\\":["src/","src/."]}}}"#,
            ),
            ("vendor/acme/views/src/x-card.view.php", "Vendor card"),
            ("vendor/acme/views/src/x-vendor.view.php", "Vendor only"),
            ("app/x-card.view.php", "Project card"),
        ],
    );
    let uri = workspace_uri(&backend, "app/index.view.php");
    let source = "<x-card /><x-vendor /><x-component is=\"x-card\" /><x-unknown />";
    open_php(&backend, &uri, source).await;
    for (needle, expected) in [
        ("<x-card", "app/x-card.view.php"),
        ("<x-vendor", "vendor/acme/views/src/x-vendor.view.php"),
        ("is=\"x-ca", "app/x-card.view.php"),
    ] {
        let pos = position_after(source, needle);
        let locations =
            definition_locations(goto_definition_at(&backend, &uri, pos.line, pos.character).await);
        assert_eq!(locations.len(), 1, "{needle}");
        assert_eq!(
            locations[0].uri,
            Uri::from_file_path(dunce::canonicalize(dir.path().join(expected)).unwrap()).unwrap()
        );
    }
    let pos = position_after(source, "<x-unknown");
    assert!(
        definition_locations(goto_definition_at(&backend, &uri, pos.line, pos.character).await)
            .is_empty()
    );
    let component = workspace_uri(&backend, "app/x-card.view.php");
    open_php(&backend, &component, "<?php /**\n * @var string $bookTitle\n * @var string $attributes\n * @var string $slots\n * @var object $this\n */ ?>").await;
    let source = "<x-card :book-title=\"'title'\" >";
    open_php(&backend, &uri, source).await;
    let pos = position_after(source, "'title'\" ");
    let names = labels(
        completion_response(&backend, &uri, pos.line, pos.character, None)
            .await
            .unwrap(),
    );
    assert!(!names.contains(&"book-title".to_string()));
    assert!(!names.contains(&"attributes".to_string()));
    let source = "<x-card book>";
    open_php(&backend, &uri, source).await;
    let pos = position_after(source, "book");
    assert!(
        labels(
            completion_response(&backend, &uri, pos.line, pos.character, None)
                .await
                .unwrap()
        )
        .contains(&"book-title".to_string())
    );
}

#[tokio::test]
async fn tempest_non_composer_and_unsaved_components_are_discoverable() {
    use crate::common::{create_psr4_workspace, workspace_uri};
    let (backend, _dir) = create_psr4_workspace("{}", &[("views/x-local.view.php", "Local")]);
    let uri = workspace_uri(&backend, "views/index.view.php");
    let source = "<x-local /><x-unsaved />";
    let unsaved = workspace_uri(&backend, "views/x-unsaved.view.php");
    open_php(&backend, &unsaved, "Unsaved").await;
    open_php(&backend, &uri, source).await;
    for needle in ["<x-local", "<x-unsaved"] {
        let pos = position_after(source, needle);
        assert_eq!(
            definition_locations(goto_definition_at(&backend, &uri, pos.line, pos.character).await)
                .len(),
            1
        );
    }
}

#[tokio::test]
async fn tempest_view_paths_handle_absolute_unsaved_missing_and_stale_files() {
    use crate::common::{create_psr4_workspace, references_response, workspace_uri};
    let (backend, dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        &[
            ("app/page.view.php", "<h1>Page</h1>"),
            ("appjoined.view.php", "Joined"),
        ],
    );
    let uri = workspace_uri(&backend, "app/Controller.php");
    let absolute = dunce::canonicalize(dir.path().join("app/page.view.php")).unwrap();
    let absolute = absolute.to_string_lossy().replace('\\', "/");
    let source = format!(
        "<?php use function Tempest\\View\\view; view('{absolute}'); view(__DIR__ . '/draft.view.php'); view('missing.view.php'); view(__DIR__ . '/missing.view.php'); view(\"\\xFF\");"
    );
    let draft = workspace_uri(&backend, "app/draft.view.php");
    let source = source + " view(__DIR__ . 'joined.view.php');";
    open_php(&backend, &draft, "Draft").await;
    open_php(&backend, &uri, &source).await;
    for needle in [&absolute, "/draft.view.php", "joined.view.php"] {
        let pos = position_of(&source, needle);
        assert_eq!(
            definition_locations(
                goto_definition_at(&backend, &uri, pos.line, pos.character + 1).await
            )
            .len(),
            1
        );
    }
    for needle in ["'missing", "/missing", "\\xFF"] {
        let pos = position_of(&source, needle);
        assert!(
            definition_locations(
                goto_definition_at(&backend, &uri, pos.line, pos.character + 1).await
            )
            .is_empty()
        );
    }
    assert!(
        references_response(&backend, &uri, position_after(&source, "'missing"), false)
            .await
            .unwrap_or_default()
            .is_empty()
    );
    let stale = workspace_uri(&backend, "app/Deleted.php");
    backend.update_ast(
        stale.as_str(),
        "<?php \\Tempest\\View\\view('page.view.php');",
    );
    backend.update_ast(
        "not a valid URI",
        "<?php \\Tempest\\View\\view('page.view.php');",
    );
    let elsewhere = workspace_uri(&backend, "Elsewhere.php");
    open_php(
        &backend,
        &elsewhere,
        "<?php \\Tempest\\View\\view('app/page.view.php');",
    )
    .await;
    let refs = references_response(&backend, &uri, position_of(&source, &absolute), true)
        .await
        .unwrap();
    assert!(refs.iter().any(|location| location.uri == uri));
    assert!(refs.iter().any(|location| location.uri == elsewhere));
    assert!(!refs.iter().any(|location| location.uri == stale));
}

#[tokio::test]
async fn tempest_component_navigation_ignores_other_attributes_and_missing_props() {
    let backend = create_test_backend();
    let uri = uri("src/components.view.php");
    let source = "<x-component is=\"x-missing\" title=\"text\" /><x-unknown :other=\"1\" />";
    open_php(&backend, &uri, source).await;
    let pos = position_of(source, "title");
    assert!(
        definition_locations(goto_definition_at(&backend, &uri, pos.line, pos.character).await)
            .is_empty()
    );
    let pos = position_after(source, "<x-unknown :oth");
    assert!(
        labels(
            completion_response(&backend, &uri, pos.line, pos.character, None)
                .await
                .unwrap()
        )
        .is_empty()
    );
}

#[tokio::test]
async fn tempest_sibling_branches_remain_inside_their_enclosing_loop() {
    let backend = create_test_backend();
    open_php(&backend, &uri("src/Book.php"), MODELS).await;
    let uri = uri("src/branches.view.php");
    let source = r#"<?php /** @var list<\App\Book> $books */ ?>
<div :foreach="$books as $book" :if="$book->title !== ''">{{ $book->title }}</div>
<p :elseif="$book->title === ''">{{ $book->summary() }}</p>
<p :else>{{ $book->title }}</p>
<input :foreach="$books as $other" :if="$other->title !== ''" :value="$other->title">
<input :else :value="$other->summary()">
"#;
    open_php(&backend, &uri, source).await;
    assert_eq!(template_diagnostics(&backend, &uri), Vec::<String>::new());
    assert!(
        hover_on(&backend, &uri, source, "summary()")
            .await
            .contains("string")
    );
    assert!(
        hover_on(&backend, &uri, source, "$other->summary")
            .await
            .contains("Book")
    );
}

#[tokio::test]
async fn tempest_class_completion_inserts_native_php_imports() {
    use crate::common::apply_edits;
    let backend = create_test_backend();
    open_php(&backend, &uri("src/Book.php"), MODELS).await;
    let uri = uri("src/imports.view.php");
    for source in [
        "<div>{{ new Bo }}</div>",
        "<?php ?>\n<div>{{ new Bo }}</div>",
    ] {
        open_php(&backend, &uri, source).await;
        let pos = position_after(source, "new Bo");
        let response = completion_response(&backend, &uri, pos.line, pos.character, None)
            .await
            .unwrap();
        let items = match response {
            CompletionResponse::CompletionItemList(items) => items,
            CompletionResponse::CompletionList(list) => list.items,
        };
        let item = items
            .iter()
            .find(|item| item.detail.as_deref() == Some("App\\Book"))
            .unwrap_or_else(|| panic!("{items:?}"));
        let edits = item.additional_text_edits.as_ref().unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range, Range::default());
        assert_eq!(edits[0].new_text, "<?php use App\\Book; ?>\n");
        let edited = apply_edits(source, edits);
        open_php(&backend, &uri, &edited).await;
        let virtual_php = backend.blade_virtual_php(uri.as_str()).unwrap();
        let mut errors = Vec::new();
        backend.collect_syntax_error_diagnostics(uri.as_str(), &virtual_php, &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
    }
}

#[tokio::test]
async fn tempest_import_code_actions_preserve_template_markup() {
    use crate::common::{apply_workspace_edit, code_actions_via_server};
    let backend = create_test_backend();
    open_php(&backend, &uri("src/Book.php"), MODELS).await;
    let uri = uri("src/actions.view.php");
    let source = "<p>{{ Book::class }}</p>";
    open_php(&backend, &uri, source).await;
    let pos = position_of(source, "Book");
    let actions = code_actions_via_server(&backend, &uri, Range::new(pos, pos))
        .await
        .unwrap_or_default();
    let action = actions
        .iter()
        .find_map(|action| match action {
            CodeActionResponse::CodeAction(action)
                if action.title.contains("App\\Book") && action.title.starts_with("Import") =>
            {
                Some(action)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("{actions:?}"));
    let edited = apply_workspace_edit(source, action.edit.as_ref().unwrap());
    assert_eq!(edited, "<?php use App\\Book; ?>\n<p>{{ Book::class }}</p>");
}

#[tokio::test]
async fn tempest_completion_keeps_generated_loop_flags_private() {
    let backend = create_test_backend();
    let uri = uri("src/variables.view.php");
    let source = "<?php $books = ['one']; ?>\n<p :foreach=\"$books as $book\">{{ $book }}</p><p :forelse>Empty</p>\n{{ $ }}";
    open_php(&backend, &uri, source).await;
    let pos = position_after(source, "{{ $ }");
    let response = completion_response(&backend, &uri, pos.line, pos.character - 2, None)
        .await
        .unwrap();
    let names = labels(response);
    assert!(names.contains(&"$books".to_string()), "{names:?}");
    assert!(
        !names
            .iter()
            .any(|name| name.starts_with("$__phpantom_tempest_empty_")),
        "{names:?}"
    );
}
