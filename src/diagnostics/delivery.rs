//! Delivering diagnostics to the editor: debounced scheduling, the
//! two-phase publish, push and pull delivery, and the background worker.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use tower_lsp::lsp_types::*;

use crate::Backend;

use super::{ignore_rules, incremental, stale, suppression};

/// How long to wait after the last keystroke before publishing diagnostics.
const DIAGNOSTIC_DEBOUNCE_MS: u64 = 500;

impl Backend {
    /// Deliver diagnostics for a single file.
    ///
    /// Called from the background diagnostic worker after debouncing.
    ///
    /// **Phase 1 (instant):** Run fast collectors (syntax errors, unused
    /// imports/variables, namespace/class-name mismatches) and assemble
    /// them with *cached* slow and PHPStan results.  In push mode the
    /// merged set is published; in pull mode it is cached and a
    /// `workspace/diagnostic/refresh` is sent so the editor re-pulls.
    /// Either way the editor shows dimming within milliseconds.
    ///
    /// **Phase 2 (background):** Compute slow diagnostics, rebuild the
    /// full set (fast + fresh slow + cached PHPStan), and deliver it the
    /// same way: push mode publishes the complete set, replacing the
    /// Phase 1 snapshot; pull mode caches it, bumps the `resultId`, and
    /// sends another `workspace/diagnostic/refresh`.  When the edit since
    /// the last pass only touched some function bodies, only those are
    /// diagnosed again (see [`incremental`]).
    pub(crate) async fn publish_diagnostics_for_file(&self, uri_str: &str, content: &Arc<String>) {
        if self.should_skip_diagnostics(uri_str) {
            return;
        }

        // The native collectors are expensive (a full-file forward walk and
        // type resolution — hundreds of ms to several seconds on a large
        // file). They are pure CPU work and must never run inline on an async
        // worker: editors pull both `textDocument/diagnostic` and
        // `workspace/diagnostic` on every keystroke, and a handful of
        // concurrent inline computes will occupy every async runtime thread,
        // stalling delivery of completion and hover responses even though
        // those handlers themselves finished. Run them on the blocking pool.

        // ── Phase 1: collect and cache fast diagnostics ─────────────
        let fast_diagnostics = {
            let backend = self.clone_for_blocking();
            let uri = uri_str.to_string();
            let content = Arc::clone(content);
            crate::server::run_blocking_cancel_safe("fast diagnostics", move || {
                let mut out = Vec::new();
                let effective = backend
                    .blade_virtual_php_arc(&uri)
                    .unwrap_or_else(|| Arc::clone(&content));
                // Hand the parse cache the shared buffer, so the collector's
                // own guard finds it instead of copying the file.
                let _parse_guard = crate::parser::with_parse_cache_arc(Arc::clone(&effective));
                backend.collect_fast_diagnostics(&uri, &effective, &mut out);
                out
            })
            .await
            .unwrap_or_default()
        };

        // The collector above can take a while; re-check that the file is
        // still open immediately before writing so a close that landed
        // mid-compute can't resurrect the caches `clear_diagnostics_for_file`
        // already purged (same rationale as the external tool workers).
        if !self.open_files.read().contains_key(uri_str) {
            return;
        }

        {
            let mut cache = self.diag.last_fast.lock();
            cache.insert(uri_str.to_string(), fast_diagnostics.clone());
        }

        // Update the assembled cache immediately so pull-capable editors
        // can see fast diagnostics before the slower native passes finish.
        self.assemble_and_refresh(uri_str).await;

        // ── Phase 2: compute and cache slow diagnostics ─────────────
        let full_pass_requested = self.diag.full_slow_pass_requested.lock().remove(uri_str);
        let baseline = if full_pass_requested {
            None
        } else {
            self.diag.slow_baselines.lock().get(uri_str).cloned()
        };
        let slow_pass = {
            let backend = self.clone_for_blocking();
            let uri = uri_str.to_string();
            let content = Arc::clone(content);
            crate::server::run_blocking_cancel_safe("slow diagnostics", move || {
                // The resolved-class cache guard contains a thread-local raw
                // pointer; activate it on this blocking thread for the
                // duration of the synchronous collection only.
                let _cache_guard = crate::virtual_members::with_active_resolved_class_cache(
                    &backend.resolved_class_cache,
                );
                let started = std::time::Instant::now();
                let pass = backend.collect_slow_diagnostics_incremental(
                    &uri,
                    &content,
                    baseline.as_deref(),
                );
                let elapsed = started.elapsed();
                if elapsed >= std::time::Duration::from_millis(100) {
                    tracing::debug!(
                        target: "performance",
                        "PHPantom: slow diagnostics took {:?} ({:?})",
                        elapsed,
                        pass.kind
                    );
                }
                pass
            })
            .await
        };
        let (slow_diagnostics, new_baseline, bodies_read) = match slow_pass {
            Some(pass) => (
                pass.diagnostics,
                pass.baseline,
                Some((pass.kind, pass.bodies_read_from)),
            ),
            None => {
                // The full pass a save asked for did not happen; the next
                // pass still owes it.
                if full_pass_requested {
                    self.request_full_slow_pass(&[uri_str]);
                }
                (Vec::new(), None, None)
            }
        };

        // Re-check again: Phase 2 runs a full-file forward walk and can
        // take seconds, plenty of time for a close to land mid-compute.
        if !self.open_files.read().contains_key(uri_str) {
            return;
        }

        {
            let mut cache = self.diag.last_slow.lock();
            cache.insert(uri_str.to_string(), slow_diagnostics);
        }
        // A pass that cannot serve as a baseline leaves the previous one in
        // place: it still describes the text it was computed on, and the
        // next pass checks that nothing else moved since.
        if let Some(baseline) = new_baseline {
            self.diag
                .slow_baselines
                .lock()
                .insert(uri_str.to_string(), baseline);
        }
        // A pass that diagnosed only part of the file saw only that part's
        // reads; the rest of the file still reads what it read before.
        if let Some((kind, reads)) = bodies_read {
            let mut map = self.diag.bodies_read_from.lock();
            match kind {
                incremental::SlowPassKind::Full(_) => {
                    map.insert(uri_str.to_string(), reads);
                }
                _ => map.entry(uri_str.to_string()).or_default().extend(reads),
            }
        }

        // Update again with fresh slow results merged in.
        self.assemble_and_refresh(uri_str).await;
    }

    /// Assemble diagnostics from all per-source caches for a URI and
    /// deliver them to the editor.
    ///
    /// Every source (fast, slow, PHPStan, PHPCS, Mago lint, Mago
    /// analyze) caches its results independently.  This helper merges
    /// them into one set, deduplicates, and filters suppressions.
    ///
    /// **Push mode:** The merged set is published via
    /// `textDocument/publishDiagnostics`.
    ///
    /// **Pull mode:** Nothing is pushed.  The full merged set is cached
    /// in `diag_last_full` with a bumped `resultId` so the next pull
    /// response returns it; the caller triggers
    /// `workspace/diagnostic/refresh` so the editor re-pulls.  Editors
    /// that support pull diagnostics merge pushed and pulled sets
    /// additively, so pushing anything here would duplicate native
    /// diagnostics.
    ///
    /// Returns `true` in pull mode when the merged set differs from the
    /// one already cached, i.e. when the editor has something new to
    /// re-pull.  Every source calls this whenever it finishes, and most
    /// of those runs reproduce the previous result exactly (a file with
    /// no diagnostics stays that way through re-index, re-open, and
    /// every keystroke).  Bumping the `resultId` for an identical set
    /// would invalidate the client's cached result for nothing and cost
    /// a workspace-wide re-pull per run, so the caller uses the return
    /// value to skip the refresh.  Always `false` in push mode: the set
    /// is delivered inline, so there is nothing to re-pull.
    pub(crate) async fn assemble_and_push(&self, uri_str: &str) -> bool {
        let uri = match uri_str.parse::<Url>() {
            Ok(u) => u,
            Err(_) => return false,
        };

        // Serialize this URI's read-merge-write. Each per-source cache
        // below is locked and released independently, so without this a
        // second call that starts reading after this one can still finish
        // writing first (or vice versa), landing a merge based on a
        // stale snapshot after a fresher one. Dropped before the
        // push-mode `.await` below since it guards only the synchronous
        // merge and the pull-mode cache write.
        let assemble_lock = {
            let mut locks = self.diag.assemble_locks.lock();
            Arc::clone(
                locks
                    .entry(uri_str.to_string())
                    .or_insert_with(|| Arc::new(parking_lot::Mutex::new(()))),
            )
        };
        let assemble_guard = assemble_lock.lock();

        // ── Read all per-source caches ──────────────────────────────
        let mut full = Vec::new();

        {
            let cache = self.diag.last_fast.lock();
            if let Some(fast) = cache.get(uri_str) {
                full.extend(fast.iter().cloned());
            }
        }
        {
            let cache = self.diag.last_slow.lock();
            if let Some(slow) = cache.get(uri_str) {
                full.extend(slow.iter().cloned());
            }
        }

        let phpstan_before: Vec<Diagnostic> = {
            let cache = self.phpstan_tool.last_diags.lock();
            cache.get(uri_str).cloned().unwrap_or_default()
        };

        // Eagerly prune stale PHPStan diagnostics against current file
        // content (e.g. an added `@phpstan-ignore` comment) — see
        // `stale::is_stale_phpstan_diagnostic` for the specific checks.
        if !phpstan_before.is_empty() {
            let content: Option<Arc<String>> = self.open_files.read().get(uri_str).cloned();
            // The checks that read the member a diagnostic points at share
            // one parse of the file.
            let _parse_guard = content.clone().map(crate::parser::with_parse_cache_arc);
            let filtered: Vec<Diagnostic> = phpstan_before
                .iter()
                .filter(|d| {
                    if let Some(ref text) = content {
                        !stale::is_stale_phpstan_diagnostic(d, text)
                    } else {
                        true
                    }
                })
                .cloned()
                .collect();
            if filtered.len() != phpstan_before.len() {
                let mut cache = self.phpstan_tool.last_diags.lock();
                cache.insert(uri_str.to_string(), filtered.clone());
            }
            full.extend(filtered);
        }

        {
            let cache = self.phpcs_tool.last_diags.lock();
            if let Some(phpcs_diags) = cache.get(uri_str) {
                full.extend(phpcs_diags.iter().cloned());
            }
        }
        {
            let cache = self.phpmd_tool.last_diags.lock();
            if let Some(phpmd_diags) = cache.get(uri_str) {
                full.extend(phpmd_diags.iter().cloned());
            }
        }
        {
            let cache = self.mago_lint_tool.last_diags.lock();
            if let Some(mago_diags) = cache.get(uri_str) {
                full.extend(mago_diags.iter().cloned());
            }
        }
        {
            let cache = self.mago_analyze_tool.last_diags.lock();
            if let Some(mago_diags) = cache.get(uri_str) {
                full.extend(mago_diags.iter().cloned());
            }
        }

        // ── Suppress imprecise overlaps and filter ──────────────────
        suppression::suppress_imprecise_overlaps(&mut full);
        let mut full = self.filter_suppressed(full);

        // ── Apply @phpantom-ignore comment suppression ─────────────
        {
            let content: Option<Arc<String>> = self.open_files.read().get(uri_str).cloned();
            if let Some(ref text) = content {
                suppression::filter_ignored_by_comment(&mut full, text);
            }
        }

        // ── Apply [[diagnostics.ignore]] config rules ────────────────
        {
            let rules = ignore_rules::compile_ignore_rules(
                &self.workspace.config.lock().diagnostics.ignore,
            );
            if !rules.is_empty()
                && let Some(file_path) = crate::util::url_to_path(&uri)
                && let Some(root) = self.workspace.workspace_root.read().clone()
            {
                let relative_path = file_path
                    .strip_prefix(&root)
                    .unwrap_or(&file_path)
                    .to_string_lossy()
                    .replace('\\', "/");
                ignore_rules::filter_ignored_by_config(&mut full, &relative_path, &rules);
            }
        }

        // If suppression removed any full-line PHPStan diagnostics
        // (because a precise native diagnostic covers the same line),
        // prune them from the PHPStan cache too so they don't resurface.
        if !phpstan_before.is_empty() {
            let pruned: Vec<Diagnostic> = phpstan_before
                .into_iter()
                .filter(|d| full.iter().any(|f| f.range == d.range))
                .collect();
            let mut cache = self.phpstan_tool.last_diags.lock();
            cache.insert(uri_str.to_string(), pruned);
        }

        let pull_mode = self.caps().pull_diagnostics;

        if pull_mode {
            // ── Pull mode ───────────────────────────────────────────
            // Pull-capable clients use `textDocument/diagnostic` as the
            // single source of truth for native diagnostics.  We only cache
            // the merged set here; the caller triggers
            // `workspace/diagnostic/refresh` so the editor re-pulls.  This
            // avoids duplicate diagnostics in clients that keep pushed and
            // pulled diagnostics in separate namespaces.
            //
            // A run that reproduces the cached set leaves both the cache
            // and the `resultId` alone, so a pull still answers
            // `Unchanged` and the caller skips the refresh.  A missing
            // cache entry counts as changed: the client has nothing yet.
            //
            // The set and its `resultId` change under one `last_full`
            // guard, so a racing pull never sees the new set paired with
            // the old id (which would answer `Unchanged` for a set that
            // just changed).
            let mut cache = self.diag.last_full.lock();
            if cache.get(uri_str).is_some_and(|prev| *prev == full) {
                return false;
            }
            cache.insert(uri_str.to_string(), full);
            let id = self.diag.result_id_seq.fetch_add(1, Ordering::Relaxed) + 1;
            self.diag.result_ids.lock().insert(uri_str.to_string(), id);
            true
        } else {
            // ── Push mode ───────────────────────────────────────────
            let Some(client) = &self.client else {
                return false;
            };
            // Nothing left to serialize: push mode caches nothing, it
            // only publishes the merged set inline.
            drop(assemble_guard);
            client.publish_diagnostics(uri, full, None).await;
            false
        }
    }

    /// Ask the editor to re-pull diagnostics.
    ///
    /// A no-op for push-mode clients, which receive their diagnostics
    /// directly.  LSP has no per-document refresh, so this invalidates
    /// the client's whole workspace result set: only call it when
    /// something actually changed (see [`Self::assemble_and_push`]).
    ///
    /// The refresh is signalled to a pump task rather than sent here.
    /// `workspace/diagnostic/refresh` is a server-to-client *request*,
    /// and tower-lsp panics the serve loop — killing the whole server —
    /// if the client's response arrives after the future awaiting it was
    /// dropped, so it must never be raced against a timeout or awaited
    /// from anything cancellable.  The pump owns each request from send
    /// to response; callers return immediately, a busy client parks
    /// nothing but the pump itself, and signals landing while a refresh
    /// is in flight coalesce into a single follow-up.
    pub(crate) async fn request_diagnostic_refresh(&self) {
        if !self.caps().pull_diagnostics {
            return;
        }
        let Some(client) = &self.client else {
            return;
        };
        if !self.diag.refresh_pump_started.swap(true, Ordering::AcqRel) {
            let client = client.clone();
            let notify = std::sync::Arc::clone(&self.diag.refresh_notify);
            tokio::spawn(async move {
                loop {
                    notify.notified().await;
                    let _ = client.workspace_diagnostic_refresh().await;
                }
            });
        }
        self.diag.refresh_notify.notify_one();
    }

    /// Assemble a URI's diagnostics and ask the editor to re-pull when
    /// the merged set changed.
    pub(crate) async fn assemble_and_refresh(&self, uri_str: &str) {
        if self.assemble_and_push(uri_str).await {
            self.request_diagnostic_refresh().await;
        }
    }

    /// Notify the diagnostic system that a file needs fresh native
    /// diagnostics.
    ///
    /// Queues the file for the debounced background diagnostic worker in
    /// both push and pull mode: in push mode the worker publishes the
    /// full assembled set, in pull mode it caches the full set for the
    /// next `textDocument/diagnostic` response.  External tool runs
    /// (PHPStan, PHPCS, Mago) are scheduled separately, by
    /// [`Self::schedule_external_diagnostics`], since they are expensive
    /// and only run on save.
    ///
    /// This returns immediately — all diagnostic computation happens
    /// in the background so that completion, hover, and signature help
    /// are never blocked.
    pub(crate) fn schedule_diagnostics(&self, uri: String) {
        // Don't schedule diagnostics before initialization is complete.
        // Files opened during startup will be diagnosed once
        // `initialized` sets `init_complete` and re-schedules them.
        if !self.init_complete.load(Ordering::Acquire) {
            return;
        }

        // In pull mode we deliberately KEEP the previously cached full set.
        // The pull handler no longer computes on the request path (that
        // wedged the transport — see `trigger_diagnostics_for_pull`), so the
        // editor would otherwise flicker to empty between an edit and the
        // background recompute. Keeping the stale set means a pull returns
        // the old diagnostics until the worker recomputes, bumps the
        // resultId, and requests a refresh, which makes the editor re-pull
        // the fresh set. The resultId is bumped only by `assemble_and_push`,
        // so the "unchanged" fast-path stays correct.

        // Both modes: queue for the debounced background worker.
        // In push mode the worker pushes the full assembled set.
        // In pull mode the worker pushes only fast diagnostics and
        // caches the full set in `diag_last_full` for pull responses.
        {
            let mut pending = self.diag.pending_uris.lock();
            pending.insert(uri.clone());
        }
        self.diag.version.fetch_add(1, Ordering::Release);
        self.diag.notify.notify_one();

        // External tools (PHPStan, PHPCS, Mago) are NOT scheduled here.
        // They are expensive (seconds per run) and would block save-
        // triggered runs due to the serialization guarantee (one process
        // at a time).
    }

    /// Schedule all external tool runs (PHPStan, PHPCS, PHPMD, Mago) for a
    /// single file.
    ///
    /// External tools are expensive (seconds per run) and at most one
    /// process runs at a time per tool, so triggering them on every
    /// keystroke would block save-triggered runs.
    pub(crate) fn schedule_external_diagnostics(&self, uri: String) {
        if !self.init_complete.load(Ordering::Acquire) {
            return;
        }
        self.schedule_phpstan(uri.clone());
        self.schedule_phpcs(uri.clone());
        self.schedule_phpmd(uri.clone());
        self.schedule_mago_lint(uri.clone());
        self.schedule_mago_analyze(uri);
    }

    /// Make the next slow pass of each of `uris` diagnose the whole file.
    ///
    /// A save is the point where everything is brought back in line: a
    /// partial pass keeps what the rest of the file reported, and an edit
    /// can reach that through a project-wide registry the file feeds (see
    /// [`incremental`]).
    pub(crate) fn request_full_slow_pass<S: AsRef<str>>(&self, uris: &[S]) {
        let mut requested = self.diag.full_slow_pass_requested.lock();
        requested.extend(uris.iter().map(|uri| uri.as_ref().to_string()));
    }

    /// Invalidate diagnostics for the open files a save can affect.
    ///
    /// Diagnostics in other open files (unknown member, unknown class,
    /// deprecated usage, argument checks) can depend on the saved file, so
    /// they have to be recomputed — but only for the files that reference
    /// something the save changed.  [`open_files_affected_by_save`] works
    /// out which those are, and falls back to every open file when it
    /// cannot tell.  The saved file itself is excluded (it is already
    /// scheduled by the caller).
    ///
    /// Queues those files for the debounced background worker in both
    /// modes.  In pull mode, the cached full diagnostic set for each
    /// file is also invalidated up front (its resultId is kept) so a
    /// pull that lands before the worker finishes triggers a fresh
    /// computation instead of returning diagnostics from before the
    /// save.
    ///
    /// [`open_files_affected_by_save`]: Backend::open_files_affected_by_save
    pub(crate) fn schedule_diagnostics_for_open_files(&self, exclude_uri: &str) {
        if !self.init_complete.load(Ordering::Acquire) {
            return;
        }

        let pull_mode = self.caps().pull_diagnostics;

        let uris = self.open_files_affected_by_save(exclude_uri);
        if uris.is_empty() {
            return;
        }

        if pull_mode {
            // Invalidate cached full diagnostics so the next pull
            // triggers a fresh computation.  Do NOT remove resultIds —
            // see the comment in schedule_diagnostics for why.
            let mut cache = self.diag.last_full.lock();
            for uri in &uris {
                cache.remove(uri);
            }
        }

        // These files are re-diagnosed because something they depend on
        // changed, so nothing of their last pass may be kept.
        self.request_full_slow_pass(&uris);

        // Both modes: queue all files for the debounced background worker.
        // In push mode the worker pushes the full assembled set.
        // In pull mode the worker pushes only fast diagnostics and
        // caches the full set in `diag_last_full` for pull responses.
        {
            let mut pending = self.diag.pending_uris.lock();
            pending.extend(uris);
        }
        self.diag.version.fetch_add(1, Ordering::Release);
        self.diag.notify.notify_one();
    }

    /// Ensure fresh native diagnostics get computed for a file (pull-mode
    /// path).
    ///
    /// Called from the pull handler (`textDocument/diagnostic`) when the
    /// cached full diagnostics are stale or missing.  Does **not**
    /// compute synchronously; it only queues the file for the debounced
    /// background worker and returns immediately (see the comment inside
    /// for why). The pull handler reads whatever is currently cached in
    /// `diag_last_full`, which may be stale until the worker finishes and
    /// requests a `workspace/diagnostic/refresh`.
    pub(crate) fn trigger_diagnostics_for_pull(&self, uri_str: &str) {
        // Don't compute diagnostics before initialization is complete.
        // The pull handler will return empty results; once `initialized`
        // finishes it schedules all open files which populates the cache.
        if !self.init_complete.load(Ordering::Acquire) {
            return;
        }

        if self.should_skip_diagnostics(uri_str) {
            return;
        }

        // NEVER compute on the request path. The native pass takes seconds on
        // a large file, and editors pull both `textDocument/diagnostic` and
        // `workspace/diagnostic` on every keystroke — far faster than the
        // compute drains. A synchronous pull holds a tower-lsp concurrency
        // slot for the whole compute, so a typing burst fills every slot and
        // wedges the transport (it can no longer even read `$/cancelRequest`).
        //
        // Instead, ensure the debounced background worker will (re)compute
        // this file off the async runtime; it caches the full set and
        // requests a workspace diagnostic refresh when done. This pull
        // returns whatever is currently cached (stale results are kept until
        // the refresh, so the editor never flickers to empty).
        {
            let mut pending = self.diag.pending_uris.lock();
            pending.insert(uri_str.to_string());
        }
        self.diag.notify.notify_one();
    }

    /// Long-lived background task that processes diagnostic requests.
    ///
    /// Active in both push and pull modes.  In push mode, the worker
    /// publishes the full assembled diagnostic set via
    /// `publishDiagnostics`.  In pull mode, nothing is pushed: it
    /// caches the full set in `diag_last_full` and asks the editor to
    /// re-pull via `workspace/diagnostic/refresh` (see
    /// [`assemble_and_push`]).
    ///
    /// Spawned once during `initialized`.  Loops forever, waiting for
    /// [`schedule_diagnostics`](Self::schedule_diagnostics) to signal
    /// new work.  On each iteration:
    ///
    /// 1. Wait for a notification (new edit arrived).
    /// 2. Debounce: sleep [`DIAGNOSTIC_DEBOUNCE_MS`], then check
    ///    whether the version counter moved (more edits).  If so,
    ///    loop back to step 2.
    /// 3. Snapshot the pending URIs and each one's current file content.
    /// 4. Run the diagnostic collectors and publish results for each URI.
    /// 5. Loop back to step 1.
    ///
    /// Because there is exactly one instance of this task, at most one
    /// diagnostic pass runs at a time.  If edits arrive during step 4
    /// the version counter will have moved, and step 1 picks up
    /// immediately after step 4 finishes — giving the two-slot
    /// (one running + one pending) behaviour.
    pub(crate) async fn diagnostic_worker(&self) {
        loop {
            if self.shutdown_flag.load(Ordering::Acquire) {
                return;
            }

            // ── Step 1: wait for work ───────────────────────────────
            self.diag.notify.notified().await;

            if self.shutdown_flag.load(Ordering::Acquire) {
                return;
            }

            // ── Step 2: debounce ────────────────────────────────────
            loop {
                let version_before = self.diag.version.load(Ordering::Acquire);
                tokio::time::sleep(std::time::Duration::from_millis(DIAGNOSTIC_DEBOUNCE_MS)).await;
                let version_after = self.diag.version.load(Ordering::Acquire);
                if version_before == version_after {
                    // No new edits during the sleep — proceed.
                    break;
                }
                // More edits arrived — loop and debounce again.
            }

            // ── Step 3: snapshot all pending URIs ────────────────────
            let uris: std::collections::HashSet<String> = {
                let mut pending = self.diag.pending_uris.lock();
                std::mem::take(&mut *pending)
            };
            if uris.is_empty() {
                continue;
            }

            // ── Step 4: collect and publish for each URI ────────────
            // Snapshot content for each URI individually, releasing the
            // read lock before each async publish call so that
            // `did_change` is never blocked.
            for uri in &uris {
                let content = {
                    let files = self.open_files.read();
                    match files.get(uri) {
                        Some(c) => c.clone(),
                        None => continue,
                    }
                };
                self.publish_diagnostics_for_file(uri, &content).await;
            }
        }
    }

    /// Clear diagnostics for a file (e.g. on `did_close`).
    pub(crate) async fn clear_diagnostics_for_file(&self, uri_str: &str) {
        // Migrate the live external tool results into the workspace
        // diagnostics store before purging, so a closed file keeps its
        // PHPStan/PHPCS/Mago findings instead of losing them until the
        // next project-wide run.  Only meaningful once the background
        // workspace pass has started.
        if self
            .diag
            .workspace_diag_pass_started
            .load(Ordering::Acquire)
        {
            let migrations: [(&'static str, Vec<Diagnostic>); 5] = [
                (
                    "phpstan",
                    self.phpstan_tool
                        .last_diags
                        .lock()
                        .get(uri_str)
                        .cloned()
                        .unwrap_or_default(),
                ),
                (
                    "phpcs",
                    self.phpcs_tool
                        .last_diags
                        .lock()
                        .get(uri_str)
                        .cloned()
                        .unwrap_or_default(),
                ),
                (
                    "phpmd",
                    self.phpmd_tool
                        .last_diags
                        .lock()
                        .get(uri_str)
                        .cloned()
                        .unwrap_or_default(),
                ),
                (
                    "mago-lint",
                    self.mago_lint_tool
                        .last_diags
                        .lock()
                        .get(uri_str)
                        .cloned()
                        .unwrap_or_default(),
                ),
                (
                    "mago-analyze",
                    self.mago_analyze_tool
                        .last_diags
                        .lock()
                        .get(uri_str)
                        .cloned()
                        .unwrap_or_default(),
                ),
            ];
            let mut ws = self.diag.workspace_diags.lock();
            for (source, diags) in migrations {
                ws.set_external_for_uri(source, uri_str, diags);
            }
        }

        // Remove all per-source caches so we don't leak memory.
        self.diag.last_fast.lock().remove(uri_str);
        self.diag.last_slow.lock().remove(uri_str);
        self.diag.slow_baselines.lock().remove(uri_str);
        self.diag.bodies_read_from.lock().remove(uri_str);
        self.diag.full_slow_pass_requested.lock().remove(uri_str);
        self.diag.epoch.forget(uri_str);
        // Remove cached PHPStan, PHPCS, PHPMD, and Mago diagnostics too.
        self.phpstan_tool.forget(uri_str);
        self.phpcs_tool.forget(uri_str);
        self.phpmd_tool.forget(uri_str);
        self.mago_lint_tool.forget(uri_str);
        self.mago_analyze_tool.forget(uri_str);
        // Remove pull-diagnostic caches.
        self.diag.result_ids.lock().remove(uri_str);
        self.diag.last_full.lock().remove(uri_str);
        // Drop the per-URI assemble lock so the map doesn't grow
        // unboundedly across a long editing session; a fresh one is
        // created on demand if the URI is reopened.
        self.diag.assemble_locks.lock().remove(uri_str);

        let client = match &self.client {
            Some(c) => c,
            None => return,
        };

        let uri = match uri_str.parse::<Url>() {
            Ok(u) => u,
            Err(_) => return,
        };

        // Always push empty diagnostics to clear any Phase 1 snapshot.
        client.publish_diagnostics(uri, Vec::new(), None).await;

        // Tell the editor to re-pull diagnostics.  Signalling the pump
        // returns immediately, so a burst of didClose notifications
        // cannot deadlock the service loop the way awaiting each
        // response here would.
        self.request_diagnostic_refresh().await;

        // Recompute the file's workspace diagnostics from disk so the
        // closed file's entry reflects the saved state (the startup
        // snapshot may be stale after in-editor edits).
        if self
            .diag
            .workspace_diag_pass_started
            .load(Ordering::Acquire)
        {
            let backend = self.clone_for_blocking();
            let uri = uri_str.to_string();
            tokio::spawn(async move {
                backend
                    .recompute_workspace_diags_for_closed_file(&uri)
                    .await;
            });
        }
    }
}
