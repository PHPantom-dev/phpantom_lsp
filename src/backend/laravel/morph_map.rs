//! The Eloquent morph-map index: `Relation::morphMap()` and
//! `Relation::enforceMorphMap()` registrations in the service providers.

use crate::Backend;
use crate::virtual_members::laravel::file_contributions::refresh_file;
use crate::virtual_members::laravel::{MorphMapScan, scan_morph_map};

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
    pub(crate) fn build_laravel_morph_map_index(&self, providers: &super::LaravelProviders) {
        let mut index = crate::virtual_members::laravel::LaravelMorphMapIndex::default();
        let scanned = self.scan_providers_into(providers, &mut index.files, |content| {
            self.scan_morph_map(content)
        });

        index.rebuild();
        let alias_count = index.all_aliases().len();
        *self.laravel_morph_map.write() = index;

        tracing::info!(
            "PHPantom: scanned {} Laravel provider files, indexed {} morph aliases",
            scanned,
            alias_count,
        );
    }

    /// One file's morph-map registrations, with every
    /// `Relation::morphMap([Post::class, …])` list entry turned into an
    /// `alias => model` entry by resolving the model's table name, which is
    /// the alias Laravel derives for it.
    ///
    /// A model whose table cannot be determined statically (it overrides
    /// `getTable()`) is dropped rather than guessed, so no wrong alias enters
    /// the index.
    fn scan_morph_map(&self, content: &str) -> MorphMapScan {
        let mut scan = scan_morph_map(content);
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
        scan
    }

    /// Re-scan a single file's morph-map registrations after an edit.
    ///
    /// A cheap no-op unless the file currently contributes registrations or its
    /// new content contains a `morphMap(` call.  Only runs for Laravel projects.
    pub(crate) fn refresh_laravel_morph_map(&self, uri: &str, content: &str) {
        if !self.resolved_class_cache.read().is_laravel() {
            return;
        }
        let has_token = memchr::memmem::find(content.as_bytes(), b"orphMap(").is_some();
        refresh_file(&self.laravel_morph_map, uri, has_token, || {
            self.scan_morph_map(content)
        });
    }
}
