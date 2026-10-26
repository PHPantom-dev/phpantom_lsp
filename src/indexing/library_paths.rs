//! Paths indexed on top of what the discovery pipelines find: the
//! `[indexing] include` entries.
//!
//! A path inside the workspace is project code. One outside it is a
//! library, registered the way a vendor directory is, so it resolves but
//! is never reported on, renamed into, or searched for references.

use std::path::{Path, PathBuf};

use super::path_aliases;
use crate::Backend;
use crate::classmap_scanner;
use crate::workspace_env::LibraryClaimant;

impl Backend {
    /// Index the extra paths named by `[indexing] include`.
    ///
    /// Runs last in every discovery pipeline so the entries it adds lose
    /// every first-wins merge: an included path fills the gaps the
    /// workspace scan left, it does not shadow what that scan found. A
    /// no-op when the option is unset, which is the default.
    pub(super) fn scan_configured_include_paths(
        &self,
        root: &Path,
        progress: Option<&crate::progress::ScanProgress>,
    ) {
        let paths = self.config().indexing.include_paths(root);
        if paths.is_empty() {
            return;
        }

        let (project, library): (Vec<PathBuf>, Vec<PathBuf>) = paths
            .iter()
            .cloned()
            .partition(|path| path.starts_with(root));
        for path in &library {
            self.add_library_path(LibraryClaimant::Include, path);
        }

        let filters = self.index_filters();
        let mut symbol_count = 0;
        for (paths, origin) in [
            (project, crate::ClassCompletionOrigin::Project),
            (library, crate::ClassCompletionOrigin::VendorExplicit),
        ] {
            if paths.is_empty() {
                continue;
            }
            let scan = classmap_scanner::scan_include_paths(&paths, origin, &filters, progress);
            symbol_count +=
                scan.classmap.len() + scan.function_index.len() + scan.constant_index.len();
            self.symbols.merge_scan(scan);
        }

        tracing::info!(
            "PHPantom: indexing.include, {} symbols from {} configured path(s)",
            symbol_count,
            paths.len()
        );
    }

    /// Register a path as library code on behalf of `claimant`. A
    /// directory is registered exactly like a vendor directory; a file
    /// gets its own URI as the prefix, since the vendor prefixes end in
    /// `/`.
    pub(super) fn add_library_path(&self, claimant: LibraryClaimant, path: &Path) {
        if !path.is_file() {
            self.add_library_dir(claimant, path);
            return;
        }
        let aliases = path_aliases(path);
        let uri_prefixes = aliases
            .iter()
            .map(|alias| crate::util::path_to_uri(alias))
            .collect();
        self.register_vendor_location(claimant, aliases, uri_prefixes);
    }
}
