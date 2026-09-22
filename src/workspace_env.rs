//! Workspace-level configuration and location metadata, grouped out of
//! `Backend`.
//!
//! Unlike the other extracted groups, `Clone` is implemented by hand rather
//! than derived: the `Arc<RwLock<…>>` fields are shared by `Arc::clone`,
//! while the `parking_lot::Mutex` fields (which are rarely accessed or always
//! written) are deep-copied into a fresh `Mutex`. This exactly preserves the
//! per-field clone semantics `Backend`'s clone had when these were individual
//! fields.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use parking_lot::{Mutex, RwLock};

use crate::ClassCompletionOrigin;
use crate::composer;
use crate::config;
use crate::types::PhpVersion;

/// Workspace root, PSR-4 mappings, vendor locations, PHP version, and the
/// loaded `.phpantom.toml` configuration.
pub(crate) struct WorkspaceEnv {
    /// The root directory of the workspace (set during `initialize`).
    pub(crate) workspace_root: Arc<RwLock<Option<PathBuf>>>,
    /// PSR-4 autoload mappings parsed from `composer.json`.
    pub(crate) psr4_mappings: Arc<RwLock<Vec<composer::Psr4Mapping>>>,
    /// `file://` URI prefixes for all known vendor directories.
    pub(crate) vendor_uri_prefixes: Mutex<Vec<String>>,
    /// Absolute raw and canonical paths of all known vendor directories.
    pub(crate) vendor_dir_paths: Mutex<Vec<PathBuf>>,
    /// Canonical vendor package roots paired with completion provenance.
    pub(crate) vendor_package_origin_roots:
        Arc<RwLock<Vec<(PathBuf, ClassCompletionOrigin, String)>>>,
    /// The target PHP version used for version-aware stub filtering.
    pub(crate) php_version: Mutex<PhpVersion>,
    /// Per-project configuration loaded from `.phpantom.toml`.
    ///
    /// Shared by `Arc` (unlike `php_version` and the other plain `Mutex`
    /// fields above) because, unlike those, it is written again after
    /// startup: a config-file watcher reloads it on a cloned `Backend`
    /// (the blocking-task and background-worker clones), and that reload
    /// must be visible to every other clone, including the long-lived one
    /// that answers LSP requests.
    pub(crate) config: Arc<Mutex<config::Config>>,
    /// Where the global `.phpantom.toml` layer is read from, or `None`
    /// to load the project config on its own.
    ///
    /// Fixed for the lifetime of the session (the file's *contents* are
    /// re-read on change, its location never moves), so unlike `config`
    /// it needs no interior mutability. Test backends set it to `None`
    /// so a `.phpantom.toml` in the developer's own config directory
    /// cannot change what the suite asserts.
    pub(crate) global_config_path: Option<PathBuf>,
    /// Compiled `[indexing]` exclude globs and extra PHP extensions,
    /// built lazily from `config` and reset when the config changes.
    /// Shared across clones so a config reload invalidates everywhere.
    pub(crate) index_filters: Arc<RwLock<Option<Arc<crate::classmap_scanner::IndexFilters>>>>,
    /// File filters the editor forwarded through `initializationOptions`
    /// or `workspace/didChangeConfiguration`.
    ///
    /// A second layer beside `config`, not a replacement for it: the two
    /// are unioned when `index_filters` compiles, so reloading one never
    /// drops the other's contribution. Shared by `Arc` for the same
    /// reason `config` is, since a `didChangeConfiguration` handled on
    /// one clone has to be visible to every other.
    pub(crate) client_indexing: Arc<RwLock<config::ClientIndexingOptions>>,
    /// Counts the filter changes that asked for a workspace rediscovery.
    ///
    /// A debounced rediscovery task captures the count it was scheduled
    /// with and gives up when it no longer matches, so a burst of
    /// settings pushes (a client re-syncing, or a `.phpantom.toml` saved
    /// twice) costs one walk rather than one per notification. Shared by
    /// `Arc` so a task holding a cloned `Backend` sees the supersession.
    pub(crate) filter_rediscovery_generation: Arc<AtomicU64>,
}

impl WorkspaceEnv {
    /// The environment a real session runs in: the global config layer
    /// comes from the platform config directory.
    pub(crate) fn new() -> Self {
        Self::with_global_config(config::global_config_path())
    }

    /// An environment with no global config layer, for tests.
    pub(crate) fn new_isolated() -> Self {
        Self::with_global_config(None)
    }

    fn with_global_config(global_config_path: Option<PathBuf>) -> Self {
        Self {
            workspace_root: Arc::new(RwLock::new(None)),
            psr4_mappings: Arc::new(RwLock::new(Vec::new())),
            vendor_uri_prefixes: Mutex::new(Vec::new()),
            vendor_dir_paths: Mutex::new(Vec::new()),
            vendor_package_origin_roots: Arc::new(RwLock::new(Vec::new())),
            php_version: Mutex::new(PhpVersion::default()),
            config: Arc::new(Mutex::new(config::Config::default())),
            global_config_path,
            index_filters: Arc::new(RwLock::new(None)),
            client_indexing: Arc::new(RwLock::new(config::ClientIndexingOptions::default())),
            filter_rediscovery_generation: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl Clone for WorkspaceEnv {
    fn clone(&self) -> Self {
        Self {
            workspace_root: Arc::clone(&self.workspace_root),
            psr4_mappings: Arc::clone(&self.psr4_mappings),
            vendor_uri_prefixes: Mutex::new(self.vendor_uri_prefixes.lock().clone()),
            vendor_dir_paths: Mutex::new(self.vendor_dir_paths.lock().clone()),
            vendor_package_origin_roots: Arc::clone(&self.vendor_package_origin_roots),
            php_version: Mutex::new(*self.php_version.lock()),
            config: Arc::clone(&self.config),
            global_config_path: self.global_config_path.clone(),
            index_filters: Arc::clone(&self.index_filters),
            client_indexing: Arc::clone(&self.client_indexing),
            filter_rediscovery_generation: Arc::clone(&self.filter_rediscovery_generation),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Backend, config};

    /// A test that writes a `.phpantom.toml` into a temp workspace has to
    /// be judged against that file alone. When the test constructors
    /// carry the platform global config path, whatever sits in the
    /// developer's own config directory merges underneath it and quietly
    /// changes the result on that machine but not on a clean CI runner.
    #[test]
    fn test_constructors_carry_no_global_config() {
        let backends = [
            ("new_test", Backend::new_test()),
            (
                "new_test_with_workspace",
                Backend::new_test_with_workspace(std::path::PathBuf::from("/tmp"), Vec::new()),
            ),
            (
                "new_test_with_stubs",
                Backend::new_test_with_stubs(Default::default()),
            ),
            (
                "new_test_with_full_stubs",
                Backend::new_test_with_full_stubs(),
            ),
        ];

        for (name, backend) in backends {
            assert!(
                backend.workspace.global_config_path.is_none(),
                "Backend::{name} must not read the global .phpantom.toml"
            );
        }
    }

    /// The clones a diagnostic worker or blocking task runs on reload the
    /// config themselves, so they have to keep pointing at the same
    /// global file the original was built with.
    #[test]
    fn clone_preserves_the_global_config_path() {
        let path = std::path::PathBuf::from("/tmp/global/.phpantom.toml");
        let mut backend = Backend::new_test();
        backend.workspace.global_config_path = Some(path.clone());

        let clone = backend.clone_for_diagnostic_worker();

        assert_eq!(clone.workspace.global_config_path.as_deref(), Some(&*path));
    }

    /// A backend rooted in a temp workspace, with `[indexing]` loaded
    /// from the given `.phpantom.toml` body.
    fn backend_with_config(body: &str) -> (Backend, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let backend = Backend::new_test();
        *backend.workspace.workspace_root.write() = Some(dir.path().to_path_buf());
        if !body.is_empty() {
            std::fs::write(dir.path().join(config::CONFIG_FILE_NAME), body).unwrap();
            backend.reload_config(dir.path());
        }
        (backend, dir)
    }

    fn client_options(exclude: &[&str], extensions: &[&str]) -> config::ClientIndexingOptions {
        config::ClientIndexingOptions {
            exclude: exclude.iter().map(|s| s.to_string()).collect(),
            extensions: extensions.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// The two layers are unioned, not one replacing the other. An
    /// editor cannot know what the project's config file already
    /// excludes, so letting either side win would silently reindex a
    /// tree the other side had ruled out.
    #[test]
    fn client_and_config_filters_are_unioned() {
        let (backend, dir) = backend_with_config(
            "[indexing]\nexclude = [\"from-config\"]\nextensions = [\"inc\"]\n",
        );
        backend.set_client_indexing_options(client_options(&["from-client"], &["module"]));

        let filters = backend.index_filters();
        assert!(filters.is_excluded_entry(&dir.path().join("from-config"), true));
        assert!(filters.is_excluded_entry(&dir.path().join("from-client"), true));
        assert!(filters.is_php_file(&dir.path().join("a.inc")));
        assert!(filters.is_php_file(&dir.path().join("a.module")));
    }

    /// Gitignore semantics give the last matching pattern priority, so
    /// ordering the config file's patterns after the client's is what
    /// lets a project re-include (`!`) something its editor hides.
    #[test]
    fn a_config_re_include_overrides_a_client_exclude() {
        let (backend, dir) =
            backend_with_config("[indexing]\nexclude = [\"!generated/keep.php\"]\n");
        backend.set_client_indexing_options(client_options(&["generated/*"], &[]));

        let filters = backend.index_filters();
        assert!(filters.is_excluded_entry(&dir.path().join("generated/other.php"), false));
        assert!(
            !filters.is_excluded_entry(&dir.path().join("generated/keep.php"), false),
            "a `!` in .phpantom.toml must win over a client exclude"
        );
    }

    /// The compiled filters are cached, so a settings push mid-session
    /// has to drop them. Otherwise every scan after the push keeps using
    /// the filters the user just changed away from.
    #[test]
    fn a_client_update_recompiles_the_cached_filters() {
        let (backend, dir) = backend_with_config("");

        // Compile and cache under the empty default.
        assert!(
            !backend
                .index_filters()
                .is_excluded_entry(&dir.path().join("generated"), true)
        );

        backend.set_client_indexing_options(client_options(&["generated"], &[]));
        assert!(
            backend
                .index_filters()
                .is_excluded_entry(&dir.path().join("generated"), true)
        );

        // …and withdrawing them again restores the unfiltered walk.
        backend.set_client_indexing_options(config::ClientIndexingOptions::default());
        assert!(
            !backend
                .index_filters()
                .is_excluded_entry(&dir.path().join("generated"), true)
        );
    }

    /// Reloading `.phpantom.toml` must not drop what the client
    /// contributed, and vice versa: each layer is stored separately and
    /// only the compiled union is invalidated.
    #[test]
    fn a_config_reload_keeps_the_client_layer() {
        let (backend, dir) = backend_with_config("");
        backend.set_client_indexing_options(client_options(&["from-client"], &["module"]));

        std::fs::write(
            dir.path().join(config::CONFIG_FILE_NAME),
            "[indexing]\nexclude = [\"from-config\"]\n",
        )
        .unwrap();
        backend.reload_config(dir.path());

        let filters = backend.index_filters();
        assert!(
            filters.is_excluded_entry(&dir.path().join("from-client"), true),
            "a config reload must not discard the client's excludes"
        );
        assert!(
            filters.is_php_file(&dir.path().join("a.module")),
            "a config reload must not discard the client's extensions"
        );
        assert!(filters.is_excluded_entry(&dir.path().join("from-config"), true));
    }

    /// Clients re-push their entire settings tree for an edit to any key,
    /// so an unchanged filter set has to be recognised as a no-op rather
    /// than invalidating the compiled globs on every keystroke in the
    /// user's settings file.
    #[test]
    fn an_unchanged_client_push_reports_no_change() {
        let (backend, _dir) = backend_with_config("");
        assert!(backend.set_client_indexing_options(client_options(&["generated"], &[])));
        assert!(!backend.set_client_indexing_options(client_options(&["generated"], &[])));
        assert!(backend.set_client_indexing_options(client_options(&["generated", "build"], &[])));
    }

    /// The layer is shared by `Arc` for the same reason the config is: a
    /// `didChangeConfiguration` handled on one clone has to be visible to
    /// the long-lived clone that answers requests.
    #[test]
    fn clone_shares_the_client_filter_layer() {
        let (backend, dir) = backend_with_config("");
        let clone = backend.clone_for_diagnostic_worker();

        clone.set_client_indexing_options(client_options(&["generated"], &[]));

        assert!(
            backend
                .index_filters()
                .is_excluded_entry(&dir.path().join("generated"), true)
        );
    }
}
