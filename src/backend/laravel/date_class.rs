//! The date class the project's providers select with `Date::use()`, which
//! is what `now()` and `today()` return.

use crate::Backend;

impl Backend {
    /// Find the date class selected by project service providers. Laravel's
    /// helpers use this factory, so `now()` and `today()` return this class
    /// rather than their broad `CarbonInterface` declaration.
    ///
    /// Runs in both the LSP `initialized` handler and the headless `analyze`
    /// pipeline so every consumer resolves the date helpers to a concrete
    /// class. Until this has run, `laravel_date_class` stays `None` and the
    /// helpers resolve to nothing rather than a stale default.
    pub(crate) fn build_laravel_date_class(&self) {
        let mut configured = None;
        // Track every file this scan reads so the single-file refresh can tell
        // whether an edit could change the configured class.  The app's
        // provider-registration files are seeds too: editing them changes which
        // providers are registered, so a `Date::use()` in a newly added (or
        // removed) provider is picked up on the next scan.
        let mut seed_uris: std::collections::HashSet<String> = std::collections::HashSet::new();
        if let Some(root) = self.workspace.workspace_root.read().clone() {
            for rel in ["bootstrap/providers.php", "config/app.php"] {
                seed_uris.insert(crate::util::path_to_uri(&root.join(rel)));
            }
        }
        let providers = self.laravel_provider_fqns();
        for fqn in providers {
            let Some(uri) = self.resolve_class_uri(&fqn) else {
                continue;
            };
            let Ok(url) = tower_lsp::lsp_types::Url::parse(&uri) else {
                continue;
            };
            let Ok(path) = url.to_file_path() else {
                continue;
            };
            if self.is_in_vendor_dir(&path) {
                continue;
            }
            seed_uris.insert(uri.clone());
            let Some(content) = self.get_file_content(&uri) else {
                continue;
            };
            if let Some(class) =
                crate::virtual_members::laravel::extract_date_factory_class(&content)
            {
                configured = Some(class);
            }
        }
        *self.laravel_date_seed_uris.write() = seed_uris;
        let previous = self.laravel_date_class.write().replace(configured.clone());
        // `now()` reaches the configured class through this slot rather than
        // through a lookup of the provider that set it, so a cached receiver
        // resolution records no dependency on it.
        if previous != Some(configured) {
            self.clear_resolved_member_files();
        }
    }
}
