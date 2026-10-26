//! The editor's other workspace folders, indexed beside the project.
//!
//! A client that holds several folders lists them all in `initialize`
//! and reports changes through `workspace/didChangeWorkspaceFolders`.
//! The folder holding the server's root is the project; every other one
//! is an extra root, walked with the same filters as the project and
//! registered as library code the way an `[indexing] include` path
//! outside the workspace is. Its classes resolve, but it gets no
//! workspace diagnostics, rename edits or reference search here: in an
//! editor that runs a server per folder, that folder's own server answers
//! for it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tower_lsp_server::gen_lsp_types::{DidChangeWorkspaceFoldersParams, WorkspaceFolder};

use super::path_aliases;
use crate::Backend;
use crate::workspace_env::LibraryClaimant;

impl Backend {
    /// The workspace folders other than the one this server is rooted
    /// in, each spelled the way the client sent it.
    ///
    /// Two folders can overlap, since an editor lets a workspace hold both
    /// a folder and its parent, and a walk of each would index the shared
    /// tree twice. So a folder inside another one, or inside the server's
    /// root, is left out, as is a duplicate of an earlier one. A folder
    /// that contains the server's root is left out too: walking it would
    /// index the project a second time as library code. A VS Code server
    /// moved below its folder by `phpantom.rootDir` is told about that
    /// folder, and has to skip it.
    pub(crate) fn extra_roots(&self) -> Vec<PathBuf> {
        let folders = self.workspace.workspace_folders.read();
        if folders.is_empty() {
            return Vec::new();
        }
        let Some(root) = self.workspace.workspace_root.read().clone() else {
            return Vec::new();
        };
        distinct_extra_roots(&root, &folders)
    }

    /// Record the workspace folders the client listed in `initialize`.
    pub(crate) fn set_workspace_folders(&self, folders: &[WorkspaceFolder]) {
        *self.workspace.workspace_folders.write() = folders
            .iter()
            .filter_map(|folder| crate::util::url_to_path(&folder.uri))
            .collect();
    }

    /// Apply a `workspace/didChangeWorkspaceFolders` notification: index
    /// a folder that was added and drop one that was removed.
    pub(crate) async fn on_did_change_workspace_folders(
        &self,
        params: DidChangeWorkspaceFoldersParams,
    ) {
        let previous_inputs = self.discovery_inputs();
        let changed = {
            let mut folders = self.workspace.workspace_folders.write();
            let before = folders.len();
            let removed: Vec<PathBuf> = params
                .event
                .removed
                .iter()
                .filter_map(|folder| crate::util::url_to_path(&folder.uri))
                .collect();
            folders.retain(|folder| !removed.contains(folder));
            let mut changed = folders.len() != before;
            for folder in &params.event.added {
                if let Some(path) = crate::util::url_to_path(&folder.uri)
                    && !folders.contains(&path)
                {
                    folders.push(path);
                    changed = true;
                }
            }
            changed
        };
        if !changed {
            return;
        }

        self.reconcile_index_for_input_change(&previous_inputs);
        self.clear_not_found_caches();
        self.request_diagnostic_refresh().await;
    }

    /// Register every extra root as library code and return them, for a
    /// discovery pipeline about to walk them.
    pub(super) fn claim_extra_roots(&self) -> Vec<PathBuf> {
        let roots = self.extra_roots();
        for root in &roots {
            self.add_library_dir(LibraryClaimant::ExtraRoot(root.clone()), root);
        }
        roots
    }

    /// Drop the extra roots that left the workspace from the index.
    ///
    /// Releases each one's library registration and evicts every file the
    /// indexes name under it, apart from the ones open in the editor.
    /// Returns how many files were evicted and whether another source
    /// (an `include` path, another root, a vendor package) still covers
    /// part of the removed trees, in which case a rediscovery walk has to
    /// give that source its files back.
    pub(super) fn retire_extra_roots(&self, removed: &[&PathBuf]) -> (usize, bool) {
        let mut spellings: Vec<PathBuf> = Vec::new();
        for root in removed {
            spellings.extend(
                self.release_library_claimant(&LibraryClaimant::ExtraRoot((*root).clone())),
            );
            spellings.extend(path_aliases(root));
        }
        spellings.sort_unstable();
        spellings.dedup();

        let under_removed = |path: &Path| spellings.iter().any(|dir| path.starts_with(dir));
        let files: HashMap<String, PathBuf> = self.indexed_files_matching(under_removed);
        let evicted = self.evict_indexed_files(files);

        let overlaps = |path: &Path| {
            spellings
                .iter()
                .any(|dir| path.starts_with(dir) || dir.starts_with(path))
        };
        let root = self.workspace.workspace_root.read().clone();
        let covered = root.as_deref().is_some_and(|root| {
            self.config()
                .indexing
                .include_paths(root)
                .iter()
                .any(|p| overlaps(p))
        }) || self.extra_roots().iter().any(|p| overlaps(p))
            || self
                .workspace
                .vendor_dir_paths
                .lock()
                .iter()
                .any(|p| overlaps(p))
            || self
                .workspace
                .vendor_package_origin_roots
                .read()
                .iter()
                .any(|(p, _, _)| overlaps(p));
        (evicted, covered)
    }
}

/// The folders of `folders` worth walking beside `root`: see
/// [`Backend::extra_roots`].
///
/// Containment is judged on canonical paths, so a folder reached through
/// a symlink still counts as the directory it names, while the returned
/// spelling stays the client's, which is how the editor names the files
/// it opens.
fn distinct_extra_roots(root: &Path, folders: &[PathBuf]) -> Vec<PathBuf> {
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let candidates: Vec<(&PathBuf, PathBuf)> = folders
        .iter()
        .filter_map(|folder| {
            let canonical = folder.canonicalize().ok()?;
            let unrelated = canonical.is_dir()
                && !canonical.starts_with(&canonical_root)
                && !canonical_root.starts_with(&canonical);
            unrelated.then_some((folder, canonical))
        })
        .collect();

    candidates
        .iter()
        .enumerate()
        .filter(|(index, (_, canonical))| {
            !candidates.iter().enumerate().any(|(other, (_, outer))| {
                other != *index
                    && canonical.starts_with(outer)
                    // Of two spellings of one directory, the first wins.
                    && (canonical != outer || other < *index)
            })
        })
        .map(|(_, (folder, _))| (*folder).clone())
        .collect()
}

#[cfg(test)]
#[path = "extra_roots_tests.rs"]
mod tests;
