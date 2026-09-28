use super::join_call_site_types;
use crate::Backend;
use crate::atom::atom;
use crate::php_type::PhpType;
use tower_lsp::lsp_types::Url;

#[cfg(unix)]
use std::os::unix::fs::symlink;

/// The joined union's member order must not depend on the order the
/// call sites were visited in, since that order comes from a
/// `HashMap` snapshot and varies across runs.
#[test]
fn union_order_is_independent_of_visit_order() {
    let a = PhpType::named(atom("App\\Item"));
    let b = PhpType::literal_string_raw("fallback");

    let forward = join_call_site_types(vec![a.clone(), b.clone()]).to_string();
    let backward = join_call_site_types(vec![b, a]).to_string();

    assert_eq!(
        forward, backward,
        "joining the same types in reverse order must produce the same union string"
    );
}

#[test]
fn headless_view_snapshot_builds_local_candidates() {
    let mut backend = Backend::new_test();
    backend.skip_reference_index = true;
    let uri = "file:///project/app/Controller.php";
    backend.update_ast(uri, "<?php\nview('shop', ['item' => new Item()]);\n");

    let snapshot = backend.view_caller_snapshot();
    let candidates = snapshot
        .local_candidates
        .as_ref()
        .and_then(|by_view| by_view.get("shop"))
        .expect("headless refresh should index the view caller locally");

    assert_eq!(candidates.len(), 1);
    assert_eq!(snapshot.files[candidates[0]].0, uri);
}

#[test]
fn headless_inference_skips_non_candidate_callers() {
    let dir = tempfile::tempdir().expect("failed to create test workspace");
    let root = dir
        .path()
        .canonicalize()
        .expect("test workspace should canonicalize");
    let views = root.join("resources/views");
    let app = root.join("app");
    std::fs::create_dir_all(&views).expect("failed to create view directory");
    std::fs::create_dir_all(&app).expect("failed to create app directory");

    let target = views.join("shop.blade.php");
    let matching = app.join("MatchingController.php");
    let unrelated = app.join("UnrelatedController.php");
    let matching_source = "<?php\nview('shop', ['kept' => 1]);\n";
    let unrelated_source = "<?php\nview('other', ['excluded' => 2]);\n";
    std::fs::write(&target, "").expect("failed to write target view");
    std::fs::write(&matching, matching_source).expect("failed to write matching caller");
    std::fs::write(&unrelated, unrelated_source).expect("failed to write unrelated caller");

    let mut backend = Backend::new_test_with_workspace(root, Vec::new());
    backend.skip_reference_index = true;
    for (path, source) in [(&matching, matching_source), (&unrelated, unrelated_source)] {
        let uri = Url::from_file_path(path).expect("caller path should become a file URI");
        backend.update_ast(uri.as_str(), source);
    }

    let snapshot = backend.view_caller_snapshot();
    let by_view = snapshot
        .local_candidates
        .as_ref()
        .expect("headless snapshot should carry local candidates");
    assert_eq!(snapshot.files.len(), 2);
    assert_eq!(by_view.get("shop").map(Vec::len), Some(1));
    assert_eq!(by_view.get("other").map(Vec::len), Some(1));

    let target_uri = Url::from_file_path(target).expect("target path should become a file URI");
    assert_eq!(
        backend.view_names_for_blade_uri(target_uri.as_str()),
        vec!["shop"]
    );
    let scope = backend.compute_blade_injected_vars(target_uri.as_str(), "", Some(&snapshot), None);

    assert!(
        scope.vars.iter().any(|(name, _)| name == "kept"),
        "matching caller should contribute its data: {scope:?}"
    );
    assert!(
        scope.vars.iter().all(|(name, _)| name != "excluded"),
        "non-candidate caller must be skipped: {scope:?}"
    );
}

#[cfg(unix)]
#[test]
fn view_name_resolution_normalizes_aliased_file_paths() {
    let dir = tempfile::tempdir().expect("failed to create test workspace");
    let real_root = dir.path().join("real-project");
    let linked_root = dir.path().join("linked-project");
    let views = real_root.join("resources/views");
    std::fs::create_dir_all(&views).expect("failed to create view directory");
    symlink(&real_root, &linked_root).expect("failed to create workspace alias");

    let real_template = views.join("shop.blade.php");
    std::fs::write(&real_template, "").expect("failed to write view");
    let linked_template = linked_root.join("resources/views/shop.blade.php");
    let linked_uri =
        Url::from_file_path(&linked_template).expect("view path should become a file URI");
    let backend = Backend::new_test_with_workspace(linked_root, Vec::new());

    assert_eq!(
        backend.view_names_for_blade_uri(linked_uri.as_str()),
        vec!["shop"]
    );

    std::fs::remove_file(real_template).expect("failed to remove view");
    assert_eq!(
        backend.view_names_for_blade_uri(linked_uri.as_str()),
        vec!["shop"]
    );
}

/// A template that is itself a symlink into a shared directory is
/// still addressable by the name it has inside the view root.
#[cfg(unix)]
#[test]
fn view_name_resolution_keeps_symlinked_templates() {
    let dir = tempfile::tempdir().expect("failed to create test workspace");
    let root = dir.path().join("project");
    let views = root.join("resources/views");
    let shared = dir.path().join("shared");
    std::fs::create_dir_all(&views).expect("failed to create view directory");
    std::fs::create_dir_all(&shared).expect("failed to create shared directory");

    let shared_template = shared.join("shop.blade.php");
    std::fs::write(&shared_template, "").expect("failed to write view");
    let template = views.join("shop.blade.php");
    symlink(&shared_template, &template).expect("failed to link view into the root");

    let uri = Url::from_file_path(&template).expect("view path should become a file URI");
    let backend = Backend::new_test_with_workspace(root, Vec::new());

    assert_eq!(backend.view_names_for_blade_uri(uri.as_str()), vec!["shop"]);
}
