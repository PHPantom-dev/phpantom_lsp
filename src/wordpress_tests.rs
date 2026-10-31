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
