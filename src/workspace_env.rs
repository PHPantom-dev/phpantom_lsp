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
    pub(crate) config: Mutex<config::Config>,
}

impl WorkspaceEnv {
    pub(crate) fn new() -> Self {
        Self {
            workspace_root: Arc::new(RwLock::new(None)),
            psr4_mappings: Arc::new(RwLock::new(Vec::new())),
            vendor_uri_prefixes: Mutex::new(Vec::new()),
            vendor_dir_paths: Mutex::new(Vec::new()),
            vendor_package_origin_roots: Arc::new(RwLock::new(Vec::new())),
            php_version: Mutex::new(PhpVersion::default()),
            config: Mutex::new(config::Config::default()),
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
            config: Mutex::new(self.config.lock().clone()),
        }
    }
}
