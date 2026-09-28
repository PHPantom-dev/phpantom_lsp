//! The many-to-many relationship sources behind the reverse pivot index.

use tower_lsp::lsp_types::Url;

use crate::Backend;
use crate::virtual_members::laravel::source_may_declare_pivot_relationship;

impl Backend {
    /// Parse every not-yet-parsed project class file that may declare a
    /// many-to-many relationship, so the reverse pivot index sees it.
    ///
    /// The index is rebuilt from the parsed classes only, and nothing forces
    /// the file declaring a relationship to be parsed: looking up the
    /// *target* model never touches it.  Without this a single-file
    /// `analyze` run, or an editor session before the workspace is indexed,
    /// reports `$pivot` as unknown on every target model.
    ///
    /// Candidates are the non-vendor files of the FQN index, read once and
    /// byte-prefiltered before parsing.  Vendor files are left out: they are
    /// the bulk of the classmap and rarely declare a relationship onto a
    /// model a project reaches.  Later edits reach the index through
    /// `update_ast`, which parses the file anyway.
    pub(crate) fn load_laravel_pivot_sources(&self) {
        let candidates: Vec<String> = {
            let idx = self.symbols.fqn_uri_index.read();
            let parsed = self.parsed_uris.read();
            let mut uris: Vec<String> = idx
                .iter()
                .map(|(_, uri)| uri)
                .filter(|uri| !uri.contains("/vendor/") && !parsed.contains(uri.as_str()))
                .cloned()
                .collect();
            uris.sort_unstable();
            uris.dedup();
            uris
        };

        let mut loaded = 0usize;
        for uri in &candidates {
            let Some(path) = Url::parse(uri).ok().and_then(|u| u.to_file_path().ok()) else {
                continue;
            };
            let Ok(content) = std::fs::read(&path) else {
                continue;
            };
            if !source_may_declare_pivot_relationship(&content) {
                continue;
            }
            if self.parse_and_cache_file(&path).is_some() {
                loaded += 1;
            }
        }

        if loaded > 0 {
            self.laravel_pivots_dirty
                .store(true, std::sync::atomic::Ordering::Relaxed);
            // A pivot accessor lands on the target model, whose cached
            // resolutions record no dependency on the relation's file.
            self.clear_resolved_member_files();
        }

        tracing::info!(
            "PHPantom: scanned {} unparsed project files for many-to-many relationships, parsed {}",
            candidates.len(),
            loaded,
        );
    }
}
