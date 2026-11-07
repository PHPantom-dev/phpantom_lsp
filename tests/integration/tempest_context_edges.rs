//! Scope invalidation and incomplete render sites.

use crate::common::{
    create_psr4_workspace, hover_on, message_text, open_php, template_diagnostics, workspace_uri,
};
use tower_lsp_server::{LanguageServer, gen_lsp_types::*};

const COMPOSER: &str = r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#;

#[tokio::test]
async fn tempest_context_named_paths_and_unrelated_calls_do_not_contribute_data() {
    let source = "{{ $entry->title }}";
    let caller = r#"<?php
namespace App;
use function Tempest\View\view as render;
class Entry { public string $title; }
class Other { public function data(...$data): self { return $this; } }
function unrelated(...$args) {}
$callback = fn () => null;
$callback();
unrelated('page.view.php', entry: false);
(new Other())->data(entry: 42)->data(entry: false);
render(entry: new Entry(), path: __DIR__.'/page.view.php');
render();
render(path: $unknown, entry: false);
render(path: 'missing.view.php', entry: false);
render(path: 'missing/' . 'page.view.php', entry: false);
render(path: 'app/page.view.php', ...$unknownData);
"#;
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[("app/caller.php", caller), ("app/page.view.php", source)],
    );
    open_php(&backend, &workspace_uri(&backend, "app/caller.php"), caller).await;
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
async fn tempest_context_missing_files_and_stale_caller_snapshots_are_discarded() {
    let source = "{{ $count }}";
    let caller = "<?php \\Tempest\\View\\view(__DIR__.'/page.view.php', count: 1);";
    let ghost = "<?php class Ghost implements \\Tempest\\View\\View { public string $path = 'app/page.view.php'; }";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/caller.php", caller),
            ("app/page.view.php", source),
            ("app/ghost.php", ghost),
        ],
    );
    let ghost_uri = workspace_uri(&backend, "app/ghost.php");
    backend.update_ast(ghost_uri.as_str(), ghost);
    std::fs::remove_file(dir.path().join("app/ghost.php")).unwrap();
    let caller_uri = workspace_uri(&backend, "app/caller.php");
    open_php(&backend, &caller_uri, caller).await;
    backend.update_ast(caller_uri.as_str(), &caller.replace("count: 1", "count: 2"));
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, source).await;
    assert!(
        !backend
            .blade_virtual_php(uri.as_str())
            .unwrap()
            .contains("$count = null")
    );
    open_php(&backend, &caller_uri, caller).await;
    assert!(
        backend
            .blade_virtual_php(uri.as_str())
            .unwrap()
            .contains("@var 1 $count")
    );
    std::fs::remove_file(dir.path().join("app/page.view.php")).unwrap();
    backend
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
        })
        .await;
    assert!(backend.blade_virtual_php(uri.as_str()).is_none());
}

#[tokio::test]
async fn tempest_context_component_deleted_before_its_watcher_event_is_safe() {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/page.view.php", "<x-card />"),
            ("app/x-card.view.php", "<x-slot />"),
        ],
    );
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, "<x-card />").await;
    std::fs::remove_file(dir.path().join("app/x-card.view.php")).unwrap();
    open_php(&backend, &uri, "<x-card />").await;
    assert_eq!(template_diagnostics(&backend, &uri), Vec::<String>::new());
}

#[tokio::test]
async fn tempest_context_recursive_components_keep_inference_bounded() {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/page.view.php", "<x-tree :entry=\"1\" />"),
            ("app/x-tree.view.php", "<x-tree :entry=\"[$entry]\" />"),
        ],
    );
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, "<x-tree :entry=\"1\" />").await;
    let child =
        Uri::from_file_path(dunce::canonicalize(dir.path().join("app/x-tree.view.php")).unwrap())
            .unwrap();
    let lowered = backend.blade_virtual_php(child.as_str()).unwrap();
    assert!(lowered.contains("$entry = null"));
    assert!(
        lowered.len() < 50_000,
        "recursive data must not grow without a bound"
    );
}

#[tokio::test]
async fn tempest_context_slot_reads_keep_other_component_locals_unused() {
    let source = "<x-card>{{ $used }}<?php $callback = fn () => $captured; ?>{{ $callback() }}<x-slot name=\"absent\">Unused slot</x-slot></x-card>";
    let component =
        "<?php $used = 'used'; $captured = 'captured'; $unused = 'unused'; ?><x-slot />";
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/page.view.php", source),
            ("app/x-card.view.php", component),
        ],
    );
    open_php(
        &backend,
        &workspace_uri(&backend, "app/page.view.php"),
        source,
    )
    .await;
    let uri = workspace_uri(&backend, "app/x-card.view.php");
    open_php(&backend, &uri, component).await;
    let mut diagnostics = Vec::new();
    backend.collect_unused_variable_diagnostics(
        uri.as_str(),
        &backend.blade_virtual_php(uri.as_str()).unwrap(),
        &mut diagnostics,
    );
    let messages: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| message_text(&diagnostic.message).to_owned())
        .collect();
    assert_eq!(messages, vec!["Unused variable $unused"]);
}

#[tokio::test]
async fn tempest_context_incomplete_php_inside_slots_remains_editable() {
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[(
            "app/x-card.view.php",
            "<?php $local = 'title'; ?><x-slot />",
        )],
    );
    let uri = workspace_uri(&backend, "app/page.view.php");
    for php in [
        "function unfinished(",
        "if (",
        "}",
        "function broken() {",
        "new class {",
        "/*",
    ] {
        let source = format!("<x-card><?php {php} ?>{{{{ $local }}}}</x-card>");
        open_php(&backend, &uri, &source).await;
        assert!(
            backend
                .blade_virtual_php(uri.as_str())
                .unwrap()
                .contains(php)
        );
    }
}
