//! View data names lead back to the template inputs they supply.

use crate::common::{
    completion_response, create_psr4_workspace, definition_locations, goto_definition_at,
    hover_response, hover_text, open_php, position_after, position_of, response_items,
    workspace_path, workspace_uri,
};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{"require":{"tempest/view":"^3.0"},"autoload":{"psr-4":{"App\\":"app/","Tempest\\View\\":"framework/"}}}"#;
const FRAMEWORK: &str = r"<?php namespace Tempest\View;
interface View {}
class GenericView implements View { public function __construct(public string $path) {} }
function view(string $path, mixed ...$params): View { return new GenericView($path); }
";

async fn project(caller: &str, template: &str) -> (Backend, tempfile::TempDir, Uri, Uri) {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("framework/View.php", FRAMEWORK),
            (
                "app/Book.php",
                "<?php namespace App; class Book { public string $title; }",
            ),
            ("app/caller.php", caller),
            ("app/page.view.php", template),
        ],
    );
    open_php(
        &backend,
        &workspace_uri(&backend, "framework/View.php"),
        FRAMEWORK,
    )
    .await;
    let uri = workspace_uri(&backend, "app/caller.php");
    let view = Uri::from_file_path(
        dunce::canonicalize(workspace_path(&backend, "app/page.view.php")).unwrap(),
    )
    .unwrap();
    open_php(&backend, &uri, caller).await;
    open_php(&backend, &view, template).await;
    (backend, dir, uri, view)
}

async fn items(backend: &Backend, uri: &Uri, source: &str, needle: &str) -> Vec<CompletionItem> {
    let at = position_after(source, needle);
    response_items(completion_response(backend, uri, at.line, at.character, None).await)
        .unwrap_or_default()
}

async fn definitions(backend: &Backend, uri: &Uri, source: &str, needle: &str) -> Vec<Location> {
    let at = position_of(source, needle);
    definition_locations(goto_definition_at(backend, uri, at.line, at.character).await)
}

#[tokio::test]
async fn tempest_view_parameters_complete_unused_inputs_and_navigate_first_reads() {
    let caller = "<?php\r\nnamespace App;\r\nuse function Tempest\\View\\view as render;\r\n// 📚\r\nrender(__DIR__.'/page.view.php', book: new Book(), title: 'Books');\r\nrender('./page.view.php', );\r\nrender(book: new Book(), path: './page.view.php', ti);\r\nrender('./page.view.php', bo, title: 'Existing');\r\n";
    let template = "<?php\r\nuse App\\Book;\r\n/** @var Book $book */\r\n/** @var string $title */\r\n?>\r\n<!-- $book is not read here. -->\r\n<p :title=\"$title\" :if=\"$title\">📚 {{ $book->title }} {{ $book->title }}</p>\r\n";
    let (backend, _dir, uri, view) = project(caller, template).await;
    let offered = items(&backend, &uri, caller, "render('./page.view.php', ").await;
    for name in ["book:", "title:"] {
        let item = offered
            .iter()
            .find(|item| item.label == name)
            .unwrap_or_else(|| panic!("{name}: {offered:?}"));
        assert_eq!(item.detail.as_deref(), Some("View parameter"));
        assert_eq!(
            item.insert_text.as_deref(),
            Some(format!("{name} ").as_str())
        );
    }
    assert!(
        !offered
            .iter()
            .any(|item| matches!(item.label.as_str(), "params:" | "path:"))
    );
    let filtered = items(&backend, &uri, caller, "path: './page.view.php', ti").await;
    assert!(filtered.iter().any(|item| item.label == "title:"));
    assert!(!filtered.iter().any(|item| item.label == "book:"));
    let later = items(&backend, &uri, caller, "render('./page.view.php', bo").await;
    assert!(later.iter().any(|item| item.label == "book:"));
    assert!(!later.iter().any(|item| item.label == "title:"));
    for (name, read, ty) in [
        ("book", "{{ $book", "Book"),
        ("title", ":title=\"$title", "string"),
    ] {
        let needle = format!("{name}:");
        let start = position_of(caller, &needle);
        let end = Position::new(start.line, start.character + name.len() as u32);
        let read_end = position_after(template, read);
        let expected = vec![Location {
            uri: view.clone(),
            range: Range {
                start: Position::new(read_end.line, read_end.character - name.len() as u32 - 1),
                end: read_end,
            },
        }];
        for at in [start, end] {
            assert_eq!(
                definition_locations(
                    goto_definition_at(&backend, &uri, at.line, at.character).await
                ),
                expected
            );
            let hover = hover_response(&backend, &uri, at.line, at.character)
                .await
                .unwrap();
            let text = hover_text(&hover);
            assert!(
                text.contains("View parameter") && text.contains(ty),
                "{hover:?}"
            );
            assert_eq!(hover.range, Some(Range { start, end }));
        }
    }
}

#[tokio::test]
async fn tempest_view_parameters_resolve_paths_through_the_shared_expression_engine() {
    let caller = r"<?php namespace App;
use function Tempest\View\view;
const TEMPLATE = __DIR__ . '/page.view.php';
$path = TEMPLATE;
view($path, title: 'Variable');
view(TEMPLATE, title: 'Constant');
view(path: TEMPLATE, );
view('./page.view.php', ...['title' => 'Passed'], );
/** @var array{title?: string} $maybe */
view('./page.view.php', ...$maybe, ti);
";
    let (backend, _dir, uri, view) = project(caller, "<h1>{{ $title }}</h1>").await;
    for needle in ["title: 'Variable'", "title: 'Constant'"] {
        let found = definitions(&backend, &uri, caller, needle).await;
        assert_eq!(found.len(), 1, "{needle}: {found:?}");
        assert_eq!(found[0].uri, view);
    }
    assert!(
        items(&backend, &uri, caller, "path: TEMPLATE, ")
            .await
            .iter()
            .any(|item| item.label == "title:")
    );
    assert!(
        !items(&backend, &uri, caller, "'Passed'], ")
            .await
            .iter()
            .any(|item| item.label == "title:")
    );
    assert!(
        items(&backend, &uri, caller, "...$maybe, ti")
            .await
            .iter()
            .any(|item| item.label == "title:")
    );
}

#[tokio::test]
async fn tempest_view_parameters_ignore_annotations_locals_nested_scopes_and_superglobals() {
    let caller = r"<?php \Tempest\View\view('./page.view.php', );";
    let template = r#"<?php
/** @var string $annotationOnly */
$local = 'local';
/** @var string $local */
echo $local;
$copy = $external;
$format = fn(string $parameter) => $arrowInput . $parameter;
$closure = function () use ($captured) { return $captured; };
function nested(string $parameter) { echo $parameter, $hidden; }
unset($gone);
$optional ??= 'default';
?>
<!-- {{ $comment }} -->
<p :foreach="$books as $book">{{ $book->title }} {{ $external }}</p>
<p :forelse>Empty</p>
{{ $_GET['page'] }} {{ $this->title }} {{ $copy }}
"#;
    let (backend, _dir, uri, _) = project(caller, template).await;
    let offered = items(&backend, &uri, caller, "'./page.view.php', ").await;
    for name in [
        "books:",
        "external:",
        "optional:",
        "arrowInput:",
        "captured:",
    ] {
        assert!(
            offered.iter().any(|item| item.label == name),
            "{name}: {offered:?}"
        );
    }
    for name in [
        "annotationOnly:",
        "local:",
        "copy:",
        "book:",
        "parameter:",
        "hidden:",
        "gone:",
        "comment:",
        "_GET:",
        "this:",
    ] {
        assert!(
            !offered.iter().any(|item| item.label == name),
            "{name}: {offered:?}"
        );
    }
}

#[tokio::test]
async fn tempest_view_parameters_keep_incomplete_calls_and_unsaved_templates_editable() {
    let caller = "<?php\r\nnamespace App;\r\nuse function Tempest\\View\\view;\r\nclass Controller { public function index() { return view('./page.view.php', ti";
    let (backend, _dir, uri, view) = project(caller, "{{ $title }}").await;
    assert!(
        items(&backend, &uri, caller, "view('./page.view.php', ti")
            .await
            .iter()
            .any(|item| item.label == "title:")
    );
    let complete =
        "<?php \\Tempest\\View\\view('./page.view.php', title: 'Books', heading: 'Heading');";
    open_php(&backend, &uri, complete).await;
    assert_eq!(
        definitions(&backend, &uri, complete, "title:").await.len(),
        1
    );
    let changed = "<p>New template</p>\n{{ $heading }}";
    open_php(&backend, &view, changed).await;
    assert!(
        definitions(&backend, &uri, complete, "title:")
            .await
            .is_empty()
    );
    let found = definitions(&backend, &uri, complete, "heading:").await;
    assert_eq!(found[0].range.start, position_of(changed, "$heading"));
    assert!(
        hover_response(
            &backend,
            &uri,
            position_of(complete, "title:").line,
            position_of(complete, "title:").character
        )
        .await
        .is_none()
    );
}

#[tokio::test]
async fn tempest_view_parameters_leave_other_calls_values_and_custom_view_constructors_alone() {
    let caller = r"<?php namespace App;
use function Tempest\View\view as render;
function view(string $path, string $ordinary) {}
class CustomView implements \Tempest\View\View { public function __construct(public string $title) {} }
view('./page.view.php', );
view('./page.view.php', ordinary: 'Native');
new CustomView();
new CustomView(title: 'Constructor');
render('./page.view.php', title: 'Forwarded');
render('./missing.view.php', title: 'Missing');
render($unknown, title: 'Unknown');
render(path: './page.view.php');
";
    let (backend, _dir, uri, _) = project(caller, "{{ $title }}").await;
    for needle in [
        "ordinary: 'Native'",
        "title: 'Constructor'",
        "'Forwarded'",
        "title: 'Missing'",
        "title: 'Unknown'",
        "path:",
    ] {
        let at = position_of(caller, needle);
        let hover = hover_response(&backend, &uri, at.line, at.character).await;
        assert!(
            hover
                .as_ref()
                .is_none_or(|hover| !hover_text(hover).contains("View parameter")),
            "{needle}: {hover:?}"
        );
    }
    assert!(
        items(
            &backend,
            &uri,
            caller,
            "
view('./page.view.php', "
        )
        .await
        .iter()
        .any(|item| item.label.starts_with("ordinary:"))
    );
    assert!(
        items(
            &backend,
            &uri,
            caller,
            "
new CustomView("
        )
        .await
        .iter()
        .any(|item| item.label.starts_with("title:"))
    );
    assert_eq!(
        definitions(&backend, &uri, caller, "title: 'Forwarded'")
            .await
            .len(),
        1
    );
}

#[tokio::test]
async fn tempest_view_parameters_find_inputs_after_conditional_local_bindings() {
    let caller = r"<?php \Tempest\View\view('./page.view.php', );
\Tempest\View\view('./page.view.php', title: 'Fallback');";
    let template = r"<?php if ($condition) { $title = 'Local'; echo $title; } ?>
<h1>{{ $title }}</h1>";
    let (backend, _dir, uri, view) = project(caller, template).await;
    let offered = items(&backend, &uri, caller, "'./page.view.php', ").await;
    assert!(
        offered.iter().any(|item| item.label == "title:"),
        "{offered:?}"
    );
    let found = definitions(&backend, &uri, caller, "title: 'Fallback'").await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].uri, view);
    assert_eq!(
        found[0].range.start,
        position_of(template, "$title }}</h1>")
    );
}

#[tokio::test]
async fn tempest_view_parameters_follow_each_namespace_blocks_function_imports() {
    let caller = r"<?php
namespace First {
    use function Tempest\View\view as draw;
    draw('./page.view.php', title: 'Forwarded');
}
namespace Second {
    function draw(string $path, string $title) {}
    draw('./page.view.php', title: 'Ordinary');
}
";
    let (backend, _dir, uri, view) = project(caller, "{{ $title }}").await;
    assert_eq!(
        definitions(&backend, &uri, caller, "title: 'Forwarded'").await[0].uri,
        view
    );
    let at = position_of(caller, "title: 'Ordinary'");
    assert!(
        hover_response(&backend, &uri, at.line, at.character)
            .await
            .as_ref()
            .is_none_or(|hover| !hover_text(hover).contains("View parameter"))
    );
}

#[tokio::test]
async fn tempest_view_parameters_translate_nested_template_calls_and_reuse_inferred_hover() {
    let (backend, dir, _, page) = project("<?php", "").await;
    let detail = "<h2>Count</h2>\n{{ $count }}";
    let detail_path = dir.path().join("app/detail.view.php");
    std::fs::write(&detail_path, detail).unwrap();
    let target = Uri::from_file_path(dunce::canonicalize(&detail_path).unwrap()).unwrap();
    let source = "<?php use function Tempest\\View\\view; ?>\n<p>📚 {{ view('./detail.view.php', count: 42) }}</p>";
    open_php(&backend, &page, source).await;
    open_php(&backend, &target, detail).await;
    let found = definitions(&backend, &page, source, "count:").await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].uri, target);
    assert_eq!(
        found[0].range,
        Range {
            start: position_of(detail, "$count"),
            end: position_after(detail, "$count")
        }
    );
    let read = position_of(detail, "$count");
    let expected = hover_response(&backend, &target, read.line, read.character)
        .await
        .unwrap();
    let at = position_of(source, "count:");
    let hover = hover_response(&backend, &page, at.line, at.character)
        .await
        .unwrap();
    assert!(
        hover_text(&hover).contains(hover_text(&expected)),
        "{hover:?} vs {expected:?}"
    );
    assert_eq!(
        hover.range,
        Some(Range {
            start: at,
            end: Position::new(at.line, at.character + 5)
        })
    );
}
