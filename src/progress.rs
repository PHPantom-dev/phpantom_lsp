//! Shared progress-reporting infrastructure for long-running scans.
//!
//! [`ScanProgress`] is a thread-safe progress sink that scanning code
//! (workspace indexing, vendor classmap builds, reference scans) writes
//! from any thread — scoped worker threads, the blocking pool, or the
//! async runtime. Updates are cheap atomic stores, so per-file
//! reporting adds no measurable cost to a scan.
//!
//! An async poller task ([`Backend::spawn_progress_poller`]) snapshots
//! the state every 100 ms and forwards changes to the client as
//! `WorkDoneProgressReport` notifications. This throttles notification
//! traffic, keeps notification I/O off the scan threads entirely, and
//! works even while the writing thread is blocked in synchronous code.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use tower_lsp::lsp_types::NumberOrString;

use crate::Backend;

/// Thread-safe progress state for one long-running operation.
///
/// Two reporting modes cooperate on the same state:
///
/// - **Direct mode** — [`set_percentage`](Self::set_percentage) stores
///   an absolute percentage and label. Used for milestones and for
///   passing through progress from a nested operation that computes
///   its own percentage (e.g. the workspace index).
/// - **Counter mode** — [`set_scope`](Self::set_scope) allocates an
///   absolute percentage range, [`begin_phase`](Self::begin_phase)
///   carves a fraction of that scope into the active window, and
///   [`add_total`](Self::add_total) / [`add_done`](Self::add_done)
///   track work units mapped linearly into the window.
///
/// The scope/phase split exists so a multi-phase operation can hand a
/// shared helper the *whole* current scope and let the helper divide
/// it into contiguous phases, while the top-level caller partitions
/// the overall bar between helpers with plain `set_scope` arithmetic
/// (this is how monorepo indexing gives each subproject its own
/// slice). Because each phase can only fill its own sub-window, a
/// phase that finishes before the next one has registered its total
/// cannot spike the bar to the ceiling of the whole operation.
///
/// A phase's total may keep growing while it runs (e.g. as
/// `require_once` chains are discovered); the percentage recalculates
/// and the poller clamps it monotonic.
pub struct ScanProgress {
    /// Inclusive lower bound of the scope `begin_phase` fractions map into.
    scope_lo: AtomicU32,
    /// Inclusive upper bound of the scope `begin_phase` fractions map into.
    scope_hi: AtomicU32,
    /// Inclusive lower bound of the active counter-mode window.
    window_lo: AtomicU32,
    /// Inclusive upper bound of the active counter-mode window.
    window_hi: AtomicU32,
    /// Work units completed in the active window.
    done: AtomicU64,
    /// Total work units expected in the active window. May grow while
    /// scanning.
    total: AtomicU64,
    /// Latest computed overall percentage (0..=100).
    percentage: AtomicU32,
    /// Whether the last update came from counter mode (controls
    /// whether the report message includes a `(done/total files)`
    /// suffix).
    counted: AtomicBool,
    /// Sticky prefix prepended to every report message (e.g.
    /// "Subproject 2/5: app/api").
    label_prefix: Mutex<String>,
    /// Current phase label, e.g. "Scanning vendor packages".
    label: Mutex<String>,
    /// Set when the state changed since the last [`take_report`](Self::take_report).
    dirty: AtomicBool,
}

impl ScanProgress {
    /// Create a fresh progress state (0%, empty label, scope 0..100).
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            scope_lo: AtomicU32::new(0),
            scope_hi: AtomicU32::new(100),
            window_lo: AtomicU32::new(0),
            window_hi: AtomicU32::new(100),
            done: AtomicU64::new(0),
            total: AtomicU64::new(0),
            percentage: AtomicU32::new(0),
            counted: AtomicBool::new(false),
            label_prefix: Mutex::new(String::new()),
            label: Mutex::new(String::new()),
            dirty: AtomicBool::new(false),
        })
    }

    /// Directly set the overall percentage and label (direct mode).
    pub fn set_percentage(&self, percentage: u32, label: impl Into<String>) {
        self.percentage
            .store(percentage.min(100), Ordering::Relaxed);
        self.counted.store(false, Ordering::Relaxed);
        *self.label.lock() = label.into();
        self.dirty.store(true, Ordering::Release);
    }

    /// Set the sticky prefix prepended to every report message. Pass
    /// an empty string to clear it.
    pub fn set_label_prefix(&self, prefix: impl Into<String>) {
        *self.label_prefix.lock() = prefix.into();
        self.dirty.store(true, Ordering::Release);
    }

    /// Enter counter mode over the absolute percentage range
    /// `lo..=hi`: the counters reset, the active window covers the
    /// whole scope, and subsequent [`begin_phase`](Self::begin_phase)
    /// fractions map into this range.
    pub fn set_scope(&self, lo: u32, hi: u32, label: impl Into<String>) {
        let lo = lo.min(100);
        let hi = hi.clamp(lo, 100);
        self.scope_lo.store(lo, Ordering::Relaxed);
        self.scope_hi.store(hi, Ordering::Relaxed);
        self.window_lo.store(lo, Ordering::Relaxed);
        self.window_hi.store(hi, Ordering::Relaxed);
        self.done.store(0, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
        *self.label.lock() = label.into();
        self.recompute();
    }

    /// Begin a phase covering `frac_lo..frac_hi` of the current scope:
    /// the counters reset and map into that sub-window. Fractions are
    /// clamped to `0.0..=1.0`.
    pub fn begin_phase(&self, frac_lo: f64, frac_hi: f64, label: impl Into<String>) {
        let scope_lo = self.scope_lo.load(Ordering::Relaxed);
        let scope_hi = self.scope_hi.load(Ordering::Relaxed);
        let span = scope_hi.saturating_sub(scope_lo) as f64;
        let lo = scope_lo + (span * frac_lo.clamp(0.0, 1.0)).round() as u32;
        let hi = scope_lo + (span * frac_hi.clamp(0.0, 1.0)).round() as u32;
        self.window_lo.store(lo.min(100), Ordering::Relaxed);
        self.window_hi.store(hi.clamp(lo, 100), Ordering::Relaxed);
        self.done.store(0, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
        *self.label.lock() = label.into();
        self.recompute();
    }

    /// Add `n` expected work units to the current phase's total.
    pub fn add_total(&self, n: u64) {
        if n == 0 {
            return;
        }
        self.total.fetch_add(n, Ordering::Relaxed);
        self.recompute();
    }

    /// Record `n` completed work units in the current phase.
    pub fn add_done(&self, n: u64) {
        if n == 0 {
            return;
        }
        self.done.fetch_add(n, Ordering::Relaxed);
        self.recompute();
    }

    /// Recompute the percentage from the counters and mark the state
    /// dirty. Concurrent updates may interleave, but any transient
    /// misordering self-corrects on the next update.
    fn recompute(&self) {
        let lo = self.window_lo.load(Ordering::Relaxed);
        let hi = self.window_hi.load(Ordering::Relaxed);
        let done = self.done.load(Ordering::Relaxed);
        let total = self.total.load(Ordering::Relaxed);
        let span = hi.saturating_sub(lo) as u64;
        // checked_div returns None when total is 0 (no work registered
        // yet) — report the window floor in that case.
        let pct = match (span * done.min(total)).checked_div(total) {
            Some(fraction) => lo + fraction as u32,
            None => lo,
        };
        self.percentage.store(pct.min(hi), Ordering::Relaxed);
        self.counted.store(true, Ordering::Relaxed);
        self.dirty.store(true, Ordering::Release);
    }

    /// Take the latest `(percentage, message)` snapshot if the state
    /// changed since the previous call, clearing the dirty flag.
    ///
    /// In counter mode the message carries a `(done/total files)`
    /// suffix so the user sees absolute counts alongside the bar.
    pub fn take_report(&self) -> Option<(u32, String)> {
        if !self.dirty.swap(false, Ordering::Acquire) {
            return None;
        }
        let pct = self.percentage.load(Ordering::Relaxed);
        let prefix = self.label_prefix.lock().clone();
        let label = self.label.lock().clone();
        let label = if prefix.is_empty() {
            label
        } else {
            format!("{prefix}: {label}")
        };
        let total = self.total.load(Ordering::Relaxed);
        let message = if self.counted.load(Ordering::Relaxed) && total > 0 {
            let done = self.done.load(Ordering::Relaxed).min(total);
            format!("{label} ({done}/{total} files)")
        } else {
            label
        };
        Some((pct, message))
    }
}

/// Handle for a running progress-poller task.
///
/// Call [`finish`](Self::finish) when the operation completes so the
/// final in-flight report flushes before the caller sends
/// `WorkDoneProgressEnd`. Merely dropping the handle would let a late
/// report race (and arrive after) the end notification.
pub(crate) struct ProgressPoller {
    stop_tx: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<()>,
}

impl ProgressPoller {
    /// Stop polling, flush the final report, and wait for the task.
    pub(crate) async fn finish(self) {
        let _ = self.stop_tx.send(());
        let _ = self.handle.await;
    }
}

impl Backend {
    /// Spawn a task that polls `state` every 100 ms and forwards
    /// changed snapshots to the client as progress reports for
    /// `token`. Percentages are clamped monotonically non-decreasing,
    /// as required by the LSP spec, even when a growing total makes
    /// the underlying ratio regress.
    pub(crate) fn spawn_progress_poller(
        &self,
        token: NumberOrString,
        state: Arc<ScanProgress>,
    ) -> ProgressPoller {
        let backend = self.clone_for_blocking();
        let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(100));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            // The first tick fires immediately; skip it so the begin
            // notification isn't followed by an instant 0% report.
            interval.tick().await;
            let mut last_sent = 0u32;
            loop {
                let stopped = tokio::select! {
                    _ = &mut stop_rx => true,
                    _ = interval.tick() => false,
                };
                if let Some((pct, message)) = state.take_report() {
                    let pct = pct.min(100).max(last_sent);
                    last_sent = pct;
                    backend.progress_report(&token, pct, Some(message)).await;
                }
                if stopped {
                    break;
                }
            }
        });
        ProgressPoller { stop_tx, handle }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The poller must stop promptly on `finish()` and flush the final
    /// pending snapshot before returning, so the caller's
    /// `WorkDoneProgressEnd` is guaranteed to be the last notification.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn poller_flushes_final_report_on_finish() {
        let backend = crate::Backend::new_test();
        let state = ScanProgress::new();
        let poller = backend.spawn_progress_poller(
            NumberOrString::String("test-token".to_string()),
            Arc::clone(&state),
        );

        state.set_percentage(50, "halfway");
        poller.finish().await;

        assert!(
            state.take_report().is_none(),
            "finish() must consume the pending snapshot"
        );
    }

    #[test]
    fn direct_mode_reports_percentage_and_label() {
        let p = ScanProgress::new();
        assert!(p.take_report().is_none(), "fresh state is not dirty");

        p.set_percentage(42, "Reading composer.json");
        let (pct, msg) = p.take_report().expect("dirty after set_percentage");
        assert_eq!(pct, 42);
        assert_eq!(msg, "Reading composer.json");
        assert!(p.take_report().is_none(), "take_report clears dirty");
    }

    #[test]
    fn set_percentage_clamps_to_100() {
        let p = ScanProgress::new();
        p.set_percentage(250, "overflow");
        assert_eq!(p.take_report().unwrap().0, 100);
    }

    #[test]
    fn counter_mode_maps_into_scope_with_counts_in_message() {
        let p = ScanProgress::new();
        p.set_scope(10, 90, "Scanning files");
        p.add_total(200);
        p.add_done(50);
        let (pct, msg) = p.take_report().unwrap();
        assert_eq!(pct, 10 + 80 / 4); // 25% of a 10..90 scope
        assert_eq!(msg, "Scanning files (50/200 files)");
    }

    #[test]
    fn empty_total_reports_scope_floor() {
        let p = ScanProgress::new();
        p.set_scope(30, 70, "Scanning");
        let (pct, msg) = p.take_report().unwrap();
        assert_eq!(pct, 30);
        assert_eq!(msg, "Scanning", "no counts suffix when total is zero");
    }

    #[test]
    fn growing_total_recalculates_percentage() {
        let p = ScanProgress::new();
        p.set_scope(0, 100, "Scanning");
        p.add_total(10);
        p.add_done(10);
        assert_eq!(p.take_report().unwrap().0, 100);

        // A newly discovered batch grows the total; the ratio (and the
        // raw percentage) regresses. The poller clamps monotonic.
        p.add_total(30);
        let (pct, msg) = p.take_report().unwrap();
        assert_eq!(pct, 25);
        assert_eq!(msg, "Scanning (10/40 files)");
    }

    #[test]
    fn done_never_exceeds_total_in_percentage_or_message() {
        let p = ScanProgress::new();
        p.set_scope(0, 100, "Scanning");
        p.add_total(5);
        p.add_done(9);
        let (pct, msg) = p.take_report().unwrap();
        assert_eq!(pct, 100);
        assert_eq!(msg, "Scanning (5/5 files)");
    }

    #[test]
    fn phases_partition_the_scope() {
        let p = ScanProgress::new();
        p.set_scope(20, 80, "Building class index");

        // A small first phase fills only its own sub-window, so the
        // bar cannot spike to the scope ceiling while a later, larger
        // phase has not registered its total yet.
        p.begin_phase(0.0, 0.25, "Scanning project files");
        p.add_total(4);
        p.add_done(4);
        let (pct, msg) = p.take_report().unwrap();
        assert_eq!(pct, 35); // 20 + 0.25 * 60
        assert_eq!(msg, "Scanning project files (4/4 files)");

        p.begin_phase(0.25, 1.0, "Scanning vendor packages");
        let (pct, msg) = p.take_report().unwrap();
        assert_eq!(pct, 35, "next phase starts at its own floor");
        assert_eq!(msg, "Scanning vendor packages");

        p.add_total(100);
        p.add_done(50);
        let (pct, msg) = p.take_report().unwrap();
        assert_eq!(pct, 35 + 45 / 2); // halfway through 35..80
        assert_eq!(msg, "Scanning vendor packages (50/100 files)");
    }

    #[test]
    fn label_prefix_prepends_to_reports() {
        let p = ScanProgress::new();
        p.set_label_prefix("Subproject 2/5: apps/api");
        p.set_scope(10, 80, "Scanning vendor packages");
        p.add_total(10);
        p.add_done(5);
        let (_, msg) = p.take_report().unwrap();
        assert_eq!(
            msg,
            "Subproject 2/5: apps/api: Scanning vendor packages (5/10 files)"
        );

        p.set_label_prefix("");
        let (_, msg) = p.take_report().unwrap();
        assert_eq!(msg, "Scanning vendor packages (5/10 files)");
    }

    #[test]
    fn set_scope_resets_counters() {
        let p = ScanProgress::new();
        p.set_scope(0, 50, "Phase A");
        p.add_total(2);
        p.add_done(2);
        assert_eq!(p.take_report().unwrap().0, 50);

        p.set_scope(50, 100, "Phase B");
        p.add_total(10);
        let (pct, msg) = p.take_report().unwrap();
        assert_eq!(pct, 50);
        assert_eq!(msg, "Phase B (0/10 files)");
    }

    #[test]
    fn direct_mode_after_counters_drops_counts_suffix() {
        let p = ScanProgress::new();
        p.set_scope(0, 100, "Scanning");
        p.add_total(10);
        p.add_done(5);
        let _ = p.take_report();

        p.set_percentage(90, "Warming caches");
        let (pct, msg) = p.take_report().unwrap();
        assert_eq!(pct, 90);
        assert_eq!(msg, "Warming caches");
    }
}
