//! Find references on a Laravel macro: from its `::macro('name', …)`
//! registration and from a call, every call site on the target class or a
//! class extending it is a reference.
//!
//! Opening the provider registers the macro in the index, so no workspace
//! scan is needed.

use crate::common::{create_test_backend, find_references, line_char_of, open_php};
use tower_lsp::lsp_types::{Location, Position, Url};

const WIDGET: &str = "\
<?php
namespace App\\Support;
class Widget {}
";

const WIDGET_PROVIDER: &str = "\
<?php
namespace App;
use App\\Support\\Widget;
Widget::macro('shine', function (): string { return 'ok'; });
";

const WIDGET_CALLER: &str = "\
<?php
namespace App;
use App\\Support\\Widget;
function demo(Widget $widget): void {
    Widget::shine();
    $widget->shine();
}
";

const BASE_COLLECTION: &str = "\
<?php
namespace App\\Support;
class BaseCollection {}
";

const COLLECTION_PROVIDER: &str = "\
<?php
namespace App;
use App\\Support\\BaseCollection;
BaseCollection::macro('shine', function (): string { return 'ok'; });
";

fn uri(name: &str) -> Url {
    Url::parse(&format!("file:///{name}")).unwrap()
}

fn at(text: &str, needle: &str) -> Position {
    let (line, character) = line_char_of(text, needle);
    Position { line, character }
}

fn has_location(locations: &[Location], uri: &Url, line: u32, character: u32) -> bool {
    locations.iter().any(|loc| {
        &loc.uri == uri && loc.range.start.line == line && loc.range.start.character == character
    })
}

#[tokio::test]
async fn a_registration_finds_its_static_and_instance_calls() {
    let backend = create_test_backend();
    let provider = uri("Provider.php");
    let caller = uri("Caller.php");
    open_php(&backend, &uri("Widget.php"), WIDGET).await;
    open_php(&backend, &provider, WIDGET_PROVIDER).await;
    open_php(&backend, &caller, WIDGET_CALLER).await;

    let locations = find_references(&backend, &provider, at(WIDGET_PROVIDER, "shine"), true).await;

    assert_eq!(
        locations.len(),
        3,
        "expected declaration + static + instance call: {locations:?}"
    );
    assert!(locations.iter().any(|loc| loc.uri == provider));
    assert!(has_location(&locations, &caller, 4, 12));
    assert!(has_location(&locations, &caller, 5, 13));
}

/// With the workspace indexed, the macro search prunes the files it reads
/// to the reference index's candidates instead of scanning every file;
/// both the static and the instance call must survive the pruning.
#[tokio::test]
async fn a_registration_finds_its_calls_through_the_reference_index() {
    let backend = create_test_backend();
    let provider = uri("Provider.php");
    let caller = uri("Caller.php");
    open_php(&backend, &uri("Widget.php"), WIDGET).await;
    open_php(&backend, &provider, WIDGET_PROVIDER).await;
    open_php(&backend, &caller, WIDGET_CALLER).await;
    open_php(
        &backend,
        &uri("Unrelated.php"),
        "<?php\nnamespace App;\nclass Unrelated {}\n",
    )
    .await;
    backend.mark_workspace_indexed();

    let locations = find_references(&backend, &provider, at(WIDGET_PROVIDER, "shine"), true).await;

    assert_eq!(
        locations.len(),
        3,
        "expected declaration + static + instance call via candidate pruning: {locations:?}"
    );
    assert!(locations.iter().any(|loc| loc.uri == provider));
    assert!(has_location(&locations, &caller, 4, 12));
    assert!(has_location(&locations, &caller, 5, 13));
}

#[tokio::test]
async fn a_call_on_a_descendant_finds_the_registration_and_the_ancestor_call() {
    let backend = create_test_backend();
    let provider = uri("Provider.php");
    let caller = uri("Caller.php");
    let caller_text = "\
<?php
namespace App;
use App\\Support\\BaseCollection;
use App\\Support\\EloquentCollection;
function demo(BaseCollection $base, EloquentCollection $eloquent): void {
    $base->shine();
    $eloquent->shine();
}
";
    open_php(&backend, &uri("BaseCollection.php"), BASE_COLLECTION).await;
    open_php(
        &backend,
        &uri("EloquentCollection.php"),
        "<?php\nnamespace App\\Support;\nclass EloquentCollection extends BaseCollection {}\n",
    )
    .await;
    open_php(&backend, &provider, COLLECTION_PROVIDER).await;
    open_php(&backend, &caller, caller_text).await;

    let locations = find_references(
        &backend,
        &caller,
        Position {
            line: 6,
            character: 16,
        },
        true,
    )
    .await;

    assert_eq!(
        locations.len(),
        3,
        "expected registration + base + descendant call: {locations:?}"
    );
    assert!(locations.iter().any(|loc| loc.uri == provider));
    assert!(has_location(&locations, &caller, 5, 11));
    assert!(has_location(&locations, &caller, 6, 15));
}

/// A call whose receiver cannot be typed (a chain off an untyped
/// parameter) may still be the macro, so it is reported rather than
/// ruled out.
#[tokio::test]
async fn a_registration_finds_a_call_on_an_unresolved_chain() {
    let backend = create_test_backend();
    let provider = uri("Provider.php");
    let caller = uri("Caller.php");
    open_php(&backend, &uri("BaseCollection.php"), BASE_COLLECTION).await;
    open_php(&backend, &provider, COLLECTION_PROVIDER).await;
    open_php(
        &backend,
        &caller,
        "<?php\nnamespace App;\nfunction demo($query): void {\n    $query->pluck('name', 'id')->shine();\n}\n",
    )
    .await;

    let locations =
        find_references(&backend, &provider, at(COLLECTION_PROVIDER, "shine"), true).await;

    assert!(
        has_location(&locations, &caller, 3, 33),
        "expected unresolved chain macro call to be included: {locations:?}"
    );
}
