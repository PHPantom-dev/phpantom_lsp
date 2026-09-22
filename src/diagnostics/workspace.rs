//! Background workspace-wide diagnostics.
//!
//! After the initial startup indexing and the full background index
//! finish, PHPantom computes diagnostics for every user file in the
//! workspace — not just the files open in the editor — so project-wide
//! problems appear in the editor's problems panel.
//!
//! ## Ordering guarantees
//!
//! Nothing here runs before the server is usable.  The pass is chained
//! onto the end of the full background index task, which itself only
//! starts after the synchronous startup indexing in `initialized`
//! completes.  The pass additionally waits for `init_complete` so the
//! post-index cache clears in `initialized` cannot race with it.
//!
//! For pull-diagnostics clients the pass is additionally deferred until
//! the first `workspace/diagnostic` request arrives: its results are
//! only deliverable through workspace pull responses, so a client that
//! never pulls never pays for the scan.  Push clients get the pass
//! unconditionally since their results are published directly.
//!
//! 1. **Native pass** — the same fast + slow collectors that diagnose
//!    open files run over every unopened user file, on a throttled
//!    worker pool (half the cores) so interactive requests stay
//!    responsive.  Results stream to the editor as files finish.  A
//!    file whose analysis never returns is given up on and reported
//!    rather than stalling every file queued behind it, and the worker
//!    it wedged is replaced so the pool keeps its throughput; the pass
//!    is driven by `drive_native_pass`, which is where that happens.
//! 2. **External tools** — after the native pass, each configured
//!    external tool (PHPStan, PHPCS, Mago lint/analyze) runs once over
//!    the whole project.  A tool only runs when it is enabled,
//!    resolvable, and has its own project-level configuration file
//!    (`phpstan.neon`, `phpcs.xml`, `mago.toml`) so the tool itself
//!    decides which paths to analyse.  Tools run sequentially to avoid
//!    saturating the machine.
//!
//! ## Delivery
//!
//! Results for unopened files are stored in [`WorkspaceDiagnostics`]
//! and delivered through the `workspace/diagnostic` pull handler
//! (advertised via the `workspace_diagnostics` server capability) or
//! published directly via `textDocument/publishDiagnostics` for push
//! clients.  Open files are skipped: they are owned by the live
//! per-file pipeline.  When a file closes, its native diagnostics are
//! recomputed from disk and its external per-source results migrate
//! here so closed files keep accurate diagnostics.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tower_lsp::lsp_types::{Diagnostic, MessageType};

use crate::Backend;
use crate::diagnostics::ignore_rules::{self, CompiledIgnoreRule};
use crate::progress::ScanProgress;

/// Minimum time between streaming deliveries while the native pass is
/// running.  A `workspace/diagnostic/refresh` makes the editor re-pull
/// every file it knows about, so refreshing every time a file finishes
/// would be wasteful.
const DELIVERY_INTERVAL: Duration = Duration::from_secs(3);

/// How often the orchestrator harvests finished files, updates
/// progress, and checks whether a worker has stopped making progress.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Where to report a file the pass could not analyse.
const ISSUE_URL: &str = "https://github.com/PHPantom-dev/phpantom_lsp/issues";

/// How many retired workers one pass will replace.
///
/// Retiring a worker without replacing it shrinks the pool for the rest
/// of the session, and the pool is as small as two on a four-core
/// machine: two pathological files in a row would otherwise leave every
/// file still queued behind them unchecked until the editor restarts.
/// Replacements are budgeted rather than unlimited because a worker
/// whose file never returns keeps its thread (and its
/// [`crate::PARSE_WORKER_STACK_SIZE`] stack) for the rest of the
/// session, so a project that wedges everything it is handed stops
/// adding threads instead of accumulating them.
const MAX_WORKER_REPLACEMENTS: usize = 8;

/// How long one file may occupy a worker before the pass names it in
/// the progress message, and before it gives up on it altogether.
///
/// Both are measured against the budget for a whole project: a large
/// one is meant to come in around 15 seconds even on a debug build, so
/// a single file still going after ten has hit a bug in the type engine
/// rather than merely brought a lot of code.  Giving up that early is
/// safe because it is not permanent: opening the file diagnoses it
/// through the live pipeline, and closing it recomputes its entry, so a
/// file abandoned because the machine was thrashing rather than because
/// analysis wedged comes back on its own.
///
/// The thresholds live in a struct rather than as plain constants so the
/// give-up path can be tested without waiting out the real timeout.
#[derive(Clone, Copy)]
struct FileWatchdog {
    /// How long before the progress message starts naming the file.
    notice: Duration,
    /// How long before the worker is retired and the file skipped.
    give_up: Duration,
}

impl Default for FileWatchdog {
    fn default() -> Self {
        Self {
            notice: Duration::from_secs(2),
            give_up: Duration::from_secs(10),
        }
    }
}

/// The work queue the native pass's workers drain.
struct NativeQueue {
    /// Index of the next file to claim.  Workers claim with a
    /// `fetch_add`, so each file is handed to exactly one worker.
    next: AtomicUsize,
    /// Files fully processed, whether or not they had diagnostics.
    done: AtomicUsize,
    /// Finished files waiting for the orchestrator to store and deliver.
    results: Mutex<Vec<(String, Vec<Diagnostic>)>>,
}

impl NativeQueue {
    fn new() -> Self {
        Self {
            next: AtomicUsize::new(0),
            done: AtomicUsize::new(0),
            results: Mutex::new(Vec::new()),
        }
    }
}

/// One worker's externally visible state.
///
/// The orchestrator polls this to drive progress and to notice a file
/// that has stopped making progress.  The in-flight file is published
/// as an index into the shared URI list rather than as a name, so a
/// worker allocates nothing to say what it is doing.
struct WorkerSlot {
    /// Index of the file being analysed, or [`Self::IDLE`].
    file: AtomicUsize,
    /// When that file was claimed, in milliseconds since the pass began.
    claimed_ms: AtomicU64,
    /// Set by the orchestrator to tell the worker to claim no more files.
    retired: AtomicBool,
    /// Set by the worker just before its loop exits.
    finished: AtomicBool,
}

impl WorkerSlot {
    /// The [`Self::file`] value meaning "between files".
    const IDLE: usize = usize::MAX;

    fn new() -> Self {
        Self {
            file: AtomicUsize::new(Self::IDLE),
            claimed_ms: AtomicU64::new(0),
            retired: AtomicBool::new(false),
            finished: AtomicBool::new(false),
        }
    }

    /// Publish the file this worker just claimed.
    ///
    /// The timestamp is stored before the index so a reader that sees a
    /// new index can never pair it with the previous file's claim time.
    /// The opposite pairing is possible and harmless: it under-reports
    /// how long the new file has been running, which at worst delays a
    /// verdict by one poll.
    fn claim(&self, file: usize, claimed_ms: u64) {
        self.claimed_ms.store(claimed_ms, Ordering::Relaxed);
        self.file.store(file, Ordering::Release);
    }

    fn release(&self) {
        self.file.store(Self::IDLE, Ordering::Release);
    }

    /// The file this worker is on and how long it has been on it, or
    /// `None` when it is between files.
    fn in_flight(&self, now_ms: u64) -> Option<(usize, Duration)> {
        let file = self.file.load(Ordering::Acquire);
        if file == Self::IDLE {
            return None;
        }
        let claimed = self.claimed_ms.load(Ordering::Relaxed);
        Some((file, Duration::from_millis(now_ms.saturating_sub(claimed))))
    }

    fn retire(&self) {
        self.retired.store(true, Ordering::Release);
    }

    /// Retire the worker and report whether it is still on `file`.
    ///
    /// The watchdog reads a slot and acts on it a moment later, and in
    /// between the worker can finish the file it timed out on and claim
    /// the next one.  Retiring first and confirming afterwards is what
    /// makes the answer trustworthy: once the flag is set the worker can
    /// claim nothing further, so a slot still holding the same file is
    /// genuinely stuck inside it, while any other reading means the file
    /// completed and must not be blamed.
    ///
    /// The flag stays set either way.  It cannot be taken back without
    /// racing the worker's own read of it, and a worker retired on a
    /// false alarm costs only the thread it is replaced with.
    fn retire_on(&self, file: usize) -> bool {
        self.retire();
        self.file.load(Ordering::Acquire) == file
    }

    fn is_retired(&self) -> bool {
        self.retired.load(Ordering::Acquire)
    }

    fn finish(&self) {
        self.finished.store(true, Ordering::Release);
    }

    fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }
}

/// What a native workspace pass got through.
#[derive(Default)]
pub(crate) struct NativePassOutcome {
    /// Files the pass set out to diagnose.
    total: usize,
    /// Files fully diagnosed.
    diagnosed: usize,
    /// Files the pass gave up on, named for a user-facing message.  Each
    /// is a file whose analysis did not finish, so this pass produced
    /// nothing for it.
    skipped: Vec<String>,
}

impl NativePassOutcome {
    /// Files the pass never got to at all.
    ///
    /// Non-zero only when every worker was retired past the pass's
    /// replacement budget (or none could be spawned) with files still
    /// queued, which is the one case where the pass stops short of the
    /// whole project.
    fn unchecked(&self) -> usize {
        self.total
            .saturating_sub(self.diagnosed)
            .saturating_sub(self.skipped.len())
    }

    /// The one-line summary for the progress end message.
    fn summary(&self) -> String {
        let mut summary = format!("Diagnosed {} files", self.diagnosed);
        if !self.skipped.is_empty() {
            summary.push_str(&format!(", {} timed out", self.skipped.len()));
        }
        let unchecked = self.unchecked();
        if unchecked > 0 {
            summary.push_str(&format!(", {unchecked} not checked"));
        }
        summary
    }
}

/// The state one native pass shares with its workers, and everything
/// needed to spawn another one mid-pass.
struct NativePass<F> {
    /// The files to diagnose, indexed by the slot's in-flight file.
    uris: Arc<Vec<String>>,
    /// The queue every worker drains.
    queue: Arc<NativeQueue>,
    shutdown: Arc<AtomicBool>,
    /// Cancels this pass specifically, separately from `shutdown` (the
    /// whole session ending). Set when a live config reload switches
    /// `[diagnostics] workspace` off mid-pass.
    cancel: Arc<AtomicBool>,
    /// When the pass began; claim times are measured against it.
    started: Instant,
    /// The per-file analysis, shared by every worker.
    work: Arc<F>,
    /// One slot per worker spawned, retired ones included, in spawn
    /// order.  A worker only ever appends to this, so a slot's index is
    /// stable for the life of the pass.
    slots: Vec<Arc<WorkerSlot>>,
    /// Replacements spawned so far, capped at [`MAX_WORKER_REPLACEMENTS`].
    replacements: usize,
}

impl<F> NativePass<F>
where
    F: Fn(&str) -> Option<Vec<Diagnostic>> + Send + Sync + 'static,
{
    fn new(
        uris: Vec<String>,
        shutdown: Arc<AtomicBool>,
        cancel: Arc<AtomicBool>,
        work: Arc<F>,
    ) -> Self {
        Self {
            uris: Arc::new(uris),
            queue: Arc::new(NativeQueue::new()),
            shutdown,
            cancel,
            started: Instant::now(),
            work,
            slots: Vec::new(),
            replacements: 0,
        }
    }

    /// Spawn the pass's initial pool.
    fn spawn_workers(&mut self, count: usize) {
        for _ in 0..count {
            self.spawn_worker();
        }
    }

    /// Spawn one worker that drains the queue by running the pass's work
    /// over each file it claims, and record its slot.
    ///
    /// The workers are detached rather than scoped because a worker that
    /// wedges on one file must not be able to hold up the rest of the
    /// scan.  A scope has to join every thread it spawned, so a single
    /// file whose analysis never returns would block the whole pass, and
    /// with it every file still queued behind it, for the rest of the
    /// session.  A detached worker can instead be retired by the
    /// orchestrator, which stops counting it and carries on.  Nothing
    /// can kill the wedged thread from outside, so it keeps running
    /// until its file finishes (if it ever does), then sees the flag and
    /// exits without claiming another.
    ///
    /// Each worker gets a [`crate::PARSE_WORKER_STACK_SIZE`] stack
    /// because it parses and walks PHP ASTs.  A worker that fails to
    /// spawn reports itself finished, so the pass runs with the workers
    /// it did get rather than failing outright.
    fn spawn_worker(&mut self) {
        let id = self.slots.len();
        let slot = Arc::new(WorkerSlot::new());
        let worker = Arc::clone(&slot);
        let uris = Arc::clone(&self.uris);
        let queue = Arc::clone(&self.queue);
        let shutdown = Arc::clone(&self.shutdown);
        let cancel = Arc::clone(&self.cancel);
        let work = Arc::clone(&self.work);
        let started = self.started;
        let spawned = std::thread::Builder::new()
            .name(format!("ws-diag-worker-{id}"))
            .stack_size(crate::PARSE_WORKER_STACK_SIZE)
            .spawn(move || {
                while !worker.is_retired()
                    && !shutdown.load(Ordering::Acquire)
                    && !cancel.load(Ordering::Acquire)
                {
                    let file = queue.next.fetch_add(1, Ordering::Relaxed);
                    if file >= uris.len() {
                        break;
                    }
                    worker.claim(file, started.elapsed().as_millis() as u64);
                    let diags = work(&uris[file]);
                    worker.release();
                    if let Some(diags) = diags {
                        queue.results.lock().push((uris[file].clone(), diags));
                    }
                    queue.done.fetch_add(1, Ordering::Relaxed);
                }
                worker.finish();
            });
        match spawned {
            // Dropping the handle detaches the thread, which is what
            // lets a wedged one be abandoned.
            Ok(_) => {}
            Err(err) => {
                tracing::warn!("PHPantom: workspace diagnostics worker {id} not spawned: {err}");
                slot.finish();
            }
        }
        self.slots.push(slot);
    }

    /// Take over from a worker the watchdog just retired, so the pool
    /// keeps the size it started with.
    ///
    /// Returns `false` once the pass has spent its replacement budget,
    /// from which point retiring a worker does shrink the pool.
    fn replace_retired_worker(&mut self) -> bool {
        if self.replacements >= MAX_WORKER_REPLACEMENTS {
            return false;
        }
        self.replacements += 1;
        self.spawn_worker();
        true
    }
}

/// Diagnostics for files that are not open in the editor.
///
/// Native results and each external tool's results are stored
/// separately so they can be updated independently; [`Self::merged`]
/// combines them per file.  Every update bumps a per-URI result id
/// (drawn from a session-global sequence) so the pull handler can
/// answer `Unchanged` cheaply.  Result ids are formatted as `ws{n}`,
/// which cannot collide with the numeric per-open-file result ids.
#[derive(Default)]
pub(crate) struct WorkspaceDiagnostics {
    /// Native diagnostics per file URI (only non-empty sets are kept).
    native: HashMap<String, Vec<Diagnostic>>,
    /// External tool diagnostics per source name per file URI.
    external: HashMap<&'static str, HashMap<String, Vec<Diagnostic>>>,
    /// Per-URI result id for pull `Unchanged` support.  A URI stays in
    /// this map once reported, even after its diagnostics clear, so the
    /// handler keeps reporting the (now empty) set until session end.
    result_ids: HashMap<String, u64>,
    /// Session-global sequence feeding `result_ids`.
    seq: u64,
    /// The result id last delivered to the client for a URI, via either
    /// a `Full` or an `Unchanged` report. Lets [`Self::diff_for_pull`]
    /// answer `Unchanged` on its own record when the client never
    /// echoes an id back (see its docs).
    delivered: HashMap<String, String>,
}

impl WorkspaceDiagnostics {
    /// Bump the result id for a URI (marks it as updated).
    fn bump(&mut self, uri: &str) {
        self.seq += 1;
        self.result_ids.insert(uri.to_string(), self.seq);
    }

    /// Store the native diagnostics for a file.  Returns `true` when
    /// the stored state changed (and the editor should be notified).
    pub(crate) fn set_native(&mut self, uri: &str, diags: Vec<Diagnostic>) -> bool {
        if diags.is_empty() {
            // Nothing stored and nothing new — the file was never
            // reported, so there is nothing to clear.
            if self.native.remove(uri).is_none() && !self.result_ids.contains_key(uri) {
                return false;
            }
        } else {
            self.native.insert(uri.to_string(), diags);
        }
        self.bump(uri);
        true
    }

    /// Replace one external tool's entire result set.  Returns the URIs
    /// whose diagnostics actually changed (old entries cleared by the new
    /// run are included so the editor drops them); URIs whose diagnostics
    /// are byte-for-byte identical between runs are left untouched so a
    /// re-run that changes nothing doesn't invalidate the client's cache
    /// for the whole project.
    pub(crate) fn set_external(
        &mut self,
        source: &'static str,
        results: HashMap<String, Vec<Diagnostic>>,
    ) -> Vec<String> {
        let entry = self.external.entry(source).or_default();
        let mut all: HashSet<&String> = entry.keys().collect();
        all.extend(results.keys());
        let updated: Vec<String> = all
            .into_iter()
            .filter(|uri| entry.get(*uri) != results.get(*uri))
            .cloned()
            .collect();
        *entry = results;
        for uri in &updated {
            self.bump(uri);
        }
        updated
    }

    /// Store one external tool's diagnostics for a single file (used
    /// when migrating live per-file results on `did_close`).
    pub(crate) fn set_external_for_uri(
        &mut self,
        source: &'static str,
        uri: &str,
        diags: Vec<Diagnostic>,
    ) {
        let entry = self.external.entry(source).or_default();
        if diags.is_empty() {
            if entry.remove(uri).is_none() {
                return;
            }
        } else {
            entry.insert(uri.to_string(), diags);
        }
        self.bump(uri);
    }

    /// Merge all sources for a file into one diagnostic set.
    ///
    /// Imprecise full-line diagnostics (external tools) are suppressed
    /// when a precise native diagnostic covers the same line, matching
    /// the behaviour of the live per-file pipeline.
    pub(crate) fn merged(&self, uri: &str) -> Vec<Diagnostic> {
        let mut out = self.native.get(uri).cloned().unwrap_or_default();
        for map in self.external.values() {
            if let Some(diags) = map.get(uri) {
                out.extend(diags.iter().cloned());
            }
        }
        super::suppression::suppress_imprecise_overlaps(&mut out);
        out
    }

    /// The result id for a URI, formatted for the wire (`ws{n}`).
    pub(crate) fn result_id(&self, uri: &str) -> Option<String> {
        self.result_ids.get(uri).map(|n| format!("ws{n}"))
    }

    /// All URIs that have ever been reported (still tracked so cleared
    /// sets keep being reported as empty).
    pub(crate) fn tracked_uris(&self) -> Vec<String> {
        self.result_ids.keys().cloned().collect()
    }

    /// Clear every stored native and external result, e.g. when
    /// `[diagnostics] workspace` is switched off mid-session.
    ///
    /// Keeps the result-id bookkeeping (so `tracked_uris` still lists
    /// these URIs and a pull client that asks again is told they are
    /// now empty) rather than dropping the struct back to its default.
    /// Returns the URIs whose stored diagnostics were non-empty and so
    /// need republishing to the editor.
    pub(crate) fn clear_all(&mut self) -> Vec<String> {
        let mut changed: HashSet<String> = self.native.keys().cloned().collect();
        for map in self.external.values() {
            changed.extend(map.keys().cloned());
        }
        self.native.clear();
        self.external.clear();
        for uri in &changed {
            self.bump(uri);
        }
        changed.into_iter().collect()
    }

    /// Decide whether to answer `Unchanged` or resend full diagnostics
    /// for a tracked `uri`, given the id (if any) the client echoed back
    /// as its previous result for this URI. Returns the current result
    /// id plus `Some(diagnostics)` when a full report is needed, or
    /// `None` when `Unchanged` suffices.
    ///
    /// The client's echoed id is trusted first. Otherwise this falls
    /// back to the id it last delivered itself: some clients (editors
    /// with monorepo path dependencies outside the opened project) have
    /// nowhere to store diagnostics for a file outside their workspace
    /// root and so never echo an id for it, which would otherwise force
    /// a full re-serialization of that file on every single pull for
    /// the rest of the session. Reusing the server's own delivery record
    /// bounds that cost to one full report per change instead.
    pub(crate) fn diff_for_pull(
        &mut self,
        uri: &str,
        client_previous: Option<&str>,
    ) -> (String, Option<Vec<Diagnostic>>) {
        let result_id = self
            .result_id(uri)
            .expect("diff_for_pull called with an untracked uri");
        let unchanged = client_previous == Some(result_id.as_str())
            || self.delivered.get(uri) == Some(&result_id);
        self.delivered.insert(uri.to_string(), result_id.clone());
        let diags = if unchanged {
            None
        } else {
            Some(self.merged(uri))
        };
        (result_id, diags)
    }
}

impl Backend {
    /// Wait until the client has sent its first `workspace/diagnostic`
    /// pull.  Returns immediately if one has already arrived, or when
    /// the server shuts down (the caller's pass is a no-op then).
    ///
    /// In pull mode the background workspace pass is gated on this:
    /// its results are only deliverable through workspace pull
    /// responses, so computing them before the client has ever asked
    /// is wasted work — and a client that never pulls never pays.
    pub(crate) async fn wait_for_first_workspace_pull(&self) {
        if self.diag.workspace_pull_seen.load(Ordering::Acquire) {
            return;
        }
        loop {
            self.diag.workspace_pull_notify.notified().await;
            if self.diag.workspace_pull_seen.load(Ordering::Acquire)
                || self.shutdown_flag.load(Ordering::Acquire)
            {
                return;
            }
        }
    }

    /// Start the pass when a live config reload has just switched it on.
    ///
    /// The full background index runs the pass from its own tail, and
    /// gives up on it when `[diagnostics] workspace` was off at the time,
    /// so without this the setting only took effect on the next restart.
    /// Called from [`Backend::reload_config`], which covers a project's
    /// own `.phpantom.toml` and the polled global config alike.
    ///
    /// Nothing happens while the index is still running: its tail reads
    /// the setting again when it gets there and starts the pass itself.
    /// The same is true of a workspace indexed under another strategy —
    /// the pass reads the whole index, so it only ever runs behind a
    /// `full` one.
    pub(crate) fn start_workspace_diagnostics_on_reload(&self) {
        if self.client.is_none() || !self.workspace_diagnostics_due_after_reload() {
            return;
        }
        // `reload_config` is synchronous, and runs on a blocking thread
        // for a watched-file batch and on the runtime for the global
        // config poller.  Both carry the runtime context; a unit test
        // calling it directly does not, and has no pass to start.
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let backend = self.clone_for_diagnostic_worker();
        runtime.spawn(async move {
            // The same gating the index tail applies: a pull client's
            // results are only deliverable through workspace pulls, so a
            // client that has never pulled does not pay for the scan.
            if backend.supports_pull_diagnostics.load(Ordering::Acquire) {
                backend.wait_for_first_workspace_pull().await;
                if backend.shutdown_flag.load(Ordering::Acquire) {
                    return;
                }
            }
            backend.run_workspace_diagnostics().await;
        });
    }

    /// Stop the pass and drop its results when a live config reload has
    /// just switched `[diagnostics] workspace` off.
    ///
    /// Cancels a pass still running (both `drive_native_pass` and its
    /// workers check `workspace_diag_cancel`), marks the pass as no
    /// longer active so [`Self::start_workspace_diagnostics_on_reload`]
    /// can start a fresh one if the setting is switched back on later,
    /// and clears the stored results so the editor drops what it was
    /// shown instead of carrying it for the rest of the session.
    pub(crate) fn stop_workspace_diagnostics_on_reload(&self) {
        if self.config().diagnostics.workspace_enabled() {
            return;
        }
        if !self
            .diag
            .workspace_diag_pass_started
            .swap(false, Ordering::AcqRel)
        {
            // Wasn't active, so there is nothing running to cancel and
            // nothing stored to clear.
            return;
        }
        self.diag
            .workspace_diag_cancel
            .store(true, Ordering::Release);

        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let backend = self.clone_for_diagnostic_worker();
        runtime.spawn(async move {
            backend.clear_workspace_diagnostics().await;
        });
    }

    /// Clear every stored workspace diagnostic and tell the editor to
    /// drop what it was shown.  Used when the setting is switched off
    /// mid-session; see [`Self::stop_workspace_diagnostics_on_reload`].
    async fn clear_workspace_diagnostics(&self) {
        let changed = self.diag.workspace_diags.lock().clear_all();
        self.flush_workspace_diag_updates(changed).await;
    }

    /// Whether a config reload has left the workspace pass owed a run.
    ///
    /// Split out from [`Self::start_workspace_diagnostics_on_reload`] so
    /// the conditions can be checked without a client to deliver to.
    fn workspace_diagnostics_due_after_reload(&self) -> bool {
        let config = self.config();
        config.diagnostics.workspace_enabled()
            && config.indexing.strategy() == crate::config::IndexingStrategy::Full
            && self.workspace.workspace_root.read().is_some()
            && !self
                .diag
                .workspace_diag_pass_started
                .load(Ordering::Acquire)
            // The pass reads the whole index, so it waits for the full
            // background index to have finished.  While that is still
            // running, its own tail re-reads the setting and starts the
            // pass itself.
            && self.workspace_indexed.load(Ordering::Acquire)
            && !self.full_index_in_progress.load(Ordering::Acquire)
    }

    /// Whether the running workspace pass has been told to stop, either
    /// because the whole session is shutting down or because a live
    /// config reload just switched `[diagnostics] workspace` off.
    fn workspace_pass_stopping(&self) -> bool {
        self.shutdown_flag.load(Ordering::Acquire)
            || self.diag.workspace_diag_cancel.load(Ordering::Acquire)
    }

    /// Run the background workspace diagnostics pass.
    ///
    /// Called from the tail of the full background index task, so the
    /// whole workspace is already parsed when this starts, and from
    /// [`Self::start_workspace_diagnostics_on_reload`] when the setting
    /// is switched on mid-session.  Guarded against duplicate
    /// invocation; waits for `init_complete` so the post-index cache
    /// clears in `initialized` cannot race the pass.
    pub(crate) async fn run_workspace_diagnostics(&self) {
        if !self.config().diagnostics.workspace_enabled() {
            return;
        }
        if self.workspace.workspace_root.read().is_none() {
            return;
        }
        if self
            .diag
            .workspace_diag_pass_started
            .swap(true, Ordering::AcqRel)
        {
            return;
        }
        // A previous run may have been cancelled by the setting being
        // switched off mid-pass; clear that before this fresh one starts.
        self.diag
            .workspace_diag_cancel
            .store(false, Ordering::Release);

        // Wait for `initialized` to finish (it clears resolution caches
        // after the startup scan; starting before that would waste the
        // eager class population below).
        if !self.wait_for_init_complete().await {
            return;
        }

        let progress_token = self.progress_create("phpantom/workspace-diagnostics").await;
        if let Some(ref tok) = progress_token {
            self.progress_begin(
                tok,
                "PHPantom: Workspace diagnostics",
                Some("Starting".to_string()),
            )
            .await;
        }
        let progress = ScanProgress::new();
        let poller = progress_token
            .as_ref()
            .map(|tok| self.spawn_progress_poller(tok.clone(), Arc::clone(&progress)));

        let outcome = self.run_native_workspace_pass(&progress).await;

        // Stopping short of the whole project is worth saying out loud:
        // the files it gave up on were logged as they happened, but the
        // ones behind them in the queue were never looked at, and a
        // silently short scan is what makes this look like a scan that
        // simply stopped.  A shutdown or the setting being switched off
        // cuts the pass off by design, so it is not reported either way.
        if outcome.unchecked() > 0 && !self.workspace_pass_stopping() {
            self.log(
                MessageType::WARNING,
                format!(
                    "Workspace diagnostics stopped with {} of {} files unchecked, after giving \
                     up on every file it had in hand. Those files keep the diagnostics they \
                     already had, if any.",
                    outcome.unchecked(),
                    outcome.total
                ),
            )
            .await;
        }

        if self.config().diagnostics.workspace_external_enabled() && !self.workspace_pass_stopping()
        {
            self.run_workspace_external_tools(&progress).await;
        }

        if let Some(poller) = poller {
            poller.finish().await;
        }
        if let Some(ref tok) = progress_token {
            self.progress_end(tok, Some(outcome.summary())).await;
        }
    }

    /// Run the native collectors over every unopened user file.
    ///
    /// Results stream to the editor as workers finish files, throttled
    /// by [`DELIVERY_INTERVAL`].
    pub(crate) async fn run_native_workspace_pass(
        &self,
        progress: &ScanProgress,
    ) -> NativePassOutcome {
        // ── Eager class population ──────────────────────────────────
        // Resolve every known class in dependency-first order so the
        // per-file collectors below hit a warm cache instead of
        // recursing into class resolution.  The full-index task already
        // ran this; classes it populated are skipped, so this only
        // resolves classes loaded since (or everything, if the pass was
        // triggered another way).
        progress.set_percentage(1, "Resolving classes");
        self.eager_populate_resolved_classes().await;

        let mut uris = self.workspace_diagnostic_target_uris();
        uris.sort();
        if uris.is_empty() {
            return NativePassOutcome::default();
        }

        let ignore_rules = ignore_rules::compile_ignore_rules(&self.config().diagnostics.ignore);
        let backend = self.clone_for_blocking();
        let work = Arc::new(move |uri: &str| {
            backend.collect_workspace_file_diagnostics(uri, &ignore_rules)
        });

        // Half the available cores, so interactive requests (hover,
        // completion) stay responsive while the pass runs in the
        // background.  Never fewer than two: giving up on a file retires
        // the worker that was on it, and a pool of one would take the
        // rest of the queue down with it.
        let workers = std::thread::available_parallelism()
            .map(|n| (n.get() / 2).max(2))
            .unwrap_or(2)
            .min(uris.len());

        let mut pass = NativePass::new(
            uris,
            Arc::clone(&self.shutdown_flag),
            Arc::clone(&self.diag.workspace_diag_cancel),
            work,
        );
        pass.spawn_workers(workers);

        self.drive_native_pass(&mut pass, FileWatchdog::default(), progress)
            .await
    }

    /// Poll the running workers until each has finished or been retired,
    /// storing and delivering results as they arrive.
    ///
    /// This is where the pass survives a file it cannot get through.  A
    /// worker that has spent [`FileWatchdog::give_up`] on one file is
    /// retired: the file is recorded as skipped and named in the log,
    /// the worker claims nothing further, a replacement takes its place
    /// (see [`NativePass::replace_retired_worker`]) and the pass carries
    /// on with the rest of the queue rather than waiting on it forever.
    /// A file merely past [`FileWatchdog::notice`] is named in the
    /// progress message first, so slow analysis is visible as slow
    /// analysis instead of looking like a scan that died.
    ///
    /// Progress is reported per file rather than per batch of them, so
    /// the count keeps moving while the pass does.
    async fn drive_native_pass<F>(
        &self,
        pass: &mut NativePass<F>,
        watchdog: FileWatchdog,
        progress: &ScanProgress,
    ) -> NativePassOutcome
    where
        F: Fn(&str) -> Option<Vec<Diagnostic>> + Send + Sync + 'static,
    {
        let total = pass.uris.len();
        let mut outcome = NativePassOutcome {
            total,
            ..Default::default()
        };
        // Indexed like `pass.slots`, and grown with it: a worker is only
        // counted retired once the watchdog has confirmed it was stuck on
        // the file it timed out.
        let mut retired = vec![false; pass.slots.len()];
        let mut budget_reported = false;
        let mut pending_updates: Vec<String> = Vec::new();
        let mut last_delivery = Instant::now();
        let mut last_message = String::new();

        loop {
            // Read this before harvesting: a worker pushes its result
            // before marking itself finished, so a harvest that follows
            // an all-stopped reading is guaranteed to see every result.
            let all_stopped = pass
                .slots
                .iter()
                .enumerate()
                .all(|(id, slot)| retired[id] || slot.is_finished());
            self.store_native_results(&pass.queue, &mut pending_updates);

            // ── Watchdog ────────────────────────────────────────────
            let now_ms = pass.started.elapsed().as_millis() as u64;
            let mut longest: Option<(usize, Duration)> = None;
            for id in 0..pass.slots.len() {
                if retired[id] {
                    continue;
                }
                let Some((file, elapsed)) = pass.slots[id].in_flight(now_ms) else {
                    continue;
                };
                if elapsed < watchdog.give_up {
                    if longest.is_none_or(|(_, worst)| elapsed > worst) {
                        longest = Some((file, elapsed));
                    }
                    continue;
                }
                let name = self.message_path(&pass.uris[file]);
                let secs = elapsed.as_secs();
                if pass.slots[id].retire_on(file) {
                    retired[id] = true;
                    tracing::warn!(
                        "PHPantom: workspace diagnostics gave up on {name} after {secs}s"
                    );
                    self.log(
                        MessageType::WARNING,
                        format!(
                            "Workspace diagnostics gave up on {name} after {secs}s and moved on \
                             to the rest of the project. Open the file to have it diagnosed on \
                             its own. Please report it at {ISSUE_URL}"
                        ),
                    )
                    .await;
                    outcome.skipped.push(name);
                } else {
                    // The worker finished the file as the deadline
                    // passed, so it is not a file the pass gave up on.
                    // It is still retired (the flag cannot be taken
                    // back), and it stays uncounted here so the pass
                    // waits for whatever it claimed next.
                    tracing::debug!(
                        "PHPantom: workspace diagnostics worker {id} finished {name} as its \
                         {secs}s deadline passed"
                    );
                }
                if !pass.replace_retired_worker() && !budget_reported {
                    budget_reported = true;
                    tracing::warn!(
                        "PHPantom: workspace diagnostics has replaced {MAX_WORKER_REPLACEMENTS} \
                         workers and will run with a smaller pool from here"
                    );
                }
                retired.resize(pass.slots.len(), false);
            }

            let done = pass.queue.done.load(Ordering::Relaxed);
            outcome.diagnosed = done;
            let message = match longest {
                Some((file, elapsed)) if elapsed >= watchdog.notice => format!(
                    "Checking files ({done}/{total}), still analysing {} ({}s)",
                    self.message_path(&pass.uris[file]),
                    elapsed.as_secs()
                ),
                _ => format!("Checking files ({done}/{total})"),
            };
            // The native pass maps into 1..80 of the progress bar; the
            // external tool runs that follow use the remaining 80..100.
            // Reporting only on a changed message keeps the poller from
            // sending a notification every poll, since this loop runs
            // far more often than a file finishes.
            if message != last_message {
                progress.set_percentage((1 + done * 79 / total) as u32, message.clone());
                last_message = message;
            }

            let stopping = all_stopped || self.workspace_pass_stopping();
            if !pending_updates.is_empty()
                && (stopping || last_delivery.elapsed() >= DELIVERY_INTERVAL)
            {
                self.flush_workspace_diag_updates(std::mem::take(&mut pending_updates))
                    .await;
                last_delivery = Instant::now();
            }
            if stopping {
                return outcome;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// Move finished files out of the queue into the workspace store,
    /// collecting the URIs whose diagnostics changed.
    ///
    /// Files opened while the pass ran are dropped: the live per-file
    /// pipeline owns them now.
    fn store_native_results(&self, queue: &NativeQueue, updated: &mut Vec<String>) {
        let finished = std::mem::take(&mut *queue.results.lock());
        if finished.is_empty() {
            return;
        }
        let open = self.open_files.read();
        let mut ws = self.diag.workspace_diags.lock();
        for (uri, diags) in finished {
            if open.contains_key(&uri) {
                continue;
            }
            if ws.set_native(&uri, diags) {
                updated.push(uri);
            }
        }
    }

    /// A file's workspace-relative path for a progress or log message,
    /// falling back to the URI when it sits outside the workspace root.
    fn message_path(&self, uri: &str) -> String {
        self.workspace_relative_path(uri)
            .unwrap_or_else(|| uri.to_string())
    }

    /// The URIs the workspace pass should diagnose: every parsed user
    /// file that is not a stub, not under a vendor directory, and not
    /// currently open in the editor.
    fn workspace_diagnostic_target_uris(&self) -> Vec<String> {
        let vendor_prefixes = self.workspace.vendor_uri_prefixes.lock().clone();
        let open_uris: HashSet<String> = self.open_files.read().keys().cloned().collect();
        let maps = self.symbol_maps.read();
        maps.keys()
            .filter(|uri| {
                !open_uris.contains(uri.as_str())
                    && !uri.starts_with("phpantom-stub://")
                    && !uri.starts_with("phpantom-stub-fn://")
                    && !vendor_prefixes.iter().any(|p| uri.starts_with(p.as_str()))
            })
            .cloned()
            .collect()
    }

    /// Compute the full native diagnostic set for one file from disk.
    ///
    /// Runs the same fast + slow collectors as the live per-file
    /// pipeline and applies the same post-processing (overlap
    /// suppression, `@phpantom-ignore` comments, config ignore rules).
    /// Returns `None` when the file cannot be read or the collectors
    /// panic.
    pub(crate) fn collect_workspace_file_diagnostics(
        &self,
        uri: &str,
        ignore_rules: &[CompiledIgnoreRule],
    ) -> Option<Vec<Diagnostic>> {
        let content = self.get_file_content(uri)?;
        // Blade files are diagnosed on their preprocessed virtual PHP
        // content (produced by `update_ast` during indexing).
        let content_view = self.analysable_content_or(uri, &content);
        let effective: &str = &content_view;

        crate::util::catch_panic_unwind_safe("workspace_diagnostics", uri, None, || {
            let _parse_guard = crate::parser::with_parse_cache(effective);
            let _cache_guard = crate::virtual_members::with_active_resolved_class_cache(
                &self.resolved_class_cache,
            );

            let mut out = Vec::new();
            self.collect_fast_diagnostics(uri, effective, &mut out);
            self.collect_slow_diagnostics(uri, effective, &mut out);

            super::suppression::suppress_imprecise_overlaps(&mut out);
            super::suppression::filter_ignored_by_comment(&mut out, effective);
            if !ignore_rules.is_empty()
                && let Some(relative) = self.workspace_relative_path(uri)
            {
                ignore_rules::filter_ignored_by_config(&mut out, &relative, ignore_rules);
            }
            out
        })
    }

    /// The `/`-separated path of `uri` relative to the workspace root, for
    /// matching `[[diagnostics.ignore]]` path globs and for naming a file in
    /// hover text.
    pub(crate) fn workspace_relative_path(&self, uri: &str) -> Option<String> {
        let path = uri
            .parse::<tower_lsp::lsp_types::Url>()
            .ok()?
            .to_file_path()
            .ok()?;
        let root = self.workspace.workspace_root.read().clone()?;
        Some(
            path.strip_prefix(&root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/"),
        )
    }

    /// Deliver updated workspace diagnostics to the editor.
    ///
    /// Pull mode: one `workspace/diagnostic/refresh` covers all
    /// updates.  Push mode: publish each file's merged set directly
    /// (skipping files that opened since the update was recorded —
    /// those are owned by the live pipeline).
    pub(crate) async fn flush_workspace_diag_updates(&self, updated: Vec<String>) {
        if updated.is_empty() {
            return;
        }
        let Some(client) = &self.client else {
            return;
        };

        if self
            .supports_pull_diagnostics
            .load(std::sync::atomic::Ordering::Acquire)
        {
            self.request_diagnostic_refresh().await;
            return;
        }

        for uri_str in updated {
            if self.open_files.read().contains_key(&uri_str) {
                continue;
            }
            let Ok(uri) = uri_str.parse::<tower_lsp::lsp_types::Url>() else {
                continue;
            };
            let diags = self.diag.workspace_diags.lock().merged(&uri_str);
            client.publish_diagnostics(uri, diags, None).await;
        }
    }

    /// Recompute one file's workspace diagnostics from disk.
    ///
    /// Called after `did_close` so the closed file's entry reflects the
    /// on-disk state instead of the startup snapshot.  A file that can
    /// no longer be read (deleted) has its entry cleared.
    pub(crate) async fn recompute_workspace_diags_for_closed_file(&self, uri: &str) {
        if !self
            .diag
            .workspace_diag_pass_started
            .load(Ordering::Acquire)
        {
            return;
        }
        // Only user files participate in workspace diagnostics.
        let vendor_prefixes = self.workspace.vendor_uri_prefixes.lock().clone();
        if uri.starts_with("phpantom-stub")
            || vendor_prefixes.iter().any(|p| uri.starts_with(p.as_str()))
        {
            return;
        }

        let backend = self.clone_for_blocking();
        let uri_owned = uri.to_string();
        let diags =
            crate::server::run_blocking_cancel_safe("workspace file diagnostics", move || {
                let rules =
                    ignore_rules::compile_ignore_rules(&backend.config().diagnostics.ignore);
                backend.collect_workspace_file_diagnostics(&uri_owned, &rules)
            })
            .await
            .flatten()
            .unwrap_or_default();

        // The file may have been reopened while we were computing; the
        // live pipeline owns it again in that case.
        if self.open_files.read().contains_key(uri) {
            return;
        }

        let changed = self.diag.workspace_diags.lock().set_native(uri, diags);
        if changed {
            self.flush_workspace_diag_updates(vec![uri.to_string()])
                .await;
        }
    }

    // ── External tools ──────────────────────────────────────────────

    /// Run each enabled external tool once over the whole project and
    /// store the results, delivering after each tool completes.
    async fn run_workspace_external_tools(&self, progress: &ScanProgress) {
        let Some(root) = self.workspace.workspace_root.read().clone() else {
            return;
        };
        let config = self.config();
        let composer_pkg = crate::composer::read_composer_package(&root);
        let bin_dir: Option<String> = composer_pkg.as_ref().map(crate::composer::get_bin_dir);

        // ── PHPStan ─────────────────────────────────────────────────
        if !config.phpstan.is_disabled()
            && crate::phpstan::has_project_config(&root)
            && let Some(resolved) = crate::phpstan::resolve_phpstan(
                Some(&root),
                &config.phpstan,
                bin_dir.as_deref(),
                composer_pkg.as_ref(),
            )
        {
            progress.set_percentage(80, "Running PHPStan (project-wide)");
            let phpstan_config = config.phpstan.clone();
            let shutdown = Arc::clone(&self.shutdown_flag);
            let root_clone = root.clone();
            let generations = self.phpstan_tool.generation_snapshot();
            let result = crate::server::run_blocking_cancel_safe("workspace phpstan", move || {
                crate::phpstan::run_phpstan_workspace(
                    &resolved,
                    &root_clone,
                    &phpstan_config,
                    &shutdown,
                )
            })
            .await;
            if let Some(Ok(map)) = result {
                self.store_workspace_external_results("phpstan", map, generations)
                    .await;
            }
        }

        if self.workspace_pass_stopping() {
            return;
        }

        // ── PHPCS ───────────────────────────────────────────────────
        if !config.phpcs.is_disabled()
            && crate::phpcs::has_project_config(&root)
            && let Some(resolved) =
                crate::phpcs::resolve_phpcs(Some(&root), &config.phpcs, bin_dir.as_deref())
        {
            progress.set_percentage(85, "Running PHPCS (project-wide)");
            let phpcs_config = config.phpcs.clone();
            let shutdown = Arc::clone(&self.shutdown_flag);
            let root_clone = root.clone();
            let generations = self.phpcs_tool.generation_snapshot();
            let result = crate::server::run_blocking_cancel_safe("workspace phpcs", move || {
                crate::phpcs::run_phpcs_workspace(&resolved, &root_clone, &phpcs_config, &shutdown)
            })
            .await;
            if let Some(Ok(map)) = result {
                self.store_workspace_external_results("phpcs", map, generations)
                    .await;
            }
        }

        if self.workspace_pass_stopping() {
            return;
        }

        // ── Mago lint + analyze ─────────────────────────────────────
        let laravel = composer_pkg
            .as_ref()
            .is_some_and(crate::composer::is_laravel_project);
        let mago_services = crate::mago::enabled_services(&root, &config.mago, laravel);
        if !mago_services.none_enabled()
            && let Some(resolved) = crate::mago::resolve_mago(
                Some(&root),
                &config.mago,
                bin_dir.as_deref(),
                composer_pkg.as_ref(),
            )
        {
            if mago_services.lint {
                progress.set_percentage(90, "Running Mago lint (project-wide)");
                let mago_config = config.mago.clone();
                let shutdown = Arc::clone(&self.shutdown_flag);
                let root_clone = root.clone();
                let resolved_clone = resolved.clone();
                let generations = self.mago_lint_tool.generation_snapshot();
                let result =
                    crate::server::run_blocking_cancel_safe("workspace mago lint", move || {
                        crate::mago::run_mago_lint_workspace(
                            &resolved_clone,
                            &root_clone,
                            &mago_config,
                            &shutdown,
                        )
                    })
                    .await;
                if let Some(Ok(map)) = result {
                    self.store_workspace_external_results("mago-lint", map, generations)
                        .await;
                }

                if self.workspace_pass_stopping() {
                    return;
                }
            }

            if mago_services.analyze {
                progress.set_percentage(95, "Running Mago analyze (project-wide)");
                let mago_config = config.mago.clone();
                let shutdown = Arc::clone(&self.shutdown_flag);
                let root_clone = root.clone();
                let generations = self.mago_analyze_tool.generation_snapshot();
                let result =
                    crate::server::run_blocking_cancel_safe("workspace mago analyze", move || {
                        crate::mago::run_mago_analyze_workspace(
                            &resolved,
                            &root_clone,
                            &mago_config,
                            &shutdown,
                        )
                    })
                    .await;
                if let Some(Ok(map)) = result {
                    self.store_workspace_external_results("mago-analyze", map, generations)
                        .await;
                }
            }
        }
    }

    /// The single-file worker that shares a source's per-file cache.
    fn external_tool_for_source(&self, source: &str) -> Option<&crate::ExternalToolWorker> {
        Some(match source {
            "phpstan" => &self.phpstan_tool,
            "phpcs" => &self.phpcs_tool,
            "mago-lint" => &self.mago_lint_tool,
            "mago-analyze" => &self.mago_analyze_tool,
            _ => return None,
        })
    }

    /// Store a project-wide external tool run's results and deliver.
    ///
    /// Results for files currently open feed the live per-file source
    /// caches instead (so the open buffer shows them immediately);
    /// everything else goes into the workspace store.  Config ignore
    /// rules are applied per file.
    ///
    /// `generations` is the source's write-counter snapshot taken before
    /// the run started; any open file the single-file worker has written
    /// to since then keeps that fresher result.
    async fn store_workspace_external_results(
        &self,
        source: &'static str,
        results: HashMap<PathBuf, Vec<Diagnostic>>,
        generations: HashMap<String, u64>,
    ) {
        let Some(worker) = self.external_tool_for_source(source) else {
            return;
        };
        let cache = &worker.last_diags;

        let rules = ignore_rules::compile_ignore_rules(&self.config().diagnostics.ignore);
        let root = self.workspace.workspace_root.read().clone();

        let mut workspace_results: HashMap<String, Vec<Diagnostic>> = HashMap::new();
        let mut open_results: HashMap<String, Vec<Diagnostic>> = HashMap::new();
        {
            let open = self.open_files.read();
            for (path, mut diags) in results {
                if !rules.is_empty()
                    && let Some(ref root) = root
                {
                    let relative = path
                        .strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    ignore_rules::filter_ignored_by_config(&mut diags, &relative, &rules);
                }
                let uri = crate::util::path_to_uri(&path);
                if open.contains_key(&uri) {
                    // Insert even when empty so a file whose diagnostics
                    // just cleared up still gets its per-file cache
                    // cleared below, matching the per-file external-tool
                    // workers, which always write their result.
                    open_results.insert(uri, diags);
                } else if !diags.is_empty() {
                    workspace_results.insert(uri, diags);
                }
            }

            // An open file this source previously reported on but that
            // didn't turn up in this scan at all (not merely filtered
            // to empty) has had its issues fixed; clear its cache entry
            // too, matching how `set_external` below clears a closed
            // file that drops out of the workspace results.
            for uri in cache.lock().keys() {
                if open.contains_key(uri) && !open_results.contains_key(uri) {
                    open_results.insert(uri.clone(), Vec::new());
                }
            }
        }

        let updated = self
            .diag
            .workspace_diags
            .lock()
            .set_external(source, workspace_results);
        self.flush_workspace_diag_updates(updated).await;

        // Feed open files through the live per-file pipeline so the
        // results appear in the open buffers immediately.
        if open_results.is_empty() {
            return;
        }
        let mut changed = false;
        for (uri, diags) in open_results {
            // The file may have closed while awaiting
            // `flush_workspace_diag_updates` above, in which case
            // `clear_diagnostics_for_file` already purged its caches.
            // Re-check immediately before writing, matching the
            // per-file external-tool workers, so scan-time results
            // don't resurrect diagnostics for a now-closed file.
            if !self.open_files.read().contains_key(&uri) {
                continue;
            }
            // Likewise, a single-file run of this same tool may have
            // finished for the file in the meantime; its result is
            // based on newer content, so leave it alone.
            if !worker.store_scan_result(&generations, &uri, diags) {
                continue;
            }
            changed |= self.assemble_and_push(&uri).await;
        }
        // One refresh covers every file the tool touched, and only when
        // at least one of them actually changed.
        if changed {
            self.request_diagnostic_refresh().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::LanguageServer;
    use tower_lsp::lsp_types::{
        DiagnosticSeverity, NumberOrString, PartialResultParams, Position, PreviousResultId, Range,
        Url, WorkDoneProgressParams, WorkspaceDiagnosticParams, WorkspaceDiagnosticReportResult,
        WorkspaceDocumentDiagnosticReport,
    };

    fn diag(code: &str, line: u32) -> Diagnostic {
        Diagnostic {
            range: Range::new(Position::new(line, 0), Position::new(line, 5)),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(code.to_string())),
            message: format!("test {code}"),
            ..Default::default()
        }
    }

    #[test]
    fn set_native_tracks_and_clears() {
        let mut ws = WorkspaceDiagnostics::default();

        // An empty set for a never-reported file is a no-op.
        assert!(!ws.set_native("file:///a.php", Vec::new()));
        assert!(ws.result_id("file:///a.php").is_none());

        // Storing diagnostics tracks the file and bumps the id.
        assert!(ws.set_native("file:///a.php", vec![diag("unknown_class", 1)]));
        let first_id = ws.result_id("file:///a.php").expect("tracked");
        assert_eq!(ws.merged("file:///a.php").len(), 1);

        // Clearing a tracked file keeps it tracked with a new id so the
        // editor receives the (now empty) set.
        assert!(ws.set_native("file:///a.php", Vec::new()));
        let second_id = ws.result_id("file:///a.php").expect("still tracked");
        assert_ne!(first_id, second_id);
        assert!(ws.merged("file:///a.php").is_empty());
    }

    #[test]
    fn set_external_reports_cleared_uris() {
        let mut ws = WorkspaceDiagnostics::default();

        let mut first = HashMap::new();
        first.insert("file:///a.php".to_string(), vec![diag("phpstan", 1)]);
        first.insert("file:///b.php".to_string(), vec![diag("phpstan", 2)]);
        let updated = ws.set_external("phpstan", first);
        assert_eq!(updated.len(), 2);

        // The second run fixed a.php: it must appear in the updated set
        // so the editor drops its stale diagnostics.
        let mut second = HashMap::new();
        second.insert("file:///b.php".to_string(), vec![diag("phpstan", 2)]);
        let updated = ws.set_external("phpstan", second);
        assert!(updated.contains(&"file:///a.php".to_string()));
        assert!(
            !updated.contains(&"file:///b.php".to_string()),
            "b.php's diagnostics did not change, so it must not be re-bumped"
        );
        assert!(ws.merged("file:///a.php").is_empty());
        assert_eq!(ws.merged("file:///b.php").len(), 1);
    }

    #[test]
    fn set_external_leaves_unchanged_uris_alone() {
        let mut ws = WorkspaceDiagnostics::default();

        let mut first = HashMap::new();
        first.insert("file:///a.php".to_string(), vec![diag("phpstan", 1)]);
        ws.set_external("phpstan", first.clone());
        let id_before = ws.result_id("file:///a.php").expect("tracked");

        // Re-running the tool with byte-for-byte identical results must
        // not bump the result id: nothing changed, so the client's cached
        // result for this (and every other untouched) file stays valid.
        let updated = ws.set_external("phpstan", first);
        assert!(updated.is_empty());
        assert_eq!(ws.result_id("file:///a.php"), Some(id_before));
    }

    #[test]
    fn merged_combines_native_and_external_sources() {
        let mut ws = WorkspaceDiagnostics::default();
        ws.set_native("file:///a.php", vec![diag("unknown_class", 1)]);
        let mut phpstan = HashMap::new();
        phpstan.insert("file:///a.php".to_string(), vec![diag("argument.type", 7)]);
        ws.set_external("phpstan", phpstan);

        let merged = ws.merged("file:///a.php");
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn diff_for_pull_falls_back_to_its_own_delivery_record() {
        let mut ws = WorkspaceDiagnostics::default();
        ws.set_native("file:///a.php", vec![diag("unknown_class", 1)]);

        // First pull: no previous id from the client, so a full report
        // is sent and the server records what it delivered.
        let (first_id, diags) = ws.diff_for_pull("file:///a.php", None);
        assert!(diags.is_some(), "first pull must send a full report");

        // A client that has nowhere to store this file's diagnostics
        // (e.g. it sits outside the opened workspace root) never echoes
        // an id back. Without server-side tracking this would resend
        // the full report forever; it must now answer Unchanged.
        let (second_id, diags) = ws.diff_for_pull("file:///a.php", None);
        assert_eq!(second_id, first_id);
        assert!(
            diags.is_none(),
            "a repeat pull with no client echo but no server-side change must be Unchanged"
        );

        // Once the diagnostics actually change, a full report is due
        // again even though the client still never echoes an id.
        ws.set_native("file:///a.php", vec![diag("unknown_class", 2)]);
        let (third_id, diags) = ws.diff_for_pull("file:///a.php", None);
        assert_ne!(third_id, second_id);
        assert!(diags.is_some(), "changed diagnostics must be re-sent");
    }

    /// End-to-end: the native pass diagnoses unopened workspace files,
    /// skips open ones, and the `workspace/diagnostic` handler reports
    /// the cached results with working `Unchanged` support.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn native_pass_diagnoses_unopened_files() {
        let dir = tempfile::tempdir().expect("temp dir");
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).expect("src dir");

        // Unopened file referencing a class that does not exist.
        std::fs::write(
            src.join("Broken.php"),
            "<?php\nnamespace App;\nclass Broken { public function f(): void { $x = new MissingClass(); } }\n",
        )
        .expect("broken file");
        // Unopened file with no problems.
        std::fs::write(
            src.join("Clean.php"),
            "<?php\nnamespace App;\nclass Clean { public function f(): void {} }\n",
        )
        .expect("clean file");
        // A file that is open in the editor: owned by the live
        // pipeline, so the pass must skip it even though it has the
        // same unknown-class problem.
        std::fs::write(
            src.join("Open.php"),
            "<?php\nnamespace App;\nclass Open { public function f(): void { $x = new MissingClass(); } }\n",
        )
        .expect("open file");

        let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), Vec::new());
        backend.ensure_workspace_indexed();

        let open_uri = crate::util::path_to_uri(&src.join("Open.php"));
        backend.open_files.write().insert(
            open_uri.clone(),
            Arc::new("<?php\nnamespace App;\nclass Open {}\n".to_string()),
        );

        let progress = ScanProgress::new();
        backend.run_native_workspace_pass(&progress).await;

        let broken_uri = crate::util::path_to_uri(&src.join("Broken.php"));
        let clean_uri = crate::util::path_to_uri(&src.join("Clean.php"));

        let (broken_diags, clean_tracked, open_tracked) = {
            let ws = backend.diag.workspace_diags.lock();
            (
                ws.merged(&broken_uri),
                ws.result_id(&clean_uri).is_some(),
                ws.result_id(&open_uri).is_some(),
            )
        };

        assert!(
            broken_diags.iter().any(|d| {
                matches!(&d.code, Some(NumberOrString::String(c)) if c == "unknown_class")
            }),
            "Broken.php should have an unknown_class diagnostic, got: {:?}",
            broken_diags
        );
        assert!(
            !clean_tracked,
            "Clean.php has no diagnostics and should not be tracked"
        );
        assert!(
            !open_tracked,
            "Open.php is open in the editor and must be skipped"
        );

        // ── workspace/diagnostic reports the cached results ──────────
        let params = WorkspaceDiagnosticParams {
            identifier: None,
            previous_result_ids: Vec::new(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let WorkspaceDiagnosticReportResult::Report(report) = backend
            .workspace_diagnostic(params)
            .await
            .expect("workspace diagnostic")
        else {
            panic!("expected a full workspace diagnostic report");
        };

        let full_item = report
            .items
            .iter()
            .find_map(|item| match item {
                WorkspaceDocumentDiagnosticReport::Full(full)
                    if full.uri.as_str() == broken_uri =>
                {
                    Some(full)
                }
                _ => None,
            })
            .expect("Broken.php should be reported");
        assert!(!full_item.full_document_diagnostic_report.items.is_empty());
        let result_id = full_item
            .full_document_diagnostic_report
            .result_id
            .clone()
            .expect("workspace reports carry a result id");
        assert!(result_id.starts_with("ws"));

        // ── A re-pull with the previous id answers Unchanged ─────────
        let params = WorkspaceDiagnosticParams {
            identifier: None,
            previous_result_ids: vec![PreviousResultId {
                uri: broken_uri.parse::<Url>().expect("uri"),
                value: result_id.clone(),
            }],
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let WorkspaceDiagnosticReportResult::Report(report) = backend
            .workspace_diagnostic(params)
            .await
            .expect("workspace diagnostic")
        else {
            panic!("expected a full workspace diagnostic report");
        };
        assert!(
            report.items.iter().any(|item| matches!(
                item,
                WorkspaceDocumentDiagnosticReport::Unchanged(u)
                    if u.uri.as_str() == broken_uri
                        && u.unchanged_document_diagnostic_report.result_id == result_id
            )),
            "a matching previous result id should answer Unchanged"
        );
    }

    #[test]
    fn worker_slot_reports_the_file_it_is_on() {
        let slot = WorkerSlot::new();
        assert!(
            slot.in_flight(1_000).is_none(),
            "a worker between files has nothing in flight"
        );

        slot.claim(7, 400);
        let (file, elapsed) = slot.in_flight(1_000).expect("claimed");
        assert_eq!(file, 7);
        assert_eq!(elapsed, Duration::from_millis(600));

        // A clock reading older than the claim (the orchestrator's
        // snapshot can predate it by a poll) must not underflow into a
        // huge elapsed time and trip the watchdog.
        assert_eq!(
            slot.in_flight(100).expect("claimed").1,
            Duration::ZERO,
            "elapsed must saturate rather than wrap"
        );

        slot.release();
        assert!(slot.in_flight(1_000).is_none());
    }

    /// The watchdog reads a slot and retires it a moment later, so the
    /// retirement only counts as "this file timed out" when the worker
    /// is still on the file that was read.
    #[test]
    fn retiring_a_slot_only_blames_the_file_it_is_still_on() {
        let stuck = WorkerSlot::new();
        stuck.claim(3, 0);
        assert!(stuck.retire_on(3), "a worker still inside file 3 is stuck");
        assert!(stuck.is_retired());

        // Finished the file between the watchdog's read and its retire:
        // its diagnostics were published, so it is not a file the pass
        // gave up on.
        let finished = WorkerSlot::new();
        finished.claim(3, 0);
        finished.release();
        assert!(!finished.retire_on(3));
        assert!(
            finished.is_retired(),
            "the flag cannot be taken back without racing the worker"
        );

        // Same, but it had already claimed the next file.
        let moved_on = WorkerSlot::new();
        moved_on.claim(4, 0);
        assert!(!moved_on.retire_on(3));
    }

    /// Retiring every worker in the pool must not take the rest of the
    /// project down with it: each one is replaced, so the files queued
    /// behind two wedged ones are still diagnosed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn native_pass_replaces_every_worker_it_retires() {
        let backend = Backend::new_test();
        let uris: Vec<String> = (0..6).map(|i| format!("file:///w{i}.php")).collect();
        // The first two files are claimed by the whole pool and never
        // return within the watchdog's budget, so without a replacement
        // the four behind them are never looked at.
        let work = Arc::new(|uri: &str| {
            if uri.ends_with("w0.php") || uri.ends_with("w1.php") {
                std::thread::sleep(Duration::from_secs(5));
            }
            Some(vec![diag("unknown_class", 1)])
        });

        let mut pass = NativePass::new(
            uris,
            Arc::clone(&backend.shutdown_flag),
            Arc::clone(&backend.diag.workspace_diag_cancel),
            work,
        );
        pass.spawn_workers(2);

        let watchdog = FileWatchdog {
            notice: Duration::from_millis(100),
            give_up: Duration::from_secs(1),
        };
        let outcome = tokio::time::timeout(
            Duration::from_secs(20),
            backend.drive_native_pass(&mut pass, watchdog, &ScanProgress::new()),
        )
        .await
        .expect("the pass must return without waiting for the wedged files");

        // Which worker claimed which of the two wedged files is up to
        // the queue, so only the set of them is fixed.
        let mut skipped = outcome.skipped.clone();
        skipped.sort();
        assert_eq!(
            skipped,
            vec!["file:///w0.php".to_string(), "file:///w1.php".to_string()],
            "both wedged files should be reported as skipped"
        );
        assert_eq!(
            outcome.unchecked(),
            0,
            "the replacement workers should have drained the queue"
        );
        assert_eq!(outcome.diagnosed, 4);
        assert_eq!(
            pass.replacements, 2,
            "one replacement per retired worker, and no more"
        );

        let ws = backend.diag.workspace_diags.lock();
        for i in 2..6 {
            let uri = format!("file:///w{i}.php");
            assert_eq!(ws.merged(&uri).len(), 1, "{uri} should be stored");
        }
    }

    /// Switching `[diagnostics] workspace` on through a live config
    /// reload must leave the pass owed a run: the index tail that
    /// normally starts it has already returned by then, so the setting
    /// used to do nothing until the editor restarted.
    #[test]
    fn a_reload_that_enables_workspace_diagnostics_leaves_the_pass_due() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config = dir.path().join(crate::config::CONFIG_FILE_NAME);
        let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), Vec::new());
        // Stand in for a finished full background index.
        backend.workspace_indexed.store(true, Ordering::Release);

        std::fs::write(&config, "[diagnostics]\nworkspace = false\n").expect("write config");
        backend.reload_config(dir.path());
        assert!(!backend.workspace_diagnostics_due_after_reload());

        std::fs::write(&config, "[diagnostics]\nworkspace = true\n").expect("write config");
        backend.reload_config(dir.path());
        assert!(
            backend.workspace_diagnostics_due_after_reload(),
            "enabling the setting mid-session must start the pass"
        );

        // An index that is still running starts the pass from its own
        // tail, so a reload must not start a second one.
        backend
            .full_index_in_progress
            .store(true, Ordering::Release);
        assert!(!backend.workspace_diagnostics_due_after_reload());
        backend
            .full_index_in_progress
            .store(false, Ordering::Release);

        // Nor may a pass that has already run be run again.
        backend
            .diag
            .workspace_diag_pass_started
            .store(true, Ordering::Release);
        assert!(!backend.workspace_diagnostics_due_after_reload());
    }

    /// A config reload that switches `[diagnostics] workspace` off while
    /// the pass is active must cancel it and mark it inactive, so a
    /// running pass stops instead of finishing and publishing results
    /// for files the user just said not to diagnose, and a later
    /// re-enable can start a fresh pass rather than being blocked
    /// forever by the one-shot guard.
    #[test]
    fn a_reload_that_disables_workspace_diagnostics_cancels_and_deactivates_the_pass() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config = dir.path().join(crate::config::CONFIG_FILE_NAME);
        let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), Vec::new());
        backend
            .diag
            .workspace_diag_pass_started
            .store(true, Ordering::Release);

        std::fs::write(&config, "[diagnostics]\nworkspace = false\n").expect("write config");
        backend.reload_config(dir.path());

        assert!(
            !backend
                .diag
                .workspace_diag_pass_started
                .load(Ordering::Acquire),
            "the pass must be marked inactive so a later re-enable can start a fresh one"
        );
        assert!(
            backend.diag.workspace_diag_cancel.load(Ordering::Acquire),
            "a config reload that disables the setting must cancel a running pass"
        );

        // A pass that was never active (or already stopped) has nothing
        // to cancel, so a reload while still off must be a no-op.
        backend
            .diag
            .workspace_diag_cancel
            .store(false, Ordering::Release);
        backend.reload_config(dir.path());
        assert!(!backend.diag.workspace_diag_cancel.load(Ordering::Acquire));
    }

    /// Clearing must drop every stored result so the editor is not left
    /// carrying stale diagnostics, but keep the URI tracked so a pull
    /// client that asks again is told it is now empty rather than
    /// hearing nothing about it at all.
    #[tokio::test]
    async fn clear_workspace_diagnostics_drops_results_but_keeps_tracking_the_uri() {
        let backend = Backend::new_test();
        backend
            .diag
            .workspace_diags
            .lock()
            .set_native("file:///w.php", vec![diag("unknown_class", 1)]);

        backend.clear_workspace_diagnostics().await;

        let ws = backend.diag.workspace_diags.lock();
        assert!(
            ws.merged("file:///w.php").is_empty(),
            "stored diagnostics must be cleared"
        );
        assert!(
            ws.tracked_uris().contains(&"file:///w.php".to_string()),
            "the uri must stay tracked so a pull client is told it is now empty"
        );
    }

    /// The replacement budget is finite: a project that wedges every
    /// worker it is handed stops accumulating threads.
    #[test]
    fn worker_replacements_are_budgeted() {
        let mut pass = NativePass::new(
            Vec::new(),
            Arc::new(AtomicBool::new(true)),
            Arc::new(AtomicBool::new(false)),
            Arc::new(|_: &str| None),
        );
        for _ in 0..MAX_WORKER_REPLACEMENTS {
            assert!(pass.replace_retired_worker());
        }
        assert!(
            !pass.replace_retired_worker(),
            "the pool shrinks rather than growing without bound"
        );
    }

    #[test]
    fn outcome_summary_reports_files_it_gave_up_on() {
        let mut outcome = NativePassOutcome {
            total: 12,
            diagnosed: 12,
            skipped: Vec::new(),
        };
        assert_eq!(outcome.summary(), "Diagnosed 12 files");
        assert_eq!(outcome.unchecked(), 0);

        // One file timed out, and the whole project is still accounted
        // for: 11 diagnosed plus the one given up on.
        outcome.diagnosed = 11;
        outcome.skipped.push("src/Huge.php".to_string());
        assert_eq!(outcome.summary(), "Diagnosed 11 files, 1 timed out");
        assert_eq!(outcome.unchecked(), 0);

        // Every worker was retired before the queue drained, so the rest
        // of the project was never looked at and must be reported.
        outcome.diagnosed = 4;
        assert_eq!(
            outcome.summary(),
            "Diagnosed 4 files, 1 timed out, 7 not checked"
        );
        assert_eq!(outcome.unchecked(), 7);
    }

    /// A file whose analysis never returns must not take the rest of the
    /// project down with it: the pass gives up on that one file, reports
    /// it, and still diagnoses everything else.
    ///
    /// The work function stands in for the collectors so the wedge is
    /// deterministic; everything else here is the production pass.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn native_pass_gives_up_on_a_wedged_file_and_finishes_the_rest() {
        let backend = Backend::new_test();
        let uris: Vec<String> = (0..4).map(|i| format!("file:///f{i}.php")).collect();
        let work = Arc::new(|uri: &str| {
            if uri.ends_with("f0.php") {
                // Far longer than the watchdog below allows, so the
                // pass has to abandon this worker to make progress.
                std::thread::sleep(Duration::from_secs(5));
            }
            Some(vec![diag("unknown_class", 1)])
        });

        let mut pass = NativePass::new(
            uris,
            Arc::clone(&backend.shutdown_flag),
            Arc::clone(&backend.diag.workspace_diag_cancel),
            work,
        );
        pass.spawn_workers(2);

        let watchdog = FileWatchdog {
            notice: Duration::from_millis(100),
            give_up: Duration::from_secs(1),
        };
        let progress = ScanProgress::new();
        let outcome = tokio::time::timeout(
            Duration::from_secs(20),
            backend.drive_native_pass(&mut pass, watchdog, &progress),
        )
        .await
        .expect("the pass must return without waiting for the wedged file");

        assert_eq!(
            outcome.skipped,
            vec!["file:///f0.php".to_string()],
            "the wedged file should be reported as skipped"
        );
        assert_eq!(
            outcome.diagnosed, 3,
            "the other three files should still be diagnosed"
        );
        assert_eq!(
            outcome.unchecked(),
            0,
            "the surviving worker should have drained the queue"
        );

        // Their results must have reached the workspace store, not just
        // the completed counter.
        let ws = backend.diag.workspace_diags.lock();
        for i in 1..4 {
            let uri = format!("file:///f{i}.php");
            assert_eq!(ws.merged(&uri).len(), 1, "{uri} should be stored");
        }
        assert!(
            ws.result_id("file:///f0.php").is_none(),
            "the wedged file never finished, so it has no results"
        );
    }

    /// Shutdown mid-pass returns promptly instead of draining the queue.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn native_pass_stops_on_shutdown() {
        let backend = Backend::new_test();
        let uris = (0..64)
            .map(|i| format!("file:///s{i}.php"))
            .collect::<Vec<_>>();
        backend
            .shutdown_flag
            .store(true, std::sync::atomic::Ordering::Release);

        let mut pass = NativePass::new(
            uris,
            Arc::clone(&backend.shutdown_flag),
            Arc::clone(&backend.diag.workspace_diag_cancel),
            Arc::new(|_: &str| Some(Vec::new())),
        );
        pass.spawn_workers(2);

        let outcome = tokio::time::timeout(
            Duration::from_secs(20),
            backend.drive_native_pass(&mut pass, FileWatchdog::default(), &ScanProgress::new()),
        )
        .await
        .expect("shutdown must end the pass");
        assert!(
            outcome.skipped.is_empty(),
            "a clean shutdown is not a file the pass gave up on"
        );
    }

    /// Closing a file migrates its live external tool results into the
    /// workspace store so they keep being reported for the closed file.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn did_close_migrates_external_results() {
        let backend = Backend::new_test();
        backend
            .diag
            .workspace_diag_pass_started
            .store(true, std::sync::atomic::Ordering::Release);

        let uri = "file:///closed.php";
        backend
            .phpstan_tool
            .last_diags
            .lock()
            .insert(uri.to_string(), vec![diag("argument.type", 3)]);

        backend.clear_diagnostics_for_file(uri).await;

        let merged = backend.diag.workspace_diags.lock().merged(uri);
        assert_eq!(
            merged.len(),
            1,
            "PHPStan results should migrate to the workspace store on close"
        );
        assert!(
            backend.phpstan_tool.last_diags.lock().get(uri).is_none(),
            "the per-file cache entry should still be purged"
        );
    }

    /// A project-wide scan that no longer reports diagnostics for an
    /// open file (the file is absent from the results map entirely, not
    /// merely reported with an empty list) must still clear that file's
    /// stale per-file cache entry.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn store_workspace_external_results_clears_open_file_dropped_from_scan() {
        let backend = Backend::new_test();
        let uri = "file:///open.php";
        backend
            .open_files
            .write()
            .insert(uri.to_string(), Arc::new("<?php\n".to_string()));
        backend
            .phpstan_tool
            .last_diags
            .lock()
            .insert(uri.to_string(), vec![diag("argument.type", 3)]);

        let generations = backend.phpstan_tool.generation_snapshot();
        backend
            .store_workspace_external_results("phpstan", HashMap::new(), generations)
            .await;

        assert!(
            backend
                .phpstan_tool
                .last_diags
                .lock()
                .get(uri)
                .is_some_and(Vec::is_empty),
            "the open file's stale phpstan entry must clear once the tool stops reporting it"
        );
    }

    /// The write-time check for `open_files` must apply per file, not
    /// just at partition time: a file that is still open when results
    /// are written gets its cache updated.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn store_workspace_external_results_updates_open_file_cache() {
        let backend = Backend::new_test();
        let uri_str = "file:///open.php";
        backend
            .open_files
            .write()
            .insert(uri_str.to_string(), Arc::new("<?php\n".to_string()));

        let mut results = HashMap::new();
        results.insert(PathBuf::from("/open.php"), vec![diag("argument.type", 5)]);
        let generations = backend.phpstan_tool.generation_snapshot();
        backend
            .store_workspace_external_results("phpstan", results, generations)
            .await;

        let cached = backend.phpstan_tool.last_diags.lock().get(uri_str).cloned();
        assert_eq!(
            cached.map(|d| d.len()),
            Some(1),
            "an open file's results should feed the live per-file cache"
        );
    }

    /// A single-file run that finishes while a project-wide run of the
    /// same tool is still in flight wins: the scan's older results must
    /// not overwrite the fresher per-file ones for a file that stayed
    /// open the whole time.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn store_workspace_external_results_keeps_fresher_per_file_result() {
        let backend = Backend::new_test();
        let uri_str = "file:///open.php";
        backend
            .open_files
            .write()
            .insert(uri_str.to_string(), Arc::new("<?php\n".to_string()));

        // The project-wide run starts here.
        let generations = backend.phpstan_tool.generation_snapshot();

        // While it is running, the user edits the file and the per-file
        // worker writes a corrected (empty) result.
        backend.phpstan_tool.store_file_result(uri_str, Vec::new());

        // The project-wide run now lands with its scan-time findings.
        let mut results = HashMap::new();
        results.insert(PathBuf::from("/open.php"), vec![diag("argument.type", 5)]);
        backend
            .store_workspace_external_results("phpstan", results, generations)
            .await;

        assert!(
            backend
                .phpstan_tool
                .last_diags
                .lock()
                .get(uri_str)
                .is_some_and(Vec::is_empty),
            "the stale scan result must not resurrect a diagnostic the per-file run cleared"
        );
    }

    /// The freshness check is per file: a single-file run for one open
    /// file must not stop the same scan from updating a different open
    /// file that nothing else has touched.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn store_workspace_external_results_skips_only_the_superseded_file() {
        let backend = Backend::new_test();
        let edited = "file:///edited.php";
        let untouched = "file:///untouched.php";
        {
            let mut open = backend.open_files.write();
            open.insert(edited.to_string(), Arc::new("<?php\n".to_string()));
            open.insert(untouched.to_string(), Arc::new("<?php\n".to_string()));
        }

        let generations = backend.phpstan_tool.generation_snapshot();
        backend.phpstan_tool.store_file_result(edited, Vec::new());

        let mut results = HashMap::new();
        results.insert(PathBuf::from("/edited.php"), vec![diag("argument.type", 5)]);
        results.insert(
            PathBuf::from("/untouched.php"),
            vec![diag("argument.type", 7)],
        );
        backend
            .store_workspace_external_results("phpstan", results, generations)
            .await;

        let cache = backend.phpstan_tool.last_diags.lock();
        assert!(
            cache.get(edited).is_some_and(Vec::is_empty),
            "the edited file keeps its fresher per-file result"
        );
        assert_eq!(
            cache.get(untouched).map(Vec::len),
            Some(1),
            "the untouched file still receives the scan's results"
        );
    }
}
