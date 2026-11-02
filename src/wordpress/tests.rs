use std::fs;
use std::path::Path;

use crate::Backend;
use crate::config::{Config, StubsConfig};

fn set_stub_paths(backend: &Backend, paths: &[&str]) {
    backend.set_config(Config {
        stubs: StubsConfig {
            paths: Some(paths.iter().map(|p| p.to_string()).collect()),
        },
        ..Config::default()
    });
    backend.refresh_stub_paths();
}

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, content).unwrap();
}

#[test]
fn wordpress_stubs_bring_the_output_constants() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "wp/wordpress-stubs.php", "<?php\nclass wpdb {}\n");
    write(root, "other/acme.php", "<?php\nclass Acme {}\n");
    let backend = Backend::new_test_with_workspace(root.to_path_buf(), Vec::new());

    set_stub_paths(&backend, &["other"]);
    assert_eq!(backend.lookup_global_constant("OBJECT"), None);

    set_stub_paths(&backend, &["wp"]);
    assert_eq!(
        backend.lookup_global_constant("OBJECT"),
        Some(Some("'OBJECT'".to_string()))
    );
    assert_eq!(
        backend.lookup_global_constant("ARRAY_A"),
        Some(Some("'ARRAY_A'".to_string()))
    );

    set_stub_paths(&backend, &["other"]);
    assert_eq!(backend.lookup_global_constant("OBJECT"), None);
    assert_eq!(backend.lookup_global_constant("ARRAY_A"), None);
}

#[test]
fn a_stub_path_declaring_the_output_constants_wins() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "wp/wordpress-stubs.php", "<?php\nclass wpdb {}\n");
    write(
        root,
        "wp/wordpress-globals.php",
        "<?php\ndefine('OBJECT', 'from-globals');\n",
    );
    let backend = Backend::new_test_with_workspace(root.to_path_buf(), Vec::new());

    set_stub_paths(&backend, &["wp"]);
    assert_eq!(
        backend.lookup_global_constant("OBJECT"),
        Some(Some("'from-globals'".to_string()))
    );
}

const WORDPRESS_STUBS: &str = "<?php\nclass wpdb {}\n/** @return mixed */\nfunction get_post_meta($post_id, $key = '', $single = false) {}\n";

fn return_types(backend: &Backend, name: &str) -> (Option<String>, Option<String>) {
    let function = backend.find_or_load_function(&[name]).unwrap();
    (
        function.return_type.map(|t| t.to_string()),
        function.conditional_return.map(|t| t.to_string()),
    )
}

#[test]
fn the_wordpress_stubs_take_the_overridden_return_type() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "wp/wordpress-stubs.php", WORDPRESS_STUBS);
    let backend = Backend::new_test_with_workspace(root.to_path_buf(), Vec::new());

    set_stub_paths(&backend, &["wp"]);
    let (return_type, conditional) = return_types(&backend, "get_post_meta");
    assert_eq!(return_type.as_deref(), Some("mixed"));
    assert!(
        conditional.is_some_and(|c| c.contains("$single is true")),
        "get_post_meta should read the override's conditional return type"
    );
}

#[test]
fn another_stub_file_keeps_its_own_return_type() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "wp/wordpress-stubs.php", WORDPRESS_STUBS);
    write(
        root,
        "mine/meta.php",
        "<?php\n/** @return int */\nfunction get_post_meta($post_id, $key = '', $single = false) {}\n",
    );
    let backend = Backend::new_test_with_workspace(root.to_path_buf(), Vec::new());

    set_stub_paths(&backend, &["mine", "wp"]);
    assert_eq!(
        return_types(&backend, "get_post_meta"),
        (Some("int".to_string()), None)
    );
}

#[test]
fn without_wpdb_a_stub_file_is_not_the_wordpress_stubs() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "other/meta.php",
        "<?php\n/** @return mixed */\nfunction get_post_meta($post_id, $key = '', $single = false) {}\n",
    );
    let backend = Backend::new_test_with_workspace(root.to_path_buf(), Vec::new());

    set_stub_paths(&backend, &["other"]);
    assert_eq!(
        return_types(&backend, "get_post_meta"),
        (Some("mixed".to_string()), None)
    );
}
