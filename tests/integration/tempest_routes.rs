//! Route actions and their parameters retain their declaring controller identity.

use crate::common::{
    completion_response, create_psr4_workspace, definition_locations, goto_definition_at, open_php,
    position_after, position_of, rename, response_items, workspace_uri,
};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

const FRAMEWORK: &str = r"<?php
namespace Tempest\Http;
enum Method { case GET; case POST; }
namespace Tempest\Router;
interface Route {}
class Get implements Route { public \Tempest\Http\Method $method = \Tempest\Http\Method::GET; public function __construct(public string $uri) {} }
class Post implements Route { public \Tempest\Http\Method $method = \Tempest\Http\Method::POST; public function __construct(public string $uri) {} }
function uri(array|string $action, mixed ...$params): string { return ''; }
";

const CONTROLLER: &str = r"<?php
namespace Routes;
use Tempest\Router\Get as Read;
use Tempest\Router\Post;
class Custom implements \Tempest\Router\Route { public function __construct(public string $uri) {} }
class NotRoute { public function __construct(public string $uri) {} }
class Books {
    #[Read('/books/{book}')]
    #[Post(uri: '/books/{book}/edit')]
    public function show(string $book, string $unused = ''): string { return $book; }
    #[Custom('/archive/{year:\d{4}}/{?month}')]
    public function archive(int $year, int $month = 1): string { return ''; }
    #[NotRoute('/fake/{book}')]
    public function fake(string $book): void {}
    #[Read('/private/{secret}')]
    private function hidden(string $secret): void {}
}
class Home {
    #[Read('/home/{locale}')]
    public function __invoke(string $locale): string { return $locale; }
}
";

async fn project(caller: &str) -> (Backend, tempfile::TempDir, Uri, Uri) {
    let (backend, dir) = create_psr4_workspace(
        r#"{"require":{"tempest/view":"^3.0","tempest/router":"^3.0"},"autoload":{"psr-4":{"Routes\\":"routes/","Consumer\\":"app/","Tempest\\Router\\":"framework/"}}}"#,
        &[
            ("framework/router.php", FRAMEWORK),
            ("routes/Books.php", CONTROLLER),
            ("app/caller.php", caller),
        ],
    );
    let controller = workspace_uri(&backend, "routes/Books.php");
    let uri = workspace_uri(&backend, "app/caller.php");
    open_php(
        &backend,
        &workspace_uri(&backend, "framework/router.php"),
        FRAMEWORK,
    )
    .await;
    open_php(&backend, &controller, CONTROLLER).await;
    open_php(&backend, &uri, caller).await;
    (backend, dir, controller, uri)
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
async fn tempest_routes_complete_uris_and_invokable_classes_with_action_details() {
    let source = r"<?php namespace Consumer;
use function Tempest\Router\uri as link;
link('');
link(action: '/b');
link('Routes\\H');
";
    let (backend, _dir, _, uri) = project(source).await;
    let all = items(&backend, &uri, source, "link('").await;
    for name in [
        "/books/{book}",
        "/books/{book}/edit",
        "/archive/{year:\\d{4}}/{?month}",
        "/home/{locale}",
        "Routes\\Home",
    ] {
        assert!(all.iter().any(|item| item.label == name), "{name}: {all:?}");
    }
    assert!(
        !all.iter()
            .any(|item| item.label.contains("fake") || item.label.contains("private"))
    );
    assert!(
        all.iter()
            .any(|item| item.detail.as_deref() == Some("GET — Routes\\Books::show"))
    );
    let filtered = items(&backend, &uri, source, "action: '/b").await;
    assert_eq!(filtered.len(), 2);
    assert!(filtered.iter().all(|item| item.label.starts_with("/books")));
    let classes = items(&backend, &uri, source, "link('Routes\\\\H").await;
    assert_eq!(
        classes
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>(),
        ["Routes\\Home"]
    );
}

#[tokio::test]
async fn tempest_routes_navigate_actions_and_forwarded_parameters() {
    let source = r"<?php namespace Consumer;
use Routes\Books as Catalog;
use Routes\Home;
use function Tempest\Router\uri as link;
link([Catalog::class, 'show'], book: 'one');
link('/books/{book}', book: 'two');
link(Home::class, locale: 'en');
link('Routes\\Home', locale: 'nl');
link(action: [Catalog::class, 'archive'], year: 2026);
";
    let (backend, _dir, controller, uri) = project(source).await;
    for (needle, target) in [
        ("show'],", "show(string"),
        ("/books/{book}", "show(string"),
        ("book: 'one'", "$book, string"),
        ("book: 'two'", "$book, string"),
        ("locale: 'en'", "$locale):"),
        ("locale: 'nl'", "$locale):"),
        ("year: 2026", "$year,"),
    ] {
        let locations = definitions(&backend, &uri, source, needle).await;
        assert_eq!(locations.len(), 1, "{needle}: {locations:?}");
        assert_eq!(locations[0].uri, controller);
        assert_eq!(
            locations[0].range.start,
            position_of(CONTROLLER, target),
            "{needle}"
        );
    }
    let target = definitions(&backend, &controller, CONTROLLER, "book}')").await;
    assert_eq!(
        target[0].range.start,
        position_of(CONTROLLER, "$book, string")
    );
    let variable = definitions(&backend, &controller, CONTROLLER, "$book; }").await;
    assert_eq!(variable.len(), 1);
    assert_eq!(
        variable[0].range.start,
        position_of(CONTROLLER, "$book, string")
    );
}

#[tokio::test]
async fn tempest_routes_complete_only_selected_placeholders_and_unused_labels() {
    let source = r"<?php namespace Consumer;
use Routes\Books;
use function Tempest\Router\uri;
uri([Books::class, 'show'], );
uri('/books/{book}', bo);
uri([Books::class, 'archive'], year: 2026, );
";
    let (backend, _dir, controller, uri) = project(source).await;
    for needle in ["'show'], ", "{book}', bo"] {
        let offered = items(&backend, &uri, source, needle).await;
        assert!(
            offered.iter().any(|item| item.label == "book:"),
            "{offered:?}"
        );
        assert!(!offered.iter().any(|item| item.label.starts_with("unused:")));
    }
    let offered = items(&backend, &uri, source, "year: 2026, ").await;
    assert!(
        offered.iter().any(|item| item.label == "month:"),
        "{offered:?}"
    );
    assert!(!offered.iter().any(|item| item.label == "year:"));
    let offered = items(&backend, &controller, CONTROLLER, "'/books/{bo").await;
    assert_eq!(
        offered
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>(),
        ["book"]
    );
}

#[tokio::test]
async fn tempest_routes_parameter_rename_updates_placeholders_and_uri_labels() {
    let source = r"<?php namespace Consumer;
use Routes\Books;
use function Tempest\Router\uri;
uri([Books::class, 'show'], book: 'one');
uri('/books/{book}', book: 'two');
uri(book: 'three', action: '/books/{book}');
uri([Books::class, 'fake'], book: 'unrelated');
";
    let (backend, _dir, controller, uri) = project(source).await;
    let at = position_of(CONTROLLER, "$book, string");
    let edit = rename(&backend, &controller, at.line, at.character + 1, "volume")
        .await
        .unwrap();
    let changes = edit.changes.unwrap();
    let local = &changes[&controller];
    assert_eq!(local.len(), 4, "{changes:?}");
    assert_eq!(
        local
            .iter()
            .filter(|edit| edit.new_text == "volume")
            .count(),
        2
    );
    assert_eq!(
        local
            .iter()
            .filter(|edit| edit.new_text == "$volume")
            .count(),
        2
    );
    assert_eq!(changes[&uri].len(), 5, "{changes:?}");
    assert!(changes[&uri].iter().all(|edit| edit.new_text == "volume"));
}

#[tokio::test]
async fn tempest_routes_do_not_claim_other_functions_attributes_or_argument_values() {
    let source = r"<?php namespace Consumer;
function uri(string $path, mixed ...$args): void {}
uri('/books/{book}', book: 'one');
\Tempest\Router\uri('/fake/{book}', book: 'two');
\Tempest\Router\uri('/books/{book}', query: 'book');
";
    let (backend, _dir, controller, uri) = project(source).await;
    for needle in [
        "/books/{book}",
        "book: 'one'",
        "/fake/{book}",
        "book: 'two'",
        "query:",
    ] {
        assert!(
            definitions(&backend, &uri, source, needle).await.is_empty(),
            "{needle}"
        );
    }
    assert!(
        definitions(&backend, &controller, CONTROLLER, "book}')]")
            .await
            .len()
            == 1
    );
    assert!(
        !items(&backend, &uri, source, "uri('/b")
            .await
            .iter()
            .any(|item| item.label == "/books/{book}")
    );
}

#[tokio::test]
async fn tempest_routes_resolve_typed_actions_and_named_action_in_any_position() {
    let source = r"<?php namespace Consumer;
use Routes\Books as Shelf;
use function Tempest\Router\uri as link;
const PATH = '/books/{book}';
$action = [Shelf::class, 'show'];
link($action, book: 'one');
link(PATH, book: 'two');
link(book: 'three', action: '/books/{book}');
link(action: [Shelf::class, 'show'], book: 'four');
";
    let (backend, _dir, controller, uri) = project(source).await;
    for needle in [
        "book: 'one'",
        "book: 'two'",
        "book: 'three'",
        "book: 'four'",
    ] {
        let locations = definitions(&backend, &uri, source, needle).await;
        assert_eq!(locations.len(), 1, "{needle}: {locations:?}");
        assert_eq!(locations[0].uri, controller);
        assert_eq!(
            locations[0].range.start,
            position_of(CONTROLLER, "$book, string")
        );
    }
    let locations = definitions(&backend, &uri, source, "Shelf::class, 'show'], book").await;
    assert_eq!(locations.len(), 1);
    assert_ne!(
        locations[0].range.start,
        position_of(CONTROLLER, "show(string")
    );
}

#[tokio::test]
async fn tempest_routes_follow_unsaved_uri_and_interface_edits() {
    let source = r"<?php \Tempest\Router\uri('');";
    let (backend, _dir, controller, uri) = project(source).await;
    let edited = CONTROLLER.replace("/books/{book}", "/catalog/{book}");
    open_php(&backend, &controller, &edited).await;
    let offered = items(&backend, &uri, source, "uri('").await;
    assert!(offered.iter().any(|item| item.label == "/catalog/{book}"));
    assert!(!offered.iter().any(|item| item.label == "/books/{book}"));
    let edited = edited.replace(
        "class Custom implements \\Tempest\\Router\\Route",
        "class Custom",
    );
    open_php(&backend, &controller, &edited).await;
    assert!(
        !items(&backend, &uri, source, "uri('")
            .await
            .iter()
            .any(|item| item.label.starts_with("/archive"))
    );
    open_php(
        &backend,
        &controller,
        "<?php namespace Routes; class Books {}",
    )
    .await;
    assert!(items(&backend, &uri, source, "uri('").await.is_empty());
}

#[tokio::test]
async fn tempest_routes_keep_namespace_identity_and_unicode_crlf_ranges() {
    let source = r"<?php namespace Consumer;
use function Tempest\Router\uri;
uri('/📚/{book}', book: 'one');
uri('/shadow/{book}', book: 'two');
uri('Home', locale: 'global');
uri('Routes\\Home', locale: 'namespaced');
"
    .replace('\n', "\r\n");
    let (backend, _dir, controller, uri) = project(&source).await;
    let edited = format!("{}\nnamespace Shadow; class Get {{ public function __construct(string $uri) {{}} }}\nclass Wrong {{ #[Get('/shadow/{{book}}')] public function show(string $book): void {{}} }}\n", CONTROLLER.replace("/books/{book}", "/📚/{book}")).replace('\n', "\r\n");
    open_php(
        &backend,
        &workspace_uri(&backend, "app/global.php"),
        "<?php class Home {}",
    )
    .await;
    open_php(&backend, &controller, &edited).await;
    let target = definitions(&backend, &uri, &source, "book: 'one'").await;
    assert_eq!(target.len(), 1);
    assert_eq!(target[0].range.start, position_of(&edited, "$book, string"));
    assert!(
        definitions(&backend, &uri, &source, "book: 'two'")
            .await
            .is_empty()
    );
    assert!(
        definitions(&backend, &uri, &source, "locale: 'global'")
            .await
            .is_empty()
    );
    assert_eq!(
        definitions(&backend, &uri, &source, "locale: 'namespaced'")
            .await
            .len(),
        1
    );
}

#[tokio::test]
async fn tempest_routes_offer_callable_actions_and_only_routed_methods() {
    let source = r"<?php namespace Consumer;
use Routes\Books as Catalog;
use function Tempest\Router\uri;
uri();
uri([Catalog::class, '']);
uri(['Routes\\Books', 'ar']);
";
    let (backend, _dir, _, uri) = project(source).await;
    let actions = items(&backend, &uri, source, "uri(").await;
    assert!(
        actions
            .iter()
            .any(|item| item.insert_text.as_deref() == Some("[\\Routes\\Books::class, 'show']")),
        "{actions:?}"
    );
    assert!(
        actions
            .iter()
            .any(|item| item.insert_text.as_deref() == Some("\\Routes\\Home::class"))
    );
    let methods = items(&backend, &uri, source, "Catalog::class, '").await;
    assert!(methods.iter().any(|item| item.label == "show"));
    assert!(methods.iter().any(|item| item.label == "archive"));
    assert!(
        !methods
            .iter()
            .any(|item| item.label == "fake" || item.label == "hidden")
    );
    let filtered = items(&backend, &uri, source, "'Routes\\\\Books', 'ar").await;
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].label, "archive");
}

#[tokio::test]
async fn tempest_routes_include_class_and_method_prefixes() {
    let source = r"<?php \Tempest\Router\uri('/api/catalog/{book}', book: 'php');";
    let (backend, _dir, controller, uri) = project(source).await;
    let edited = CONTROLLER
        .replace(
            "class Books {",
            "#[\\Tempest\\Router\\Prefix('api')]\nclass Books {",
        )
        .replace(
            "#[Read('/books/{book}')]",
            "#[Read('/{book}')]\n    #[\\Tempest\\Router\\Prefix(prefix: '/catalog')]",
        );
    open_php(&backend, &controller, &edited).await;
    let locations = definitions(&backend, &uri, source, "book: 'php'").await;
    assert_eq!(locations.len(), 1);
    assert_eq!(
        locations[0].range.start,
        position_of(&edited, "$book, string")
    );
    let offered = items(&backend, &uri, source, "uri('/api/c").await;
    assert_eq!(offered.len(), 2);
    assert!(
        offered
            .iter()
            .any(|item| item.label == "/api/catalog/{book}")
    );
    assert!(
        offered
            .iter()
            .any(|item| item.label == "/api/catalog/books/{book}/edit")
    );
}

#[tokio::test]
async fn tempest_routes_normalize_relative_and_empty_attribute_paths() {
    let source = r"<?php
use function Tempest\Router\uri;
uri('');
uri('/books/{book}', book: 'php');
uri([\Routes\Books::class, 'show'], );
uri('/');
";
    let (backend, _dir, controller, uri) = project(source).await;
    let edited = CONTROLLER
        .replace("Read('/books/{book}')", "Read('books/{book}')")
        .replace("Read('/home/{locale}')", "Read('')")
        .replace("NotRoute('/fake/{book}')", "NotRoute('fake/{book}')");
    open_php(&backend, &controller, &edited).await;
    let offered = items(&backend, &uri, source, "uri('").await;
    assert!(offered.iter().any(|item| item.label == "/books/{book}"));
    assert!(offered.iter().any(|item| item.label == "/"));
    assert!(!offered.iter().any(|item| item.label.contains("fake")));
    let parameters = items(&backend, &uri, source, "'show'], ").await;
    assert!(parameters.iter().any(|item| item.label == "book:"));
    for (needle, target) in [
        ("book: 'php'", "$book, string"),
        ("/');", "__invoke(string"),
    ] {
        let locations = definitions(&backend, &uri, source, needle).await;
        assert_eq!(locations.len(), 1, "{needle}: {locations:?}");
        assert_eq!(locations[0].uri, controller);
        assert_eq!(locations[0].range.start, position_of(&edited, target));
    }
    let locations = definitions(&backend, &controller, &edited, "book}')]").await;
    assert_eq!(
        locations[0].range.start,
        position_of(&edited, "$book, string")
    );
}

#[tokio::test]
async fn tempest_routes_complete_partial_actions_without_claiming_values_or_patterns() {
    let source = r"<?php
use function Tempest\Router\uri;
uri(Boo);
uri(NoMatchingController);
uri(book: 'one', action: '/b');
uri('/books/{book}', book: '/b');
uri('/arch
";
    let (backend, _dir, controller, uri) = project(source).await;
    let actions = items(&backend, &uri, source, "uri(Boo").await;
    assert!(
        actions
            .iter()
            .any(|item| item.label == "Routes\\Books::show")
    );
    assert!(
        !actions
            .iter()
            .any(|item| item.label == "Routes\\Home::__invoke")
    );
    let actions = items(&backend, &uri, source, "uri(NoMatchingController").await;
    assert!(
        !actions
            .iter()
            .any(|item| item.label.starts_with("Routes\\"))
    );
    for (needle, expected) in [
        ("action: '/b", "/books/{book}"),
        ("uri('/arch", "/archive/{year:\\d{4}}/{?month}"),
    ] {
        let offered = items(&backend, &uri, source, needle).await;
        assert!(
            offered.iter().any(|item| item.label == expected),
            "{needle}: {offered:?}"
        );
    }
    assert!(
        !items(&backend, &uri, source, "book: '/b")
            .await
            .iter()
            .any(|item| item.label == "/books/{book}")
    );
    for needle in ["'/archive/{year:\\d", "'/fake/{bo", "'/books/{book}"] {
        assert!(
            !items(&backend, &controller, CONTROLLER, needle)
                .await
                .iter()
                .any(|item| item.label == "book"),
            "{needle}"
        );
    }
    assert!(
        definitions(&backend, &controller, CONTROLLER, "'/books")
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn tempest_routes_preserve_union_actions_and_unknown_inputs() {
    let source = r"<?php namespace Consumer;
use function Tempest\Router\uri;
/** @param '/books/{book}'|'/home/{locale}' $action */
function links(string $action): void {
    uri($action, );
}
uri(action: unknown(), book: 'one');
uri(action: unknown(), );
";
    let (backend, _dir, controller, uri) = project(source).await;
    let offered = items(&backend, &uri, source, "uri($action, ").await;
    assert!(
        offered.iter().any(|item| item.label == "book:"),
        "{offered:?}"
    );
    assert!(
        offered.iter().any(|item| item.label == "locale:"),
        "{offered:?}"
    );
    assert!(
        definitions(&backend, &uri, source, "book: 'one'")
            .await
            .is_empty()
    );
    assert!(
        !items(&backend, &uri, source, "uri(action: unknown(), ")
            .await
            .iter()
            .any(|item| item.label == "book:")
    );
    let at = position_of(CONTROLLER, "$book, string");
    let edit = rename(&backend, &controller, at.line, at.character + 1, "volume")
        .await
        .unwrap();
    assert!(!edit.changes.unwrap().contains_key(&uri));
}

#[tokio::test]
async fn tempest_routes_read_http_methods_from_inherited_attribute_defaults() {
    let source = r"<?php \Tempest\Router\uri('');";
    let (backend, _dir, controller, uri) = project(source).await;
    let edited = r"<?php namespace Routes;
class Read extends \Tempest\Router\Get {}
class Dynamic implements \Tempest\Router\Route { public \Tempest\Http\Method $method; }
class Cyclic extends Cyclic implements \Tempest\Router\Route {}
class InvalidDefault implements \Tempest\Router\Route { public mixed $method = unknown(); }
class Books {
    #[Read('/inherited')]
    public function inherited(): void {}
    #[Dynamic('/dynamic')]
    public function dynamic(): void {}
    #[Cyclic('/cycle')]
    public function cycle(): void {}
    #[InvalidDefault('/invalid')]
    public function invalid(): void {}
}
";
    open_php(&backend, &controller, edited).await;
    let offered = items(&backend, &uri, source, "uri('").await;
    assert_eq!(offered.len(), 4, "{offered:?}");
    for item in offered {
        assert_eq!(
            item.detail.as_deref(),
            Some(
                format!(
                    "{} — Routes\\Books::{}",
                    if item.label == "/inherited" {
                        "GET"
                    } else {
                        "Tempest route"
                    },
                    &item.label[1..]
                )
                .as_str()
            )
        );
    }
}

#[tokio::test]
async fn tempest_routes_resolve_and_rename_prefix_placeholders() {
    let source = r"<?php
\Tempest\Router\uri('/{locale}/books', locale: 'en');
\Tempest\Router\uri([\Routes\Books::class, 'authors'], locale: 'nl');
";
    let (backend, _dir, controller, uri) = project(source).await;
    let edited = r"<?php namespace Routes;
#[\Tempest\Router\Prefix('/{locale}')]
class Books {
    #[\Tempest\Router\Get('/books')]
    public function show(string $locale): void {}
    #[\Tempest\Router\Get('/authors')]
    public function authors(string $locale): string { return $locale; }
}
";
    open_php(&backend, &controller, edited).await;
    let locations = definitions(&backend, &controller, edited, "locale}')").await;
    assert_eq!(locations.len(), 2);
    assert_eq!(locations[0].range.start, position_of(edited, "$locale"));
    let offered = items(&backend, &uri, source, "uri('/").await;
    assert_eq!(offered.len(), 2);
    assert!(offered.iter().any(|item| item.label == "/{locale}/books"));
    let at = position_of(edited, "$locale");
    let changes = rename(&backend, &controller, at.line, at.character + 1, "language")
        .await
        .unwrap()
        .changes
        .unwrap();
    assert_eq!(changes[&controller].len(), 4, "{changes:?}");
    assert_eq!(changes[&uri].len(), 3, "{changes:?}");
}

#[tokio::test]
async fn tempest_routes_parameter_rename_includes_template_uri_calls() {
    let (backend, _dir, controller, _) = project("<?php").await;
    let uri = workspace_uri(&backend, "app/books.view.php");
    let source = r#"<a href="{{ \Tempest\Router\uri('/books/{book}', book: 'one') }}">📚</a>"#;
    open_php(&backend, &uri, source).await;
    let locations = definitions(&backend, &uri, source, "book: 'one'").await;
    assert_eq!(locations.len(), 1, "{locations:?}");
    assert_eq!(locations[0].uri, controller);
    let at = position_of(CONTROLLER, "$book, string");
    let changes = rename(&backend, &controller, at.line, at.character + 1, "volume")
        .await
        .unwrap()
        .changes
        .unwrap();
    let edits = &changes[&uri];
    assert_eq!(edits.len(), 2, "{changes:?}");
    assert!(edits.iter().all(|edit| edit.new_text == "volume"));
    assert!(
        edits
            .iter()
            .any(|edit| edit.range.start == position_of(source, "book: 'one'"))
    );
}

#[tokio::test]
async fn tempest_routes_do_not_panic_on_incomplete_prefix_attributes() {
    let source = r"<?php \Tempest\Router\uri('');";
    let (backend, _dir, controller, uri) = project(source).await;
    for prefix in [
        "#[\\Tempest\\Router\\Prefix]",
        "#[\\Tempest\\Router\\Prefix(other: '/api')]",
        "#[\\Tempest\\Router\\Prefix(UNKNOWN)]",
    ] {
        let edited = CONTROLLER.replace("class Books {", &format!("{prefix}\nclass Books {{"));
        open_php(&backend, &controller, &edited).await;
        let offered = items(&backend, &uri, source, "uri('").await;
        assert!(offered.iter().any(|item| item.label == "/home/{locale}"));
        assert!(!offered.iter().any(|item| item.label.starts_with("/books")));
    }
}

#[tokio::test]
async fn tempest_routes_callable_parameters_follow_the_first_route() {
    let source = r"<?php
use function Tempest\Router\uri;
uri([\Routes\Books::class, 'show'], );
uri('/alternate/{edition}', );
uri([\Routes\Books::class, 'show'], edition: 'query');
uri('/alternate/{edition}', edition: 'one');
/** @param '/alternate/{edition}'|'/missing' $action */
function link(string $action): void { uri($action, ); }
";
    let (backend, _dir, controller, uri) = project(source).await;
    let edited = CONTROLLER
        .replace("/books/{book}/edit", "/alternate/{edition}")
        .replace("$unused", "$edition");
    open_php(&backend, &controller, &edited).await;
    let primary = items(&backend, &uri, source, "'show'], ").await;
    assert!(primary.iter().any(|item| item.label == "book:"));
    assert!(!primary.iter().any(|item| item.label == "edition:"));
    let alternate = items(&backend, &uri, source, "'/alternate/{edition}', ").await;
    assert!(alternate.iter().any(|item| item.label == "edition:"));
    assert!(!alternate.iter().any(|item| item.label == "book:"));
    let union = items(&backend, &uri, source, "uri($action, ").await;
    assert!(union.iter().any(|item| item.label == "edition:"));
    assert!(
        definitions(&backend, &uri, source, "edition: 'query'")
            .await
            .is_empty()
    );
    assert_eq!(
        definitions(&backend, &uri, source, "edition: 'one'")
            .await
            .len(),
        1
    );
    let at = position_of(&edited, "$edition");
    let changes = rename(&backend, &controller, at.line, at.character + 1, "version")
        .await
        .unwrap()
        .changes
        .unwrap();
    let edits = &changes[&uri];
    // Both literal paths and the forwarded label name the alternate placeholder.
    assert_eq!(edits.len(), 3, "{changes:?}");
    assert!(
        !edits
            .iter()
            .any(|edit| edit.range.start == position_of(source, "edition: 'query'"))
    );
}

#[tokio::test]
async fn tempest_routes_escape_completion_edits_for_the_surrounding_quote() {
    let source = r#"<?php \Tempest\Router\uri(''); \Tempest\Router\uri("");"#;
    let (backend, _dir, controller, uri) = project(source).await;
    let edited = CONTROLLER.replace("/books/{book}", "/it\\'s/$name/\"quotes\"");
    open_php(&backend, &controller, &edited).await;
    for (needle, expected) in [
        ("uri('", "/it\\'s/$name/\"quotes\""),
        ("uri(\"", "/it's/\\$name/\\\"quotes\\\""),
    ] {
        let offered = items(&backend, &uri, source, needle).await;
        let item = offered
            .iter()
            .find(|item| item.label == "/it's/$name/\"quotes\"")
            .unwrap();
        let Some(CompletionItemTextEdit::TextEdit(edit)) = &item.text_edit else {
            panic!("missing replacement")
        };
        assert_eq!(edit.new_text, expected);
    }
}

#[tokio::test]
async fn tempest_routes_return_each_action_for_a_uri_shared_by_http_methods() {
    let source = r"<?php \Tempest\Router\uri('/books/{book}', book: 'one');";
    let (backend, _dir, controller, uri) = project(source).await;
    let other = workspace_uri(&backend, "app/Other.php");
    let alternate = r"<?php namespace Consumer; class Other { #[\Tempest\Router\Post('/books/{book}')] public function update(string $book): void {} }";
    open_php(&backend, &other, alternate).await;
    let locations = definitions(&backend, &uri, source, "book: 'one'").await;
    assert_eq!(locations.len(), 2, "{locations:?}");
    assert_eq!(locations[0].uri, other);
    assert_eq!(locations[0].range.start, position_of(alternate, "$book"));
    assert_eq!(locations[1].uri, controller);
}
