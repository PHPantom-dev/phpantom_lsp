//! `global $x;` with no type of its own takes the type the `@global` tags of
//! library and project code give `$x`.

use crate::common::{line_char_of, markup_hover_at, open_php};
use std::fs;
use std::path::Path;

use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const MAIN: &str = "<?php\nfunction acme_count(): void\n{\n    global $wpdb;\n    $wpdb;\n}\n";

const WPDB_CLASS: &str = r#"<?php
class wpdb
{
    public function get_var(): ?string { return null; }
}
"#;

const OPTION_FUNCTIONS: &str = r#"<?php
/**
 * Retrieves an option value.
 *
 * @global wpdb $wpdb WordPress database abstraction object.
 */
function get_option($option) {}
"#;

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// A backend for the workspace `root`/`plugin` holding `main.php`, with
/// `files` written relative to the workspace, and `main.php` open.
async fn open_plugin(root: &Path, files: &[(&str, &str)], main: &str) -> (Backend, Uri) {
    let plugin = root.join("plugin");
    for (name, content) in files {
        write(&plugin.join(name), content);
    }
    write(&plugin.join("main.php"), main);

    let backend = Backend::new_test_with_workspace(plugin.clone(), Vec::new());
    backend.initialized(InitializedParams {}).await;
    let uri = Uri::from_file_path(plugin.join("main.php")).unwrap();
    open_php(&backend, &uri, main).await;
    (backend, uri)
}

async fn hover_on(backend: &Backend, uri: &Uri, main: &str, needle: &str) -> String {
    let (line, character) = line_char_of(main, needle);
    let indent = needle.len() - needle.trim_start().len();
    markup_hover_at(backend, uri, line, (character as usize + indent + 1) as u32).await
}

/// WordPress core indexed as library code, with no stubs, types `$wpdb`
/// through the tags its functions write.
#[tokio::test]
async fn a_global_takes_its_type_from_the_tags_of_library_code() {
    let dir = tempfile::tempdir().unwrap();
    let files = [
        (".phpantom.toml", "[indexing]\ninclude = [\"../core\"]\n"),
        ("../core/wp-includes/class-wpdb.php", WPDB_CLASS),
        ("../core/wp-includes/option.php", OPTION_FUNCTIONS),
    ];
    let (backend, uri) = open_plugin(dir.path(), &files, MAIN).await;
    let hover = hover_on(&backend, &uri, MAIN, "    $wpdb;").await;
    assert!(hover.contains("$wpdb = wpdb\n"), "{hover}");
}

/// `wp-settings.php` declares nothing, and documents the globals it
/// assigns at the top level.
#[tokio::test]
async fn a_tag_on_a_top_level_assignment_counts() {
    let dir = tempfile::tempdir().unwrap();
    let settings = r#"<?php
/**
 * WordPress Query object
 *
 * @global WP_Query $wp_the_query WordPress Query object.
 */
$GLOBALS['wp_the_query'] = new WP_Query();
"#;
    let files = [
        (
            "wp-includes/class-wp-query.php",
            "<?php\nclass WP_Query {}\n",
        ),
        ("wp-settings.php", settings),
    ];
    let main =
        "<?php\nfunction acme_query(): void\n{\n    global $wp_the_query;\n    $wp_the_query;\n}\n";
    let (backend, uri) = open_plugin(dir.path(), &files, main).await;
    let hover = hover_on(&backend, &uri, main, "    $wp_the_query;").await;
    assert!(hover.contains("$wp_the_query = WP_Query\n"), "{hover}");
}

/// A plugin's own global, documented on one function, types it where
/// another function imports it untagged.
#[tokio::test]
async fn a_plugins_own_tag_types_the_global_elsewhere() {
    let dir = tempfile::tempdir().unwrap();
    let registry = r#"<?php
class Acme_Registry {}

/** @global Acme_Registry $acme_registry */
function acme_boot(): void
{
    global $acme_registry;
    $acme_registry = new Acme_Registry();
}
"#;
    let main =
        "<?php\nfunction acme_use(): void\n{\n    global $acme_registry;\n    $acme_registry;\n}\n";
    let (backend, uri) = open_plugin(dir.path(), &[("boot.php", registry)], main).await;
    let hover = hover_on(&backend, &uri, main, "    $acme_registry;").await;
    assert!(
        hover.contains("$acme_registry = Acme_Registry\n"),
        "{hover}"
    );
}

/// A tag's type names the class its own file's namespace and imports say,
/// not the one of the same short name the caller sees.
#[tokio::test]
async fn a_library_tag_is_read_with_its_own_files_names() {
    let dir = tempfile::tempdir().unwrap();
    let library = r#"<?php
namespace Acme;

use Lib\Db;

/**
 * @global Db $acme_db
 * @global Cache $acme_cache
 */
function acme_boot() {}
"#;
    let classes = "<?php\nnamespace Lib { class Db {} }\nnamespace Acme { class Cache {} }\nnamespace { class Db {} class Cache {} }\n";
    let main = "<?php\nnamespace App;\n\nclass Db {}\n\nfunction report(): void\n{\n    global $acme_db, $acme_cache;\n    $acme_db;\n    $acme_cache;\n}\n";
    let files = [("library.php", library), ("classes.php", classes)];
    let (backend, uri) = open_plugin(dir.path(), &files, main).await;
    let db = hover_on(&backend, &uri, main, "    $acme_db;").await;
    assert!(db.contains("namespace Lib;\n$acme_db = Db\n"), "{db}");
    let cache = hover_on(&backend, &uri, main, "    $acme_cache;").await;
    assert!(
        cache.contains("namespace Acme;\n$acme_cache = Cache\n"),
        "{cache}"
    );
}

/// Where a stub path documents the global, only its tags count.
#[tokio::test]
async fn the_tags_of_the_stubs_win_over_the_others() {
    let dir = tempfile::tempdir().unwrap();
    let legacy =
        "<?php\nclass LegacyDb {}\n\n/** @global LegacyDb $wpdb */\nfunction legacy_boot() {}\n";
    let stubs =
        format!("{WPDB_CLASS}\n/** @global wpdb $wpdb */\nfunction get_option($option) {{}}\n",);
    let files = [
        (".phpantom.toml", "[stubs]\npaths = [\"stubs\"]\n"),
        ("stubs/wordpress.php", stubs.as_str()),
        ("legacy.php", legacy),
    ];
    let (backend, uri) = open_plugin(dir.path(), &files, MAIN).await;
    let hover = hover_on(&backend, &uri, MAIN, "    $wpdb;").await;
    assert!(hover.contains("$wpdb = wpdb\n"), "{hover}");
}

/// A saved change to a file's tags is what the global then holds.
#[tokio::test]
async fn a_changed_tag_replaces_the_old_one() {
    let dir = tempfile::tempdir().unwrap();
    let classes = "<?php\nclass wpdb {}\nclass OtherDb {}\n";
    let files = [("classes.php", classes), ("option.php", OPTION_FUNCTIONS)];
    let (backend, uri) = open_plugin(dir.path(), &files, MAIN).await;
    let hover = hover_on(&backend, &uri, MAIN, "    $wpdb;").await;
    assert!(hover.contains("$wpdb = wpdb\n"), "{hover}");

    let option = dir.path().join("plugin/option.php");
    write(
        &option,
        &OPTION_FUNCTIONS.replace("@global wpdb", "@global OtherDb"),
    );
    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: Uri::from_file_path(&option).unwrap(),
                kind: FileChangeType::Changed,
            }],
        })
        .await;

    let hover = hover_on(&backend, &uri, MAIN, "    $wpdb;").await;
    assert!(hover.contains("$wpdb = OtherDb\n"), "{hover}");
}

/// A tag written into an open file counts once the file is saved, though
/// the file held none before.
#[tokio::test]
async fn a_tag_saved_in_an_open_file_counts() {
    let dir = tempfile::tempdir().unwrap();
    let untagged = "<?php\nfunction get_option($option) {}\n";
    let files = [("classes.php", WPDB_CLASS), ("option.php", untagged)];
    let (backend, uri) = open_plugin(dir.path(), &files, MAIN).await;
    let hover = hover_on(&backend, &uri, MAIN, "    $wpdb;").await;
    assert!(!hover.contains("$wpdb = wpdb\n"), "{hover}");

    let option = dir.path().join("plugin/option.php");
    let option_uri = Uri::from_file_path(&option).unwrap();
    open_php(&backend, &option_uri, OPTION_FUNCTIONS).await;
    write(&option, OPTION_FUNCTIONS);
    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: option_uri,
                kind: FileChangeType::Changed,
            }],
        })
        .await;

    let hover = hover_on(&backend, &uri, MAIN, "    $wpdb;").await;
    assert!(hover.contains("$wpdb = wpdb\n"), "{hover}");
}
