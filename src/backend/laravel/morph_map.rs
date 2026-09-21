//! The Eloquent morph-map index: `Relation::morphMap()` and
//! `Relation::enforceMorphMap()` registrations in the service providers.

use crate::Backend;
use crate::virtual_members::laravel::file_contributions::Contribution;

impl Backend {
    /// Build the Eloquent morph-map index by scanning the project's registered
    /// service providers for `Relation::morphMap()` /
    /// `Relation::enforceMorphMap()` calls.
    ///
    /// Uses the same provider set as the macro scan (vendor packages'
    /// auto-discovered providers plus those the app lists in
    /// `bootstrap/providers.php` / `config/app.php`), since a morph map is
    /// registered from a provider's `boot()`.  Files are byte-prefiltered for
    /// the `orphMap(` token so only candidates are parsed.
    pub(crate) fn build_laravel_morph_map_index(&self) {
        let mut index = crate::virtual_members::laravel::LaravelMorphMapIndex::default();
        let mut scanned = 0usize;

        for fqn in self.laravel_provider_fqns() {
            let Some(uri) = self.resolve_class_uri(&fqn) else {
                continue;
            };
            if index.files.has_uri(&uri) {
                continue;
            }
            let Some(content) = self.get_file_content(&uri) else {
                continue;
            };
            scanned += 1;
            let mut scan = crate::virtual_members::laravel::scan_morph_map(&content);
            if scan.is_empty() {
                continue;
            }
            self.resolve_morph_map_table_aliases(&mut scan);
            index.files.set_file(uri, scan);
        }

        index.rebuild();
        let alias_count = index.all_aliases().len();
        *self.laravel_morph_map.write() = index;

        tracing::info!(
            "PHPantom: scanned {} Laravel provider files, indexed {} morph aliases",
            scanned,
            alias_count,
        );
    }

    /// Turn a `Relation::morphMap([Post::class, …])` list registration into
    /// `alias => model` entries by resolving each model's table name, which is
    /// the alias Laravel derives for it.
    ///
    /// A model whose table cannot be determined statically (it overrides
    /// `getTable()`) is dropped rather than guessed, so no wrong alias enters
    /// the index.
    fn resolve_morph_map_table_aliases(
        &self,
        scan: &mut crate::virtual_members::laravel::MorphMapScan,
    ) {
        for target in std::mem::take(&mut scan.table_keyed) {
            let Some(class) = self.find_or_load_class(&target.target_fqn) else {
                continue;
            };
            let Some(table) = crate::virtual_members::laravel::model_table_name(&class) else {
                continue;
            };
            scan.entries
                .push(crate::virtual_members::laravel::MorphMapEntry {
                    alias: table,
                    target_fqn: target.target_fqn,
                    alias_offset: target.offset,
                });
        }
    }

    /// Re-scan a single file's morph-map registrations after an edit.
    ///
    /// A cheap no-op unless the file currently contributes registrations or its
    /// new content contains a `morphMap(` call.  Only runs for Laravel projects.
    pub(crate) fn refresh_laravel_morph_map(&self, uri: &str, content: &str) {
        if !self.resolved_class_cache.read().is_laravel() {
            return;
        }
        let was_contributor = self.laravel_morph_map.read().files.has_uri(uri);
        let has_token = memchr::memmem::find(content.as_bytes(), b"orphMap(").is_some();
        if !was_contributor && !has_token {
            return;
        }

        let mut scan = crate::virtual_members::laravel::scan_morph_map(content);
        self.resolve_morph_map_table_aliases(&mut scan);

        let mut index = self.laravel_morph_map.write();
        index.files.set_file(uri.to_string(), scan);
        index.rebuild();
    }
}
