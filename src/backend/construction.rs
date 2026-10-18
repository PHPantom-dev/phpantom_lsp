//! Building a `Backend`: the constructors for the server, the CLI and the
//! tests, and the cheap clones handed to worker threads.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};
use tower_lsp::Client;

use crate::backend::external_tool_worker::ExternalToolWorker;
use crate::backend::laravel::{LaravelStringKeyBuildLocks, LaravelStringKeyCache};
use crate::backend::requests::WholeFileCoalesce;
use crate::ci_map::CiMap;
use crate::symbol_index::SymbolIndex;
use crate::workspace_env::WorkspaceEnv;
use crate::{
    Backend, blade, composer, reference_counts, reference_index, resolution, stubs, virtual_members,
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
    cache.write().set_laravel_aliases(Arc::clone(&slot));
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

impl Backend {
    /// Shared defaults for all Backend constructors.
    ///
    /// Returns a `Backend` with no LSP client and empty maps, reading its
    /// workspace environment and standard library from the arguments.
    /// Each public constructor customises only the fields that differ.
    fn defaults_with(workspace: WorkspaceEnv, stubs: StubIndices) -> Self {
        let (laravel_aliases, resolved_class_cache) = new_alias_slot_and_cache();
        Self {
            name: "PHPantom".to_string(),
            version: env!("PHPANTOM_GIT_VERSION").to_string(),
            client_name: Mutex::new(String::new()),
            open_files: Arc::new(RwLock::new(HashMap::new())),
            symbol_maps: Arc::new(RwLock::new(HashMap::new())),
            reference_index: reference_index::new_reference_index(),
            skip_reference_index: false,
            symbols: SymbolIndex::new(),
            workspace,
            parse_errors: Arc::new(RwLock::new(HashMap::new())),
            did_change_parse_locks: Arc::new(Mutex::new(HashMap::new())),
            whole_file_coalesce: Arc::new(WholeFileCoalesce::default()),
            client: None,
            file_imports: Arc::new(RwLock::new(HashMap::new())),
            file_symbol_imports: Arc::new(RwLock::new(HashMap::new())),
            resolved_names: Arc::new(RwLock::new(HashMap::new())),
            file_namespaces: Arc::new(RwLock::new(HashMap::new())),
            phar_archives: Arc::new(RwLock::new(HashMap::new())),
            parsed_uris: Arc::new(RwLock::new(HashSet::new())),
            parse_inflight: Arc::new(resolution::ParseInflight::new()),
            stub_index: Arc::new(RwLock::new(stubs.classes)),
            stub_function_index: Arc::new(RwLock::new(blade::with_marker_stubs(stubs.functions))),
            stub_constant_index: Arc::new(RwLock::new(stubs.constants)),
            resolved_class_cache,
            auth_user_type_cache: Arc::new(RwLock::new(HashMap::new())),
            storage_disk_type_cache: Arc::new(RwLock::new(None)),
            laravel_storage_drivers: Arc::new(RwLock::new(Default::default())),
            laravel_aliases,
            laravel_macros: Arc::new(RwLock::new(
                virtual_members::laravel::LaravelMacroIndex::default(),
            )),
            laravel_has_macros: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            laravel_pivots: Arc::new(RwLock::new(
                virtual_members::laravel::LaravelPivotIndex::default(),
            )),
            laravel_has_pivots: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            laravel_pivots_dirty: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            laravel_commands: Arc::new(RwLock::new(
                virtual_members::laravel::LaravelCommandIndex::default(),
            )),
            laravel_has_commands: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            laravel_morph_map: Arc::new(RwLock::new(
                virtual_members::laravel::LaravelMorphMapIndex::default(),
            )),
            laravel_gates: Arc::new(RwLock::new(
                virtual_members::laravel::LaravelGateIndex::default(),
            )),
            laravel_runtime_config_keys: Arc::new(RwLock::new(HashMap::new())),
            is_application: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            laravel_macro_seeds: Arc::new(RwLock::new(HashMap::new())),
            laravel_macro_mixin_uris: Arc::new(RwLock::new(std::collections::HashSet::new())),
            laravel_date_class: Arc::new(RwLock::new(None)),
            pest_configs: Arc::new(RwLock::new(None)),
            file_pest_uses: Arc::new(RwLock::new(HashMap::new())),
            laravel_date_seed_uris: Arc::new(RwLock::new(std::collections::HashSet::new())),
            laravel_provider_resources: Arc::new(RwLock::new(
                virtual_members::laravel::ProviderResources::default(),
            )),
            laravel_provider_scans: Arc::new(RwLock::new(
                virtual_members::laravel::ProviderScans::default(),
            )),
            blade_directives: Arc::new(RwLock::new(blade::directives::BladeDirectives::default())),
            laravel_string_key_cache: Arc::new(RwLock::new(LaravelStringKeyCache::default())),
            laravel_string_key_build_locks: Arc::new(LaravelStringKeyBuildLocks::default()),
            schema_index: Arc::new(RwLock::new(
                virtual_members::laravel::database_schema::SchemaIndex::default(),
            )),
            member_completion_cache: Arc::new(Mutex::new(HashMap::new())),
            diag: crate::diagnostics::state::DiagnosticState::new(),
            phpstan_tool: ExternalToolWorker::new(),
            phpcs_tool: ExternalToolWorker::new(),
            phpmd_tool: ExternalToolWorker::new(),
            mago_lint_tool: ExternalToolWorker::new(),
            mago_analyze_tool: ExternalToolWorker::new(),

            supports_pull_diagnostics: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_file_rename: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_file_create: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_code_action_resolve: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_work_done_progress: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_type_hierarchy_dynamic_registration: Arc::new(
                std::sync::atomic::AtomicBool::new(false),
            ),
            registered_watcher_state: Arc::new(RwLock::new(None)),
            supports_watched_files_dynamic_registration: Arc::new(
                std::sync::atomic::AtomicBool::new(false),
            ),
            supports_relative_pattern_watchers: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_show_document: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_semantic_tokens_refresh: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_code_lens_refresh: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_inlay_hint_refresh: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            member_ref_counts: reference_counts::new_member_ref_counts(),
            init_complete: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            shutdown_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            blade_virtual_content: Arc::new(RwLock::new(HashMap::new())),
            blade_source_maps: Arc::new(RwLock::new(HashMap::new())),
            blade_publish_lock: Arc::new(Mutex::new(())),
            blade_uris: Arc::new(RwLock::new(std::collections::HashSet::new())),
            blade_injected_vars: Arc::new(RwLock::new(HashMap::new())),
            typed_receiver_view_spans_cache: Arc::new(RwLock::new(HashMap::new())),
            morph_column_spans_cache: Arc::new(RwLock::new(HashMap::new())),
            workspace_indexed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            workspace_index_lock: Arc::new(Mutex::new(())),
            full_index_in_progress: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            workspace_index_status: Arc::new(Mutex::new(None)),
            request_progress: None,
            sync_ast_updates: false,
        }
    }

    /// The standard `Backend` every test constructor starts from.
    ///
    /// Identical to [`Backend::defaults`] but with **empty** stub indices,
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
            // real client advertises this capability there.
            supports_file_create: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            supports_code_action_resolve: Arc::new(std::sync::atomic::AtomicBool::new(true)),
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

    /// Create a new `Backend` connected to an LSP client.
    pub fn new(client: Client) -> Self {
        Self {
            client: Some(client),
            ..Self::defaults()
        }
    }

    /// Create a `Backend` without an LSP client but with full embedded
    /// stub indices.
    ///
    /// Use this for headless / CLI operation (e.g. the `analyze` command)
    /// where there is no LSP client but the backend still needs access to
    /// the PHP standard library stubs.
    pub fn new_headless() -> Self {
        Self {
            skip_reference_index: true,
            ..Self::defaults()
        }
    }

    /// Create a headless backend for refactoring commands that need the
    /// workspace-wide reference index.
    pub fn new_headless_refactoring() -> Self {
        Self::defaults()
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
        let backend = Self::defaults_with(WorkspaceEnv::new_isolated(), StubIndices::embedded());
        backend.set_php_version(backend.php_version());
        backend
    }

    /// Create a `Backend` for tests with custom stub class index.
    ///
    /// This allows tests to inject minimal stub content (e.g. `UnitEnum`,
    /// `BackedEnum`) without depending on `composer install` having been run.
    pub fn new_test_with_stubs(stub_index: HashMap<&'static str, &'static str>) -> Self {
        let backend = Self {
            stub_index: Arc::new(RwLock::new(CiMap::from(stub_index))),
            ..Self::test_defaults()
        };
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
        let backend = Self {
            stub_index: Arc::new(RwLock::new(CiMap::from(stub_index))),
            stub_function_index: Arc::new(RwLock::new(blade::with_marker_stubs(CiMap::from(
                stub_function_index,
            )))),
            stub_constant_index: Arc::new(RwLock::new(stub_constant_index)),
            ..Self::test_defaults()
        };
        backend.set_php_version(backend.php_version());
        backend
    }

    /// Create a `Backend` for tests with a specific workspace root and PSR-4
    /// mappings pre-configured.
    pub fn new_test_with_workspace(
        workspace_root: PathBuf,
        psr4_mappings: Vec<composer::Psr4Mapping>,
    ) -> Self {
        Self {
            workspace: WorkspaceEnv {
                workspace_root: Arc::new(RwLock::new(Some(workspace_root))),
                psr4_mappings: Arc::new(RwLock::new(psr4_mappings)),
                ..WorkspaceEnv::new_isolated()
            },
            ..Self::test_defaults()
        }
    }

    /// Create a shallow clone of this `Backend` that shares every
    /// `Arc`-wrapped field with the original.
    ///
    /// Non-`Arc` fields (`php_version`, `vendor_uri_prefixes`,
    /// `vendor_dir_paths`) are snapshotted at call time.  The stub
    /// indices (`stub_index`, `stub_function_index`,
    /// `stub_constant_index`) are cloned (they are static `&str`
    /// maps, so this is cheap).
    ///
    /// Used by `initialized()` to build a `Backend` value that can be
    /// moved into the `tokio::spawn`-ed diagnostic worker task while
    /// still observing every mutation the "real" `Backend` makes to
    /// the shared `Arc<Mutex<…>>` maps.
    ///
    /// Also used by [`clone_for_blocking`](Self::clone_for_blocking).
    pub(crate) fn clone_for_diagnostic_worker(&self) -> Self {
        Self {
            name: self.name.clone(),
            version: self.version.clone(),
            client_name: Mutex::new(self.client_name.lock().clone()),
            open_files: Arc::clone(&self.open_files),
            symbol_maps: Arc::clone(&self.symbol_maps),
            reference_index: Arc::clone(&self.reference_index),
            skip_reference_index: self.skip_reference_index,
            symbols: self.symbols.clone(),
            parse_errors: Arc::clone(&self.parse_errors),
            did_change_parse_locks: Arc::clone(&self.did_change_parse_locks),
            whole_file_coalesce: Arc::clone(&self.whole_file_coalesce),
            // RwLock fields are shared by Arc::clone — the diagnostic
            // worker reads them concurrently with the main Backend.
            client: self.client.clone(),
            file_imports: Arc::clone(&self.file_imports),
            file_symbol_imports: Arc::clone(&self.file_symbol_imports),
            resolved_names: Arc::clone(&self.resolved_names),
            file_namespaces: Arc::clone(&self.file_namespaces),
            phar_archives: Arc::clone(&self.phar_archives),
            parsed_uris: Arc::clone(&self.parsed_uris),
            parse_inflight: Arc::clone(&self.parse_inflight),
            stub_index: Arc::clone(&self.stub_index),
            resolved_class_cache: Arc::clone(&self.resolved_class_cache),
            auth_user_type_cache: Arc::clone(&self.auth_user_type_cache),
            storage_disk_type_cache: Arc::clone(&self.storage_disk_type_cache),
            laravel_storage_drivers: Arc::clone(&self.laravel_storage_drivers),
            laravel_aliases: Arc::clone(&self.laravel_aliases),
            laravel_macros: Arc::clone(&self.laravel_macros),
            laravel_has_macros: Arc::clone(&self.laravel_has_macros),
            laravel_pivots: Arc::clone(&self.laravel_pivots),
            laravel_has_pivots: Arc::clone(&self.laravel_has_pivots),
            laravel_pivots_dirty: Arc::clone(&self.laravel_pivots_dirty),
            laravel_commands: Arc::clone(&self.laravel_commands),
            laravel_has_commands: Arc::clone(&self.laravel_has_commands),
            laravel_morph_map: Arc::clone(&self.laravel_morph_map),
            laravel_gates: Arc::clone(&self.laravel_gates),
            laravel_runtime_config_keys: Arc::clone(&self.laravel_runtime_config_keys),
            is_application: Arc::clone(&self.is_application),
            laravel_macro_seeds: Arc::clone(&self.laravel_macro_seeds),
            laravel_macro_mixin_uris: Arc::clone(&self.laravel_macro_mixin_uris),
            laravel_date_class: Arc::clone(&self.laravel_date_class),
            pest_configs: Arc::clone(&self.pest_configs),
            file_pest_uses: Arc::clone(&self.file_pest_uses),
            laravel_date_seed_uris: Arc::clone(&self.laravel_date_seed_uris),
            laravel_provider_resources: Arc::clone(&self.laravel_provider_resources),
            laravel_provider_scans: Arc::clone(&self.laravel_provider_scans),
            blade_directives: Arc::clone(&self.blade_directives),
            laravel_string_key_cache: Arc::clone(&self.laravel_string_key_cache),
            laravel_string_key_build_locks: Arc::clone(&self.laravel_string_key_build_locks),
            schema_index: Arc::clone(&self.schema_index),
            member_completion_cache: Arc::clone(&self.member_completion_cache),
            stub_function_index: Arc::clone(&self.stub_function_index),
            stub_constant_index: Arc::clone(&self.stub_constant_index),
            diag: self.diag.clone(),
            workspace: self.workspace.clone(),
            phpstan_tool: self.phpstan_tool.clone(),
            phpcs_tool: self.phpcs_tool.clone(),
            phpmd_tool: self.phpmd_tool.clone(),
            mago_lint_tool: self.mago_lint_tool.clone(),
            mago_analyze_tool: self.mago_analyze_tool.clone(),
            supports_pull_diagnostics: Arc::clone(&self.supports_pull_diagnostics),
            supports_file_rename: Arc::clone(&self.supports_file_rename),
            supports_file_create: Arc::clone(&self.supports_file_create),
            supports_code_action_resolve: Arc::clone(&self.supports_code_action_resolve),
            supports_work_done_progress: Arc::clone(&self.supports_work_done_progress),
            supports_type_hierarchy_dynamic_registration: Arc::clone(
                &self.supports_type_hierarchy_dynamic_registration,
            ),
            supports_watched_files_dynamic_registration: Arc::clone(
                &self.supports_watched_files_dynamic_registration,
            ),
            supports_relative_pattern_watchers: Arc::clone(
                &self.supports_relative_pattern_watchers,
            ),
            registered_watcher_state: Arc::clone(&self.registered_watcher_state),
            supports_show_document: Arc::clone(&self.supports_show_document),
            supports_semantic_tokens_refresh: Arc::clone(&self.supports_semantic_tokens_refresh),
            supports_code_lens_refresh: Arc::clone(&self.supports_code_lens_refresh),
            supports_inlay_hint_refresh: Arc::clone(&self.supports_inlay_hint_refresh),
            member_ref_counts: Arc::clone(&self.member_ref_counts),
            init_complete: Arc::clone(&self.init_complete),
            shutdown_flag: Arc::clone(&self.shutdown_flag),
            blade_virtual_content: Arc::clone(&self.blade_virtual_content),
            blade_source_maps: Arc::clone(&self.blade_source_maps),
            blade_publish_lock: Arc::clone(&self.blade_publish_lock),
            blade_uris: Arc::clone(&self.blade_uris),
            blade_injected_vars: Arc::clone(&self.blade_injected_vars),
            typed_receiver_view_spans_cache: Arc::clone(&self.typed_receiver_view_spans_cache),
            morph_column_spans_cache: Arc::clone(&self.morph_column_spans_cache),
            workspace_indexed: Arc::clone(&self.workspace_indexed),
            workspace_index_lock: Arc::clone(&self.workspace_index_lock),
            full_index_in_progress: Arc::clone(&self.full_index_in_progress),
            workspace_index_status: Arc::clone(&self.workspace_index_status),
            // Deliberately not propagated: a request's progress sink is
            // attached explicitly to that request's own clone only.
            request_progress: None,
            sync_ast_updates: self.sync_ast_updates,
        }
    }

    /// Cheap clone that shares all `Arc`-wrapped state with the original.
    ///
    /// Used by LSP handlers (hover, definition, references, etc.) to move
    /// blocking sync work onto a `spawn_blocking` thread while keeping
    /// the async runtime free to process cancellations and other requests.
    pub(crate) fn clone_for_blocking(&self) -> Self {
        self.clone_for_diagnostic_worker()
    }
}
