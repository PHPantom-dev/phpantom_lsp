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
use crate::resolution::ProjectScopes;
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
    /// Who registered each entry of `vendor_dir_paths` and
    /// `vendor_uri_prefixes`, which are the union of these.
    ///
    /// One spelling can be registered for more than one reason (a folder
    /// of the editor's workspace that is also an `[indexing] include`
    /// path), so a source that lets go of its locations only removes the
    /// spellings no other source still claims.
    pub(crate) library_claims: Mutex<Vec<LibraryClaim>>,
    /// Canonical vendor package roots paired with completion provenance.
    pub(crate) vendor_package_origin_roots:
        Arc<RwLock<Vec<(PathBuf, ClassCompletionOrigin, String)>>>,
    /// What each indexed Composer project (the workspace root, or each
    /// subproject of a monorepo) contributed to `psr4_mappings`,
    /// `vendor_package_origin_roots`, and the stub paths.
    ///
    /// Kept per project so a `composer update` in one subproject replaces
    /// that subproject's share and leaves the others' alone.
    pub(crate) composer_projects: Arc<RwLock<Vec<ComposerProjectPaths>>>,
    /// The class-lookup scopes built from `composer_projects`, or `None`
    /// until first asked for since the projects last changed.
    pub(crate) project_scopes: Arc<RwLock<Option<Arc<ProjectScopes>>>>,
    /// The editor's workspace folders, as `initialize` and
    /// `workspace/didChangeWorkspaceFolders` report them.
    ///
    /// The one holding `workspace_root` is this server's own project;
    /// the others are indexed as library code beside it (see
    /// [`Backend::extra_roots`](crate::Backend::extra_roots)). Shared by
    /// `Arc` so the clone that walks for a newly added folder reads the
    /// list the notification handler wrote.
    pub(crate) workspace_folders: Arc<RwLock<Vec<PathBuf>>>,
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
    /// The stub directory the editor extension forwarded, beside the
    /// `[stubs]` paths of `config`. Shared by `Arc` for the same reason
    /// `client_indexing` is.
    pub(crate) client_stubs: Arc<RwLock<config::ClientStubOptions>>,
    /// Directory symlinks the workspace walks have indexed through.
    ///
    /// Shared by `Arc` because the walk that discovers a link runs on a
    /// blocking clone while the watcher registration it feeds is built on
    /// the clone that answers requests.
    pub(crate) followed_links: crate::classmap_scanner::FollowedLinks,
    /// Counts the filter changes that asked for a workspace rediscovery.
    ///
    /// A debounced rediscovery task captures the count it was scheduled
    /// with and gives up when it no longer matches, so a burst of
    /// settings pushes (a client re-syncing, or a `.phpantom.toml` saved
    /// twice) costs one walk rather than one per notification. Shared by
    /// `Arc` so a task holding a cloned `Backend` sees the supersession.
    pub(crate) filter_rediscovery_generation: Arc<AtomicU64>,
}

/// Why a location is registered as library code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LibraryClaimant {
    /// A Composer project's vendor directory.
    Composer,
    /// An `[indexing] include` path outside the workspace.
    Include,
    /// Another folder of the editor's workspace, by the spelling it was
    /// registered under.
    ExtraRoot(PathBuf),
    /// A `[stubs]` path or the editor's stub directory, inside the
    /// workspace or not: a stub is never analysed or edited.
    Stubs,
}

/// One registration of library locations: the path spellings that mark a
/// file under them and the matching `file://` URI prefixes.
#[derive(Clone)]
pub(crate) struct LibraryClaim {
    pub(crate) claimant: LibraryClaimant,
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) uri_prefixes: Vec<String>,
}

/// The share of the workspace-wide autoload lists one Composer project
/// contributed.
#[derive(Default)]
pub(crate) struct ComposerProjectPaths {
    /// The directory holding the project's `composer.json`.
    pub(crate) root: PathBuf,
    pub(crate) psr4_mappings: Vec<composer::Psr4Mapping>,
    /// Every spelling of the project's vendor directory, or empty when it
    /// has none on disk.
    pub(crate) vendor_paths: Vec<PathBuf>,
    /// The packages it installed from outside its vendor directory (see
    /// [`composer::linked_package_roots`]), which can lie in another
    /// project's tree.
    pub(crate) linked_packages: Vec<PathBuf>,
    pub(crate) package_roots: Vec<(PathBuf, ClassCompletionOrigin, String)>,
    /// The roots of the stub packages it installed, which are stub paths.
    pub(crate) stub_packages: Vec<PathBuf>,
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
            library_claims: Mutex::new(Vec::new()),
            vendor_package_origin_roots: Arc::new(RwLock::new(Vec::new())),
            composer_projects: Arc::new(RwLock::new(Vec::new())),
            project_scopes: Arc::new(RwLock::new(None)),
            workspace_folders: Arc::new(RwLock::new(Vec::new())),
            php_version: Mutex::new(PhpVersion::default()),
            config: Arc::new(Mutex::new(config::Config::default())),
            global_config_path,
            index_filters: Arc::new(RwLock::new(None)),
            client_indexing: Arc::new(RwLock::new(config::ClientIndexingOptions::default())),
            client_stubs: Arc::new(RwLock::new(config::ClientStubOptions::default())),
            followed_links: crate::classmap_scanner::FollowedLinks::default(),
            filter_rediscovery_generation: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl WorkspaceEnv {
    /// The class-lookup scope of each Composer project.
    pub(crate) fn project_scopes(&self) -> Arc<ProjectScopes> {
        if let Some(scopes) = self.project_scopes.read().as_ref() {
            return Arc::clone(scopes);
        }
        // Built under the write lock, so a build from projects read before
        // a change cannot be stored after that change's `forget`.
        let mut slot = self.project_scopes.write();
        let scopes = slot
            .get_or_insert_with(|| Arc::new(ProjectScopes::build(&self.composer_projects.read())));
        Arc::clone(scopes)
    }

    /// Rebuild the project scopes on next use, after `composer_projects`
    /// changed.  Call it with `composer_projects` unlocked: building the
    /// scopes takes that lock while holding this one.
    pub(crate) fn forget_project_scopes(&self) {
        *self.project_scopes.write() = None;
    }
}

impl Clone for WorkspaceEnv {
    fn clone(&self) -> Self {
        Self {
            workspace_root: Arc::clone(&self.workspace_root),
            psr4_mappings: Arc::clone(&self.psr4_mappings),
            vendor_uri_prefixes: Mutex::new(self.vendor_uri_prefixes.lock().clone()),
            vendor_dir_paths: Mutex::new(self.vendor_dir_paths.lock().clone()),
            library_claims: Mutex::new(self.library_claims.lock().clone()),
            vendor_package_origin_roots: Arc::clone(&self.vendor_package_origin_roots),
            composer_projects: Arc::clone(&self.composer_projects),
            project_scopes: Arc::clone(&self.project_scopes),
            workspace_folders: Arc::clone(&self.workspace_folders),
            php_version: Mutex::new(*self.php_version.lock()),
            config: Arc::clone(&self.config),
            global_config_path: self.global_config_path.clone(),
            index_filters: Arc::clone(&self.index_filters),
            client_indexing: Arc::clone(&self.client_indexing),
            client_stubs: Arc::clone(&self.client_stubs),
            followed_links: self.followed_links.clone(),
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
        backend.state_mut().workspace.global_config_path = Some(path.clone());

        let clone = backend.clone_for_blocking();

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
        let clone = backend.clone_for_blocking();

        clone.set_client_indexing_options(client_options(&["generated"], &[]));

        assert!(
            backend
                .index_filters()
                .is_excluded_entry(&dir.path().join("generated"), true)
        );
    }
}
