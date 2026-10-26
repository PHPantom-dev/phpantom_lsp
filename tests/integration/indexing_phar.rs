use std::fs;

use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

use crate::common::collect_diagnostics_with;

/// Build an uncompressed phar holding `files` (internal path, content).
fn build_phar(files: &[(&str, &str)]) -> Vec<u8> {
    let mut manifest = Vec::new();
    manifest.extend_from_slice(&(files.len() as u32).to_le_bytes());
    manifest.extend_from_slice(&0x1100u16.to_le_bytes()); // API version
    manifest.extend_from_slice(&0u32.to_le_bytes()); // global flags
    manifest.extend_from_slice(&0u32.to_le_bytes()); // alias length
    manifest.extend_from_slice(&0u32.to_le_bytes()); // metadata length
    for (name, content) in files {
        manifest.extend_from_slice(&(name.len() as u32).to_le_bytes());
        manifest.extend_from_slice(name.as_bytes());
        manifest.extend_from_slice(&(content.len() as u32).to_le_bytes()); // uncompressed size
        manifest.extend_from_slice(&0u32.to_le_bytes()); // timestamp
        manifest.extend_from_slice(&(content.len() as u32).to_le_bytes()); // compressed size
        manifest.extend_from_slice(&0u32.to_le_bytes()); // crc32
        manifest.extend_from_slice(&0u32.to_le_bytes()); // flags
        manifest.extend_from_slice(&0u32.to_le_bytes()); // metadata length
    }

    let mut phar = b"<?php __HALT_COMPILER(); ?>\r\n".to_vec();
    phar.extend_from_slice(&(manifest.len() as u32).to_le_bytes());
    phar.extend_from_slice(&manifest);
    for (_, content) in files {
        phar.extend_from_slice(content.as_bytes());
    }
    phar
}

/// A class inside a phar is indexed under the `phar://` URI its parse is
/// cached under, so the index spells it the same way before and after the
/// class is loaded. A lookup racing the first parse used to read the
/// rewritten URI, fail to open it, and remember the class as missing.
#[tokio::test]
async fn phar_classes_are_indexed_under_their_phar_uri() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let root = dir.path();
    fs::write(root.join("composer.json"), "{}\n").unwrap();

    let tool = root.join("vendor/acme/tool");
    fs::create_dir_all(&tool).unwrap();
    fs::write(
        tool.join("tool.phar"),
        build_phar(&[(
            "src/Rule.php",
            "<?php\nnamespace Acme\\Tool;\n\ninterface Rule {}\n",
        )]),
    )
    .unwrap();
    fs::write(
        tool.join("bootstrap.php"),
        "<?php\n\nrequire_once 'phar://' . __DIR__ . '/tool.phar/vendor/autoload.php';\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("vendor/composer")).unwrap();
    fs::write(
        root.join("vendor/composer/autoload_files.php"),
        "<?php\n\n$vendorDir = dirname(__DIR__);\n$baseDir = dirname($vendorDir);\n\nreturn array(\n    'abc123' => $vendorDir . '/acme/tool/bootstrap.php',\n);\n",
    )
    .unwrap();

    let backend = Backend::new_test_with_workspace(root.to_path_buf(), Vec::new());
    backend.initialized(InitializedParams {}).await;

    let phar = tool.join("tool.phar").canonicalize().unwrap();
    let expected = format!("phar://{}/src/Rule.php", phar.display());
    let indexed = backend
        .fqn_uri_index()
        .read()
        .get("Acme\\Tool\\Rule")
        .cloned();
    assert_eq!(indexed.as_deref(), Some(expected.as_str()));

    let diags = collect_diagnostics_with(
        &backend,
        "<?php\nuse Acme\\Tool\\Rule;\n\nclass MyRule implements Rule {}\n",
        Backend::collect_unknown_class_diagnostics,
    );
    assert!(
        diags.is_empty(),
        "the phar class should resolve, got: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    assert_eq!(
        backend
            .fqn_uri_index()
            .read()
            .get("Acme\\Tool\\Rule")
            .cloned()
            .as_deref(),
        Some(expected.as_str()),
        "loading the class must not respell its index entry"
    );
}
