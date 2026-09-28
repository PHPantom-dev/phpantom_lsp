//! The authorization gate index: `Gate::define()`, `Gate::policy()`, and
//! `$policies` registrations in the service providers.

use crate::Backend;
use crate::virtual_members::laravel::file_contributions::refresh_file;
use crate::virtual_members::laravel::scan_gate_registrations;

impl Backend {
    /// Build the authorization gate index by scanning the project's
    /// registered service providers for `Gate::define()` / `Gate::policy()`
    /// calls and `$policies` arrays.
    ///
    /// Uses the same provider set as the macro and morph-map scans (vendor
    /// packages' auto-discovered providers plus those the app lists in
    /// `bootstrap/providers.php` / `config/app.php`), since abilities and the
    /// policy map are registered from a provider's `boot()`.  Files are
    /// byte-prefiltered inside
    /// [`scan_gate_registrations`](crate::virtual_members::laravel::scan_gate_registrations)
    /// so only candidates are parsed.
    pub(crate) fn build_laravel_gate_index(&self, providers: &super::LaravelProviders) {
        let mut index = crate::virtual_members::laravel::LaravelGateIndex::default();
        // Read from `composer.json` during init and not recoverable from the
        // provider scan below, so it has to survive the fresh index.
        index
            .set_runtime_permission_package(self.laravel_gates.read().runtime_permission_package());
        let scanned =
            self.scan_providers_into(providers, &mut index.files, scan_gate_registrations);

        index.rebuild();
        let ability_count = index.definition_names().len();
        *self.laravel_gates.write() = index;
        self.laravel_string_key_cache.write().gate_abilities = None;

        tracing::info!(
            "PHPantom: scanned {} Laravel provider files, indexed {} gate abilities",
            scanned,
            ability_count,
        );
    }

    /// Re-scan a single file's gate registrations after an edit.
    ///
    /// A cheap no-op unless the file currently contributes registrations or
    /// its new content mentions `Gate` or a `$policies` property.  Only runs
    /// for Laravel projects.
    pub(crate) fn refresh_laravel_gates(&self, uri: &str, content: &str) {
        if !self.resolved_class_cache.read().is_laravel() {
            return;
        }
        let bytes = content.as_bytes();
        let has_token = memchr::memmem::find(bytes, b"Gate").is_some()
            || memchr::memmem::find(bytes, b"$policies").is_some();
        refresh_file(&self.laravel_gates, uri, has_token, || {
            scan_gate_registrations(content)
        });
    }
}
