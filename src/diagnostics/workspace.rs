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
//! 1. **Native pass** — the same fast + slow collectors that diagnose
//!    open files run over every unopened user file, on a throttled
//!    worker pool (half the cores) so interactive requests stay
//!    responsive.  Results stream to the editor in batches.
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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use tower_lsp::lsp_types::Diagnostic;

use crate::Backend;
use crate::diagnostics::ignore_rules::{self, CompiledIgnoreRule};
use crate::progress::ScanProgress;

/// Number of files processed per blocking batch.  Between batches the
/// async orchestrator updates progress, streams new results to the
/// editor, and checks the shutdown flag.
const NATIVE_BATCH_SIZE: usize = 128;

/// Minimum time between streaming deliveries while the native pass is
/// running.  A `workspace/diagnostic/refresh` makes the editor re-pull
/// every file it knows about, so refreshing after every batch would be
/// wasteful.
const DELIVERY_INTERVAL: Duration = Duration::from_secs(3);

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

    /// Replace one external tool's entire result set.  Returns the
    /// URIs whose diagnostics changed (old entries cleared by the new
    /// run are included so the editor drops them).
    pub(crate) fn set_external(
        &mut self,
        source: &'static str,
        results: HashMap<String, Vec<Diagnostic>>,
    ) -> Vec<String> {
        let entry = self.external.entry(source).or_default();
        let mut updated: HashSet<String> = entry.keys().cloned().collect();
        updated.extend(results.keys().cloned());
        *entry = results;
        let updated: Vec<String> = updated.into_iter().collect();
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
        super::suppress_imprecise_overlaps(&mut out);
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
}

impl Backend {
    /// Run the background workspace diagnostics pass.
    ///
    /// Called from the tail of the full background index task, so the
    /// whole workspace is already parsed when this starts.  Guarded
    /// against duplicate invocation; waits for `init_complete` so the
    /// post-index cache clears in `initialized` cannot race the pass.
    pub(crate) async fn run_workspace_diagnostics(&self) {
        if !self.config().diagnostics.workspace_enabled() {
            return;
        }
        if self.workspace_root.read().is_none() {
            return;
        }
        if self
            .workspace_diag_pass_started
            .swap(true, Ordering::AcqRel)
        {
            return;
        }

        // Wait for `initialized` to finish (it clears resolution caches
        // after the startup scan; starting before that would waste the
        // eager class population below).
        while !self.init_complete.load(Ordering::Acquire) {
            if self.shutdown_flag.load(Ordering::Acquire) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
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

        let diagnosed = self.run_native_workspace_pass(&progress).await;

        if self.config().diagnostics.workspace_external_enabled()
            && !self.shutdown_flag.load(Ordering::Acquire)
        {
            self.run_workspace_external_tools(&progress).await;
        }

        if let Some(poller) = poller {
            poller.finish().await;
        }
        if let Some(ref tok) = progress_token {
            self.progress_end(tok, Some(format!("Diagnosed {} files", diagnosed)))
                .await;
        }
    }

    /// Run the native collectors over every unopened user file.
    ///
    /// Returns the number of files diagnosed.  Results stream to the
    /// editor between batches, throttled by [`DELIVERY_INTERVAL`].
    pub(crate) async fn run_native_workspace_pass(&self, progress: &ScanProgress) -> usize {
        // ── Eager class population ──────────────────────────────────
        // Resolve every known class in dependency-first order so the
        // per-file collectors below hit a warm cache instead of
        // recursing into class resolution.  Same approach as the CLI
        // analyse pipeline.
        progress.set_percentage(1, "Resolving classes");
        {
            let backend = self.clone_for_blocking();
            crate::server::run_blocking_cancel_safe(move || {
                let sorted_fqns = {
                    let uri_classes_index = backend.uri_classes_index.read();
                    crate::toposort::toposort_from_uri_classes_index(&uri_classes_index)
                };
                // Dedicated large-stack thread: class resolution can
                // nest deeply when the toposort misses dependencies.
                std::thread::scope(|s| {
                    let backend = &backend;
                    let sorted_fqns = &sorted_fqns;
                    std::thread::Builder::new()
                        .name("ws-diag-populate".into())
                        .stack_size(crate::PARSE_WORKER_STACK_SIZE)
                        .spawn_scoped(s, move || {
                            let class_loader = |name: &str| backend.find_or_load_class(name);
                            crate::virtual_members::populate_from_sorted(
                                sorted_fqns,
                                &backend.resolved_class_cache,
                                &class_loader,
                            );
                        })
                        .expect("failed to spawn ws-diag-populate thread");
                });
            })
            .await;
        }

        // ── Per-file diagnostics, batched ───────────────────────────
        let mut uris = self.workspace_diagnostic_target_uris();
        uris.sort();
        let total = uris.len();
        let ignore_rules = Arc::new(ignore_rules::compile_ignore_rules(
            &self.config().diagnostics.ignore,
        ));

        let mut done = 0usize;
        let mut pending_updates: Vec<String> = Vec::new();
        let mut last_delivery = Instant::now();

        for chunk in uris.chunks(NATIVE_BATCH_SIZE) {
            if self.shutdown_flag.load(Ordering::Acquire) {
                return done;
            }

            let batch: Vec<String> = chunk.to_vec();
            let backend = self.clone_for_blocking();
            let rules = Arc::clone(&ignore_rules);
            let results = crate::server::run_blocking_cancel_safe(move || {
                backend.collect_workspace_batch(&batch, &rules)
            })
            .await
            .unwrap_or_default();

            done += chunk.len();
            // The native pass maps into 1..80 of the progress bar; the
            // external tool runs that follow use the remaining 80..100.
            progress.set_percentage(
                (1 + done * 79 / total.max(1)) as u32,
                format!("Checking files ({done}/{total})"),
            );

            // Store results; skip files that were opened mid-pass (the
            // live pipeline owns them now).
            {
                let open = self.open_files.read();
                let mut ws = self.workspace_diags.lock();
                for (uri, diags) in results {
                    if open.contains_key(&uri) {
                        continue;
                    }
                    if ws.set_native(&uri, diags) {
                        pending_updates.push(uri);
                    }
                }
            }

            let finished = done >= total;
            if !pending_updates.is_empty()
                && (finished || last_delivery.elapsed() >= DELIVERY_INTERVAL)
            {
                self.flush_workspace_diag_updates(std::mem::take(&mut pending_updates))
                    .await;
                last_delivery = Instant::now();
            }
        }

        done
    }

    /// The URIs the workspace pass should diagnose: every parsed user
    /// file that is not a stub, not under a vendor directory, and not
    /// currently open in the editor.
    fn workspace_diagnostic_target_uris(&self) -> Vec<String> {
        let vendor_prefixes = self.vendor_uri_prefixes.lock().clone();
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

    /// Diagnose a batch of files on a throttled worker pool.
    ///
    /// Uses half the available cores so interactive requests (hover,
    /// completion) stay responsive while the pass runs in the
    /// background.  Workers get [`crate::PARSE_WORKER_STACK_SIZE`]
    /// stacks because they parse and walk PHP ASTs.
    fn collect_workspace_batch(
        &self,
        uris: &[String],
        ignore_rules: &[CompiledIgnoreRule],
    ) -> Vec<(String, Vec<Diagnostic>)> {
        let n_threads = std::thread::available_parallelism()
            .map(|n| (n.get() / 2).max(1))
            .unwrap_or(2)
            .min(uris.len().max(1));

        let next_idx = AtomicUsize::new(0);

        std::thread::scope(|s| {
            let handles: Vec<_> = (0..n_threads)
                .map(|_| {
                    let backend = self;
                    let next_idx = &next_idx;
                    std::thread::Builder::new()
                        .name("ws-diag-worker".into())
                        .stack_size(crate::PARSE_WORKER_STACK_SIZE)
                        .spawn_scoped(s, move || {
                            let mut results: Vec<(String, Vec<Diagnostic>)> = Vec::new();
                            loop {
                                let i = next_idx.fetch_add(1, Ordering::Relaxed);
                                if i >= uris.len() {
                                    break;
                                }
                                let uri = &uris[i];
                                if backend.shutdown_flag.load(Ordering::Acquire) {
                                    break;
                                }
                                if let Some(diags) =
                                    backend.collect_workspace_file_diagnostics(uri, ignore_rules)
                                {
                                    results.push((uri.clone(), diags));
                                }
                            }
                            results
                        })
                        .expect("failed to spawn ws-diag-worker thread")
                })
                .collect();

            handles
                .into_iter()
                .flat_map(|h| h.join().unwrap_or_default())
                .collect()
        })
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
        let blade_content;
        let effective: &str = if self.is_blade_file(uri) {
            if let Some(vc) = self.blade_virtual_content.read().get(uri) {
                blade_content = vc.clone();
                &blade_content
            } else {
                &content
            }
        } else {
            &content
        };

        crate::util::catch_panic_unwind_safe("workspace_diagnostics", uri, None, || {
            let _parse_guard = crate::parser::with_parse_cache(effective);
            let _cache_guard = crate::virtual_members::with_active_resolved_class_cache(
                &self.resolved_class_cache,
            );

            let mut out = Vec::new();
            self.collect_fast_diagnostics(uri, effective, &mut out);
            self.collect_slow_diagnostics(uri, effective, &mut out);

            super::suppress_imprecise_overlaps(&mut out);
            super::filter_ignored_by_comment(&mut out, effective);
            if !ignore_rules.is_empty()
                && let Some(relative) = self.workspace_relative_path(uri)
            {
                ignore_rules::filter_ignored_by_config(&mut out, &relative, ignore_rules);
            }
            out
        })
    }

    /// The `/`-separated path of `uri` relative to the workspace root,
    /// for matching `[[diagnostics.ignore]]` path globs.
    fn workspace_relative_path(&self, uri: &str) -> Option<String> {
        let path = uri
            .parse::<tower_lsp::lsp_types::Url>()
            .ok()?
            .to_file_path()
            .ok()?;
        let root = self.workspace_root.read().clone()?;
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
            let _ = client.workspace_diagnostic_refresh().await;
            return;
        }

        for uri_str in updated {
            if self.open_files.read().contains_key(&uri_str) {
                continue;
            }
            let Ok(uri) = uri_str.parse::<tower_lsp::lsp_types::Url>() else {
                continue;
            };
            let diags = self.workspace_diags.lock().merged(&uri_str);
            client.publish_diagnostics(uri, diags, None).await;
        }
    }

    /// Recompute one file's workspace diagnostics from disk.
    ///
    /// Called after `did_close` so the closed file's entry reflects the
    /// on-disk state instead of the startup snapshot.  A file that can
    /// no longer be read (deleted) has its entry cleared.
    pub(crate) async fn recompute_workspace_diags_for_closed_file(&self, uri: &str) {
        if !self.workspace_diag_pass_started.load(Ordering::Acquire) {
            return;
        }
        // Only user files participate in workspace diagnostics.
        let vendor_prefixes = self.vendor_uri_prefixes.lock().clone();
        if uri.starts_with("phpantom-stub")
            || vendor_prefixes.iter().any(|p| uri.starts_with(p.as_str()))
        {
            return;
        }

        let backend = self.clone_for_blocking();
        let uri_owned = uri.to_string();
        let diags = crate::server::run_blocking_cancel_safe(move || {
            let rules = ignore_rules::compile_ignore_rules(&backend.config().diagnostics.ignore);
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

        let changed = self.workspace_diags.lock().set_native(uri, diags);
        if changed {
            self.flush_workspace_diag_updates(vec![uri.to_string()])
                .await;
        }
    }

    // ── External tools ──────────────────────────────────────────────

    /// Run each enabled external tool once over the whole project and
    /// store the results, delivering after each tool completes.
    async fn run_workspace_external_tools(&self, progress: &ScanProgress) {
        let Some(root) = self.workspace_root.read().clone() else {
            return;
        };
        let config = self.config();
        let bin_dir: Option<String> = crate::composer::read_composer_package(&root)
            .map(|pkg| crate::composer::get_bin_dir(&pkg));

        // ── PHPStan ─────────────────────────────────────────────────
        if !config.phpstan.is_disabled()
            && crate::phpstan::has_project_config(&root)
            && let Some(resolved) =
                crate::phpstan::resolve_phpstan(Some(&root), &config.phpstan, bin_dir.as_deref())
        {
            progress.set_percentage(80, "Running PHPStan (project-wide)");
            let phpstan_config = config.phpstan.clone();
            let shutdown = Arc::clone(&self.shutdown_flag);
            let root_clone = root.clone();
            let result = crate::server::run_blocking_cancel_safe(move || {
                crate::phpstan::run_phpstan_workspace(
                    &resolved,
                    &root_clone,
                    &phpstan_config,
                    &shutdown,
                )
            })
            .await;
            if let Some(Ok(map)) = result {
                self.store_workspace_external_results("phpstan", map).await;
            }
        }

        if self.shutdown_flag.load(Ordering::Acquire) {
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
            let result = crate::server::run_blocking_cancel_safe(move || {
                crate::phpcs::run_phpcs_workspace(&resolved, &root_clone, &phpcs_config, &shutdown)
            })
            .await;
            if let Some(Ok(map)) = result {
                self.store_workspace_external_results("phpcs", map).await;
            }
        }

        if self.shutdown_flag.load(Ordering::Acquire) {
            return;
        }

        // ── Mago lint + analyze ─────────────────────────────────────
        if !config.mago.is_disabled()
            && crate::mago::has_mago_config(&root)
            && let Some(resolved) =
                crate::mago::resolve_mago(Some(&root), &config.mago, bin_dir.as_deref())
        {
            progress.set_percentage(90, "Running Mago lint (project-wide)");
            let mago_config = config.mago.clone();
            let shutdown = Arc::clone(&self.shutdown_flag);
            let root_clone = root.clone();
            let resolved_clone = resolved.clone();
            let result = crate::server::run_blocking_cancel_safe(move || {
                crate::mago::run_mago_lint_workspace(
                    &resolved_clone,
                    &root_clone,
                    &mago_config,
                    &shutdown,
                )
            })
            .await;
            if let Some(Ok(map)) = result {
                self.store_workspace_external_results("mago-lint", map)
                    .await;
            }

            if self.shutdown_flag.load(Ordering::Acquire) {
                return;
            }

            progress.set_percentage(95, "Running Mago analyze (project-wide)");
            let mago_config = config.mago.clone();
            let shutdown = Arc::clone(&self.shutdown_flag);
            let root_clone = root.clone();
            let result = crate::server::run_blocking_cancel_safe(move || {
                crate::mago::run_mago_analyze_workspace(
                    &resolved,
                    &root_clone,
                    &mago_config,
                    &shutdown,
                )
            })
            .await;
            if let Some(Ok(map)) = result {
                self.store_workspace_external_results("mago-analyze", map)
                    .await;
            }
        }
    }

    /// Store a project-wide external tool run's results and deliver.
    ///
    /// Results for files currently open feed the live per-file source
    /// caches instead (so the open buffer shows them immediately);
    /// everything else goes into the workspace store.  Config ignore
    /// rules are applied per file.
    async fn store_workspace_external_results(
        &self,
        source: &'static str,
        results: HashMap<PathBuf, Vec<Diagnostic>>,
    ) {
        let rules = ignore_rules::compile_ignore_rules(&self.config().diagnostics.ignore);
        let root = self.workspace_root.read().clone();

        let mut workspace_results: HashMap<String, Vec<Diagnostic>> = HashMap::new();
        let mut open_results: Vec<(String, Vec<Diagnostic>)> = Vec::new();
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
                if diags.is_empty() {
                    continue;
                }
                let uri = crate::util::path_to_uri(&path);
                if open.contains_key(&uri) {
                    open_results.push((uri, diags));
                } else {
                    workspace_results.insert(uri, diags);
                }
            }
        }

        let updated = self
            .workspace_diags
            .lock()
            .set_external(source, workspace_results);
        self.flush_workspace_diag_updates(updated).await;

        // Feed open files through the live per-file pipeline so the
        // results appear in the open buffers immediately.
        if open_results.is_empty() {
            return;
        }
        for (uri, diags) in open_results {
            let cache = match source {
                "phpstan" => &self.phpstan_tool.last_diags,
                "phpcs" => &self.phpcs_tool.last_diags,
                "mago-lint" => &self.mago_lint_tool.last_diags,
                "mago-analyze" => &self.mago_analyze_tool.last_diags,
                _ => continue,
            };
            cache.lock().insert(uri.clone(), diags);
            self.assemble_and_push(&uri).await;
        }
        if let Some(client) = &self.client
            && self
                .supports_pull_diagnostics
                .load(std::sync::atomic::Ordering::Acquire)
        {
            let _ = client.workspace_diagnostic_refresh().await;
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
        assert!(ws.merged("file:///a.php").is_empty());
        assert_eq!(ws.merged("file:///b.php").len(), 1);
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
            let ws = backend.workspace_diags.lock();
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

    /// Closing a file migrates its live external tool results into the
    /// workspace store so they keep being reported for the closed file.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn did_close_migrates_external_results() {
        let backend = Backend::new_test();
        backend
            .workspace_diag_pass_started
            .store(true, std::sync::atomic::Ordering::Release);

        let uri = "file:///closed.php";
        backend
            .phpstan_tool
            .last_diags
            .lock()
            .insert(uri.to_string(), vec![diag("argument.type", 3)]);

        backend.clear_diagnostics_for_file(uri).await;

        let merged = backend.workspace_diags.lock().merged(uri);
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
}
