//! How far the workspace index has got.

use std::sync::atomic::AtomicBool;

use parking_lot::Mutex;

/// The state of the workspace index: whether it is built, the lock its
/// build holds, and the progress a request waiting on it reports.
#[derive(Default)]
pub(crate) struct WorkspaceIndexState {
    /// Whether the workspace directory has been fully scanned for PHP and
    /// resource files.
    ///
    /// Set to `true` after the initial `ensure_workspace_indexed` pass.
    /// Per-symbol consumers reuse that index, watched-file notifications
    /// update it incrementally, and an explicit reference search may refresh
    /// it once to discover filesystem changes the editor did not report.
    pub(crate) indexed: AtomicBool,
    /// Serializes whole-workspace indexing so a foreground request does not
    /// duplicate the background full-index parse.
    pub(crate) lock: Mutex<()>,
    /// Prevents duplicate background full-index tasks when initialization and
    /// a request both race to parse the whole workspace.
    pub(crate) full_in_progress: AtomicBool,
    /// Latest `(percentage, message)` published by the indexing pass that
    /// currently holds `workspace_index_lock`, or `None` when no pass is
    /// running.
    ///
    /// Requests that need a complete index (find references, rename,
    /// go-to-implementation, Laravel string keys) block on that lock while
    /// the background full index runs. They mirror this status into their
    /// own progress token so the wait reads as "the workspace is still
    /// indexing" rather than a stalled request.
    pub(crate) status: Mutex<Option<(u32, String)>>,
}
