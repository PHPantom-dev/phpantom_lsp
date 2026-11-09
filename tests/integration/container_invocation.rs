//! Container calls forward names from the callable's actual declaration.

use crate::common::{
    completion_response, create_psr4_workspace, definition_locations, goto_definition_at, open_php,
    position_after, position_of, response_items, workspace_uri,
};
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{"autoload":{"psr-4":{"App\\":"app/"},"files":["framework.php"]}}"#;
const FRAMEWORK: &str = r#"<?php
namespace Tempest\Container {
    function invoke(callable|string $callable, mixed ...$params): mixed {}
    interface Container {
        public function invoke(callable|string $method, mixed ...$params): mixed;
    }
}
namespace Tempest {
    function invoke(callable|string $callable, mixed ...$params): mixed {}
}
namespace Illuminate\Contracts\Container {
    interface Container {
        public function call($callback, array $parameters = [], $defaultMethod = null);
    }
}
namespace Illuminate\Container {
    class Container implements \Illuminate\Contracts\Container\Container {
        public function call($callback, array $parameters = [], $defaultMethod = null) {}
    }
}
namespace {
    function app(): \Illuminate\Container\Container {}
}
"#;
const IMPORTER: &str = r#"<?php
namespace App;
class Importer {
    public static function import(string $path, int $limit = 10): void {}
}
"#;

fn project(composer: &str, files: &[(&str, &str)]) -> (phpantom_lsp::Backend, tempfile::TempDir) {
    let (backend, dir) = create_psr4_workspace(composer, files);
    for (path, source) in files {
        backend.update_ast(workspace_uri(&backend, path).as_str(), source);
    }
    (backend, dir)
}

#[tokio::test]
async fn tempest_invoke_helpers_follow_function_imports_and_invokable_classes() {
    let importer = "<?php namespace App; class Importer { public function __invoke(string $path, int $limit = 1): void {} }\n";
    let (backend, _dir) = project(
        COMPOSER,
        &[("framework.php", FRAMEWORK), ("app/Importer.php", importer)],
    );
    let uri = workspace_uri(&backend, "app/caller.php");
    for (imports, call) in [
        (
            "use function Tempest\\Container\\invoke;",
            "invoke(Importer::class, path: '/books')",
        ),
        (
            "use function Tempest\\Container\\invoke as execute;",
            "execute(callable: Importer::class, path: '/books')",
        ),
        (
            "use Tempest\\Container as DI;",
            "DI\\invoke(Importer::class, path: '/books')",
        ),
        (
            "",
            "\\Tempest\\Container\\invoke(Importer::class, path: '/books')",
        ),
        ("", "\\Tempest\\invoke(Importer::class, path: '/books')"),
    ] {
        let source = format!("<?php namespace Client; use App\\Importer; {imports}\n{call};\n");
        open_php(&backend, &uri, &source).await;
        let at = position_after(&source, "path");
        let items =
            response_items(completion_response(&backend, &uri, at.line, at.character, None).await)
                .unwrap_or_default();
        assert!(
            items
                .iter()
                .any(|item| item.insert_text.as_deref() == Some("path: ")),
            "{call}: {items:?}"
        );
        let locations =
            definition_locations(goto_definition_at(&backend, &uri, at.line, at.character).await);
        assert_eq!(locations.len(), 1, "{call}: {locations:?}");
        assert_eq!(
            locations[0].uri,
            workspace_uri(&backend, "app/Importer.php")
        );
        assert_eq!(locations[0].range.start, position_of(importer, "$path"));
    }
    let source = "<?php namespace Client; function invoke($callable, ...$args) {} invoke(\\App\\Importer::class, path: '/books');\n";
    open_php(&backend, &uri, source).await;
    let at = position_after(source, "path");
    let items =
        response_items(completion_response(&backend, &uri, at.line, at.character, None).await)
            .unwrap_or_default();
    assert!(
        !items
            .iter()
            .any(|item| item.insert_text.as_deref() == Some("path: "))
    );
    assert!(
        definition_locations(goto_definition_at(&backend, &uri, at.line, at.character).await)
            .is_empty()
    );
}

#[tokio::test]
async fn container_invocation_reads_captured_callables_at_their_creation_site() {
    let source = r#"<?php
namespace App {
    class First { public function import(string $path): void {} }
    class Second { public function import(string $other): void {} }
    $worker = new First();
    $callback = $worker->import(...);
    $worker = new Second();
}
namespace Client {
    function run(\Tempest\Container\Container $container): void {}
    /** @var \Tempest\Container\Container $container */
    $container->invoke($callback, path: '/books');
}
"#;
    let (backend, _dir) = project(COMPOSER, &[("framework.php", FRAMEWORK)]);
    let uri = workspace_uri(&backend, "app/caller.php");
    open_php(&backend, &uri, source).await;
    let at = position_of(source, "path:");
    let items =
        response_items(completion_response(&backend, &uri, at.line, at.character, None).await)
            .unwrap_or_default();
    assert!(
        items
            .iter()
            .any(|item| item.insert_text.as_deref() == Some("path: ")),
        "{items:?}"
    );
    assert!(
        !items
            .iter()
            .any(|item| item.insert_text.as_deref() == Some("other: "))
    );
    let locations =
        definition_locations(goto_definition_at(&backend, &uri, at.line, at.character).await);
    assert_eq!(locations[0].uri, uri);
    assert_eq!(locations[0].range.start, position_of(source, "$path"));
}

#[tokio::test]
async fn tempest_container_completes_and_navigates_forwarded_named_arguments() {
    let (backend, _dir) = project(
        COMPOSER,
        &[("framework.php", FRAMEWORK), ("app/Importer.php", IMPORTER)],
    );
    let uri = workspace_uri(&backend, "app/caller.php");
    let source = r#"<?php
namespace Client;
use App\Importer as Action;
use Tempest\Container\Container as Injector;
function run(Injector $container): void {
    $container->invoke([Action::class, 'import'], path: '/books', limit: 5);
    $container->invoke([Action::class, 'import'], pa);
    $container->invoke(method: [Action::class, 'import'], li);
}
"#;
    open_php(&backend, &uri, source).await;
    for (needle, expected) in [("], pa", "path: "), ("], li", "limit: ")] {
        let at = position_after(source, needle);
        let items =
            response_items(completion_response(&backend, &uri, at.line, at.character, None).await)
                .unwrap();
        assert!(
            items
                .iter()
                .any(|item| item.insert_text.as_deref() == Some(expected)),
            "{items:?}"
        );
    }
    for name in ["path", "limit"] {
        let at = position_of(source, &format!("{name}:"));
        let locations =
            definition_locations(goto_definition_at(&backend, &uri, at.line, at.character).await);
        let target = workspace_uri(&backend, "app/Importer.php");
        assert_eq!(
            locations,
            vec![Location {
                uri: target,
                range: Range {
                    start: position_of(IMPORTER, &format!("${name}")),
                    end: position_after(IMPORTER, &format!("${name}"))
                },
            }]
        );
    }
}

#[tokio::test]
async fn laravel_container_completes_and_navigates_parameter_array_keys() {
    let (backend, _dir) = project(
        COMPOSER,
        &[("framework.php", FRAMEWORK), ("app/Importer.php", IMPORTER)],
    );
    let uri = workspace_uri(&backend, "app/caller.php");
    let source = r#"<?php
namespace Client;
use App\Importer;
app()->call([Importer::class, 'import'], ['path' => '/books']);
app()->call([Importer::class, 'import'], ['pa']);
app()->call(parameters: ['li'], callback: [Importer::class, 'import']);
app()->call('App\\Importer::import', ['pat']);
"#;
    open_php(&backend, &uri, source).await;
    for (needle, expected) in [
        ("['pa", "path"),
        ("['li", "limit"),
        ("::import', ['pat", "path"),
    ] {
        let at = position_after(source, needle);
        let items =
            response_items(completion_response(&backend, &uri, at.line, at.character, None).await)
                .unwrap();
        let item = items
            .iter()
            .find(|item| item.label == expected)
            .unwrap_or_else(|| panic!("{items:?}"));
        let Some(CompletionItemTextEdit::TextEdit(edit)) = &item.text_edit else {
            panic!("{item:?}");
        };
        assert_eq!(edit.new_text, expected);
    }
    let at = position_after(source, "['pat");
    let locations =
        definition_locations(goto_definition_at(&backend, &uri, at.line, at.character).await);
    assert_eq!(
        locations,
        vec![Location {
            uri: workspace_uri(&backend, "app/Importer.php"),
            range: Range {
                start: position_of(IMPORTER, "$path"),
                end: position_after(IMPORTER, "$path")
            },
        }]
    );
}

#[tokio::test]
async fn container_callbacks_preserve_receiver_generics() {
    let worker = r#"<?php
namespace App;
/** @template T */
class Worker {
    /** @param T $record */
    public function process(object $record, string $label = ''): void {}
}

class Book {}
"#;
    let source = r#"<?php
namespace App;
use Tempest\Container\Container;
/** @param Worker<Book> $worker */
function run(Container $container, Worker $worker): void {
    $container->invoke([$worker, 'process'], rec);
    $container->invoke([$worker, 'process'], record: new Book());
}
"#;
    let (backend, _dir) = project(
        COMPOSER,
        &[("framework.php", FRAMEWORK), ("app/Worker.php", worker)],
    );
    let uri = workspace_uri(&backend, "app/caller.php");
    open_php(&backend, &uri, source).await;
    let at = position_after(source, "], rec");
    let items =
        response_items(completion_response(&backend, &uri, at.line, at.character, None).await)
            .unwrap();
    let record = items
        .iter()
        .find(|item| item.insert_text.as_deref() == Some("record: "))
        .unwrap_or_else(|| panic!("{items:?}"));
    assert!(record.label.contains("Book"), "{record:?}");
    let at = position_of(source, "record:");
    let locations =
        definition_locations(goto_definition_at(&backend, &uri, at.line, at.character).await);
    assert_eq!(locations[0].uri, workspace_uri(&backend, "app/Worker.php"));
    assert_eq!(
        locations[0].range.start,
        position_of(worker, "$record, string")
    );
}

#[tokio::test]
async fn container_callable_forms_use_their_declarations() {
    let targets = r#"<?php
namespace {
    function import_books(string $globalPath): void {}
    class Importer { public static function import(string $globalPath): void {} }
}
namespace App {
    function import_books(string $path): void {}
    trait Imports {
        public function import(string $path, int $limit = 10): void {}
    }
    class ParentImporter { use Imports; }
    class Invokable extends ParentImporter {
        public function __invoke(string $path): void {}
    }
}
"#;
    let composer =
        r#"{"autoload":{"psr-4":{"App\\":"app/"},"files":["framework.php","targets.php"]}}"#;
    let (backend, _dir) = project(
        composer,
        &[
            ("framework.php", FRAMEWORK),
            ("targets.php", targets),
            ("app/Importer.php", IMPORTER),
        ],
    );
    let uri = workspace_uri(&backend, "app/caller.php");
    for (callback, setup, declaration, parameter) in [
        ("Action::import(...)", "", "import(string $path", "path"),
        (
            "$callback",
            "$callback = Action::import(...);",
            "import(string $path",
            "path",
        ),
        (
            "'App\\\\Importer::import'",
            "",
            "import(string $path",
            "path",
        ),
        (
            "'App\\\\import_books'",
            "",
            "import_books(string $path",
            "path",
        ),
        ("import_alias(...)", "", "import_books(string $path", "path"),
        (
            "'import_books'",
            "",
            "import_books(string $globalPath",
            "globalPath",
        ),
        (
            "['Importer', 'import']",
            "",
            "import(string $globalPath",
            "globalPath",
        ),
        (
            "[Action::class, 'IMPORT']",
            "",
            "import(string $path",
            "path",
        ),
        (
            "[$worker, 'import']",
            "$worker = new Invokable();",
            "import(string $path",
            "path",
        ),
        ("new Invokable()", "", "__invoke(string $path", "path"),
        ("Invokable::class", "", "__invoke(string $path", "path"),
        ("'App\\\\Invokable'", "", "__invoke(string $path", "path"),
        (
            "static fn (string $path): string => $path",
            "",
            "fn (string $path",
            "path",
        ),
        (
            "function (string $path) {}",
            "",
            "function (string $path",
            "path",
        ),
    ] {
        let source = format!(
            "<?php\nnamespace Client;\nuse App\\Importer as Action;\nuse App\\Invokable;\nuse function App\\import_books as import_alias;\nfunction run(\\Tempest\\Container\\Container $container): void {{\n{setup}\n$container->invoke({callback}, {parameter}: '/books');\n}}\n"
        );
        open_php(&backend, &uri, &source).await;
        let at = position_of(&source, &format!("{parameter}:"));
        let items =
            response_items(completion_response(&backend, &uri, at.line, at.character, None).await)
                .unwrap_or_default();
        assert!(
            items
                .iter()
                .any(|item| item.insert_text.as_deref() == Some(&format!("{parameter}: "))),
            "{callback}: {items:?}"
        );
        let locations =
            definition_locations(goto_definition_at(&backend, &uri, at.line, at.character).await);
        let (target_uri, target_source) =
            if callback.starts_with("static fn") || callback.starts_with("function") {
                (uri.clone(), source.as_str())
            } else if callback.contains("Action")
                || callback.contains("App\\\\Importer")
                || callback == "$callback"
            {
                (workspace_uri(&backend, "app/Importer.php"), IMPORTER)
            } else {
                (workspace_uri(&backend, "targets.php"), targets)
            };
        let before = declaration.split_once('$').unwrap().0;
        let mut expected = position_after(target_source, &format!("{before}${parameter}"));
        expected.character -= parameter.len() as u32 + 1;
        assert_eq!(locations.len(), 1, "{callback}: {locations:?}");
        assert_eq!(locations[0].uri, target_uri, "{callback}");
        assert_eq!(locations[0].range.start, expected, "{callback}");
    }
}

#[tokio::test]
async fn laravel_container_resolves_instance_class_callbacks_and_default_methods() {
    let importer = IMPORTER.replace("static function", "function");
    let (backend, _dir) = project(
        COMPOSER,
        &[
            ("framework.php", FRAMEWORK),
            ("app/Importer.php", &importer),
        ],
    );
    let uri = workspace_uri(&backend, "app/caller.php");
    for call in [
        "app()->call('App\\\\Importer@import', ['path' => '/books'])",
        "app()->call(\\App\\Importer::class, ['path' => '/books'], 'import')",
        "app()->call(callback: 'App\\\\Importer', defaultMethod: 'import', parameters: ['path' => '/books'])",
        "app()->call([new \\App\\Importer(), 'import'], array('path' => '/books'))",
        "app()->call([new \\App\\Importer(), 'import'], ['path' => '/books'], null)",
    ] {
        let source = format!("<?php\nnamespace Client;\n{call};\n");
        open_php(&backend, &uri, &source).await;
        let at = position_after(&source, "'pat");
        let items =
            response_items(completion_response(&backend, &uri, at.line, at.character, None).await)
                .unwrap_or_default();
        assert!(
            items.iter().any(|item| item.label == "path"),
            "{call}: {items:?}"
        );
        let locations =
            definition_locations(goto_definition_at(&backend, &uri, at.line, at.character).await);
        assert_eq!(locations.len(), 1, "{call}: {locations:?}");
        assert_eq!(
            locations[0].range.start,
            position_of(&importer, "$path"),
            "{call}"
        );
    }
}

#[tokio::test]
async fn container_invocation_completes_in_incomplete_calls_and_tracks_unsaved_targets() {
    let (backend, _dir) = project(
        COMPOSER,
        &[("framework.php", FRAMEWORK), ("app/Importer.php", IMPORTER)],
    );
    let uri = workspace_uri(&backend, "app/caller.php");
    for source in [
        "<?php function run(\\Tempest\\Container\\Container $c) { $c->invoke([\\App\\Importer::class, 'import'], pa",
        "<?php app()->call([\\App\\Importer::class, 'import'], ['pa",
    ] {
        open_php(&backend, &uri, source).await;
        let at = position_after(source, "pa");
        let items =
            response_items(completion_response(&backend, &uri, at.line, at.character, None).await)
                .unwrap_or_default();
        assert!(
            items
                .iter()
                .any(|item| item.label == "path" || item.insert_text.as_deref() == Some("path: ")),
            "{source}: {items:?}"
        );
    }
    let modified = IMPORTER.replace("$path", "$file");
    let target = workspace_uri(&backend, "app/Importer.php");
    open_php(&backend, &target, &modified).await;
    let source = "<?php app()->call([\\App\\Importer::class, 'import'], ['file' => '/books']);\n";
    open_php(&backend, &uri, source).await;
    let at = position_after(source, "['fi");
    let items =
        response_items(completion_response(&backend, &uri, at.line, at.character, None).await)
            .unwrap();
    assert_eq!(items[0].label, "file");
    let locations =
        definition_locations(goto_definition_at(&backend, &uri, at.line, at.character).await);
    assert_eq!(locations[0].uri, target);
    assert_eq!(locations[0].range.start, position_of(&modified, "$file"));
}

#[tokio::test]
async fn container_invocation_ignores_nonpublic_methods_and_dynamic_signatures() {
    let source = r#"<?php
use Tempest\Container\Container;
class Hidden {
    private static function secret(string $path): void {}
    public function instance(string $path): void {}
}
/** @method static void invented(string $path) */
class Magic {}
/** @param callable(string): void $callback */
function run(Container $container, callable $callback, string $name): void {
    $container->invoke([Hidden::class, 'secret'], path: '/books');
    $container->invoke([Hidden::class, 'instance'], path: '/books');
    $container->invoke([Magic::class, 'invented'], path: '/books');
    $container->invoke([Hidden::class, $name], path: '/books');
    $container->invoke($callback(...), path: '/books');
    $container->invoke(($callback)(...), path: '/books');
}
"#;
    let (backend, _dir) = project(COMPOSER, &[("framework.php", FRAMEWORK)]);
    let uri = workspace_uri(&backend, "app/caller.php");
    open_php(&backend, &uri, source).await;
    for (line, text) in source
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains("->invoke("))
    {
        let at = Position {
            line: line as u32,
            character: text.find("path:").unwrap() as u32 + 2,
        };
        let items =
            response_items(completion_response(&backend, &uri, at.line, at.character, None).await)
                .unwrap_or_default();
        assert!(
            !items
                .iter()
                .any(|item| item.insert_text.as_deref() == Some("path: ")),
            "{text}: {items:?}"
        );
        let locations =
            definition_locations(goto_definition_at(&backend, &uri, at.line, at.character).await);
        assert!(locations.is_empty(), "{text}: {locations:?}");
    }
}

#[tokio::test]
async fn container_completion_filters_bound_names_and_preserves_edit_ranges() {
    let (backend, _dir) = project(
        COMPOSER,
        &[("framework.php", FRAMEWORK), ("app/Importer.php", IMPORTER)],
    );
    let uri = workspace_uri(&backend, "app/caller.php");
    let source = "<?php\r\nuse App\\Importer;\r\nfunction run(\\Tempest\\Container\\Container $c): void {\r\n$c->invoke([Importer::class, 'import'], '/books', li);\r\n$c->invoke([Importer::class, 'import'], path: '/books', li);\r\n}\r\n/* 📚 */ app()->call([Importer::class, 'import'], ['path' => '/books', \"limit\" => 2]);\r\n";
    open_php(&backend, &uri, source).await;
    for needle in ["'/books', li", "path: '/books', li"] {
        let at = position_after(source, needle);
        let items =
            response_items(completion_response(&backend, &uri, at.line, at.character, None).await)
                .unwrap_or_default();
        assert_eq!(
            items
                .iter()
                .filter(|item| item
                    .detail
                    .as_deref()
                    .is_some_and(|detail| detail.starts_with("Named argument")))
                .filter_map(|item| item.insert_text.as_deref())
                .collect::<Vec<_>>(),
            ["limit: "]
        );
    }
    let at = position_after(source, "\"li");
    let items =
        response_items(completion_response(&backend, &uri, at.line, at.character, None).await)
            .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "limit");
    assert_eq!(
        items[0].text_edit,
        Some(CompletionItemTextEdit::TextEdit(TextEdit {
            range: Range {
                start: position_after(source, "\""),
                end: position_after(source, "\"limit")
            },
            new_text: "limit".to_string(),
        }))
    );
    let source =
        "<?php app()->call([\\App\\Importer::class, 'import'], ['path' => '/books', '']);\n";
    open_php(&backend, &uri, source).await;
    let at = position_after(source, "'/books', '");
    let items =
        response_items(completion_response(&backend, &uri, at.line, at.character, None).await)
            .unwrap();
    assert_eq!(
        items
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>(),
        ["limit"]
    );
}

#[tokio::test]
async fn container_calls_do_not_guess_unrelated_or_invalid_targets() {
    let framework = format!(
        "{FRAMEWORK}\nnamespace Other {{\nclass Container {{\npublic function invoke($method, ...$params) {{}}\npublic function call($callback, array $parameters = []) {{}}\n}}\n}}\n"
    );
    let (backend, _dir) = project(
        COMPOSER,
        &[
            ("framework.php", &framework),
            ("app/Importer.php", IMPORTER),
        ],
    );
    let uri = workspace_uri(&backend, "app/caller.php");
    for expression in [
        "$other->invoke([Importer::class, 'import'], path: '/books')",
        "$other->call([Importer::class, 'import'], ['path' => '/books'])",
        "$container->invoke([Importer::class, 'missing'], path: '/books')",
        "$container->invoke($unknown, path: '/books')",
        "$container->invoke('unknown_callable', path: '/books')",
        "$container->invoke(...$arguments, path: '/books')",
        "$container->invoke([Importer::class, 'import'], absent: '/books')",
        "app()->call([Importer::class, 'import'], ['limit' => 'path'])",
        "app()->call([Importer::class, 'import'], ['limit' => ['path' => '/books']])",
        "app()->call([Importer::class, 'import'], ['path' => '/books'], $unknown)",
        "app()->call([Importer::class, 'import'], ['absent' => '/books'])",
    ] {
        let source = format!(
            "<?php\nuse App\\Importer;\nfunction run(\\Tempest\\Container\\Container $container, \\Other\\Container $other, $unknown, array $arguments): void {{\n{expression};\n}}\n"
        );
        open_php(&backend, &uri, &source).await;
        let needle = if expression.contains("absent") {
            "absent"
        } else {
            "path"
        };
        let at = position_after(&source, needle);
        let locations =
            definition_locations(goto_definition_at(&backend, &uri, at.line, at.character).await);
        assert!(locations.is_empty(), "{expression}: {locations:?}");
    }
}
