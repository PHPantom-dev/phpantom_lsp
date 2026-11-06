//! The types of the variables an `include`/`require` brings into scope.

use crate::common::{
    complete_labels_at, create_test_backend, goto_definition_at, hover_at, hover_text,
    message_text, open_php,
};

/// Write `files` into a temp directory and parse every one of them; the
/// URIs come back in the same order.
fn project(files: &[(&str, &str)]) -> (tempfile::TempDir, phpantom_lsp::Backend, Vec<String>) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let mut uris = Vec::new();
    for (rel_path, content) in files {
        let full = dir.path().join(rel_path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, content).unwrap();
        uris.push(crate::common::file_uri(&full));
    }
    let backend = create_test_backend();
    for (uri, (_, content)) in uris.iter().zip(files) {
        backend.update_ast(uri, content);
    }
    (dir, backend, uris)
}

/// The hover markdown at `line`/`character` of the project's first file.
fn hover_in_project(files: &[(&str, &str)], line: u32, character: u32) -> Option<String> {
    let (_dir, backend, uris) = project(files);
    hover_at(&backend, &uris[0], files[0].1, line, character).map(|h| hover_text(&h).to_string())
}

const VERSION_PHP: &str = r#"<?php
$wp_version = '6.7';
$wp_db_version = 58975;
function helper() {
    $inner = new \stdClass();
}
"#;

#[test]
fn a_required_files_variable_has_its_type_in_a_function() {
    let main = r#"<?php
function core_upgrade() {
    require __DIR__ . '/version.php';
    return $wp_db_version;
}
"#;
    let hover = hover_in_project(&[("main.php", main), ("version.php", VERSION_PHP)], 3, 13)
        .expect("hover on $wp_db_version");
    assert!(hover.contains("$wp_db_version = 58975"), "got: {hover}");
}

#[test]
fn a_required_files_variable_has_its_type_at_the_top_level() {
    let main = r#"<?php
require_once 'version.php';
echo $wp_version;
"#;
    let hover = hover_in_project(&[("main.php", main), ("version.php", VERSION_PHP)], 2, 7)
        .expect("hover on $wp_version");
    assert!(hover.contains("$wp_version = '6.7'"), "got: {hover}");
}

#[test]
fn the_include_replaces_what_the_variable_held_before() {
    let main = r#"<?php
function f() {
    $wp_version = 1;
    include __DIR__ . '/version.php';
    return $wp_version;
}
"#;
    let hover = hover_in_project(&[("main.php", main), ("version.php", VERSION_PHP)], 4, 13)
        .expect("hover on $wp_version");
    assert!(
        hover.contains("$wp_version = '6.7'"),
        "the included file reassigns it, got: {hover}"
    );
}

#[test]
fn includes_of_includes_are_followed() {
    let main = r#"<?php
function f() {
    require __DIR__ . '/outer.php';
    return $wp_version;
}
"#;
    let outer = "<?php\nrequire __DIR__ . '/inc/version.php';\n";
    let hover = hover_in_project(
        &[
            ("main.php", main),
            ("outer.php", outer),
            ("inc/version.php", VERSION_PHP),
        ],
        3,
        13,
    )
    .expect("hover on $wp_version");
    assert!(hover.contains("$wp_version = '6.7'"), "got: {hover}");
}

#[test]
fn a_file_that_includes_itself_terminates() {
    let main = r#"<?php
function f() {
    require __DIR__ . '/loop.php';
    return $count;
}
"#;
    let looping = "<?php\n$count = 1;\nrequire __DIR__ . '/loop.php';\n";
    let hover = hover_in_project(&[("main.php", main), ("loop.php", looping)], 3, 13)
        .expect("hover on $count");
    assert!(hover.contains("$count = 1"), "got: {hover}");
}

#[tokio::test]
async fn completion_reads_the_class_an_included_file_assigns() {
    let main = r#"<?php
function f() {
    require __DIR__ . '/bootstrap.php';
    $app->
}
"#;
    let bootstrap = r#"<?php
class Application {
    public function boot(): void {}
}
$app = new Application();
"#;
    let (_dir, backend, uris) = project(&[("main.php", main), ("bootstrap.php", bootstrap)]);
    let uri = uris[0].parse().unwrap();
    let labels = complete_labels_at(&backend, &uri, main, 3, 10).await;
    assert!(
        labels.iter().any(|l| l.starts_with("boot")),
        "expected boot(), got: {labels:?}"
    );
}

#[test]
fn diagnostics_check_members_of_a_variable_an_included_file_assigns() {
    let main = r#"<?php
function f(): void {
    require __DIR__ . '/bootstrap.php';
    $app->boot();
    $app->shutdown();
}
"#;
    let bootstrap = r#"<?php
class Application {
    public function boot(): void {}
}
$app = new Application();
"#;
    let (_dir, backend, uris) = project(&[("main.php", main), ("bootstrap.php", bootstrap)]);
    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(&uris[0], main, &mut diags);
    let messages: Vec<&str> = diags.iter().map(|d| message_text(&d.message)).collect();
    assert!(
        messages.iter().any(|m| m.contains("shutdown")),
        "expected an unknown-method diagnostic for shutdown(), got: {messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("boot")),
        "boot() exists, got: {messages:?}"
    );
}

#[tokio::test]
async fn definition_follows_a_method_on_a_variable_an_included_file_assigns() {
    let main = r#"<?php
function f(): void {
    require __DIR__ . '/bootstrap.php';
    $app->boot();
}
"#;
    let bootstrap = r#"<?php
class Application {
    public function boot(): void {}
}
$app = new Application();
"#;
    let (_dir, backend, uris) = project(&[("main.php", main), ("bootstrap.php", bootstrap)]);
    let uri: tower_lsp_server::gen_lsp_types::Uri = uris[0].parse().unwrap();
    open_php(&backend, &uri, main).await;
    let response = goto_definition_at(&backend, &uri, 3, 11)
        .await
        .expect("definition of boot()");
    let text = format!("{response:?}");
    assert!(text.contains("bootstrap.php"), "got: {text}");
}

const MODELS_PHP: &str = r#"<?php
class User {
    public function userName(): string { return ''; }
}
class Post {
    public function postTitle(): string { return ''; }
}
"#;

#[test]
fn an_included_file_reads_the_including_scopes_variables() {
    let main = r#"<?php
function render(User $user): void {
    require __DIR__ . '/header.php';
    $title;
}
"#;
    let hover = hover_in_project(
        &[
            ("main.php", main),
            ("header.php", "<?php\n$title = $user;\n"),
            ("models.php", MODELS_PHP),
        ],
        3,
        5,
    )
    .expect("hover on $title");
    assert!(hover.contains("User"), "got: {hover}");
}

#[test]
fn an_included_file_reads_the_including_scopes_variables_through_a_nested_include() {
    let main = r#"<?php
function render(User $user): void {
    require __DIR__ . '/layout.php';
    $title;
}
"#;
    let hover = hover_in_project(
        &[
            ("main.php", main),
            ("layout.php", "<?php\nrequire __DIR__ . '/header.php';\n"),
            ("header.php", "<?php\n$title = $user;\n"),
            ("models.php", MODELS_PHP),
        ],
        3,
        5,
    )
    .expect("hover on $title");
    assert!(hover.contains("User"), "got: {hover}");
}

#[test]
fn each_include_site_hands_the_included_file_its_own_types() {
    let main = r#"<?php
function showUser(User $item): void {
    require __DIR__ . '/header.php';
    $title->userName();
    $title->postTitle();
}
function showPost(Post $item): void {
    require __DIR__ . '/header.php';
    $title->postTitle();
    $title->userName();
}
"#;
    let (_dir, backend, uris) = project(&[
        ("main.php", main),
        ("header.php", "<?php\n$title = $item;\n"),
        ("models.php", MODELS_PHP),
    ]);
    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(&uris[0], main, &mut diags);
    let lines: Vec<(u32, &str)> = diags
        .iter()
        .map(|d| (d.range.start.line, message_text(&d.message)))
        .collect();
    let flagged =
        |line: u32, method: &str| lines.iter().any(|(l, m)| *l == line && m.contains(method));
    assert!(flagged(4, "postTitle"), "got: {lines:?}");
    assert!(flagged(9, "userName"), "got: {lines:?}");
    assert!(
        !flagged(3, "userName") && !flagged(8, "postTitle"),
        "got: {lines:?}"
    );
}
