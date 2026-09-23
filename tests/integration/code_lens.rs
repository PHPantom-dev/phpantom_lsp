use crate::common::{create_psr4_workspace, create_test_backend, open_php};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// Helper: open a file in the backend and return its code lenses.
fn get_code_lenses(backend: &phpantom_lsp::Backend, uri: &str, content: &str) -> Vec<CodeLens> {
    backend.update_ast(uri, content);
    backend.handle_code_lens(uri, content).unwrap_or_default()
}

/// Helper: extract just the titles from a list of code lenses.
fn lens_titles(lenses: &[CodeLens]) -> Vec<&str> {
    lenses
        .iter()
        .filter_map(|l| l.command.as_ref().map(|c| c.title.as_str()))
        .collect()
}

#[tokio::test]
async fn zero_candidate_reference_lenses_need_no_resolve_requests() {
    let content = r#"<?php
namespace App;

final class LargeTestCase {
    public function case01(): void {}
    public function case02(): void {}
    public function case03(): void {}
    public function case04(): void {}
    public function case05(): void {}
    public function case06(): void {}
    public function case07(): void {}
    public function case08(): void {}
    public function case09(): void {}
    public function case10(): void {}
    public function case11(): void {}
    public function case12(): void {}
    public function case13(): void {}
    public function case14(): void {}
    public function case15(): void {}
    public function case16(): void {}
    public function case17(): void {}
    public function case18(): void {}
    public function case19(): void {}
    public function case20(): void {}
    public function case21(): void {}
    public function case22(): void {}
    public function case23(): void {}
    public function case24(): void {}
    public function case25(): void {}
    public function case26(): void {}
    public function case27(): void {}
    public function case28(): void {}
    public function case29(): void {}
    public function case30(): void {}
    public function case31(): void {}
    public function case32(): void {}
}
"#;
    let (backend, dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[("src/LargeTestCase.php", content)],
    );
    let uri = Url::from_file_path(dir.path().join("src/LargeTestCase.php")).unwrap();
    open_php(&backend, &uri, content).await;

    // Drive workspace indexing through the public LSP path, as a real client
    // would before requesting lenses from an index reported as ready.
    backend
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position::new(3, 12),
            },
            context: ReferenceContext {
                include_declaration: true,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();

    let lenses = backend
        .code_lens(CodeLensParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .expect("expected declaration reference lenses");
    let reference_lenses: Vec<_> = lenses
        .iter()
        .filter(|lens| {
            lens.command
                .as_ref()
                .is_some_and(|command| command.title.ends_with("references"))
        })
        .collect();

    assert_eq!(reference_lenses.len(), 33);
    assert!(reference_lenses.iter().all(|lens| {
        lens.command
            .as_ref()
            .is_some_and(|command| command.title == "0 references")
            && lens.data.is_none()
    }));
}

#[tokio::test]
async fn member_reference_lens_resolves_only_the_declaring_hierarchy() {
    let order = r#"<?php
namespace App;
final class Order {
    public function save(): void {}
}
function persist(Order $order): void {
    $order->save();
    $order->save();
}
"#;
    let unrelated = r#"<?php
namespace App;
final class Unrelated {
    public function save(): void {}
}
function persistUnrelated(Unrelated $value): void {
    $value->save();
    $value->save();
    $value->save();
}
"#;
    let (backend, dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[("src/Order.php", order), ("src/Unrelated.php", unrelated)],
    );
    let uri = Url::from_file_path(dir.path().join("src/Order.php")).unwrap();
    open_php(&backend, &uri, order).await;

    backend
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position::new(3, 20),
            },
            context: ReferenceContext {
                include_declaration: false,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();

    let lenses = backend
        .code_lens(CodeLensParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .expect("expected declaration reference lenses");
    let lens = lenses
        .into_iter()
        .find(|lens| lens.range.start.line == 3 && lens.command.is_none())
        .expect("expected an unresolved reference lens above Order::save");

    let resolved = backend
        .code_lens_resolve(lens)
        .await
        .expect("reference lens should resolve");
    assert_eq!(
        resolved
            .command
            .as_ref()
            .map(|command| command.title.as_str()),
        Some("2 references")
    );
    let locations: Vec<Location> = serde_json::from_value(
        resolved
            .command
            .as_ref()
            .and_then(|command| command.arguments.as_ref())
            .and_then(|arguments| arguments.get(2))
            .cloned()
            .expect("expected reference locations"),
    )
    .expect("reference targets should be locations");
    assert_eq!(locations.len(), 2);
    assert!(locations.iter().all(|location| location.uri == uri));
}

#[tokio::test]
async fn refresh_capable_clients_receive_only_warm_member_reference_lenses() {
    let content = r#"<?php
namespace App;
final class Order {
    public function save(): void {}
}
function persist(Order $order): void {
    $order->save();
}
"#;
    let (backend, dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[("src/Order.php", content)],
    );
    let initialize = backend
        .initialize(
            serde_json::from_value(serde_json::json!({
                "capabilities": {
                    "workspace": {
                        "codeLens": { "refreshSupport": true }
                    }
                }
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(matches!(
        initialize.capabilities.code_lens_provider,
        Some(CodeLensOptions {
            resolve_provider: Some(true)
        })
    ));

    let uri = Url::from_file_path(dir.path().join("src/Order.php")).unwrap();
    open_php(&backend, &uri, content).await;
    backend
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position::new(3, 20),
            },
            context: ReferenceContext {
                include_declaration: false,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();

    let params = CodeLensParams {
        text_document: TextDocumentIdentifier { uri },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    let cold = backend
        .code_lens(params.clone())
        .await
        .unwrap()
        .unwrap_or_default();
    let cold_lens = cold
        .iter()
        .find(|lens| lens.range.start.line == 3)
        .unwrap_or_else(|| panic!("the member lens should hold its line while cold: {cold:?}"));
    assert_eq!(
        cold_lens
            .command
            .as_ref()
            .map(|command| command.title.as_str()),
        Some("- references"),
        "a cold lens carries a placeholder, not a count it cannot back up"
    );
    assert!(
        cold_lens.data.is_none(),
        "and no resolve payload, which would make the client resolve it eagerly"
    );

    let warm = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let lenses = backend
                .code_lens(params.clone())
                .await
                .unwrap()
                .unwrap_or_default();
            if let Some(lens) = lenses.into_iter().find(|lens| {
                lens.range.start.line == 3
                    && lens
                        .command
                        .as_ref()
                        .is_some_and(|command| command.title == "1 reference")
            }) {
                break lens;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("background member-reference cache did not warm");
    assert!(warm.data.is_none());
}

#[tokio::test]
async fn class_and_function_reference_lenses_resolve_exact_locations() {
    let content = r#"<?php
namespace App;
final class Widget {}
function makeWidget(): Widget { return new Widget(); }
makeWidget();
makeWidget();
"#;
    let (backend, dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[("src/functions.php", content)],
    );
    let uri = Url::from_file_path(dir.path().join("src/functions.php")).unwrap();
    open_php(&backend, &uri, content).await;
    backend
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position::new(2, 12),
            },
            context: ReferenceContext {
                include_declaration: false,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();

    let lenses = backend
        .code_lens(CodeLensParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .expect("expected class and function reference lenses");

    for (line, expected_title) in [(2, "2 references"), (3, "2 references")] {
        let lens = lenses
            .iter()
            .find(|lens| lens.range.start.line == line && lens.command.is_none())
            .unwrap_or_else(|| panic!("expected an unresolved lens on line {line}: {lenses:?}"));
        let resolved = backend
            .code_lens_resolve(lens.clone())
            .await
            .expect("reference lens should resolve");
        assert_eq!(
            resolved
                .command
                .as_ref()
                .map(|command| command.title.as_str()),
            Some(expected_title)
        );
    }
}

/// The title of the reference lens on `line`, resolved the way a client
/// that displays it does.
async fn resolved_reference_title(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    line: u32,
) -> Option<String> {
    let lenses = backend
        .code_lens(CodeLensParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .unwrap_or_default();
    let lens = lenses
        .into_iter()
        .find(|lens| lens.range.start.line == line)?;
    backend
        .code_lens_resolve(lens)
        .await
        .unwrap()
        .command
        .map(|command| command.title)
}

/// Drive workspace indexing the way a client does before asking for lenses.
async fn warm_workspace_index(backend: &phpantom_lsp::Backend, uri: &Url, position: Position) {
    backend
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            context: ReferenceContext {
                include_declaration: false,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn a_function_lens_counts_unqualified_calls_from_a_namespaced_file() {
    let helpers = "<?php\nfunction helper(): void {}\n";
    let service =
        "<?php\nnamespace App;\nfunction run(): void {\n    helper();\n    helper();\n}\n";
    let (backend, dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[("src/helpers.php", helpers), ("src/Service.php", service)],
    );
    let uri = Url::from_file_path(dir.path().join("src/helpers.php")).unwrap();
    open_php(&backend, &uri, helpers).await;
    warm_workspace_index(&backend, &uri, Position::new(1, 9)).await;

    assert_eq!(
        resolved_reference_title(&backend, &uri, 1).await.as_deref(),
        Some("2 references"),
        "an unqualified call in a namespaced file falls back to the global function"
    );
}

async fn close_document(backend: &phpantom_lsp::Backend, uri: &Url) {
    backend
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
        })
        .await;
}

/// The workspace index is walked once, so a file the editor closes has to
/// stay in it: dropping it on close took its references out of every
/// lens until an explicit Find References refreshed the index.
#[tokio::test]
async fn closing_a_workspace_file_keeps_its_references_in_the_lens() {
    let helpers = "<?php\nfunction helper(): void {}\n";
    let service =
        "<?php\nnamespace App;\nfunction run(): void {\n    helper();\n    helper();\n}\n";
    let (backend, dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[("src/helpers.php", helpers), ("src/Service.php", service)],
    );
    let helpers_uri = Url::from_file_path(dir.path().join("src/helpers.php")).unwrap();
    open_php(&backend, &helpers_uri, helpers).await;
    warm_workspace_index(&backend, &helpers_uri, Position::new(1, 9)).await;

    let service_uri = Url::from_file_path(dir.path().join("src/Service.php")).unwrap();
    open_php(&backend, &service_uri, service).await;
    close_document(&backend, &service_uri).await;

    assert_eq!(
        resolved_reference_title(&backend, &helpers_uri, 1)
            .await
            .as_deref(),
        Some("2 references"),
        "the closed file's calls must still be counted"
    );
}

/// Closing a buffer discards its unsaved edits, so what the index keeps is
/// the file on disk, not the buffer the editor last sent.
#[tokio::test]
async fn closing_a_workspace_file_reindexes_it_from_disk() {
    let helpers = "<?php\nfunction helper(): void {}\n";
    let service =
        "<?php\nnamespace App;\nfunction run(): void {\n    helper();\n    helper();\n}\n";
    let (backend, dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[("src/helpers.php", helpers), ("src/Service.php", service)],
    );
    let helpers_uri = Url::from_file_path(dir.path().join("src/helpers.php")).unwrap();
    open_php(&backend, &helpers_uri, helpers).await;
    warm_workspace_index(&backend, &helpers_uri, Position::new(1, 9)).await;

    let service_uri = Url::from_file_path(dir.path().join("src/Service.php")).unwrap();
    let unsaved = "<?php\nnamespace App;\nfunction run(): void {\n    helper();\n    helper();\n    helper();\n}\n";
    open_php(&backend, &service_uri, unsaved).await;
    assert_eq!(
        resolved_reference_title(&backend, &helpers_uri, 1)
            .await
            .as_deref(),
        Some("3 references"),
        "the open buffer's calls are counted while it is open"
    );

    close_document(&backend, &service_uri).await;
    assert_eq!(
        resolved_reference_title(&backend, &helpers_uri, 1)
            .await
            .as_deref(),
        Some("2 references"),
        "the unsaved call must go away with the buffer"
    );
}

#[tokio::test]
async fn a_function_lens_ignores_the_case_a_call_is_spelled_with() {
    let helpers = "<?php\nfunction helper(): void {}\n";
    let service = "<?php\nnamespace App;\nfunction run(): void {\n    HELPER();\n}\n";
    let (backend, dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[("src/helpers.php", helpers), ("src/Service.php", service)],
    );
    let uri = Url::from_file_path(dir.path().join("src/helpers.php")).unwrap();
    open_php(&backend, &uri, helpers).await;
    warm_workspace_index(&backend, &uri, Position::new(1, 9)).await;

    assert_eq!(
        resolved_reference_title(&backend, &uri, 1).await.as_deref(),
        Some("1 reference"),
        "PHP function names are case-insensitive"
    );
}

/// A method PHP or Laravel can reach through a static call is indexed under
/// the static member key, so the lens must not answer a conclusive zero from
/// the instance key alone.
#[tokio::test]
async fn statically_forwarded_instance_method_is_not_reported_as_zero() {
    let (backend, dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/", "Illuminate\\": "vendor/illuminate/" } } }"#,
        &[
            (
                "vendor/illuminate/Model.php",
                "<?php namespace Illuminate\\Database\\Eloquent; abstract class Model { public static function query() {} }",
            ),
            (
                "vendor/illuminate/Builder.php",
                "<?php namespace Illuminate\\Database\\Eloquent; class Builder { /** @return $this */ public function where($c) { return $this; } }",
            ),
            (
                "src/Models/UserBuilder.php",
                r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Builder;
class UserBuilder extends Builder {
    /** @return $this */
    public function active() { return $this; }
}
"#,
            ),
            (
                "src/Models/User.php",
                r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Attributes\UseEloquentBuilder;
#[UseEloquentBuilder(UserBuilder::class)]
class User extends Model {}
"#,
            ),
            (
                "usage.php",
                "<?php\nuse App\\Models\\User;\n\nUser::active();\n",
            ),
        ],
    );
    for path in [
        "vendor/illuminate/Builder.php",
        "vendor/illuminate/Model.php",
        "src/Models/UserBuilder.php",
        "src/Models/User.php",
        "usage.php",
    ] {
        let uri = Url::from_file_path(dir.path().join(path)).unwrap();
        let text = std::fs::read_to_string(dir.path().join(path)).unwrap();
        open_php(&backend, &uri, &text).await;
    }

    let builder_uri = Url::from_file_path(dir.path().join("src/Models/UserBuilder.php")).unwrap();
    // Drive the workspace index the way a client would before asking for lenses.
    backend
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(builder_uri.clone()),
                position: Position::new(5, 21),
            },
            context: ReferenceContext {
                include_declaration: false,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap();

    let lenses = backend
        .code_lens(CodeLensParams {
            text_document: TextDocumentIdentifier::new(builder_uri),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .expect("expected declaration reference lenses");
    let lens = lenses
        .into_iter()
        .find(|lens| lens.range.start.line == 5)
        .expect("expected a reference lens above UserBuilder::active");
    assert!(
        lens.command.is_none(),
        "`User::active()` is indexed as a static access, so a zero drawn from \
         the instance key alone would contradict Find References: {lens:?}"
    );

    let resolved = backend
        .code_lens_resolve(lens)
        .await
        .expect("reference lens should resolve");
    assert_eq!(
        resolved
            .command
            .as_ref()
            .map(|command| command.title.as_str()),
        Some("1 reference")
    );
}

// ─── Basic Override Detection ───────────────────────────────────────────────

#[test]
fn parent_class_method_override() {
    let backend = create_test_backend();
    let content = r#"<?php
class Animal {
    public function speak(): string { return ''; }
    public function eat(): void {}
}

class Dog extends Animal {
    public function speak(): string { return 'woof'; }
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);
    let titles = lens_titles(&lenses);

    assert_eq!(titles.len(), 1);
    assert_eq!(titles[0], "↑ Animal::speak");
}

#[test]
fn interface_method_implementation() {
    let backend = create_test_backend();
    let content = r#"<?php
interface Greetable {
    public function greet(): string;
}

class Greeter implements Greetable {
    public function greet(): string { return 'hello'; }
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);
    let titles = lens_titles(&lenses);

    assert_eq!(titles.len(), 1);
    assert_eq!(titles[0], "◆ Greetable::greet");
}

#[test]
fn no_lens_for_methods_without_prototype() {
    let backend = create_test_backend();
    let content = r#"<?php
class Standalone {
    public function doSomething(): void {}
    public function doMore(): void {}
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);

    assert!(lenses.is_empty());
}

#[test]
fn multiple_overrides_in_one_class() {
    let backend = create_test_backend();
    let content = r#"<?php
class Base {
    public function foo(): void {}
    public function bar(): void {}
    public function baz(): void {}
}

class Child extends Base {
    public function foo(): void {}
    public function bar(): void {}
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);
    let titles = lens_titles(&lenses);

    assert_eq!(titles.len(), 2);
    assert!(titles.contains(&"↑ Base::foo"));
    assert!(titles.contains(&"↑ Base::bar"));
}

// ─── Inheritance Chain ──────────────────────────────────────────────────────

#[test]
fn grandparent_override() {
    let backend = create_test_backend();
    let content = r#"<?php
class GrandParent_ {
    public function legacy(): void {}
}

class Parent_ extends GrandParent_ {
}

class Child extends Parent_ {
    public function legacy(): void {}
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);
    let titles = lens_titles(&lenses);

    assert_eq!(titles.len(), 1);
    // Should point to the grandparent since that's where the method
    // is actually declared.
    assert_eq!(titles[0], "↑ GrandParent_::legacy");
}

#[test]
fn parent_overrides_grandparent_lens_points_to_parent() {
    let backend = create_test_backend();
    let content = r#"<?php
class A {
    public function run(): void {}
}

class B extends A {
    public function run(): void {}
}

class C extends B {
    public function run(): void {}
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);

    // B overrides A::run, C overrides B::run (nearest ancestor wins)
    let b_lens: Vec<_> = lenses
        .iter()
        .filter(|l| {
            let line = l.range.start.line;
            // B::run is around line 7
            line > 5 && line < 9
        })
        .collect();
    let c_lens: Vec<_> = lenses
        .iter()
        .filter(|l| {
            let line = l.range.start.line;
            // C::run is around line 11
            line > 9
        })
        .collect();

    assert_eq!(b_lens.len(), 1);
    assert_eq!(b_lens[0].command.as_ref().unwrap().title, "↑ A::run");

    assert_eq!(c_lens.len(), 1);
    assert_eq!(c_lens[0].command.as_ref().unwrap().title, "↑ B::run");
}

// ─── Trait Methods ──────────────────────────────────────────────────────────

#[test]
fn trait_method_override() {
    let backend = create_test_backend();
    let content = r#"<?php
trait Loggable {
    public function log(string $msg): void {}
}

class Service {
    use Loggable;

    public function log(string $msg): void {
        // custom logging
    }
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);
    let titles = lens_titles(&lenses);

    assert_eq!(titles.len(), 1);
    assert_eq!(titles[0], "↑ Loggable::log");
}

// ─── Interface + Parent Combination ─────────────────────────────────────────

#[test]
fn parent_takes_precedence_over_interface() {
    let backend = create_test_backend();
    let content = r#"<?php
interface Renderable {
    public function render(): string;
}

class BaseView implements Renderable {
    public function render(): string { return ''; }
}

class ChildView extends BaseView {
    public function render(): string { return '<div>child</div>'; }
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);

    // BaseView should get ◆ Renderable::render
    let base_lenses: Vec<_> = lenses.iter().filter(|l| l.range.start.line < 9).collect();
    // ChildView should get ↑ BaseView::render (parent wins over interface)
    let child_lenses: Vec<_> = lenses.iter().filter(|l| l.range.start.line >= 9).collect();

    assert_eq!(base_lenses.len(), 1);
    assert_eq!(
        base_lenses[0].command.as_ref().unwrap().title,
        "◆ Renderable::render"
    );

    assert_eq!(child_lenses.len(), 1);
    assert_eq!(
        child_lenses[0].command.as_ref().unwrap().title,
        "↑ BaseView::render"
    );
}

// ─── Constructor Override ───────────────────────────────────────────────────

#[test]
fn constructor_override() {
    let backend = create_test_backend();
    let content = r#"<?php
class BaseModel {
    public function __construct() {}
}

class User extends BaseModel {
    public function __construct(string $name) {
        parent::__construct();
    }
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);
    let titles = lens_titles(&lenses);

    assert_eq!(titles.len(), 1);
    assert_eq!(titles[0], "↑ BaseModel::__construct");
}

// ─── Interface with no Override ─────────────────────────────────────────────

#[test]
fn interface_itself_has_no_lens() {
    let backend = create_test_backend();
    let content = r#"<?php
interface Cacheable {
    public function getCacheKey(): string;
    public function getCacheTTL(): int;
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);

    assert!(lenses.is_empty());
}

// ─── Code Lens Range ────────────────────────────────────────────────────────

#[test]
fn lens_range_is_on_method_line() {
    let backend = create_test_backend();
    let content = r#"<?php
class Base {
    public function process(): void {}
}

class Handler extends Base {
    public function process(): void {}
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);

    assert_eq!(lenses.len(), 1);
    let lens = &lenses[0];
    // The method `process` in Handler is on line 6 (0-based)
    assert_eq!(lens.range.start.line, 6);
    assert_eq!(lens.range.start.character, 4);
}

// ─── Code Lens Command ─────────────────────────────────────────────────────

const OVERRIDE_SOURCE: &str = r#"<?php
class Parent_ {
    public function action(): void {}
}

class Child extends Parent_ {
    public function action(): void {}
}
"#;

#[test]
fn lens_command_uses_navigate_to_prototype_when_client_shows_documents() {
    let backend = create_test_backend();
    backend.set_supports_show_document(true);
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, OVERRIDE_SOURCE);

    assert_eq!(lenses.len(), 1);
    let cmd = lenses[0].command.as_ref().unwrap();
    assert_eq!(cmd.command, "phpantom.navigateToPrototype");
    let args = cmd.arguments.as_ref().unwrap();
    assert_eq!(args.len(), 2);
}

/// A client that never answers `window/showDocument` (Zed) has to be able
/// to act on the lens by itself, so it gets the `showReferences` triple.
#[test]
fn lens_command_uses_show_references_without_show_document() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, OVERRIDE_SOURCE);

    assert_eq!(lenses.len(), 1);
    let cmd = lenses[0].command.as_ref().unwrap();
    assert_eq!(cmd.command, "editor.action.showReferences");
    let args = cmd.arguments.as_ref().unwrap();
    assert_eq!(args.len(), 3);

    serde_json::from_value::<Url>(args[0].clone()).expect("first argument is the target uri");
    serde_json::from_value::<Position>(args[1].clone())
        .expect("second argument is the target position");
    let locations: Vec<Location> =
        serde_json::from_value(args[2].clone()).expect("third argument is a location list");
    assert_eq!(locations.len(), 1);
    assert_eq!(locations[0].range.start.line, 2);
}

// ─── Multiple Interfaces ────────────────────────────────────────────────────

#[test]
fn implements_multiple_interfaces() {
    let backend = create_test_backend();
    let content = r#"<?php
interface Countable_ {
    public function count(): int;
}

interface Serializable_ {
    public function serialize(): string;
}

class Collection implements Countable_, Serializable_ {
    public function count(): int { return 0; }
    public function serialize(): string { return ''; }
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);
    let titles = lens_titles(&lenses);

    assert_eq!(titles.len(), 2);
    assert!(titles.contains(&"◆ Countable_::count"));
    assert!(titles.contains(&"◆ Serializable_::serialize"));
}

// ─── Interface Extends Interface ────────────────────────────────────────────

#[test]
fn interface_extends_interface() {
    let backend = create_test_backend();
    let content = r#"<?php
interface BaseRepo {
    public function find(int $id): ?object;
}

interface UserRepo extends BaseRepo {
    public function findByEmail(string $email): ?object;
}

class EloquentUserRepo implements UserRepo {
    public function find(int $id): ?object { return null; }
    public function findByEmail(string $email): ?object { return null; }
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);
    let titles = lens_titles(&lenses);

    assert_eq!(titles.len(), 2);
    // find() comes from BaseRepo via the extends chain
    assert!(titles.contains(&"◆ BaseRepo::find"));
    assert!(titles.contains(&"◆ UserRepo::findByEmail"));
}

// ─── Cross-File Override ────────────────────────────────────────────────────

#[test]
fn cross_file_parent_class() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[
            (
                "src/Base.php",
                r#"<?php
namespace App;

class Base {
    public function handle(): void {}
}
"#,
            ),
            (
                "src/Handler.php",
                r#"<?php
namespace App;

class Handler extends Base {
    public function handle(): void {}
}
"#,
            ),
        ],
    );

    let base_uri = format!("file://{}", _dir.path().join("src/Base.php").display());
    let handler_uri = format!("file://{}", _dir.path().join("src/Handler.php").display());

    let base_content = std::fs::read_to_string(_dir.path().join("src/Base.php")).unwrap();
    let handler_content = std::fs::read_to_string(_dir.path().join("src/Handler.php")).unwrap();

    backend.update_ast(&base_uri, &base_content);
    backend.update_ast(&handler_uri, &handler_content);

    let lenses = backend
        .handle_code_lens(&handler_uri, &handler_content)
        .unwrap_or_default();
    let titles = lens_titles(&lenses);

    assert_eq!(titles.len(), 2);
    assert!(titles.contains(&"↑ Base::handle"));
    assert!(titles.contains(&"0 references"));
}

// ─── Abstract Method Implementation ────────────────────────────────────────

#[test]
fn abstract_method_implementation() {
    let backend = create_test_backend();
    let content = r#"<?php
abstract class Shape {
    abstract public function area(): float;
    abstract public function perimeter(): float;
}

class Circle extends Shape {
    public function area(): float { return 3.14; }
    public function perimeter(): float { return 6.28; }
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);
    let titles = lens_titles(&lenses);

    assert_eq!(titles.len(), 2);
    assert!(titles.contains(&"↑ Shape::area"));
    assert!(titles.contains(&"↑ Shape::perimeter"));
}

// ─── Static Method Override ─────────────────────────────────────────────────

#[test]
fn static_method_override() {
    let backend = create_test_backend();
    let content = r#"<?php
class Factory {
    public static function create(): static { return new static(); }
}

class UserFactory extends Factory {
    public static function create(): static { return new static(); }
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);
    let titles = lens_titles(&lenses);

    assert_eq!(titles.len(), 1);
    assert_eq!(titles[0], "↑ Factory::create");
}

// ─── Empty File / No Classes ────────────────────────────────────────────────

#[test]
fn empty_file_returns_none() {
    let backend = create_test_backend();
    let content = "<?php\n// nothing here\n";
    let uri = "file:///test.php";
    backend.update_ast(uri, content);
    let result = backend.handle_code_lens(uri, content);

    assert!(result.is_none());
}

// ─── Mixed: Some Methods Override, Some Don't ───────────────────────────────

#[test]
fn only_overriding_methods_get_lenses() {
    let backend = create_test_backend();
    let content = r#"<?php
class Transport {
    public function send(): void {}
}

class EmailTransport extends Transport {
    public function send(): void {}
    public function formatBody(): string { return ''; }
    public function addAttachment(): void {}
}
"#;
    let uri = "file:///test.php";
    let lenses = get_code_lenses(&backend, uri, content);
    let titles = lens_titles(&lenses);

    // Only send() overrides; formatBody and addAttachment are new.
    assert_eq!(titles.len(), 1);
    assert_eq!(titles[0], "↑ Transport::send");
}

// ─── Cross-File Interface Implementation ────────────────────────────────────

#[test]
fn cross_file_interface_implementation() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[
            (
                "src/Printable.php",
                r#"<?php
namespace App;

interface Printable {
    public function print(): string;
}
"#,
            ),
            (
                "src/Document.php",
                r#"<?php
namespace App;

class Document implements Printable {
    public function print(): string { return 'doc'; }
}
"#,
            ),
        ],
    );

    let iface_uri = format!("file://{}", _dir.path().join("src/Printable.php").display());
    let doc_uri = format!("file://{}", _dir.path().join("src/Document.php").display());

    let iface_content = std::fs::read_to_string(_dir.path().join("src/Printable.php")).unwrap();
    let doc_content = std::fs::read_to_string(_dir.path().join("src/Document.php")).unwrap();

    backend.update_ast(&iface_uri, &iface_content);
    backend.update_ast(&doc_uri, &doc_content);

    let lenses = backend
        .handle_code_lens(&doc_uri, &doc_content)
        .unwrap_or_default();
    let titles = lens_titles(&lenses);

    assert_eq!(titles.len(), 2);
    assert!(titles.contains(&"◆ Printable::print"));
    assert!(titles.contains(&"0 references"));
}

// ─── PHPUnit Coverage Lens ("which tests cover this class") ────────────────
//
// The coverage search runs through the reference index, which only answers
// once the workspace has been indexed, so these use real files on disk
// rather than a bare `create_test_backend()`.

/// Build a workspace, open every file, and return the lenses for `subject`.
fn covers_lens_titles_for(files: &[(&str, &str)], subject: &str) -> Vec<String> {
    let (backend, dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/", "App\\Tests\\": "tests/" } } }"#,
        files,
    );

    for (rel_path, _) in files {
        let path = dir.path().join(rel_path);
        let uri = format!("file://{}", path.display());
        let content = std::fs::read_to_string(&path).unwrap();
        backend.update_ast(&uri, &content);
    }

    let subject_path = dir.path().join(subject);
    let subject_uri = format!("file://{}", subject_path.display());
    let subject_content = std::fs::read_to_string(&subject_path).unwrap();

    backend
        .handle_code_lens(&subject_uri, &subject_content)
        .unwrap_or_default()
        .iter()
        .filter_map(|l| l.command.as_ref().map(|c| c.title.clone()))
        .collect()
}

const CALCULATOR: (&str, &str) = (
    "src/Calculator.php",
    r#"<?php
namespace App;

class Calculator {
    public function add(int $a, int $b): int { return $a + $b; }
}
"#,
);

#[test]
fn covers_lens_from_method_level_docblock_tag() {
    let titles = covers_lens_titles_for(
        &[
            CALCULATOR,
            (
                "tests/CalculatorTest.php",
                r#"<?php
namespace App\Tests;

use App\Calculator;

class CalculatorTest {
    /**
     * @covers Calculator
     */
    public function testAdd(): void {}
}
"#,
            ),
        ],
        "src/Calculator.php",
    );

    assert!(
        titles.iter().any(|t| t == "Tests: CalculatorTest"),
        "titles: {titles:?}"
    );
}

#[test]
fn covers_lens_from_class_level_covers_default_class() {
    let titles = covers_lens_titles_for(
        &[
            CALCULATOR,
            (
                "tests/CalculatorTest.php",
                r#"<?php
namespace App\Tests;

/**
 * @coversDefaultClass \App\Calculator
 */
class CalculatorTest {
    /**
     * @covers ::add
     */
    public function testAdd(): void {}
}
"#,
            ),
        ],
        "src/Calculator.php",
    );

    assert!(
        titles.iter().any(|t| t == "Tests: CalculatorTest"),
        "titles: {titles:?}"
    );
}

#[test]
fn covers_lens_from_covers_class_attribute() {
    let titles = covers_lens_titles_for(
        &[
            CALCULATOR,
            (
                "tests/CalculatorTest.php",
                r#"<?php
namespace App\Tests;

use App\Calculator;
use PHPUnit\Framework\Attributes\CoversClass;

#[CoversClass(Calculator::class)]
class CalculatorTest {
    public function testAdd(): void {}
}
"#,
            ),
        ],
        "src/Calculator.php",
    );

    assert!(
        titles.iter().any(|t| t == "Tests: CalculatorTest"),
        "titles: {titles:?}"
    );
}

#[test]
fn covers_lens_from_covers_class_attribute_written_as_a_string() {
    let titles = covers_lens_titles_for(
        &[
            CALCULATOR,
            (
                "tests/CalculatorTest.php",
                r#"<?php
namespace App\Tests;

use PHPUnit\Framework\Attributes\CoversClass;

#[CoversClass('App\Calculator')]
class CalculatorTest {
    public function testAdd(): void {}
}
"#,
            ),
        ],
        "src/Calculator.php",
    );

    assert!(
        titles.iter().any(|t| t == "Tests: CalculatorTest"),
        "titles: {titles:?}"
    );
}

#[test]
fn covers_lens_counts_multiple_covering_tests() {
    let titles = covers_lens_titles_for(
        &[
            CALCULATOR,
            (
                "tests/CalculatorAddTest.php",
                r#"<?php
namespace App\Tests;

use App\Calculator;

/**
 * @covers Calculator
 */
class CalculatorAddTest {
    public function testAdd(): void {}
}
"#,
            ),
            (
                "tests/CalculatorRegressionTest.php",
                r#"<?php
namespace App\Tests;

use App\Calculator;

/**
 * @covers Calculator
 */
class CalculatorRegressionTest {
    public function testRegression(): void {}
}
"#,
            ),
        ],
        "src/Calculator.php",
    );

    assert!(
        titles.iter().any(|t| t == "Tests: 2 tests"),
        "titles: {titles:?}"
    );
}

#[test]
fn no_covers_lens_for_an_uncovered_class() {
    let titles = covers_lens_titles_for(&[CALCULATOR], "src/Calculator.php");

    assert!(
        !titles.iter().any(|t| t.starts_with("Tests:")),
        "titles: {titles:?}"
    );
}

#[test]
fn an_ordinary_reference_is_not_a_covers_lens() {
    // `new Calculator()` names the class without declaring coverage for it,
    // so it must not be mistaken for a covering test.
    let titles = covers_lens_titles_for(
        &[
            CALCULATOR,
            (
                "src/Consumer.php",
                r#"<?php
namespace App;

class Consumer {
    public function run(): int {
        return (new Calculator())->add(1, 2);
    }
}
"#,
            ),
        ],
        "src/Calculator.php",
    );

    assert!(
        !titles.iter().any(|t| t.starts_with("Tests:")),
        "titles: {titles:?}"
    );
}

// ─── Reference counts on declarations ───────────────────────────────────
//
// These drive `handle_code_lens` / `resolve_code_lens_item` directly so a
// count is asserted the way a client that displays the lens sees it, with
// the background search run to completion first.

/// Seed `uri` as an open file the way `didOpen` would, for a client that
/// supports lens refresh and a workspace that has finished indexing.
fn seed_open_file(backend: &phpantom_lsp::Backend, uri: &str, content: &str) {
    backend
        .open_files()
        .write()
        .insert(uri.to_string(), std::sync::Arc::new(content.to_string()));
    backend.update_ast(uri, content);
    backend.mark_workspace_indexed();
    backend.set_supports_code_lens_refresh(true);
}

/// Open a file and return its lenses once the member references the first
/// request queued have been computed.
fn declaration_lenses(backend: &phpantom_lsp::Backend, uri: &str, content: &str) -> Vec<CodeLens> {
    seed_open_file(backend, uri, content);

    backend.handle_code_lens(uri, content);
    backend.compute_pending_member_ref_counts();
    backend.handle_code_lens(uri, content).unwrap_or_default()
}

/// The title of the lens on `line`, resolved the way a client that
/// displays it would.
fn title_on_line(
    backend: &phpantom_lsp::Backend,
    lenses: &[CodeLens],
    line: u32,
) -> Option<String> {
    let lens = lenses.iter().find(|lens| lens.range.start.line == line)?;
    backend
        .resolve_code_lens_item(lens.clone())
        .command
        .map(|command| command.title)
}

/// The title of the lens on `line` as `handle_code_lens` returns it,
/// without the resolve round-trip.
fn unresolved_title_on_line(lenses: &[CodeLens], line: u32) -> Option<String> {
    lenses
        .iter()
        .find(|lens| lens.range.start.line == line)
        .and_then(|lens| lens.command.as_ref())
        .map(|command| command.title.clone())
}

#[test]
fn a_function_nothing_calls_is_reported_as_zero() {
    let backend = create_test_backend();

    let lenses = declaration_lenses(
        &backend,
        "file:///helpers.php",
        "<?php\nfunction unused(): void {}\n",
    );

    assert_eq!(
        title_on_line(&backend, &lenses, 1).as_deref(),
        Some("0 references"),
        "a file with no classes at all still reports its functions"
    );
}

#[test]
fn a_magic_method_gets_no_reference_lens() {
    let backend = create_test_backend();
    let content = r#"<?php
class User {
    public function __construct() {}
    public function save(): void {}
}
"#;

    let lenses = declaration_lenses(&backend, "file:///test.php", content);

    assert!(lenses.iter().any(|lens| lens.range.start.line == 1));
    assert!(lenses.iter().any(|lens| lens.range.start.line == 3));
    assert!(!lenses.iter().any(|lens| lens.range.start.line == 2));
}

#[test]
fn a_member_lens_ignores_a_member_of_the_same_name_on_another_class() {
    let backend = create_test_backend();
    let content = r#"<?php
class User {
    public int $id = 0;
    public function save(): void {}
}
class Order {
    public int $id = 0;
    public function save(): void {}
}
function persist(Order $order): void {
    echo $order->id;
    $order->save();
}
"#;

    let lenses = declaration_lenses(&backend, "file:///test.php", content);

    assert_eq!(
        title_on_line(&backend, &lenses, 2).as_deref(),
        Some("0 references")
    );
    assert_eq!(
        title_on_line(&backend, &lenses, 3).as_deref(),
        Some("0 references")
    );
    assert_eq!(
        title_on_line(&backend, &lenses, 6).as_deref(),
        Some("1 reference")
    );
    assert_eq!(
        title_on_line(&backend, &lenses, 7).as_deref(),
        Some("1 reference")
    );
}

#[test]
fn a_member_lens_counts_references_through_a_subclass() {
    let backend = create_test_backend();
    let content = r#"<?php
class Model {
    public function save(): void {}
}
class Order extends Model {
}
function persist(Order $order, Model $model): void {
    $order->save();
    $model->save();
}
"#;

    let lenses = declaration_lenses(&backend, "file:///test.php", content);

    assert_eq!(
        title_on_line(&backend, &lenses, 2).as_deref(),
        Some("2 references")
    );
}

/// A method that overrides a parent still gets its own reference-count
/// lens alongside the `↑ Parent::method` navigation lens (GH #412):
/// implementing a contract should not hide the usage count.
#[test]
fn an_overriding_method_keeps_its_reference_count_lens() {
    let backend = create_test_backend();
    let content = r#"<?php
class Shape {
    public function area(): float { return 0.0; }
}
class Circle extends Shape {
    public function area(): float { return 3.14; }
}
function measure(Circle $circle): void {
    $circle->area();
}
"#;

    let lenses = declaration_lenses(&backend, "file:///test.php", content);
    let titles: Vec<String> = lenses
        .iter()
        .filter(|lens| lens.range.start.line == 5)
        .map(|lens| backend.resolve_code_lens_item(lens.clone()))
        .filter_map(|lens| lens.command.map(|c| c.title))
        .collect();

    assert_eq!(titles.len(), 2);
    assert!(titles.contains(&"↑ Shape::area".to_string()));
    assert!(titles.contains(&"1 reference".to_string()));
}

#[test]
fn a_class_lens_ignores_a_class_of_the_same_name_in_another_namespace() {
    let backend = create_test_backend();
    let content = r#"<?php
class Widget {}
namespace App;
class Widget {}
function build(): void {
    $first = new \App\Widget();
    $second = new \App\Widget();
}
"#;

    let lenses = declaration_lenses(&backend, "file:///test.php", content);

    assert_eq!(
        title_on_line(&backend, &lenses, 1).as_deref(),
        Some("0 references")
    );
    assert_eq!(
        title_on_line(&backend, &lenses, 3).as_deref(),
        Some("2 references")
    );
}

#[test]
fn a_new_parent_class_recomputes_the_count() {
    const URI: &str = "file:///test.php";
    let backend = create_test_backend();
    let unrelated = r#"<?php
class Model {
    public function save(): void {}
}
class Order {
    public function save(): void {}
}
function persist(Order $order): void {
    $order->save();
}
"#;
    let lenses = declaration_lenses(&backend, URI, unrelated);
    assert_eq!(
        unresolved_title_on_line(&lenses, 2).as_deref(),
        Some("0 references")
    );

    let inherited = unrelated.replace(
        "class Order {\n    public function save(): void {}",
        "class Order extends Model {",
    );
    seed_open_file(&backend, URI, &inherited);
    backend.handle_code_lens(URI, &inherited);
    assert!(backend.compute_pending_member_ref_counts());
    let lenses = backend
        .handle_code_lens(URI, &inherited)
        .unwrap_or_default();
    assert_eq!(
        unresolved_title_on_line(&lenses, 2).as_deref(),
        Some("1 reference")
    );
}

#[test]
fn chain_cache_does_not_leak_a_resolution_across_files() {
    let backend = create_test_backend();

    const URI_A: &str = "file:///PenA.php";
    const URI_B: &str = "file:///PenB.php";
    const URI_CONSUMER_A: &str = "file:///ConsumerA.php";
    const URI_CONSUMER_B: &str = "file:///ConsumerB.php";

    // Two unrelated classes that happen to share a bare name and an
    // identically-shaped `make()->write()` chain, each imported under
    // that bare name by its own consumer file.
    let pen_a = r#"<?php
namespace App\A;

class Pen {
    public static function make(): self {
        return new self();
    }

    public function write(): void {}
}
"#;
    let pen_b = pen_a.replace("App\\A", "App\\B");

    let consumer_a = r#"<?php
namespace App;

use App\A\Pen;

function useA(): void {
    Pen::make()->write();
}
"#;
    let consumer_b = consumer_a
        .replace("App\\A", "App\\B")
        .replace("useA", "useB");

    seed_open_file(&backend, URI_A, pen_a);
    seed_open_file(&backend, URI_B, &pen_b);
    seed_open_file(&backend, URI_CONSUMER_A, consumer_a);
    seed_open_file(&backend, URI_CONSUMER_B, &consumer_b);

    // Queue both `write()` declarations for a count, then resolve them in
    // the same `compute_pending_member_ref_counts` pass so both consumer
    // files are scanned under one chain-cache activation — the scenario
    // where a text-only cache key leaks a resolution from one file's `use`
    // scope into the other's.
    backend.handle_code_lens(URI_A, pen_a);
    backend.handle_code_lens(URI_B, &pen_b);
    backend.compute_pending_member_ref_counts();

    assert_eq!(
        unresolved_title_on_line(
            &backend.handle_code_lens(URI_A, pen_a).unwrap_or_default(),
            8
        )
        .as_deref(),
        Some("1 reference"),
        "App\\A\\Pen::write must only count ConsumerA's call, not ConsumerB's \
         identically-spelled `Pen::make()->write()` against `App\\B\\Pen`"
    );
    assert_eq!(
        unresolved_title_on_line(
            &backend.handle_code_lens(URI_B, &pen_b).unwrap_or_default(),
            8
        )
        .as_deref(),
        Some("1 reference"),
        "App\\B\\Pen::write must only count ConsumerB's call"
    );
}

/// A receiver walk resolves only the member names the search asked about,
/// and caches the file under those names.  A later search for a different
/// name in an already-walked file has to walk it again rather than read the
/// narrower entry as "this file has no receivers".
#[test]
fn a_second_member_name_is_still_found_in_an_already_walked_file() {
    let backend = create_test_backend();

    const URI_ALPHA: &str = "file:///Alpha.php";
    const URI_BETA: &str = "file:///Beta.php";
    const URI_CONSUMER: &str = "file:///Consumer.php";

    let alpha = r#"<?php
namespace App;

class Alpha {
    public function ring(): void {}
}
"#;
    let beta = r#"<?php
namespace App;

class Beta {
    public function chime(): void {}
}
"#;
    // One file calls both, so the first count walks it for `ring` alone and
    // the second has to come back for `chime`.
    let consumer = r#"<?php
namespace App;

function play(Alpha $alpha, Beta $beta): void {
    $alpha->ring();
    $beta->chime();
}
"#;

    seed_open_file(&backend, URI_ALPHA, alpha);
    seed_open_file(&backend, URI_BETA, beta);
    seed_open_file(&backend, URI_CONSUMER, consumer);

    backend.handle_code_lens(URI_ALPHA, alpha);
    backend.compute_pending_member_ref_counts();
    assert_eq!(
        unresolved_title_on_line(
            &backend
                .handle_code_lens(URI_ALPHA, alpha)
                .unwrap_or_default(),
            4
        )
        .as_deref(),
        Some("1 reference"),
        "Alpha::ring is called once from Consumer.php"
    );

    backend.handle_code_lens(URI_BETA, beta);
    backend.compute_pending_member_ref_counts();
    assert_eq!(
        unresolved_title_on_line(
            &backend.handle_code_lens(URI_BETA, beta).unwrap_or_default(),
            4
        )
        .as_deref(),
        Some("1 reference"),
        "Beta::chime is called once from Consumer.php, which the count for \
         Alpha::ring already walked for `ring` alone"
    );
}

#[tokio::test]
async fn the_request_path_counts_off_the_request() {
    const URI: &str = "file:///test.php";
    const ONE_CALL: &str = r#"<?php
class Order {
    public function save(): void {}
}
function persist(Order $order): void {
    $order->save();
}
"#;
    let backend = create_test_backend();
    seed_open_file(&backend, URI, ONE_CALL);

    let params = CodeLensParams {
        text_document: TextDocumentIdentifier {
            uri: Url::parse(URI).unwrap(),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let first = backend
        .code_lens(params.clone())
        .await
        .unwrap()
        .unwrap_or_default();
    assert_eq!(
        unresolved_title_on_line(&first, 2).as_deref(),
        Some("- references"),
        "the first request answers before the count is known"
    );

    let counted = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let lenses = backend
                .code_lens(params.clone())
                .await
                .unwrap()
                .unwrap_or_default();
            if let Some(title) = unresolved_title_on_line(&lenses, 2)
                && title != "- references"
            {
                break title;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the background count did not land");
    assert_eq!(counted, "1 reference");
}
