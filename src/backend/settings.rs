//! The project configuration, the indexing filters compiled from it, and
//! the PHP version the stubs are filtered for.

use std::collections::HashMap;
use std::sync::Arc;

use crate::{Backend, classmap_scanner, config, stubs, types};

impl Backend {
    /// Return the current project configuration.
    ///
    /// Returns a clone of the [`Config`](config::Config) loaded from
    /// `.phpantom.toml` (or the default config when the file is missing).
    pub fn config(&self) -> config::Config {
        self.workspace.config.lock().clone()
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

        // The overwhelmingly common case is one layer or the other being
        // empty, so only pay for a merged allocation when both contribute.
        let compiled = if client.is_empty() {
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
            let extensions = [client.extensions.as_slice(), indexing.extensions()].concat();
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

    /// Return the configured PHP version.
    pub fn php_version(&self) -> types::PhpVersion {
        *self.workspace.php_version.lock()
    }

    /// Set the PHP version (used by integration tests and during
    /// server initialization after reading `composer.json`).
    ///
    /// Also filters `stub_function_index`, `stub_index`, and
    /// `stub_constant_index` to remove entries that do not exist in
    /// the given PHP version.
    pub fn set_php_version(&self, version: types::PhpVersion) {
        *self.workspace.php_version.lock() = version;
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
