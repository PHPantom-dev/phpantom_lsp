//! The event index: `Event::listen()`, `$listen`, `$observers`, and
//! `Model::observe()` registrations in the service providers.

use crate::Backend;
use crate::virtual_members::laravel::events::{
    LaravelEventIndex, may_register_events, scan_event_registrations,
};
use crate::virtual_members::laravel::file_contributions::refresh_file;

impl Backend {
    /// Build the event index from the registered service providers, where
    /// listeners and observers are wired up.
    pub(crate) fn build_laravel_event_index(&self, providers: &super::LaravelProviders) {
        let mut index = LaravelEventIndex::default();
        let scanned =
            self.scan_providers_into(providers, &mut index.files, scan_event_registrations);
        index.rebuild();
        tracing::info!(
            "PHPantom: scanned {} Laravel provider files, indexed {} listener and {} observer registrations",
            scanned,
            index.listeners().len(),
            index.observers().len(),
        );
        *self.laravel.events.write() = index;
    }

    /// Re-scan a single file's listener and observer registrations after an
    /// edit.  A cheap no-op unless the file contributes registrations now or
    /// mentions `listen` or `observe`.
    pub(crate) fn refresh_laravel_events(&self, uri: &str, content: &str) {
        if !self.resolved_class_cache.read().laravel().is_enabled() {
            return;
        }
        refresh_file(
            &self.laravel.events,
            uri,
            may_register_events(content),
            || scan_event_registrations(content),
        );
    }
}
