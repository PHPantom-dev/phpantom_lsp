//! The stub paths: the `[stubs]` entries of `.phpantom.toml`, the stub
//! packages Composer installed, and the stub directory the editor
//! extension forwards.
//!
//! What a stub path declares wins over every other declaration of the same
//! name, PHP's embedded stubs included, which is what sets it apart from an
//! `[indexing] include` path (that only fills gaps). The paths are scanned
//! into an [`ExternalStubIndex`] the class, function and constant lookups
//! consult before anything else, and registered as library code so that a
//! stub file is never analysed, renamed into, or searched for references,
//! whether or not it sits inside the workspace.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::path_aliases;
use crate::Backend;
use crate::ci_map::CiMap;
use crate::classmap_scanner::{self, WorkspaceScanResult};
use crate::config;
use crate::workspace_env::LibraryClaimant;

/// Where each name the stub paths declare is declared, in priority order:
/// the `[stubs]` paths in the order written, then the Composer stub
/// packages, then the editor's directory.
#[derive(Default)]
pub(crate) struct ExternalStubIndex {
    /// The stub paths the index was built from, as resolved.
    roots: Vec<PathBuf>,
    /// Every spelling of each root, for matching a file under one.
    trees: Vec<PathBuf>,
    /// Each tree's own `file://` URI, which a root that is a single file
    /// matches exactly, and its URI as a directory prefix.
    uri_prefixes: Vec<(String, String)>,
    /// The URI prefixes of the roots that are phpstorm-stubs checkouts.
    phpstorm_stubs_prefixes: Vec<String>,
    pub(crate) classes: CiMap<PathBuf>,
    pub(crate) functions: CiMap<PathBuf>,
    /// Keyed by name as declared: constants are case-sensitive.
    pub(crate) constants: HashMap<String, PathBuf>,
}

impl ExternalStubIndex {
    fn new(roots: Vec<PathBuf>, scan: &WorkspaceScanResult) -> Self {
        let trees: Vec<PathBuf> = roots.iter().flat_map(|root| path_aliases(root)).collect();
        let uri_prefixes = trees
            .iter()
            .map(|tree| {
                (
                    crate::util::path_to_uri(tree),
                    crate::util::dir_uri_prefix(tree),
                )
            })
            .collect();
        let phpstorm_stubs_prefixes = roots
            .iter()
            .filter(|root| classmap_scanner::is_phpstorm_stubs_checkout(root))
            .flat_map(|root| path_aliases(root))
            .map(|tree| crate::util::dir_uri_prefix(&tree))
            .collect();
        let mut classes = CiMap::new();
        for (name, path) in &scan.classmap {
            classes.insert(name.as_str(), path.clone());
        }
        let mut functions = CiMap::new();
        for (name, path) in &scan.function_index {
            functions.insert(name.as_str(), path.clone());
        }
        Self {
            roots,
            trees,
            uri_prefixes,
            phpstorm_stubs_prefixes,
            classes,
            functions,
            constants: scan.constant_index.clone(),
        }
    }

    /// Whether no stub path is in force.
    pub(crate) fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// Whether `uri` names a file under one of the stub paths.
    pub(crate) fn contains_uri(&self, uri: &str) -> bool {
        self.uri_prefixes
            .iter()
            .any(|(exact, prefix)| uri == exact || uri.starts_with(prefix.as_str()))
    }

    /// Whether `uri` names a file of a stub path that is a phpstorm-stubs
    /// checkout, which has the same gaps as the embedded copy and gets the
    /// same corrections.
    pub(crate) fn is_phpstorm_stubs_uri(&self, uri: &str) -> bool {
        self.phpstorm_stubs_prefixes
            .iter()
            .any(|prefix| uri.starts_with(prefix.as_str()))
    }

    /// Whether `path` is under one of the stub paths.
    pub(crate) fn contains_path(&self, path: &Path) -> bool {
        self.trees.iter().any(|tree| path.starts_with(tree))
    }
}

impl Backend {
    /// The stub index in force, shared rather than locked, so a caller can
    /// hold it across other locks.
    pub(crate) fn external_stubs(&self) -> Arc<ExternalStubIndex> {
        Arc::clone(&self.external_stubs.read())
    }

    /// Whether any stub path is in force, without taking the index lock.
    pub(crate) fn has_external_stubs(&self) -> bool {
        self.has_external_stubs.load(Ordering::Acquire)
    }

    /// Whether `uri` names a file under one of the stub paths.
    pub(crate) fn is_external_stub_uri(&self, uri: &str) -> bool {
        self.has_external_stubs() && self.external_stubs.read().contains_uri(uri)
    }

    /// Whether `uri` names phpstorm-stubs source, embedded or a stub path
    /// holding a checkout, whose declarations [`crate::stub_patches`]
    /// corrects.
    pub(crate) fn is_phpstorm_stubs_uri(&self, uri: &str) -> bool {
        crate::stubs::is_embedded_stub_uri(uri)
            || (self.has_external_stubs() && self.external_stubs.read().is_phpstorm_stubs_uri(uri))
    }

    /// The stub paths in priority order: the `[stubs]` paths that exist,
    /// then the stub packages each Composer project installed, then the
    /// editor's stub directory if it exists. A path already listed is not
    /// listed again.
    pub(crate) fn stub_paths(&self) -> Vec<PathBuf> {
        let mut paths = match self.workspace.workspace_root.read().as_deref() {
            Some(root) => self.config().stubs.resolved_paths(root),
            None => Vec::new(),
        };
        for project in self.workspace.composer_projects.read().iter() {
            for package in &project.stub_packages {
                if !paths.contains(package) {
                    paths.push(package.clone());
                }
            }
        }
        if let Some(path) = self.workspace.client_stubs.read().path.as_ref()
            && path.exists()
            && !paths.contains(path)
        {
            paths.push(path.clone());
        }
        paths
    }

    /// Replace the stub directory the editor forwarded. Returns whether
    /// it changed.
    pub fn set_client_stub_options(&self, options: config::ClientStubOptions) -> bool {
        let mut current = self.workspace.client_stubs.write();
        if *current == options {
            return false;
        }
        *current = options;
        true
    }

    /// Rebuild the stub index from the stub paths now in force.
    ///
    /// Everything parsed from a file under the old or the new paths is
    /// dropped first. A file under a removed path must stop overriding the
    /// declaration the user went back to, and one under an added path was
    /// parsed as ordinary code, so both are parsed again in their new role
    /// the next time a lookup needs them. A no-op when there were no stub
    /// paths and still are none, which is the default.
    pub(crate) fn refresh_stub_paths(&self) {
        let roots = self.stub_paths();
        let previous = self.external_stubs();
        if roots.is_empty() && previous.is_empty() {
            return;
        }

        let filters = self.index_filters();
        let scan = classmap_scanner::scan_stub_paths(&roots, &filters, self.php_version());
        let index = ExternalStubIndex::new(roots, &scan);
        let symbol_count = index.classes.len() + index.functions.len() + index.constants.len();

        let trees: Vec<PathBuf> = previous.trees.iter().chain(&index.trees).cloned().collect();
        let parsed = self.indexed_files_under(&trees);
        self.evict_indexed_files(parsed);

        // The scanned names go into the discovery indexes too, so stub
        // symbols are offered by completion and workspace symbol search
        // before anything has loaded them. Merged rather than replacing:
        // where project code declares the same name the lookups above
        // still pick the stub, and the project's entry is what is left
        // once the stub path goes.
        self.symbols
            .replace_tree(&trees, WorkspaceScanResult::default());
        self.symbols.merge_scan(scan);

        self.release_library_claimant(&LibraryClaimant::Stubs);
        for root in &index.roots {
            self.add_library_path(LibraryClaimant::Stubs, root);
        }

        tracing::info!(
            "PHPantom: stubs, {} symbols from {} path(s)",
            symbol_count,
            index.roots.len()
        );
        let active = !index.is_empty();
        *self.external_stubs.write() = Arc::new(index);
        self.has_external_stubs.store(active, Ordering::Release);

        self.clear_not_found_caches();
        self.clear_resolved_class_cache();
        self.member_completion_cache.lock().clear();
    }
}

#[cfg(test)]
#[path = "stub_paths_tests.rs"]
mod tests;
