//! Re-parsing an open file must evict the standalone functions and
//! `define()`/`const` constants that an edit deleted or renamed, and must
//! propagate value/offset changes to constants that stayed.
//!
//! Regression tests: previously the global function/define maps were
//! insert-only, so deleting `function foo()` left it in completion, hover, and
//! go-to-definition for the rest of the session, and editing `define('X', 1)`
//! to `define('X', 2)` kept showing the old value.

use crate::common::create_test_backend;

/// Deleting a standalone function removes it from `global_functions` on the
/// next parse of the same file.
#[tokio::test]
async fn deleted_function_is_evicted() {
    let backend = create_test_backend();
    let uri = "file:///funcs.php";

    backend.update_ast(uri, "<?php\nfunction alpha() {}\nfunction beta() {}\n");
    {
        let fmap = backend.global_functions().read();
        assert!(fmap.get("alpha").is_some(), "alpha should be registered");
        assert!(fmap.get("beta").is_some(), "beta should be registered");
    }

    // Remove `beta`.
    backend.update_ast(uri, "<?php\nfunction alpha() {}\n");
    {
        let fmap = backend.global_functions().read();
        assert!(fmap.get("alpha").is_some(), "alpha should survive");
        assert!(
            fmap.get("beta").is_none(),
            "beta should be evicted after deletion"
        );
    }
}

/// Renaming a function evicts the old name and registers the new one.
#[tokio::test]
async fn renamed_function_swaps_entries() {
    let backend = create_test_backend();
    let uri = "file:///rename.php";

    backend.update_ast(uri, "<?php\nfunction old_name() {}\n");
    assert!(backend.global_functions().read().get("old_name").is_some());

    backend.update_ast(uri, "<?php\nfunction new_name() {}\n");
    let fmap = backend.global_functions().read();
    assert!(
        fmap.get("old_name").is_none(),
        "old_name should be gone after rename"
    );
    assert!(fmap.get("new_name").is_some(), "new_name should be present");
}

/// Deleting the last function in a file (leaving none) still evicts it, even
/// though the new parse contributes zero functions.
#[tokio::test]
async fn deleting_only_function_evicts_it() {
    let backend = create_test_backend();
    let uri = "file:///only.php";

    backend.update_ast(uri, "<?php\nfunction solo() {}\n");
    assert!(backend.global_functions().read().get("solo").is_some());

    backend.update_ast(uri, "<?php\n// nothing here now\n");
    assert!(
        backend.global_functions().read().get("solo").is_none(),
        "solo should be evicted when it is the last function removed"
    );
}

/// A function with the same name defined in another file is not clobbered when
/// the first file's copy is deleted.
#[tokio::test]
async fn eviction_does_not_clobber_same_name_in_other_file() {
    let backend = create_test_backend();
    let uri_a = "file:///a.php";
    let uri_b = "file:///b.php";

    backend.update_ast(uri_a, "<?php\nfunction shared() {}\n");
    // `b.php` redefines the same bare name; its entry now wins.
    backend.update_ast(uri_b, "<?php\nfunction shared() {}\n");

    // Deleting the function from a.php must not remove b.php's entry.
    backend.update_ast(uri_a, "<?php\n");
    let fmap = backend.global_functions().read();
    let entry = fmap.get("shared").expect("shared should remain from b.php");
    assert_eq!(entry.0, uri_b, "the surviving entry should belong to b.php");
}

/// Editing a constant's value propagates to `global_defines` (overwrite, not
/// first-write-wins).
#[tokio::test]
async fn changed_define_value_propagates() {
    let backend = create_test_backend();
    let uri = "file:///val.php";

    backend.update_ast(uri, "<?php\ndefine('X', 1);\n");
    assert_eq!(
        backend
            .global_defines()
            .read()
            .get("X")
            .unwrap()
            .value
            .as_deref(),
        Some("1")
    );

    backend.update_ast(uri, "<?php\ndefine('X', 2);\n");
    assert_eq!(
        backend
            .global_defines()
            .read()
            .get("X")
            .unwrap()
            .value
            .as_deref(),
        Some("2"),
        "editing the define value should update global_defines"
    );
}

/// Inserting lines above a `define` updates its stored name offset.
#[tokio::test]
async fn define_offset_updates_on_edit() {
    let backend = create_test_backend();
    let uri = "file:///offset.php";

    backend.update_ast(uri, "<?php\ndefine('OFF', 1);\n");
    let first = backend
        .global_defines()
        .read()
        .get("OFF")
        .unwrap()
        .name_offset;

    backend.update_ast(uri, "<?php\n// inserted comment line\ndefine('OFF', 1);\n");
    let second = backend
        .global_defines()
        .read()
        .get("OFF")
        .unwrap()
        .name_offset;

    assert!(
        second > first,
        "name_offset should move forward after inserting a line above the define \
         (was {first}, now {second})"
    );
}

/// Deleting a `const` declaration evicts it from `global_defines`.
#[tokio::test]
async fn deleted_const_is_evicted() {
    let backend = create_test_backend();
    let uri = "file:///consts.php";

    backend.update_ast(uri, "<?php\nconst KEEP = 1;\nconst DROP = 2;\n");
    {
        let dmap = backend.global_defines().read();
        assert!(dmap.get("KEEP").is_some());
        assert!(dmap.get("DROP").is_some());
    }

    backend.update_ast(uri, "<?php\nconst KEEP = 1;\n");
    let dmap = backend.global_defines().read();
    assert!(dmap.get("KEEP").is_some(), "KEEP should survive");
    assert!(dmap.get("DROP").is_none(), "DROP should be evicted");
}
