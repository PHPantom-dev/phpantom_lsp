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

/// Classify where a class file originates (project source, a direct vendor
/// dependency, or a transitive vendor dependency) for completion ranking.
pub(crate) fn classify_class_origin(
    path: &Path,
    vendor_path: &Path,
    vendor_package_roots: &[(PathBuf, crate::ClassCompletionOrigin, String)],
) -> crate::ClassCompletionOrigin {
    if !path.starts_with(vendor_path) {
        return crate::ClassCompletionOrigin::Project;
    }
    for (root, origin, _pkg_name) in vendor_package_roots {
        if path.starts_with(root) {
            return *origin;
        }
    }
    crate::ClassCompletionOrigin::VendorTransitive
}
