//! Accessors for the `Backend` state that the integration tests and the
//! binary reach from outside the crate.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};

use crate::ci_map::CiMap;
use crate::types::{ClassInfo, DefineInfo, FunctionInfo};
use crate::{Backend, ClassCompletionOrigin, composer};

impl Backend {
    /// Borrow the workspace root mutex (used by integration tests to set a
    /// custom workspace directory).
    pub fn workspace_root(&self) -> &Arc<RwLock<Option<PathBuf>>> {
        &self.workspace.workspace_root
    }

    /// Borrow the global functions mutex (used by integration tests to
    /// inject user-defined functions or inspect the cache).
    pub fn global_functions(&self) -> &Arc<RwLock<CiMap<(String, FunctionInfo)>>> {
        &self.symbols.global_functions
    }

    /// The preprocessed virtual PHP for a Blade file (used by
    /// integration tests to diagnose the virtual content the way the
    /// live pipeline does).
    pub fn blade_virtual_php(&self, uri: &str) -> Option<String> {
        self.blade_virtual_php_arc(uri)
            .map(|php| String::clone(&php))
    }

    /// [`Self::blade_virtual_php`] without the copy, for the request paths
    /// that only need to read the text.
    pub(crate) fn blade_virtual_php_arc(&self, uri: &str) -> Option<Arc<String>> {
        let files = self.files.read();
        Some(Arc::clone(&files.get(uri)?.blade.as_ref()?.virtual_php))
    }

    /// The source map from a template's virtual PHP back to the template.
    pub(crate) fn blade_source_map(
        &self,
        uri: &str,
    ) -> Option<Arc<crate::blade::source_map::BladeSourceMap>> {
        let files = self.files.read();
        Some(Arc::clone(&files.get(uri)?.blade.as_ref()?.source_map))
    }

    /// Every template whose lowering has been recorded.
    pub(crate) fn lowered_template_uris(&self) -> Vec<String> {
        self.files
            .read()
            .iter()
            .filter(|(_, state)| state.blade.is_some())
            .map(|(uri, _)| uri.clone())
            .collect()
    }

    /// Borrow the global defines mutex (used by integration tests to
    /// inject user-defined constants or inspect the cache).
    pub fn global_defines(&self) -> &Arc<RwLock<HashMap<String, DefineInfo>>> {
        &self.symbols.global_defines
    }

    /// Borrow the class index mutex (used by integration tests to
    /// populate discovered class entries).
    pub fn fqn_uri_index(&self) -> &Arc<RwLock<CiMap<String>>> {
        &self.symbols.fqn_uri_index
    }

    /// Borrow the FQN → ClassInfo index mutex (used by integration tests
    /// to populate class metadata for context-aware completion filtering).
    pub fn fqn_class_index(&self) -> &Arc<RwLock<CiMap<Arc<ClassInfo>>>> {
        &self.symbols.fqn_class_index
    }

    /// Borrow the PSR-4 mappings mutex (used by integration tests to
    /// configure autoload mappings).
    pub fn psr4_mappings(&self) -> &Arc<RwLock<Vec<composer::Psr4Mapping>>> {
        &self.workspace.psr4_mappings
    }

    /// Borrow the configured Laravel date class (used by integration tests
    /// to verify that provider edits keep the `Date::use()` selection
    /// current).  The outer option is `None` until startup discovery runs;
    /// the inner option is `None` when no project override was found.
    pub fn laravel_date_class(&self) -> &RwLock<Option<Option<String>>> {
        &self.laravel.date_class
    }

    /// Borrow the set of parsed file URIs (used by integration tests to
    /// mark a workspace file as already loaded, mirroring a lazy parse).
    pub fn parsed_uris(&self) -> &Arc<RwLock<HashSet<String>>> {
        &self.parsed_uris
    }

    /// Read the stub constant index (used by integration tests to
    /// verify built-in constants are present).
    pub fn stub_constant_index(
        &self,
    ) -> parking_lot::RwLockReadGuard<'_, HashMap<&'static str, &'static str>> {
        self.stub_constant_index.read()
    }

    pub fn stub_function_index_mut(
        &self,
    ) -> parking_lot::RwLockWriteGuard<'_, CiMap<&'static str>> {
        self.stub_function_index.write()
    }

    /// Write-access the stub constant index (used by integration tests
    /// to inject test stub entries).
    pub fn stub_constant_index_mut(
        &self,
    ) -> parking_lot::RwLockWriteGuard<'_, HashMap<&'static str, &'static str>> {
        self.stub_constant_index.write()
    }

    /// Borrow the autoload function index (used by integration tests to
    /// populate discovered function entries for non-Composer projects).
    pub fn autoload_function_index(&self) -> &Arc<RwLock<CiMap<PathBuf>>> {
        &self.symbols.autoload_function_index
    }

    pub fn autoload_function_origin_index(&self) -> &Arc<RwLock<CiMap<ClassCompletionOrigin>>> {
        &self.symbols.autoload_function_origin_index
    }

    /// Borrow the autoload constant index (used by integration tests to
    /// populate discovered constant entries for non-Composer projects).
    pub fn autoload_constant_index(&self) -> &Arc<RwLock<HashMap<String, PathBuf>>> {
        &self.symbols.autoload_constant_index
    }

    pub fn autoload_constant_origin_index(
        &self,
    ) -> &Arc<RwLock<HashMap<String, ClassCompletionOrigin>>> {
        &self.symbols.autoload_constant_origin_index
    }

    /// Borrow the autoload file paths list (used by integration tests
    /// to simulate Composer autoload file discovery).
    pub fn autoload_file_paths(&self) -> &Arc<RwLock<Vec<PathBuf>>> {
        &self.symbols.autoload_file_paths
    }

    /// Pretend the client cannot call `codeAction/resolve` (used by
    /// integration tests, which skip the `initialize` round-trip).
    pub fn disable_code_action_resolve(&self) {
        self.client_caps.write().code_action_resolve = false;
    }

    /// Borrow the open files map (used by integration tests to inject
    /// file content without going through the LSP `didOpen` path).
    pub fn open_files(&self) -> &Arc<RwLock<HashMap<String, Arc<String>>>> {
        &self.open_files
    }

    /// Mark the workspace as indexed (used by integration tests that need
    /// the state `ensure_workspace_indexed` leaves behind without running
    /// a real workspace scan).
    pub fn mark_workspace_indexed(&self) {
        self.workspace_index
            .indexed
            .store(true, std::sync::atomic::Ordering::Release);
    }

    /// Declare whether the client supports `workspace/codeLens/refresh`,
    /// which decides whether a lens may be answered cold and refreshed
    /// once its count lands (used by integration tests to pick the path
    /// without going through `initialize`).
    pub fn set_supports_code_lens_refresh(&self, supported: bool) {
        self.client_caps.write().code_lens_refresh = supported;
    }

    /// Borrow the PHPStan diagnostics cache (used by integration tests
    /// to inject PHPStan diagnostics without running PHPStan).
    pub fn phpstan_last_diags(
        &self,
    ) -> &Arc<Mutex<HashMap<String, Vec<tower_lsp_server::gen_lsp_types::Diagnostic>>>> {
        &self.phpstan_tool.last_diags
    }

    /// Borrow the PHPCS diagnostics cache (used by integration tests
    /// to inject PHPCS diagnostics without running PHPCS).
    pub fn phpcs_last_diags(
        &self,
    ) -> &Arc<Mutex<HashMap<String, Vec<tower_lsp_server::gen_lsp_types::Diagnostic>>>> {
        &self.phpcs_tool.last_diags
    }

    /// Clear the member completion cache.
    pub fn clear_completion_cache(&self) {
        self.member_completion_cache.lock().clear();
    }

    /// Record whether the client handles `window/showDocument`.
    ///
    /// Set from the `initialize` handshake; integration tests use it to
    /// exercise both code lens command shapes.
    pub fn set_supports_show_document(&self, supported: bool) {
        self.client_caps.write().show_document = supported;
    }

    /// What the connected client supports.
    pub(crate) fn caps(&self) -> crate::backend::ClientCaps {
        *self.client_caps.read()
    }
}
