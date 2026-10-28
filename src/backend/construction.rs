//! Building a `Backend`: the constructors for the server, the CLI and the
//! tests, and the cheap clones handed to worker threads.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};
use tower_lsp_server::Client;

use crate::backend::external_tool_worker::ExternalToolWorker;
use crate::backend::requests::WholeFileCoalesce;
use crate::ci_map::CiMap;
use crate::symbol_index::SymbolIndex;
use crate::workspace_env::WorkspaceEnv;
use crate::{
    Backend, BackendState, blade, composer, framework, reference_counts, reference_index,
    resolution, stubs, virtual_members,
};

/// The Laravel alias slot and the resolved-class cache that reads it.
///
/// The cache holds a handle to the same slot the `Backend` builds the alias
/// tables into, which is how the facade virtual member provider maps a
/// container-binding accessor to its concrete class: a provider is handed
/// the cache but never the `Backend`.
fn new_alias_slot_and_cache() -> (
    virtual_members::laravel::LaravelAliasSlot,
    virtual_members::ResolvedClassCache,
) {
    let slot = virtual_members::laravel::new_alias_slot();
    let cache = virtual_members::new_resolved_class_cache();
    cache.write().laravel_mut().set_aliases(Arc::clone(&slot));
    (slot, cache)
}

/// The three stub indices a `Backend` starts life with.
///
/// [`StubIndices::embedded`] is the PHP standard library compiled into the
/// binary; [`StubIndices::empty`] skips building it for test backends that
/// never consult it.
struct StubIndices {
    classes: CiMap<&'static str>,
    functions: CiMap<&'static str>,
    constants: HashMap<&'static str, &'static str>,
}

impl StubIndices {
    /// The full embedded standard library (1,455 classes, 5,023
    /// functions, 8,119 constants).
    fn embedded() -> Self {
        Self {
            classes: CiMap::from(stubs::build_stub_class_index()),
            functions: CiMap::from(stubs::build_stub_function_index()),
            constants: stubs::build_stub_constant_index(),
        }
    }

    /// No stubs at all, avoiding the cost of building three large
    /// `HashMap`s (14,597 entries total).
    fn empty() -> Self {
        Self {
            classes: CiMap::new(),
            functions: CiMap::new(),
            constants: HashMap::new(),
        }
    }
}

impl BackendState {
    /// Shared defaults for all Backend constructors.
    ///
    /// Returns a state with no LSP client and empty maps, reading its
    /// workspace environment and standard library from the arguments.
    /// Each public constructor customises only the fields that differ.
    fn defaults_with(workspace: WorkspaceEnv, stubs: StubIndices) -> Self {
        let (laravel_aliases, resolved_class_cache) = new_alias_slot_and_cache();
        Self {
            name: "PHPantom".to_string(),
            version: env!("PHPANTOM_GIT_VERSION").to_string(),
            client_name: Mutex::new(String::new()),
            open_files: Arc::new(RwLock::new(HashMap::new())),
            files: Arc::new(RwLock::new(HashMap::new())),
            framework_references: framework::new_framework_reference_index(),
            framework_doctrine_repositories: framework::new_doctrine_repository_index(),
            reference_index: reference_index::new_reference_index(),
            skip_reference_index: false,
            symbols: SymbolIndex::new(),
            workspace,
            did_change_parse_locks: Arc::new(Mutex::new(HashMap::new())),
            parsed_for_diagnostics: Mutex::new(None),
            whole_file_coalesce: Arc::new(WholeFileCoalesce::default()),
            client: None,
            phar_archives: Arc::new(RwLock::new(HashMap::new())),
            parsed_uris: Arc::new(RwLock::new(HashSet::new())),
            parse_inflight: Arc::new(resolution::ParseInflight::new()),
            stub_index: Arc::new(RwLock::new(stubs.classes)),
            stub_function_index: Arc::new(RwLock::new(blade::with_marker_stubs(stubs.functions))),
            stub_constant_index: Arc::new(RwLock::new(stubs.constants)),
            external_stubs: Arc::new(RwLock::new(Arc::default())),
            has_external_stubs: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            resolved_class_cache,
            laravel: crate::backend::laravel::LaravelState::new(laravel_aliases),
            blade: blade::state::BladeState::default(),
            is_application: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            is_doctrine: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            is_symfony: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            symfony: crate::symfony::SymfonyState::default(),
            is_twig: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            is_yii: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            twig: crate::twig::TwigState::default(),
            yii: crate::yii::YiiState::default(),
            pest_configs: Arc::new(RwLock::new(None)),
            member_completion_cache: Arc::new(Mutex::new(HashMap::new())),
            diag: crate::diagnostics::state::DiagnosticState::new(),
            phpstan_tool: ExternalToolWorker::new(),
            phpcs_tool: ExternalToolWorker::new(),
            phpmd_tool: ExternalToolWorker::new(),
            mago_lint_tool: ExternalToolWorker::new(),
            mago_analyze_tool: ExternalToolWorker::new(),
            client_caps: Arc::new(RwLock::new(crate::backend::ClientCaps::default())),
            registered_watcher_state: Arc::new(RwLock::new(None)),
            member_ref_counts: reference_counts::new_member_ref_counts(),
            init_complete: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            shutdown_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            workspace_index: crate::backend::WorkspaceIndexState::default(),
            sync_ast_updates: false,
        }
    }

    /// The standard `Backend` every test constructor starts from.
    ///
    /// Identical to [`BackendState::defaults`] but with **empty** stub indices,
    /// so a test pays nothing for a standard library it never consults.
    /// Tests that need specific stubs override the relevant fields after
    /// construction.
    ///
    /// The workspace environment is also isolated from the global
    /// `.phpantom.toml`, so a test asserts against the project config it
    /// writes itself rather than against the config directory of whoever
    /// happens to be running the suite.
    pub(crate) fn test_defaults() -> Self {
        Self {
            // Tests drive the backend without an `initialize` round-trip; a
            // real client advertises these capabilities there.
            client_caps: Arc::new(RwLock::new(crate::backend::ClientCaps {
                file_create: true,
                code_action_resolve: true,
                ..Default::default()
            })),
            // A test asserts right after the edit that triggered the parse,
            // so the parse has to have committed by the time the edit
            // returns.
            sync_ast_updates: true,
            ..Self::defaults_with(WorkspaceEnv::new_isolated(), StubIndices::empty())
        }
    }

    /// Shared defaults for the non-test `Backend` constructors: the real
    /// workspace environment and the full embedded standard library.
    fn defaults() -> Self {
        Self::defaults_with(WorkspaceEnv::new(), StubIndices::embedded())
    }
}

impl Backend {
    /// A `Backend` over `state`, the first of its clones.
    fn from_state(state: BackendState) -> Self {
        Self {
            state: Arc::new(state),
            request_progress: None,
        }
    }

    /// The state of a `Backend` that has not been cloned yet, for a test
    /// to adjust after construction.
    ///
    /// # Panics
    ///
    /// When a clone of this `Backend` exists, which would not see the
    /// change.
    #[cfg(test)]
    pub(crate) fn state_mut(&mut self) -> &mut BackendState {
        Arc::get_mut(&mut self.state).expect("a Backend is adjusted before it is cloned")
    }

    /// [`BackendState::test_defaults`] as a `Backend`.
    pub(crate) fn test_defaults() -> Self {
        Self::from_state(BackendState::test_defaults())
    }

    /// Create a new `Backend` connected to an LSP client.
    pub fn new(client: Client) -> Self {
        Self::from_state(BackendState {
            client: Some(client),
            ..BackendState::defaults()
        })
    }

    /// Create a `Backend` without an LSP client but with full embedded
    /// stub indices.
    ///
    /// Use this for headless / CLI operation (e.g. the `analyze` command)
    /// where there is no LSP client but the backend still needs access to
    /// the PHP standard library stubs.
    pub fn new_headless() -> Self {
        Self::from_state(BackendState {
            skip_reference_index: true,
            ..BackendState::defaults()
        })
    }

    /// Create a headless backend for refactoring commands that need the
    /// workspace-wide reference index.
    pub fn new_headless_refactoring() -> Self {
        Self::from_state(BackendState::defaults())
    }

    /// Create a `Backend` without an LSP client (for unit / integration tests).
    ///
    /// Uses empty stub indices for fast construction.  Tests that need
    /// specific stubs should use [`new_test_with_stubs`] or
    /// [`new_test_with_all_stubs`] instead.
    pub fn new_test() -> Self {
        Self::test_defaults()
    }

    /// Create a `Backend` for tests that need the full embedded stub
    /// indices (e.g. benchmarks, end-to-end tests exercising real PHP
    /// stdlib classes).
    ///
    /// This is significantly slower than [`new_test`] because it builds
    /// three large `HashMap`s from the embedded phpstorm-stubs.  Only
    /// use this when the test specifically exercises stub-backed
    /// behaviour.
    pub fn new_test_with_full_stubs() -> Self {
        let backend = Self::from_state(BackendState::defaults_with(
            WorkspaceEnv::new_isolated(),
            StubIndices::embedded(),
        ));
        backend.set_php_version(backend.php_version());
        backend
    }

    /// Create a `Backend` for tests with custom stub class index.
    ///
    /// This allows tests to inject minimal stub content (e.g. `UnitEnum`,
    /// `BackedEnum`) without depending on `composer install` having been run.
    pub fn new_test_with_stubs(stub_index: HashMap<&'static str, &'static str>) -> Self {
        let backend = Self::from_state(BackendState {
            stub_index: Arc::new(RwLock::new(CiMap::from(stub_index))),
            ..BackendState::test_defaults()
        });
        backend.set_php_version(backend.php_version());
        backend
    }

    /// Create a `Backend` for tests with custom class, function, and constant
    /// stub indices.
    ///
    /// This allows tests to inject minimal stub content so that they are
    /// fully self-contained and do not depend on `composer install`.
    pub fn new_test_with_all_stubs(
        stub_index: HashMap<&'static str, &'static str>,
        stub_function_index: HashMap<&'static str, &'static str>,
        stub_constant_index: HashMap<&'static str, &'static str>,
    ) -> Self {
        let backend = Self::from_state(BackendState {
            stub_index: Arc::new(RwLock::new(CiMap::from(stub_index))),
            stub_function_index: Arc::new(RwLock::new(blade::with_marker_stubs(CiMap::from(
                stub_function_index,
            )))),
            stub_constant_index: Arc::new(RwLock::new(stub_constant_index)),
            ..BackendState::test_defaults()
        });
        backend.set_php_version(backend.php_version());
        backend
    }

    /// Create a `Backend` for tests with a specific workspace root and PSR-4
    /// mappings pre-configured.
    pub fn new_test_with_workspace(
        workspace_root: PathBuf,
        psr4_mappings: Vec<composer::Psr4Mapping>,
    ) -> Self {
        Self::from_state(BackendState {
            workspace: WorkspaceEnv {
                workspace_root: Arc::new(RwLock::new(Some(workspace_root))),
                psr4_mappings: Arc::new(RwLock::new(psr4_mappings)),
                ..WorkspaceEnv::new_isolated()
            },
            ..BackendState::test_defaults()
        })
    }

    /// A `Backend` sharing this one's state, without its request's
    /// progress sink.
    ///
    /// Used by LSP handlers (hover, definition, references, etc.) to move
    /// blocking sync work onto a `spawn_blocking` thread while keeping
    /// the async runtime free to process cancellations and other requests,
    /// and to hand the background workers a `Backend` of their own.
    pub(crate) fn clone_for_blocking(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            request_progress: None,
        }
    }
}
