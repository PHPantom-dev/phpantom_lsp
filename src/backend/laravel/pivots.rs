//! The reverse pivot index: the many-to-many relationship sources behind
//! it, and keeping it current as they change.

use std::sync::Arc;

use crate::Backend;
use crate::class_lookup::Fqn;
use crate::types::ClassInfo;
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
            let Some(path) = crate::util::uri_to_path(uri) else {
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
            self.laravel.pivots.mark_dirty();
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

    /// Mark the reverse pivot index stale when `content` (at `uri`) affects
    /// many-to-many relationships — either it now looks like it declares
    /// one, or it previously contributed one (an edit may have
    /// removed the last such relation). A no-op for unrelated edits.
    pub(crate) fn refresh_laravel_pivots(&self, uri: &str, content: &str) {
        if self.laravel.pivots.is_dirty() {
            return;
        }
        let has_m2m = source_may_declare_pivot_relationship(content.as_bytes());
        if has_m2m || self.laravel.pivots.read().contributes(uri) {
            self.laravel.pivots.mark_dirty();
            // A pivot accessor is attached to the *target* model, which the
            // file declaring the relation never has to be looked up for, so a
            // cached receiver resolution records no dependency on this index.
            self.clear_resolved_member_files();
        }
    }

    /// Rebuild the reverse pivot index from every parsed class.
    ///
    /// Collects an `(uri, class)` snapshot, releases the index lock, then
    /// resolves related/pivot class names via the normal loader. A no-op-shaped
    /// empty index is stored for non-Laravel projects so the gate stays off.
    pub(crate) fn rebuild_laravel_pivot_index(&self) {
        let index = if self.resolved_class_cache.read().laravel().is_enabled() {
            let classes: Vec<(String, Arc<ClassInfo>)> = {
                let idx = self.symbols.uri_classes_index.read();
                idx.iter()
                    .flat_map(|(uri, list)| list.iter().map(move |c| (uri.clone(), Arc::clone(c))))
                    .collect()
            };
            let loader = |name: Fqn<'_>| self.find_or_load_class(&name);
            crate::virtual_members::laravel::build_pivot_index(&classes, &loader)
        } else {
            crate::virtual_members::laravel::LaravelPivotIndex::default()
        };
        let non_empty = !index.is_empty();
        self.laravel.pivots.replace(index, non_empty);
    }
}
