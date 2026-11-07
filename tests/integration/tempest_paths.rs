//! Static view paths keep the context of the file that declared them.

use crate::common::{create_psr4_workspace, hover_on, hover_text_at, open_php, workspace_uri};

#[tokio::test]
async fn tempest_paths_wrappers_bind_named_arguments_without_leaking_between_calls() {
    let functions = r"<?php namespace App;
use Tempest\View\View;
use function Tempest\View\view;
const FIRST = __DIR__.'/first.view.php';
function renderPage(string $path, string $entry): View {
    $selected = $path;
    return view($selected, entry: $entry);
}
function nestedPage(string $path, string $entry): View {
    return renderPage(entry: $entry, path: $path);
}
class Pages {
    public static function render(string $path, string $entry): View {
        return view($path, entry: $entry);
    }
}
class ChildPages extends Pages {}
";
    let caller = r"<?php namespace Consumer;
use function App\nestedPage as render;
use const App\FIRST;
use App\ChildPages;
$path = FIRST;
render(entry: 'first', path: $path);
render(path: 'app/' . 'second.view.php', entry: 'second');
ChildPages::render(entry: 'method', path: 'app/method.view.php');
";
    let source = "<p>{{ $entry }}</p>";
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/","Consumer\\":"consumer/"}}}"#,
        &[
            ("app/helpers.php", functions),
            ("consumer/caller.php", caller),
            ("app/first.view.php", source),
            ("app/second.view.php", source),
            ("app/method.view.php", source),
        ],
    );
    open_php(
        &backend,
        &workspace_uri(&backend, "app/helpers.php"),
        functions,
    )
    .await;
    open_php(
        &backend,
        &workspace_uri(&backend, "consumer/caller.php"),
        caller,
    )
    .await;
    for name in ["first", "second", "method"] {
        let uri = workspace_uri(&backend, &format!("app/{name}.view.php"));
        open_php(&backend, &uri, source).await;
        let hover = hover_on(&backend, &uri, source, "$entry").await;
        assert!(hover.contains(&format!("'{name}'")), "{name}: {hover}");
        for other in ["first", "second", "method"] {
            if other != name {
                assert!(!hover.contains(&format!("'{other}'")), "{name}: {hover}");
            }
        }
    }
}

#[tokio::test]
async fn tempest_paths_default_arguments_and_instance_helpers_use_the_declaration_file() {
    let functions = r"<?php namespace App;
use Tempest\View\View;
use function Tempest\View\view;
function defaults(string $path = __DIR__.'/default.view.php', string $entry = 'fallback'): View {
    return view($path, entry: $entry);
}
function literalDefault(string $path = 'literal.view.php'): View { return view($path, entry: 'literal'); }
function loop(string $path): View { return loop($path); }
class Pages {
    public function render(string $path): View {
        return view($path, entry: 'original');
    }
}
";
    let caller = r"<?php namespace Consumer;
use function App\defaults;
use function App\loop;
use App\Pages;
defaults(entry: 'default');
\App\literalDefault();
$pages = new Pages();
$pages->render('app/method.view.php')->data(entry: 'override');
loop('app/recursive.view.php');
";
    let source = "<p>{{ $entry }}</p>";
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/","Consumer\\":"consumer/"}}}"#,
        &[
            ("app/helpers.php", functions),
            ("consumer/caller.php", caller),
            ("app/default.view.php", source),
            ("app/literal.view.php", source),
            ("app/method.view.php", source),
            ("app/recursive.view.php", "Recursive"),
        ],
    );
    open_php(
        &backend,
        &workspace_uri(&backend, "app/helpers.php"),
        functions,
    )
    .await;
    open_php(
        &backend,
        &workspace_uri(&backend, "consumer/caller.php"),
        caller,
    )
    .await;
    for (name, expected) in [
        ("default", "default"),
        ("literal", "literal"),
        ("method", "override"),
    ] {
        let uri = workspace_uri(&backend, &format!("app/{name}.view.php"));
        open_php(&backend, &uri, source).await;
        let hover = hover_on(&backend, &uri, source, "$entry").await;
        assert!(hover.contains(&format!("'{expected}'")), "{name}: {hover}");
        assert!(!hover.contains("'original'"), "{name}: {hover}");
    }
}

#[tokio::test]
async fn tempest_paths_wrapper_names_do_not_match_unrelated_receivers_or_functions() {
    let helpers = r"<?php namespace {
function renderPage(string $path): \Tempest\View\View { return \Tempest\View\view($path, entry: 'global'); }
}
namespace App {
class Pages { public function render(string $path): \Tempest\View\View { return \Tempest\View\view($path, entry: 'view'); } }
class Other { public function render(string $path): string { return $path; } }
}
";
    let caller = r"<?php namespace Consumer;
function renderPage(string $path): string { return $path; }
\renderPage('app/global.view.php');
renderPage('app/unused.view.php');
$other = new \App\Other();
$other->render('app/unused.view.php');
\App\Other::render('app/unused.view.php');
$unknown->render('app/unused.view.php');
\Missing::render('app/unused.view.php');
";
    let source = "{{ $entry }}";
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/","Consumer\\":"consumer/"}}}"#,
        &[
            ("app/helpers.php", helpers),
            ("consumer/caller.php", caller),
            ("app/global.view.php", source),
            ("app/unused.view.php", source),
        ],
    );
    open_php(
        &backend,
        &workspace_uri(&backend, "app/helpers.php"),
        helpers,
    )
    .await;
    open_php(
        &backend,
        &workspace_uri(&backend, "consumer/caller.php"),
        caller,
    )
    .await;
    let uri = workspace_uri(&backend, "app/global.view.php");
    open_php(&backend, &uri, source).await;
    assert!(
        hover_on(&backend, &uri, source, "$entry")
            .await
            .contains("'global'")
    );
    let uri = workspace_uri(&backend, "app/unused.view.php");
    open_php(&backend, &uri, source).await;
    assert!(
        !hover_text_at(&backend, &uri, 0, 5)
            .await
            .unwrap_or_default()
            .contains("'global'")
    );
}

#[tokio::test]
async fn tempest_paths_stale_and_missing_wrapper_declarations_clear_inferred_data() {
    use tower_lsp_server::{LanguageServer, gen_lsp_types::*};
    let helper = r"<?php function renderPage(string $path = __DIR__.'/page.view.php'): \Tempest\View\View {
    return \Tempest\View\view($path, entry: 'fresh');
}";
    let caller = "<?php renderPage();";
    let source = "{{ $entry }}";
    let (backend, dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        &[
            ("app/helper.php", helper),
            ("app/caller.php", caller),
            ("app/page.view.php", source),
        ],
    );
    let helper_uri = workspace_uri(&backend, "app/helper.php");
    let caller_uri = workspace_uri(&backend, "app/caller.php");
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &helper_uri, helper).await;
    open_php(&backend, &caller_uri, caller).await;
    open_php(&backend, &uri, source).await;
    assert!(
        hover_on(&backend, &uri, source, "$entry")
            .await
            .contains("'fresh'")
    );
    backend
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: helper_uri.clone(),
            },
        })
        .await;
    std::fs::write(
        dir.path().join("app/helper.php"),
        helper.replace("<?php ", "<?php\n\n"),
    )
    .unwrap();
    open_php(
        &backend,
        &caller_uri,
        &format!("{caller}\n// Disk changed before a watcher event."),
    )
    .await;
    assert!(
        !hover_text_at(&backend, &uri, 0, 5)
            .await
            .unwrap_or_default()
            .contains("'fresh'")
    );
    open_php(&backend, &helper_uri, helper).await;
    std::fs::write(dir.path().join("app/helper.php"), helper).unwrap();
    backend
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: helper_uri },
        })
        .await;
    assert!(
        hover_on(&backend, &uri, source, "$entry")
            .await
            .contains("'fresh'")
    );
    std::fs::remove_file(dir.path().join("app/helper.php")).unwrap();
    open_php(
        &backend,
        &caller_uri,
        &format!("{caller}\n// Helper removed before a watcher event."),
    )
    .await;
    assert!(
        !hover_text_at(&backend, &uri, 0, 5)
            .await
            .unwrap_or_default()
            .contains("'fresh'")
    );
}

#[tokio::test]
async fn tempest_paths_custom_view_chains_only_contribute_the_final_data() {
    let caller = r"<?php namespace App;
class Page implements \Tempest\View\View {
    public string $path = __DIR__.'/page.view.php';
    public function data(mixed ...$values): self { return $this; }
}
(new Page())->data(entry: 'first')->data(entry: 'last');
";
    let source = "{{ $entry }}";
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        &[("app/caller.php", caller), ("app/page.view.php", source)],
    );
    open_php(&backend, &workspace_uri(&backend, "app/caller.php"), caller).await;
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, source).await;
    let hover = hover_on(&backend, &uri, source, "$entry").await;
    assert!(hover.contains("'last'"), "{hover}");
    assert!(!hover.contains("'first'"), "{hover}");
}

#[tokio::test]
async fn tempest_paths_unsaved_helper_edits_refresh_their_callers() {
    use tower_lsp_server::{LanguageServer, gen_lsp_types::*};
    let helper = r"<?php function renderPage(string $path = __DIR__.'/page.view.php'): \Tempest\View\View {
    return \Tempest\View\view($path, entry: 'before');
}";
    let caller = "<?php renderPage();";
    let source = "{{ $entry }}";
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        &[
            ("app/helper.php", helper),
            ("app/caller.php", caller),
            ("app/page.view.php", source),
        ],
    );
    let helper_uri = workspace_uri(&backend, "app/helper.php");
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &helper_uri, helper).await;
    open_php(&backend, &workspace_uri(&backend, "app/caller.php"), caller).await;
    open_php(&backend, &uri, source).await;
    assert!(
        hover_on(&backend, &uri, source, "$entry")
            .await
            .contains("'before'")
    );
    backend
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                text_document_identifier: TextDocumentIdentifier { uri: helper_uri },
                version: 2,
            },
            content_changes: vec![
                TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                    TextDocumentContentChangeWholeDocument {
                        text: helper.replace("'before'", "'after'"),
                    },
                ),
            ],
        })
        .await;
    let hover = hover_on(&backend, &uri, source, "$entry").await;
    assert!(hover.contains("'after'"), "{hover}");
}

#[tokio::test]
async fn tempest_paths_deep_branching_helper_chains_carry_data() {
    // Each helper calls the next one twice, so a walk without memoisation
    // reads the innermost body 2^12 times.
    let mut helpers = String::from(
        "<?php namespace App;\nuse Tempest\\View\\View;\nuse function Tempest\\View\\view;\n\
         function level0(string $path, string $entry): View { return view($path, entry: $entry); }\n",
    );
    for level in 1..=12 {
        let next = level - 1;
        helpers.push_str(&format!(
            "function level{level}(string $path, string $entry): View {{\n    \
             if ($entry === '') {{ return level{next}($path, $entry); }}\n    \
             return level{next}($path, $entry);\n}}\n"
        ));
    }
    let caller =
        "<?php namespace Consumer;\n\\App\\level12(__DIR__ . '/../app/page.view.php', 'deep');\n";
    let source = "<p>{{ $entry }}</p>";
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/","Consumer\\":"consumer/"}}}"#,
        &[
            ("app/helpers.php", helpers.as_str()),
            ("consumer/caller.php", caller),
            ("app/page.view.php", source),
        ],
    );
    open_php(
        &backend,
        &workspace_uri(&backend, "app/helpers.php"),
        &helpers,
    )
    .await;
    open_php(
        &backend,
        &workspace_uri(&backend, "consumer/caller.php"),
        caller,
    )
    .await;
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, source).await;
    let hover = hover_on(&backend, &uri, source, "$entry").await;
    assert!(hover.contains("'deep'"), "{hover}");
}
