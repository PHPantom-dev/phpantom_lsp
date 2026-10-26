use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::*;
use crate::ClassCompletionOrigin;

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// A workspace of two folders, `app` (the server's root) and `lib`,
/// each declaring one class.
fn two_folders() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let app = dir.path().join("app");
    let lib = dir.path().join("lib");
    write(&app.join("src/App.php"), "<?php\nclass App {}\n");
    write(&lib.join("src/Shared.php"), "<?php\nclass Shared {}\n");
    (dir, app, lib)
}

fn backend_for(root: &Path, folders: &[&Path]) -> Backend {
    let backend = Backend::new_test();
    *backend.workspace.workspace_root.write() = Some(root.to_path_buf());
    *backend.workspace.workspace_folders.write() =
        folders.iter().map(|folder| folder.to_path_buf()).collect();
    backend
}

async fn discover(backend: &Backend, root: &Path) {
    backend
        .discover_workspace_symbols(root, Default::default(), None, None)
        .await;
}

fn is_library_uri(backend: &Backend, path: &Path) -> bool {
    let uri = crate::util::path_to_uri(path);
    backend
        .workspace
        .vendor_uri_prefixes
        .lock()
        .iter()
        .any(|prefix| uri.starts_with(prefix.as_str()))
}

/// The folder holding the root is the project, and a folder inside it
/// is already walked as part of it. A parent of the root would walk the
/// project a second time, and a folder listed twice, or inside another
/// listed folder, would walk the shared tree twice.
#[test]
fn overlapping_folders_are_walked_once() {
    let dir = tempfile::tempdir().unwrap();
    let app = dir.path().join("app");
    let nested = app.join("packages/inner");
    let lib = dir.path().join("lib");
    let lib_child = lib.join("sub");
    let other = dir.path().join("other");
    for path in [&nested, &lib_child, &other] {
        std::fs::create_dir_all(path).unwrap();
    }

    let roots = distinct_extra_roots(
        &app,
        &[
            app.clone(),
            dir.path().to_path_buf(),
            nested,
            lib_child,
            lib.clone(),
            other.clone(),
            lib.clone(),
            dir.path().join("missing"),
        ],
    );

    assert_eq!(roots, vec![lib, other]);
}

#[cfg(unix)]
#[test]
fn a_folder_spelled_through_a_link_is_a_duplicate() {
    let (dir, app, lib) = two_folders();
    let alias = dir.path().join("lib-alias");
    std::os::unix::fs::symlink(&lib, &alias).unwrap();

    assert_eq!(distinct_extra_roots(&app, &[lib.clone(), alias]), vec![lib]);
}

/// The point of the feature: a class in another folder of the workspace
/// resolves from this one, as library code, so this server neither
/// reports on that folder nor edits it.
#[tokio::test]
async fn another_workspace_folder_is_indexed_as_library_code() {
    let (_dir, app, lib) = two_folders();
    let backend = backend_for(&app, &[&app, &lib]);

    discover(&backend, &app).await;

    let origins = backend.symbols.fqn_origin_index.read();
    assert_ne!(
        origins.get("App").copied(),
        Some(ClassCompletionOrigin::VendorExplicit)
    );
    assert_eq!(
        origins.get("Shared").copied(),
        Some(ClassCompletionOrigin::VendorExplicit)
    );
    assert!(is_library_uri(&backend, &lib.join("src/Shared.php")));
    assert!(!is_library_uri(&backend, &app.join("src/App.php")));
}

/// The Composer strategy never walks the project's root, so the folders
/// beside it get a walk of their own rather than none.
#[tokio::test]
async fn a_composer_project_still_indexes_the_other_folders() {
    let (_dir, app, lib) = two_folders();
    write(
        &app.join("composer.json"),
        r#"{ "autoload": { "psr-4": { "": "src/" } } }"#,
    );
    let backend = backend_for(&app, &[&app, &lib]);
    backend.set_config(toml::from_str("[indexing]\nstrategy = \"composer\"\n").unwrap());

    let package = crate::composer::read_composer_package(&app);
    backend
        .discover_workspace_symbols(&app, Default::default(), package, None)
        .await;

    assert!(backend.symbols.fqn_uri_index.read().get("Shared").is_some());
}

/// A link inside the project pointing at another folder loses to that
/// folder's own walk, so its classes are indexed once, under the
/// spelling the editor opens them by.
#[cfg(unix)]
#[tokio::test]
async fn a_link_into_another_folder_does_not_index_it_twice() {
    let (_dir, app, lib) = two_folders();
    std::os::unix::fs::symlink(&lib, app.join("linked-lib")).unwrap();
    let backend = backend_for(&app, &[&app, &lib]);

    discover(&backend, &app).await;

    let uri = backend
        .symbols
        .fqn_uri_index
        .read()
        .get("Shared")
        .cloned()
        .unwrap();
    assert_eq!(uri, crate::util::path_to_uri(&lib.join("src/Shared.php")));
}

/// Taking a folder out of the workspace is routine, so what it declared
/// stops resolving at once instead of lingering until a restart, and its
/// files stop being treated as library code.
#[tokio::test]
async fn a_removed_folder_leaves_the_index() {
    let (_dir, app, lib) = two_folders();
    let backend = backend_for(&app, &[&app, &lib]);
    discover(&backend, &app).await;
    assert!(backend.symbols.fqn_uri_index.read().get("Shared").is_some());

    let previous = backend.discovery_inputs();
    backend
        .workspace
        .workspace_folders
        .write()
        .retain(|f| *f != lib);
    backend.reconcile_index_for_input_change(&previous);

    let index = backend.symbols.fqn_uri_index.read();
    assert!(index.get("Shared").is_none());
    assert!(index.get("App").is_some(), "the project must be untouched");
    assert!(!is_library_uri(&backend, &lib.join("src/Shared.php")));
}

/// The document in front of the user is served whatever the workspace
/// holds, the same as one under a newly excluded path.
#[tokio::test]
async fn an_open_file_survives_its_folder_being_removed() {
    let (_dir, app, lib) = two_folders();
    let backend = backend_for(&app, &[&app, &lib]);
    discover(&backend, &app).await;
    let uri = crate::util::path_to_uri(&lib.join("src/Shared.php"));
    backend
        .open_files
        .write()
        .insert(uri, Arc::new("<?php\nclass Shared {}\n".to_string()));

    let previous = backend.discovery_inputs();
    backend
        .workspace
        .workspace_folders
        .write()
        .retain(|f| *f != lib);
    backend.reconcile_index_for_input_change(&previous);

    assert!(backend.symbols.fqn_uri_index.read().get("Shared").is_some());
}

/// A folder added mid-session names files no walk has seen. A folder
/// that was hidden inside a removed parent counts as added too, since the
/// parent's eviction takes its files with it.
#[test]
fn an_added_or_uncovered_folder_asks_for_a_walk() {
    let dir = tempfile::tempdir().unwrap();
    let app = dir.path().join("app");
    let parent = dir.path().join("libs");
    let child = parent.join("one");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::create_dir_all(&child).unwrap();
    let backend = backend_for(&app, &[&app, &parent, &child]);

    let with_parent = backend.discovery_inputs();
    assert_eq!(with_parent.extra_roots, vec![parent.clone()]);

    backend
        .workspace
        .workspace_folders
        .write()
        .retain(|f| *f != parent);
    let without_parent = backend.discovery_inputs();
    assert_eq!(without_parent.extra_roots, vec![child]);
    assert!(without_parent.may_admit_more_than(&with_parent));
    assert!(!with_parent.may_admit_more_than(&with_parent));
}

/// One directory can be library code for two reasons. Removing the
/// folder must not stop treating it as library code while the
/// `[indexing] include` entry naming it still stands.
#[test]
fn a_location_stays_registered_while_another_source_claims_it() {
    let (_dir, app, lib) = two_folders();
    let backend = backend_for(&app, &[]);
    backend.add_library_dir(LibraryClaimant::Include, &lib);
    backend.add_library_dir(LibraryClaimant::ExtraRoot(lib.clone()), &lib);

    backend.release_library_claimant(&LibraryClaimant::ExtraRoot(lib.clone()));
    assert!(is_library_uri(&backend, &lib.join("src/Shared.php")));

    backend.release_library_claimant(&LibraryClaimant::Include);
    assert!(!is_library_uri(&backend, &lib.join("src/Shared.php")));
}
