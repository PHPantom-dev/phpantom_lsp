//! Workspace initialization and indexing.
//!
//! This module owns the "scan the workspace into symbol indexes" pipeline.
//! It has nothing to do with LSP dispatch (`server.rs`) or reference finding
//! (`references/`); those modules delegate here.
//!
//! - [`init`] — the three workspace-shape initializers (single Composer
//!   project, monorepo, no-Composer).
//! - [`scan`] — vendor registration and Composer-derived index (re)builds,
//!   autoload-file and PHAR scanning.
//! - [`preload`] — autoload preloading and the `ensure_workspace_indexed*`
//!   parallel parse pipeline.
//! - [`watch`] — applying `didChangeWatchedFiles` batches to the indexes.
//! - [`reconcile`] — realigning the indexes with the file filters when
//!   those change mid-session.

use std::path::{Path, PathBuf};

mod init;
pub(crate) mod preload;
mod reconcile;
mod scan;
mod watch;

/// Return the path as supplied plus its canonical spelling when the
/// filesystem exposes the same location through an alias.
pub(crate) fn path_aliases(path: &Path) -> Vec<PathBuf> {
    let mut paths = vec![path.to_path_buf()];
    if let Ok(canonical) = path.canonicalize()
        && canonical != path
    {
        paths.push(canonical);
    }
    paths
}

/// Classify where a class file originates (project source, a direct vendor
/// dependency, or a transitive vendor dependency) for completion ranking.
pub(crate) fn classify_class_origin(
    path: &Path,
    vendor_paths: &[PathBuf],
    vendor_package_roots: &[(PathBuf, crate::ClassCompletionOrigin, String)],
) -> crate::ClassCompletionOrigin {
    if !vendor_paths.iter().any(|vendor| path.starts_with(vendor)) {
        return crate::ClassCompletionOrigin::Project;
    }
    for (root, origin, _pkg_name) in vendor_package_roots {
        if path.starts_with(root) {
            return *origin;
        }
    }
    crate::ClassCompletionOrigin::VendorTransitive
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn canonical_vendor_files_stay_vendor_through_an_aliased_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let canonical_vendor = dir.path().join("packages");
        let package_src = canonical_vendor.join("acme/package/src");
        std::fs::create_dir_all(&package_src).expect("create package directory");
        let aliased_vendor = dir.path().join("vendor");
        symlink(&canonical_vendor, &aliased_vendor).expect("create vendor alias");

        let vendor_paths = path_aliases(&aliased_vendor);
        let class_path = package_src.join("Service.php");
        std::fs::write(&class_path, "<?php").expect("write class file");
        let class_path = class_path.canonicalize().expect("canonical class path");

        assert_eq!(
            classify_class_origin(&class_path, &vendor_paths, &[]),
            crate::ClassCompletionOrigin::VendorTransitive
        );
    }
}
