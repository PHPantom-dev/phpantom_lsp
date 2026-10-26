//! The state a background worker for one external diagnostic tool shares
//! with the `Backend`.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

/// Shared state for one external diagnostic tool's dedicated background
/// worker (PHPStan, PHPCS, PHPMD, Mago lint, Mago analyze).
///
/// Each worker runs as its own task so that a slow external process never
/// blocks native diagnostics or the other external tools. At most one
/// process per tool runs at a time; if the user edits while it is running,
/// `pending_uri` is overwritten and the worker picks up the newest file
/// once the current run finishes (older requests are superseded, not
/// queued — these tools are too slow to queue).
pub(crate) struct ExternalToolWorker {
    /// Wakes the worker task when a new URI is pending.
    pub(crate) notify: Arc<tokio::sync::Notify>,
    /// The single file URI the worker should analyse next.
    pub(crate) pending_uri: Arc<Mutex<Option<String>>>,
    /// Last-published diagnostics per file URI, merged into fast/slow
    /// diagnostic publishes so results stay visible between tool runs.
    pub(crate) last_diags:
        Arc<Mutex<HashMap<String, Vec<tower_lsp_server::gen_lsp_types::Diagnostic>>>>,
    /// Per-URI write counter, bumped every time this worker stores a
    /// single-file result in [`last_diags`].
    ///
    /// A project-wide run of the same tool snapshots this map before it
    /// starts and compares each entry again just before writing, so its
    /// scan-time results can never clobber a fresher single-file result
    /// that landed while the project-wide run was still in flight.
    generations: Arc<Mutex<HashMap<String, u64>>>,
}

impl ExternalToolWorker {
    pub(crate) fn new() -> Self {
        Self {
            notify: Arc::new(tokio::sync::Notify::new()),
            pending_uri: Arc::new(Mutex::new(None)),
            last_diags: Arc::new(Mutex::new(HashMap::new())),
            generations: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Store a single-file run's result and mark the URI as freshly
    /// written, superseding any project-wide run still in flight.
    pub(crate) fn store_file_result(
        &self,
        uri: &str,
        diags: Vec<tower_lsp_server::gen_lsp_types::Diagnostic>,
    ) {
        let mut cache = self.last_diags.lock();
        cache.insert(uri.to_string(), diags);
        *self.generations.lock().entry(uri.to_string()).or_insert(0) += 1;
    }

    /// Snapshot the per-URI write counters, to be handed back to
    /// [`store_scan_result`](Self::store_scan_result) once a
    /// project-wide run of this tool finishes.
    pub(crate) fn generation_snapshot(&self) -> HashMap<String, u64> {
        self.generations.lock().clone()
    }

    /// Store a project-wide run's result for one URI, unless a
    /// single-file run has written to that URI since `snapshot` was
    /// taken.  Returns whether the write happened.
    ///
    /// Both locks are taken in the same order as `store_file_result`,
    /// which makes the check-then-write atomic against it.
    pub(crate) fn store_scan_result(
        &self,
        snapshot: &HashMap<String, u64>,
        uri: &str,
        diags: Vec<tower_lsp_server::gen_lsp_types::Diagnostic>,
    ) -> bool {
        let mut cache = self.last_diags.lock();
        if self.generations.lock().get(uri).copied() != snapshot.get(uri).copied() {
            return false;
        }
        cache.insert(uri.to_string(), diags);
        true
    }

    /// Drop a URI's cached result and write counter (on `did_close`).
    pub(crate) fn forget(&self, uri: &str) {
        self.last_diags.lock().remove(uri);
        self.generations.lock().remove(uri);
    }
}

impl Clone for ExternalToolWorker {
    fn clone(&self) -> Self {
        Self {
            notify: Arc::clone(&self.notify),
            pending_uri: Arc::clone(&self.pending_uri),
            last_diags: Arc::clone(&self.last_diags),
            generations: Arc::clone(&self.generations),
        }
    }
}
