use crate::common::create_test_backend;
use std::fs;
use tower_lsp_server::gen_lsp_types::*;

/// Helper: open a file in the backend and return its document links.
fn get_document_links(
    backend: &phpantom_lsp::Backend,
    uri: &str,
    content: &str,
) -> Vec<DocumentLink> {
    backend.update_ast(uri, content);
    backend
        .handle_document_link(uri, content)
        .unwrap_or_default()
}

/// Helper: extract just the target URLs (as strings) from document links.
fn link_targets(links: &[DocumentLink]) -> Vec<String> {
    links
        .iter()
        .filter_map(|l| l.target.as_ref().map(|u| u.to_string()))
        .collect()
}

// ─── Include/Require Links ──────────────────────────────────────────────────

#[test]
fn require_once_string_literal() {
    let dir = tempfile::tempdir().unwrap();
    let target_file = dir.path().join("bootstrap.php");
    fs::write(&target_file, "<?php // bootstrap").unwrap();

    let main_content = format!("<?php\nrequire_once '{}';\n", target_file.to_string_lossy());
    let main_uri = format!("file://{}", dir.path().join("main.php").to_string_lossy());

    let backend = create_test_backend();
    let links = get_document_links(&backend, &main_uri, &main_content);

    assert_eq!(
        links.len(),
        1,
        "Expected one require_once link, got: {:?}",
        link_targets(&links)
    );
    let target = links[0].target.as_ref().unwrap();
    assert!(
        target.to_file_path().unwrap().ends_with("bootstrap.php"),
        "Expected link to bootstrap.php, got: {}",
        target
    );
}

#[test]
fn include_string_literal() {
    let dir = tempfile::tempdir().unwrap();
    let target_file = dir.path().join("config.php");
    fs::write(&target_file, "<?php // config").unwrap();

    let main_content = format!("<?php\ninclude '{}';\n", target_file.to_string_lossy());
    let main_uri = format!("file://{}", dir.path().join("main.php").to_string_lossy());

    let backend = create_test_backend();
    let links = get_document_links(&backend, &main_uri, &main_content);

    assert_eq!(
        links.len(),
        1,
        "Expected one include link, got: {:?}",
        link_targets(&links)
    );
}

#[test]
fn require_string_literal() {
    let dir = tempfile::tempdir().unwrap();
    let target_file = dir.path().join("helpers.php");
    fs::write(&target_file, "<?php // helpers").unwrap();

    let main_content = format!("<?php\nrequire '{}';\n", target_file.to_string_lossy());
    let main_uri = format!("file://{}", dir.path().join("main.php").to_string_lossy());

    let backend = create_test_backend();
    let links = get_document_links(&backend, &main_uri, &main_content);

    assert_eq!(
        links.len(),
        1,
        "Expected one require link, got: {:?}",
        link_targets(&links)
    );
}

#[test]
fn include_once_string_literal() {
    let dir = tempfile::tempdir().unwrap();
    let target_file = dir.path().join("utils.php");
    fs::write(&target_file, "<?php // utils").unwrap();

    let main_content = format!("<?php\ninclude_once '{}';\n", target_file.to_string_lossy());
    let main_uri = format!("file://{}", dir.path().join("main.php").to_string_lossy());

    let backend = create_test_backend();
    let links = get_document_links(&backend, &main_uri, &main_content);

    assert_eq!(
        links.len(),
        1,
        "Expected one include_once link, got: {:?}",
        link_targets(&links)
    );
}

#[test]
fn require_once_dir_concat() {
    let dir = tempfile::tempdir().unwrap();
    let target_file = dir.path().join("bootstrap.php");
    fs::write(&target_file, "<?php // bootstrap").unwrap();

    let main_content = "<?php\nrequire_once __DIR__ . '/bootstrap.php';\n";
    let main_uri = format!("file://{}", dir.path().join("main.php").to_string_lossy());

    let backend = create_test_backend();
    let links = get_document_links(&backend, &main_uri, main_content);

    assert_eq!(
        links.len(),
        1,
        "Expected one __DIR__ concat link, got: {:?}",
        link_targets(&links)
    );
    let target = links[0].target.as_ref().unwrap();
    assert!(
        target.to_file_path().unwrap().ends_with("bootstrap.php"),
        "Expected link to bootstrap.php, got: {}",
        target
    );
}

#[test]
fn require_once_dirname_dir() {
    let dir = tempfile::tempdir().unwrap();
    let sub_dir = dir.path().join("src");
    fs::create_dir_all(&sub_dir).unwrap();
    let target_file = dir.path().join("vendor/autoload.php");
    fs::create_dir_all(target_file.parent().unwrap()).unwrap();
    fs::write(&target_file, "<?php // autoload").unwrap();

    let main_content = "<?php\nrequire_once dirname(__DIR__) . '/vendor/autoload.php';\n";
    let main_uri = format!("file://{}", sub_dir.join("main.php").to_string_lossy());

    let backend = create_test_backend();
    let links = get_document_links(&backend, &main_uri, main_content);

    assert_eq!(
        links.len(),
        1,
        "Expected one dirname(__DIR__) link, got: {:?}",
        link_targets(&links)
    );
    let target = links[0].target.as_ref().unwrap();
    let target_path = target.to_file_path().unwrap();
    assert!(
        target_path.ends_with("vendor/autoload.php"),
        "Expected link to vendor/autoload.php, got: {:?}",
        target_path
    );
}

#[test]
fn no_link_for_nonexistent_file() {
    let dir = tempfile::tempdir().unwrap();
    // Do NOT create the target file.

    let main_content = format!(
        "<?php\nrequire_once '{}';\n",
        dir.path().join("nonexistent.php").to_string_lossy()
    );
    let main_uri = format!("file://{}", dir.path().join("main.php").to_string_lossy());

    let backend = create_test_backend();
    let links = get_document_links(&backend, &main_uri, &main_content);

    assert!(
        links.is_empty(),
        "Expected no links for nonexistent file, got: {:?}",
        link_targets(&links)
    );
}

#[test]
fn require_relative_path() {
    let dir = tempfile::tempdir().unwrap();
    let target_file = dir.path().join("helpers.php");
    fs::write(&target_file, "<?php // helpers").unwrap();

    // A relative path should resolve relative to the file's directory.
    let main_content = "<?php\nrequire_once 'helpers.php';\n";
    let main_uri = format!("file://{}", dir.path().join("main.php").to_string_lossy());

    let backend = create_test_backend();
    let links = get_document_links(&backend, &main_uri, main_content);

    assert_eq!(
        links.len(),
        1,
        "Expected one relative path link, got: {:?}",
        link_targets(&links)
    );
}

#[test]
fn multiple_includes_in_one_file() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.php"), "<?php").unwrap();
    fs::write(dir.path().join("b.php"), "<?php").unwrap();
    fs::write(dir.path().join("c.php"), "<?php").unwrap();

    let main_content = format!(
        "<?php\nrequire_once '{a}';\ninclude '{b}';\nrequire '{c}';\n",
        a = dir.path().join("a.php").to_string_lossy(),
        b = dir.path().join("b.php").to_string_lossy(),
        c = dir.path().join("c.php").to_string_lossy(),
    );
    let main_uri = format!("file://{}", dir.path().join("main.php").to_string_lossy());

    let backend = create_test_backend();
    let links = get_document_links(&backend, &main_uri, &main_content);

    assert_eq!(
        links.len(),
        3,
        "Expected three include links, got: {:?}",
        link_targets(&links)
    );
}

#[test]
fn include_inside_function_body() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("config.php");
    fs::write(&target, "<?php // config").unwrap();

    let main_content = format!(
        "<?php\nfunction boot() {{\n    require_once '{}';\n}}\n",
        target.to_string_lossy()
    );
    let main_uri = format!("file://{}", dir.path().join("main.php").to_string_lossy());

    let backend = create_test_backend();
    let links = get_document_links(&backend, &main_uri, &main_content);

    assert_eq!(
        links.len(),
        1,
        "Expected include inside function body, got: {:?}",
        link_targets(&links)
    );
}

#[test]
fn include_inside_method_body() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("routes.php");
    fs::write(&target, "<?php // routes").unwrap();

    let main_content = format!(
        "<?php\nclass App {{\n    public function boot(): void {{\n        require_once '{}';\n    }}\n}}\n",
        target.to_string_lossy()
    );
    let main_uri = format!("file://{}", dir.path().join("main.php").to_string_lossy());

    let backend = create_test_backend();
    let links = get_document_links(&backend, &main_uri, &main_content);

    assert_eq!(
        links.len(),
        1,
        "Expected include inside method body, got: {:?}",
        link_targets(&links)
    );
}

#[test]
fn include_inside_if_statement() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("debug.php");
    fs::write(&target, "<?php // debug").unwrap();

    let main_content = format!(
        "<?php\nif (true) {{\n    require_once '{}';\n}}\n",
        target.to_string_lossy()
    );
    let main_uri = format!("file://{}", dir.path().join("main.php").to_string_lossy());

    let backend = create_test_backend();
    let links = get_document_links(&backend, &main_uri, &main_content);

    assert_eq!(
        links.len(),
        1,
        "Expected include inside if, got: {:?}",
        link_targets(&links)
    );
}

#[test]
fn include_inside_namespace() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("helpers.php");
    fs::write(&target, "<?php // helpers").unwrap();

    let main_content = format!(
        "<?php\nnamespace App;\n\nrequire_once '{}';\n",
        target.to_string_lossy()
    );
    let main_uri = format!("file://{}", dir.path().join("main.php").to_string_lossy());

    let backend = create_test_backend();
    let links = get_document_links(&backend, &main_uri, &main_content);

    assert_eq!(
        links.len(),
        1,
        "Expected include inside namespace, got: {:?}",
        link_targets(&links)
    );
}

// ─── Link Range Accuracy ────────────────────────────────────────────────────

#[test]
fn include_link_range_spans_value_expression() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("boot.php");
    fs::write(&target, "<?php").unwrap();

    let main_content = format!("<?php\nrequire_once '{}';\n", target.to_string_lossy());
    let main_uri = format!("file://{}", dir.path().join("main.php").to_string_lossy());

    let backend = create_test_backend();
    let links = get_document_links(&backend, &main_uri, &main_content);

    assert_eq!(links.len(), 1);
    let range = links[0].range;
    // The link range should be on line 1.
    assert_eq!(range.start.line, 1);
}

// ─── Edge Cases ─────────────────────────────────────────────────────────────

#[test]
fn empty_file_returns_no_links() {
    let backend = create_test_backend();
    let links = get_document_links(&backend, "file:///test.php", "<?php\n");

    assert!(links.is_empty());
}

#[test]
fn no_links_in_plain_code() {
    let backend = create_test_backend();
    let content = r#"<?php
$x = 42;
echo $x;
"#;
    let uri = "file:///test.php";
    let links = get_document_links(&backend, uri, content);

    assert!(links.is_empty());
}

#[test]
fn variable_include_path_not_resolved() {
    let backend = create_test_backend();
    let content = "<?php\nrequire_once $path;\n";
    let uri = "file:///test.php";
    let links = get_document_links(&backend, uri, content);

    // Dynamic paths should not produce links.
    assert!(
        links.is_empty(),
        "Dynamic paths should not produce file links"
    );
}

#[test]
fn dirname_dir_with_levels() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("a/b");
    fs::create_dir_all(&sub).unwrap();
    let target = dir.path().join("vendor/autoload.php");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, "<?php // autoload").unwrap();

    // dirname(__DIR__, 2) from a/b/ should go up 2 levels to dir/
    let main_content = "<?php\nrequire_once dirname(__DIR__, 2) . '/vendor/autoload.php';\n";
    let main_uri = format!("file://{}", sub.join("main.php").to_string_lossy());

    let backend = create_test_backend();
    let links = get_document_links(&backend, &main_uri, main_content);

    assert_eq!(
        links.len(),
        1,
        "Expected dirname(__DIR__, 2) link, got: {:?}",
        link_targets(&links)
    );
    let target_path = links[0].target.as_ref().unwrap().to_file_path().unwrap();
    assert!(
        target_path.ends_with("vendor/autoload.php"),
        "Expected vendor/autoload.php, got: {:?}",
        target_path
    );
}

#[test]
fn dirname_file_concat() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("helpers.php");
    fs::write(&target, "<?php // helpers").unwrap();

    // dirname(__FILE__) . '/helpers.php' should resolve to file_dir/helpers.php
    let main_content = "<?php\nrequire_once dirname(__FILE__) . '/helpers.php';\n";
    let main_uri = format!("file://{}", dir.path().join("main.php").to_string_lossy());

    let backend = create_test_backend();
    let links = get_document_links(&backend, &main_uri, main_content);

    assert_eq!(
        links.len(),
        1,
        "Expected dirname(__FILE__) link, got: {:?}",
        link_targets(&links)
    );
    let target_path = links[0].target.as_ref().unwrap().to_file_path().unwrap();
    assert!(
        target_path.ends_with("helpers.php"),
        "Expected helpers.php, got: {:?}",
        target_path
    );
}

#[test]
fn nested_dirname_calls() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("src/app");
    fs::create_dir_all(&sub).unwrap();
    let target = dir.path().join("vendor/autoload.php");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, "<?php // autoload").unwrap();

    // dirname(dirname(__DIR__)) from src/app/ goes up 2 levels
    let main_content = "<?php\nrequire_once dirname(dirname(__DIR__)) . '/vendor/autoload.php';\n";
    let main_uri = format!("file://{}", sub.join("main.php").to_string_lossy());

    let backend = create_test_backend();
    let links = get_document_links(&backend, &main_uri, main_content);

    assert_eq!(
        links.len(),
        1,
        "Expected nested dirname link, got: {:?}",
        link_targets(&links)
    );
    let target_path = links[0].target.as_ref().unwrap().to_file_path().unwrap();
    assert!(
        target_path.ends_with("vendor/autoload.php"),
        "Expected vendor/autoload.php, got: {:?}",
        target_path
    );
}

#[test]
fn include_in_try_block() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("risky.php");
    fs::write(&target, "<?php // risky").unwrap();

    let main_content = format!(
        "<?php\ntry {{\n    require_once '{}';\n}} catch (\\Exception $e) {{}}\n",
        target.to_string_lossy()
    );
    let main_uri = format!("file://{}", dir.path().join("main.php").to_string_lossy());

    let backend = create_test_backend();
    let links = get_document_links(&backend, &main_uri, &main_content);

    assert_eq!(
        links.len(),
        1,
        "Expected include inside try block, got: {:?}",
        link_targets(&links)
    );
}

#[test]
fn include_in_switch_case() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("handler.php");
    fs::write(&target, "<?php // handler").unwrap();

    let main_content = format!(
        "<?php\nswitch ($x) {{\n    case 1:\n        require_once '{}';\n        break;\n}}\n",
        target.to_string_lossy()
    );
    let main_uri = format!("file://{}", dir.path().join("main.php").to_string_lossy());

    let backend = create_test_backend();
    let links = get_document_links(&backend, &main_uri, &main_content);

    assert_eq!(
        links.len(),
        1,
        "Expected include in switch case, got: {:?}",
        link_targets(&links)
    );
}

#[test]
fn include_in_foreach() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("item.php");
    fs::write(&target, "<?php // item").unwrap();

    let main_content = format!(
        "<?php\nforeach ($items as $item) {{\n    require_once '{}';\n}}\n",
        target.to_string_lossy()
    );
    let main_uri = format!("file://{}", dir.path().join("main.php").to_string_lossy());

    let backend = create_test_backend();
    let links = get_document_links(&backend, &main_uri, &main_content);

    assert_eq!(
        links.len(),
        1,
        "Expected include in foreach, got: {:?}",
        link_targets(&links)
    );
}

#[test]
fn double_quoted_include_path() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("helpers.php");
    fs::write(&target, "<?php").unwrap();

    let main_content = format!("<?php\nrequire_once \"{}\";\n", target.to_string_lossy());
    let main_uri = format!("file://{}", dir.path().join("main.php").to_string_lossy());

    let backend = create_test_backend();
    let links = get_document_links(&backend, &main_uri, &main_content);

    assert_eq!(
        links.len(),
        1,
        "Expected double-quoted include link, got: {:?}",
        link_targets(&links)
    );
}
