//! Diagnostic scheduling and pull-model cache state.
//!
//! Grouped out of `Backend` so the diagnostic subsystem's fields live
//! together. All fields are `Arc`-wrapped, so `#[derive(Clone)]` shares
//! them with a cloned `Backend` (the same semantics the per-request clone
//! had when these were individual `Backend` fields).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};

use parking_lot::Mutex;
use tokio::sync::Notify;
use tower_lsp::lsp_types::Diagnostic;

use crate::diagnostics::workspace::WorkspaceDiagnostics;

/// Diagnostic debouncing state and the pull-model diagnostic caches.
#[derive(Clone)]
pub(crate) struct DiagnosticState {
    /// Monotonically increasing version counter for diagnostic debouncing.
    pub(crate) version: Arc<AtomicU64>,
    /// Notification handle used to wake the diagnostic worker task.
    pub(crate) notify: Arc<Notify>,
    /// File URIs that need a diagnostic pass, drained by the worker.
    pub(crate) pending_uris: Arc<Mutex<HashSet<String>>>,
    /// Last-published slow diagnostics per file URI.
    pub(crate) last_slow: Arc<Mutex<HashMap<String, Vec<Diagnostic>>>>,
    /// Last-computed fast diagnostics per file URI.
    pub(crate) last_fast: Arc<Mutex<HashMap<String, Vec<Diagnostic>>>>,
    /// Per-file `resultId` for pull diagnostics, drawn from
    /// `result_id_seq` so a value is never reused within the session
    /// even after `did_close` drops a file's entry (a reopened file
    /// starting back at 0 could otherwise land on an id the client
    /// still holds from before the close).
    pub(crate) result_ids: Arc<Mutex<HashMap<String, u64>>>,
    /// Session-global sequence feeding `result_ids`.
    pub(crate) result_id_seq: Arc<AtomicU64>,
    /// Combined diagnostic cache (fast + slow + external tools) per file URI.
    pub(crate) last_full: Arc<Mutex<HashMap<String, Vec<Diagnostic>>>>,
    /// Diagnostics to suppress from the next publish cycle.
    pub(crate) suppressed: Arc<Mutex<Vec<Diagnostic>>>,
    /// Diagnostics for files not open in the editor (background pass).
    pub(crate) workspace_diags: Arc<Mutex<WorkspaceDiagnostics>>,
    /// Whether the background workspace diagnostics pass is currently
    /// active (started and not since switched off by a config reload).
    /// Also prevents duplicate concurrent passes.
    pub(crate) workspace_diag_pass_started: Arc<AtomicBool>,
    /// Set to stop a running native pass and its workers when a live
    /// config reload switches `[diagnostics] workspace` off mid-pass.
    /// Cleared again before a later pass starts.
    pub(crate) workspace_diag_cancel: Arc<AtomicBool>,
    /// Wakes the diagnostic-refresh pump (see
    /// [`crate::Backend::request_diagnostic_refresh`]).  `Notify` holds at
    /// most one stored permit, so refresh requests signalled while one is
    /// in flight coalesce into a single follow-up refresh.
    pub(crate) refresh_notify: Arc<Notify>,
    /// Whether the refresh pump task has been spawned yet.
    pub(crate) refresh_pump_started: Arc<AtomicBool>,
    /// Whether the client has sent at least one `workspace/diagnostic`
    /// pull.  In pull mode the background workspace pass waits for this:
    /// its results are only deliverable through workspace pull responses,
    /// so computing them for a client that never asks is wasted work.
    pub(crate) workspace_pull_seen: Arc<AtomicBool>,
    /// Wakes the deferred background workspace pass when the first
    /// `workspace/diagnostic` pull arrives.
    pub(crate) workspace_pull_notify: Arc<Notify>,
    /// What each open file declared when it was last opened or saved, used
    /// to work out which other open files a save can affect.  See
    /// [`crate::diagnostics::cross_file`].
    pub(crate) decl_baselines: Arc<Mutex<crate::diagnostics::cross_file::DeclarationBaselines>>,
    /// Per-URI locks serializing `assemble_and_push`'s read-merge-write
    /// across the six per-source diagnostic caches.
    ///
    /// Each source cache is locked and released independently, so without
    /// this, two calls completing around the same time (e.g. a fast-phase
    /// native worker and an external tool) can each read a different
    /// snapshot and the one that started reading first can still finish
    /// writing last, clobbering a fresher merge with a stale one.
    pub(crate) assemble_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl DiagnosticState {
    pub(crate) fn new() -> Self {
        Self {
            version: Arc::new(AtomicU64::new(0)),
            notify: Arc::new(Notify::new()),
            pending_uris: Arc::new(Mutex::new(HashSet::new())),
            last_slow: Arc::new(Mutex::new(HashMap::new())),
            last_fast: Arc::new(Mutex::new(HashMap::new())),
            result_ids: Arc::new(Mutex::new(HashMap::new())),
            result_id_seq: Arc::new(AtomicU64::new(0)),
            last_full: Arc::new(Mutex::new(HashMap::new())),
            suppressed: Arc::new(Mutex::new(Vec::new())),
            workspace_diags: Arc::new(Mutex::new(WorkspaceDiagnostics::default())),
            workspace_diag_pass_started: Arc::new(AtomicBool::new(false)),
            workspace_diag_cancel: Arc::new(AtomicBool::new(false)),
            refresh_notify: Arc::new(Notify::new()),
            refresh_pump_started: Arc::new(AtomicBool::new(false)),
            workspace_pull_seen: Arc::new(AtomicBool::new(false)),
            workspace_pull_notify: Arc::new(Notify::new()),
            decl_baselines: Arc::new(Mutex::new(HashMap::new())),
            assemble_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}
