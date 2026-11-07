//! Static view paths keep the context of the file that declared them.

use crate::common::{create_psr4_workspace, hover_on, open_php, workspace_uri};

#[tokio::test]
async fn file_paths_keep_their_declaration_context() {
    let declarations = r"<?php
namespace { const ROOT = 'global'; class Base { const PATH = 'global'; } }
namespace App {
    const ROOT = __DIR__;
    const SOURCE = __FILE__;
    class Base { const PATH = __DIR__ . '/page.view.php'; }
}
";
    let trait_source =
        "<?php namespace App; trait Paths { const TRAIT_PATH = __DIR__ . '/part.view.php'; }";
    let caller = r"<?php namespace Consumer;
use App\Base;
use const App\ROOT;
use const App\SOURCE;
class Child extends Base { use \App\Paths; }
$directory = ROOT;
$file = SOURCE;
$path = Child::PATH;
$trait = Child::TRAIT_PATH;
$local = __DIR__;
$current = __FILE__;
echo $directory;
echo $file;
echo $path;
echo $trait;
echo $local;
echo $current;
";
    let (backend, dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/","Consumer\\":"consumer/"}}}"#,
        &[
            ("app/definitions.php", declarations),
            ("app/traits/Paths.php", trait_source),
            ("consumer/caller.php", caller),
        ],
    );
    for (path, source) in [
        ("app/definitions.php", declarations),
        ("app/traits/Paths.php", trait_source),
    ] {
        open_php(&backend, &workspace_uri(&backend, path), source).await;
    }
    let uri = workspace_uri(&backend, "consumer/caller.php");
    open_php(&backend, &uri, caller).await;
    let root = dunce::canonicalize(dir.path()).unwrap();
    for (needle, expected) in [
        ("$directory;", root.join("app")),
        ("$file;", root.join("app/definitions.php")),
        ("$path;", root.join("app/page.view.php")),
        ("$trait;", root.join("app/traits/part.view.php")),
        ("$local;", root.join("consumer")),
        ("$current;", root.join("consumer/caller.php")),
    ] {
        let value = expected
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('\'', "\\'");
        let hover = hover_on(&backend, &uri, caller, needle).await;
        assert!(
            hover.contains(&value),
            "{needle}: expected {value}, got {hover}"
        );
    }
}

#[tokio::test]
async fn file_paths_fold_dirname_without_overriding_namespaced_functions() {
    let source = r"<?php
namespace App {
    function dirname(string $path): string { return 'custom'; }
    $shadow = dirname(__FILE__);
    $directory = \dirname(__FILE__);
    $nested = \dirname(__DIR__) . '/views';
    echo $shadow;
    echo $directory;
    echo $nested;
}
namespace Consumer {
    use function App\dirname;
    $imported = dirname(__FILE__);
    echo $imported;
}
";
    let (backend, dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        &[("app/paths.php", source)],
    );
    let uri = workspace_uri(&backend, "app/paths.php");
    open_php(&backend, &uri, source).await;
    let root = dunce::canonicalize(dir.path()).unwrap();
    for (needle, expected) in [
        ("$directory;", root.join("app")),
        ("$nested;", root.join("views")),
    ] {
        let expected = expected
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('\'', "\\'");
        let hover = hover_on(&backend, &uri, source, needle).await;
        assert!(hover.contains(&expected), "{needle}: {hover}");
    }
    for needle in ["$shadow;", "$imported;"] {
        let hover = hover_on(&backend, &uri, source, needle).await;
        assert!(hover.contains("string"), "{needle}: {hover}");
        assert!(!hover.contains("paths.php"), "{needle}: {hover}");
    }
}
