use crate::Backend;
use crate::php_type::PhpType;

/// `__()` return types are resolved through the shared loaders, which run
/// inside the workspace index pass. Building the translation shapes must
/// therefore not wait on the index lock, or the index would lock it twice.
#[test]
fn translation_types_resolve_while_the_workspace_index_is_held() {
    let dir = tempfile::tempdir().expect("temp dir");
    let lang = dir.path().join("lang/en");
    std::fs::create_dir_all(&lang).expect("lang dir");
    std::fs::write(
        lang.join("messages.php"),
        "<?php\nreturn [\n    'welcome' => 'Welcome',\n    'checkout' => ['headline' => 'Check out'],\n];\n",
    )
    .expect("lang file");

    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), Vec::new());
    let indexing = backend.workspace_index_lock.lock();

    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let resolver = {
        let backend = backend.clone_for_blocking();
        std::thread::spawn(move || {
            let leaf = backend.resolve_trans_type("messages.welcome");
            let group = backend.resolve_trans_type("messages.checkout");
            done_tx.send((leaf, group)).expect("report result");
        })
    };

    let (leaf, group) = done_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("translation types must not wait on the workspace index");
    drop(indexing);
    resolver.join().expect("resolver thread");

    assert_eq!(leaf, Some(PhpType::string()));
    assert_eq!(group, Some(super::trans_group_type()));
}

#[test]
fn only_the_applications_own_group_files_are_groups() {
    use super::app_lang_group;

    let root = "file:///app";
    for (uri, group) in [
        ("file:///app/lang/en/messages.php", Some("messages")),
        (
            "file:///app/resources/lang/en/validation.php",
            Some("validation"),
        ),
        ("file:///app/lang/en/admin/users.php", Some("admin/users")),
        ("file:///app/lang/vendor/billing/en/invoice.php", None),
        ("file:///app/packages/billing/lang/en/invoice.php", None),
        ("file:///app/lang/en.php", None),
        ("file:///app/language/en/messages.php", None),
        ("file:///other/lang/en/messages.php", None),
    ] {
        assert_eq!(app_lang_group(root, uri), group, "{uri}");
    }
}
