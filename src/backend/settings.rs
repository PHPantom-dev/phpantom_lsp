//! The project configuration, the indexing filters compiled from it, and
//! the PHP version the stubs are filtered for.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::{Backend, classmap_scanner, config, stubs, types};

impl Backend {
    /// Return the current project configuration.
    ///
    /// Returns a clone of the [`Config`](config::Config) loaded from
    /// `.phpantom.toml` (or the default config when the file is missing).
    pub fn config(&self) -> config::Config {
        self.workspace.config.lock().clone()
    }

    /// Whether the editor is shown any diagnostics at all; see
    /// [`DiagnosticsConfig::enabled`](config::DiagnosticsConfig::enabled).
    /// Reads the one flag without cloning the whole config.
    pub(crate) fn diagnostics_enabled(&self) -> bool {
        self.workspace.config.lock().diagnostics.enabled()
    }

    /// Whether a type mismatch that only a `null` or `false` causes is
    /// reported; see
    /// [`DiagnosticsConfig::possibly_empty_mismatches_enabled`](config::DiagnosticsConfig::possibly_empty_mismatches_enabled).
    /// Reads the one flag without cloning the whole config.
    pub(crate) fn possibly_empty_mismatches_enabled(&self) -> bool {
        self.workspace
            .config
            .lock()
            .diagnostics
            .possibly_empty_mismatches_enabled()
    }

    /// Whether a type mismatch some members of the value's union fit is
    /// reported; see
    /// [`DiagnosticsConfig::partial_union_mismatches_enabled`](config::DiagnosticsConfig::partial_union_mismatches_enabled).
    /// Reads the one flag without cloning the whole config.
    pub(crate) fn partial_union_mismatches_enabled(&self) -> bool {
        self.workspace
            .config
            .lock()
            .diagnostics
            .partial_union_mismatches_enabled()
    }

    /// Whether type mismatches are checked against docblock types, not
    /// just native ones; see
    /// [`DiagnosticsConfig::docblock_type_mismatches_enabled`](config::DiagnosticsConfig::docblock_type_mismatches_enabled).
    /// Reads the one flag without cloning the whole config.
    pub(crate) fn docblock_type_mismatches_enabled(&self) -> bool {
        self.workspace
            .config
            .lock()
            .diagnostics
            .docblock_type_mismatches_enabled()
    }

    /// The directory symlinks the workspace walks have indexed through.
    ///
    /// Walks report into it as they discover links; the watcher
    /// registration and the watched-file handler read it back.
    pub(crate) fn followed_links(&self) -> &crate::classmap_scanner::FollowedLinks {
        &self.workspace.followed_links
    }

    /// Replace the current configuration.
    ///
    /// Used when (re)loading `.phpantom.toml` and by integration tests
    /// to enable opt-in diagnostics like `unresolved-member-access`
    /// without needing a `.phpantom.toml` file. Resets the compiled
    /// `[indexing]` filters so the next scan sees the new settings.
    pub fn set_config(&self, config: config::Config) {
        *self.workspace.config.lock() = config;
        *self.workspace.index_filters.write() = None;
    }

    /// Return the compiled `[indexing]` exclude globs and extra PHP
    /// extensions, building them on first use from the union of the
    /// `.phpantom.toml` layer and the client-supplied layer, with the
    /// `[indexing] include` paths exempted from both.
    ///
    /// The two layers are unioned rather than one overriding the other:
    /// both name files that are not worth indexing, and a client cannot
    /// know what a project's config file already excludes. The compiled
    /// result is cached until [`set_config`](Self::set_config) or
    /// [`set_client_indexing_options`](Self::set_client_indexing_options)
    /// invalidates it, so glob compilation never runs on a per-file path.
    pub(crate) fn index_filters(&self) -> Arc<classmap_scanner::IndexFilters> {
        if let Some(filters) = self.workspace.index_filters.read().as_ref() {
            return Arc::clone(filters);
        }
        let root = self.workspace.workspace_root.read().clone();
        let indexing = self.config().indexing;
        let client = self.workspace.client_indexing.read();
        let is_drupal = self.workspace.is_drupal.load(Ordering::Acquire);

        // The overwhelmingly common case is one layer or the other being
        // empty, so only pay for a merged allocation when both contribute.
        let compiled = if client.is_empty() && !is_drupal {
            classmap_scanner::IndexFilters::compile(
                root.as_deref(),
                indexing.exclude(),
                indexing.extensions(),
            )
        } else {
            // The config file's patterns go last so a `!` re-include in
            // `.phpantom.toml` can still override an exclude the editor
            // forwarded: gitignore semantics give the last match priority.
            let exclude = [client.exclude.as_slice(), indexing.exclude()].concat();
            let mut extensions = [client.extensions.as_slice(), indexing.extensions()].concat();
            if is_drupal {
                extensions.extend(
                    classmap_scanner::DRUPAL_PHP_EXTENSIONS
                        .iter()
                        .map(|ext| (*ext).to_string()),
                );
            }
            classmap_scanner::IndexFilters::compile(root.as_deref(), &exclude, &extensions)
        };
        drop(client);
        let compiled = match root.as_deref() {
            Some(root) => compiled.with_included(indexing.include_paths(root)),
            None => compiled,
        };

        let compiled = Arc::new(compiled);
        *self.workspace.index_filters.write() = Some(Arc::clone(&compiled));
        compiled
    }

    /// Record that a Composer project of the workspace is a Drupal site,
    /// so every workspace walk reads Drupal's `.module`, `.install`, …
    /// files as PHP the way the web-root class scan does, and Drupal's own
    /// declarations, such as its services, are read (see [`crate::drupal`]).
    ///
    /// Only ever turned on: a workspace whose subprojects include one
    /// Drupal site treats those extensions as PHP throughout.
    pub(crate) fn mark_drupal_project(&self) {
        // Before the flag, so no provider finds the fields unreadable.
        self.drupal.bind_fields(&self.state);
        if !self.workspace.is_drupal.swap(true, Ordering::AcqRel) {
            *self.workspace.index_filters.write() = None;
        }
    }

    /// Replace the client-supplied file filters.
    ///
    /// Called from the `initialize` handshake and again whenever a
    /// `workspace/didChangeConfiguration` arrives. Returns whether the
    /// filters actually changed, so the caller can skip the rescan and
    /// watcher churn a no-op settings push would otherwise cause (clients
    /// re-send their whole settings tree for edits to unrelated keys).
    pub fn set_client_indexing_options(&self, options: config::ClientIndexingOptions) -> bool {
        let mut current = self.workspace.client_indexing.write();
        if *current == options {
            return false;
        }
        *current = options;
        drop(current);
        *self.workspace.index_filters.write() = None;
        true
    }

    /// The inlay hint kinds in force, from `[inlay-hints]` and the
    /// editor's toggles together. Reads the one section without cloning
    /// the whole config.
    pub(crate) fn inlay_hint_kinds(&self) -> config::InlayHintKinds {
        let client = *self.workspace.client_inlay_hints.read();
        config::InlayHintKinds::resolve(&self.workspace.config.lock().inlay_hints, &client)
    }

    /// Replace the editor's inlay hint toggles. Returns whether the kinds
    /// in force changed, so a settings push that leaves them as they were
    /// does not make the editor re-pull every hint.
    pub fn set_client_inlay_hint_options(&self, options: config::ClientInlayHintOptions) -> bool {
        let before = self.inlay_hint_kinds();
        *self.workspace.client_inlay_hints.write() = options;
        self.inlay_hint_kinds() != before
    }

    /// Return the configured PHP version.
    pub fn php_version(&self) -> types::PhpVersion {
        *self.workspace.php_version.lock()
    }

    /// Set the PHP version (used by integration tests and during
    /// server initialization after reading `composer.json`).
    ///
    /// Also filters `stub_function_index`, `stub_index`, and
    /// `stub_constant_index` to remove entries that do not exist in
    /// the given PHP version, and rebuilds the stub-path index, which is
    /// filtered the same way.
    pub fn set_php_version(&self, version: types::PhpVersion) {
        let previous = std::mem::replace(&mut *self.workspace.php_version.lock(), version);
        if previous != version && self.has_external_stubs() {
            self.refresh_stub_paths();
        }
        // Every symbol a stub file declares shares that file's `&'static str`,
        // so each file is scanned once for its removed declarations and the
        // filter below is a set lookup per symbol.
        let mut removed_by_file: HashMap<usize, Option<stubs::RemovedStubNames<'static>>> =
            HashMap::new();
        let mut is_removed = |source: &'static str, name: &str, kind: stubs::StubSymbolKind| {
            removed_by_file
                .entry(source.as_ptr() as usize)
                .or_insert_with(|| stubs::removed_stub_names(source, version))
                .as_ref()
                .is_some_and(|names| names.contains(kind, name))
        };
        self.stub_function_index
            .write()
            .retain(|name, source| !is_removed(source, name, stubs::StubSymbolKind::Function));
        self.stub_index
            .write()
            .retain(|name, source| !is_removed(source, name, stubs::StubSymbolKind::Class));
        self.stub_constant_index
            .write()
            .retain(|name, source| !is_removed(source, name, stubs::StubSymbolKind::Constant));
    }
}
