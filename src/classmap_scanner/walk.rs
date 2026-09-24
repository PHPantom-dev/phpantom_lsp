//! `.gitignore`-aware walks of the workspace for the passes that read
//! project files by path rather than through the symbol index: the full
//! workspace index, namespace rename, and the Laravel enumerations that
//! find `config/` and `lang/` files on disk.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{FollowedLinks, IndexFilters, LinkClaims, workspace_walk_builder};

/// Recursively collect all `.php` files under a workspace root,
/// respecting `.gitignore` rules (including nested and global
/// gitignore files).
///
/// Unlike the PSR-4 walkers, this uses the `ignore` crate's
/// [`ignore::WalkBuilder`] so that generated/cached directories listed in
/// `.gitignore` (e.g. `storage/framework/views/`, `var/cache/`,
/// `node_modules/`) are automatically skipped.
///
/// All known vendor directories are always skipped regardless of
/// `.gitignore` content, since some projects commit their vendor
/// directory.  `vendor_dir_paths` contains absolute paths of all known
/// vendor directories (one per subproject in monorepo mode).
///
/// Hidden files and directories are skipped by default (handled by the
/// `ignore` crate).
pub(crate) fn collect_php_files_gitignore(
    root: &Path,
    vendor_dir_paths: &[PathBuf],
    filters: &Arc<IndexFilters>,
    followed: Option<&FollowedLinks>,
) -> Vec<PathBuf> {
    let mut result = Vec::new();
    visit_workspace_files_gitignore(root, vendor_dir_paths, filters, followed, |path| {
        if filters.is_php_file(path) {
            result.push(path.to_path_buf());
        }
    });
    result
}

/// Collect the PHP and schema-free YAML/XML inputs used by the full workspace
/// index in one `.gitignore`-aware walk.
pub(crate) fn collect_workspace_index_files_gitignore(
    root: &Path,
    vendor_dir_paths: &[PathBuf],
    filters: &Arc<IndexFilters>,
    followed: Option<&FollowedLinks>,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut php_files = Vec::new();
    let mut resource_files = Vec::new();
    visit_workspace_files_gitignore(root, vendor_dir_paths, filters, followed, |path| {
        if filters.is_php_file(path) {
            php_files.push(path.to_path_buf());
        } else if crate::resource_navigation::is_resource_path(path) {
            resource_files.push(path.to_path_buf());
        }
    });
    (php_files, resource_files)
}

fn visit_workspace_files_gitignore(
    root: &Path,
    vendor_dir_paths: &[PathBuf],
    filters: &Arc<IndexFilters>,
    followed: Option<&FollowedLinks>,
    mut visit: impl FnMut(&Path),
) {
    let walker = workspace_walk_builder(
        root,
        Arc::new(vendor_dir_paths.to_vec()),
        Arc::clone(filters),
        false,
        LinkClaims::new([root.to_path_buf()], followed),
    )
    .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if path.is_file() {
            visit(path);
        }
    }
}
