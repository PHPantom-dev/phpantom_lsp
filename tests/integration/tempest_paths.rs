//! Static view paths keep the context of the file that declared them.

use crate::common::{create_psr4_workspace, hover_on, open_php, workspace_uri};

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
function defaults(string $path = __DIR__.'/default.view.php'): View {
    return view($path, entry: 'default');
}
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
defaults();
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
    for (name, expected) in [("default", "default"), ("method", "override")] {
        let uri = workspace_uri(&backend, &format!("app/{name}.view.php"));
        open_php(&backend, &uri, source).await;
        let hover = hover_on(&backend, &uri, source, "$entry").await;
        assert!(hover.contains(&format!("'{expected}'")), "{name}: {hover}");
        assert!(!hover.contains("'original'"), "{name}: {hover}");
    }
}
