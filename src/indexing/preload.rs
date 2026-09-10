//! Autoload preloading and the workspace-indexing pipeline.
//!
//! Keeps all "parse the workspace into symbol maps" logic in one place:
//! autoload preloading and the `ensure_workspace_indexed*` pipeline with
//! its parallel parse workers.
//!
//! Several methods spawn OS threads sized with
//! [`PARSE_WORKER_STACK_SIZE`](crate::PARSE_WORKER_STACK_SIZE); see the
//! "Performance Anti-Patterns" note in the contributor guide.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use tower_lsp::lsp_types::Url;

use crate::Backend;

impl Backend {
    /// Eagerly full-parse the given autoload helper files in parallel.
    ///
    /// [`scan_autoload_files`](Self::scan_autoload_files) only byte-scans
    /// these files, which misses functions defined inside
    /// `function_exists` guards.  A full parse populates `global_functions`
    /// with those guarded helpers so that
    /// [`find_or_load_function`](Self::find_or_load_function) resolves them
    /// via its fast path instead of falling back to a serial parse of
    /// every autoload file on the first interactive lookup.
    ///
    /// Files already present in `parsed_uris` are skipped.
    pub fn preload_autoload_files(&self, paths: &[PathBuf]) {
        self.preload_autoload_files_with_progress(paths, None);
    }

    /// Like [`preload_autoload_files`](Self::preload_autoload_files),
    /// reporting per-file progress when a sink is attached.
    pub(crate) fn preload_autoload_files_with_progress(
        &self,
        paths: &[PathBuf],
        progress: Option<&crate::progress::ScanProgress>,
    ) {
        // Skip files that have already been parsed (e.g. opened in the
        // editor before indexing reached them).
        let pending: Vec<&PathBuf> = paths
            .iter()
            .filter(|p| {
                let uri = crate::util::path_to_uri(p);
                !self.parsed_uris.read().contains(&uri)
            })
            .collect();

        let file_count = pending.len();
        if file_count == 0 {
            return;
        }
        if let Some(p) = progress {
            p.add_total(file_count as u64);
        }

        let n_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(file_count);
        let next_idx = std::sync::atomic::AtomicUsize::new(0);
        let pending = &pending;
        let next_idx = &next_idx;

        std::thread::scope(|s| {
            for _ in 0..n_threads {
                std::thread::Builder::new()
                    .name("autoload-preload".into())
                    .stack_size(crate::PARSE_WORKER_STACK_SIZE)
                    .spawn_scoped(s, move || {
                        loop {
                            let i = next_idx.fetch_add(1, Ordering::Relaxed);
                            if i >= file_count {
                                break;
                            }
                            if let Some(p) = progress {
                                p.add_done(1);
                            }
                            let path = pending[i];
                            if let Ok(content) = std::fs::read_to_string(path) {
                                let uri = crate::util::path_to_uri(path);
                                self.update_ast(&uri, &content);
                            }
                        }
                    })
                    .expect("failed to spawn autoload-preload thread");
            }
        });
    }

    /// Ensure all workspace PHP files have been parsed and have symbol maps.
    ///
    /// This lazily parses files that are in the workspace directory but
    /// have not been opened or indexed yet.  It also covers files known
    /// via the fqn_uri_index.  The vendor directory (read from
    /// `composer.json`'s `config.vendor-dir`, defaulting to `vendor`) is
    /// skipped during the filesystem walk.
    pub(crate) fn ensure_workspace_indexed(&self) {
        self.ensure_workspace_indexed_with_progress(None);
    }

    /// Ensure the workspace index is built, forwarding indexing
    /// progress into the current request's progress sink when one is
    /// attached (go-to-implementation, find-references, type
    /// hierarchy).
    ///
    /// The indexing pass maps into 0..80 of the request's progress
    /// bar; the per-file reference/implementor scans that follow
    /// report into the remaining 80..100.
    pub(crate) fn ensure_workspace_indexed_for_request(&self) {
        match self.request_progress.as_deref() {
            Some(state) => {
                let forward = |percentage: u32, message: String| {
                    state.set_percentage(percentage.min(100) * 4 / 5, message);
                };
                self.ensure_workspace_indexed_with_progress(Some(&forward));
            }
            None => self.ensure_workspace_indexed(),
        }
    }

    /// Wait for the initial workspace index when necessary, but reuse a
    /// completed index without refreshing the filesystem.
    ///
    /// Internal consumers such as declaration CodeLens and cached reference
    /// counts call this once per symbol. Explicit Find References requests use
    /// [`ensure_workspace_indexed_for_request`](Self::ensure_workspace_indexed_for_request)
    /// once at their entry point so they retain the existing on-demand refresh
    /// that discovers files created without a watcher notification.
    pub(crate) fn ensure_workspace_index_ready_for_request(&self) {
        match self.request_progress.as_deref() {
            Some(state) => {
                let forward = |percentage: u32, message: String| {
                    state.set_percentage(percentage.min(100) * 4 / 5, message);
                };
                self.ensure_workspace_index_ready_with_progress(Some(&forward));
            }
            None => self.ensure_workspace_index_ready_with_progress(None),
        }
    }

    /// Acquire `workspace_index_lock`, mirroring the in-flight index's own
    /// progress into `progress` while another thread holds it.
    ///
    /// The background full index holds this lock for its entire run, so a
    /// request that needs a complete index (find references, rename,
    /// go-to-implementation, Laravel string keys) can wait for the rest of
    /// the parse. Waiting is the right behaviour — partial results mean
    /// missed references and false-positive diagnostics — but the wait has
    /// to be visible, otherwise the request's progress token sits at
    /// "Resolving…" with no indication that the index is what it is
    /// waiting for.
    fn acquire_workspace_index_lock(
        &self,
        progress: Option<&(dyn Fn(u32, String) + Sync)>,
    ) -> parking_lot::MutexGuard<'_, ()> {
        if let Some(guard) = self.workspace_index_lock.try_lock() {
            return guard;
        }
        // Nothing to report the wait to; take the plain blocking path.
        let Some(progress) = progress else {
            return self.workspace_index_lock.lock();
        };

        let waiting_since = std::time::Instant::now();
        loop {
            let (percentage, message) = match self.workspace_index_status.lock().as_ref() {
                Some((percentage, message)) => (
                    *percentage,
                    format!("Waiting for workspace index: {message}"),
                ),
                None => (0, "Waiting for workspace index".to_string()),
            };
            progress(percentage, message);

            if let Some(guard) = self
                .workspace_index_lock
                .try_lock_for(std::time::Duration::from_millis(100))
            {
                tracing::debug!(
                    "ensure_workspace_indexed: waited {:?} for the in-flight workspace index",
                    waiting_since.elapsed()
                );
                return guard;
            }
        }
    }

    /// Publish an indexing milestone to both the caller's progress sink
    /// and `workspace_index_status`, so requests blocked on the lock can
    /// mirror it.
    fn report_workspace_index_progress(
        &self,
        progress: Option<&(dyn Fn(u32, String) + Sync)>,
        percentage: u32,
        message: impl Into<String>,
    ) {
        let percentage = percentage.min(100);
        let message = message.into();
        *self.workspace_index_status.lock() = Some((percentage, message.clone()));
        if let Some(progress) = progress {
            progress(percentage, message);
        }
    }

    pub(crate) fn ensure_workspace_indexed_with_progress(
        &self,
        progress: Option<&(dyn Fn(u32, String) + Sync)>,
    ) {
        self.ensure_workspace_indexed_with_progress_mode(progress, true);
    }

    pub(crate) fn ensure_workspace_index_ready_with_progress(
        &self,
        progress: Option<&(dyn Fn(u32, String) + Sync)>,
    ) {
        self.ensure_workspace_indexed_with_progress_mode(progress, false);
    }

    fn ensure_workspace_indexed_with_progress_mode(
        &self,
        progress: Option<&(dyn Fn(u32, String) + Sync)>,
        refresh_completed: bool,
    ) {
        // Reference counts and CodeLens resolution can ask for the complete
        // index once per declaration.  Once the initial pass has published
        // every batch, those requests must reuse it instead of walking the
        // workspace again.  Watched-file notifications keep the completed
        // index current after this point.
        if !refresh_completed && self.workspace_indexed.load(Ordering::Acquire) {
            return;
        }

        let _workspace_index_guard = self.acquire_workspace_index_lock(progress);

        // Another request may have completed the index while this one was
        // waiting for the single-flight lock.
        if !refresh_completed && self.workspace_indexed.load(Ordering::Acquire) {
            if let Some(progress) = progress {
                progress(100, "Workspace index ready".to_string());
            }
            return;
        }

        let start = std::time::Instant::now();
        self.report_workspace_index_progress(progress, 1, "Preparing workspace index");
        let existing_uris: HashSet<String> = self.symbol_maps.read().keys().cloned().collect();

        // Build the vendor URI prefixes so we can skip vendor files in
        // Phase 1 (fqn_uri_index may contain vendor URIs from prior
        // resolution, but we only need symbol maps for user files).
        let vendor_prefixes = self.workspace.vendor_uri_prefixes.lock().clone();

        // ── Phase 1: fqn_uri_index files (user only) ─────────────────────
        let index_uris: Vec<String> = self
            .symbols
            .fqn_uri_index
            .read()
            .values()
            .cloned()
            .collect();

        let phase1_uris: Vec<&String> = index_uris
            .iter()
            .filter(|uri| {
                !existing_uris.contains(*uri)
                    && !vendor_prefixes.iter().any(|p| uri.starts_with(p.as_str()))
                    && !uri.starts_with("phpantom-stub://")
                    && !uri.starts_with("phpantom-stub-fn://")
            })
            .collect();

        // ── Phase 2: workspace directory scan ───────────────────────────
        //
        // The initial pass discovers every PHP and resource file. Watched-file
        // notifications apply later changes incrementally. Explicit reference
        // requests may still refresh this walk to discover a file created
        // without a watcher event; per-symbol internal consumers only wait for
        // the initial pass and reuse it.
        let workspace_root = self.workspace.workspace_root.read().clone();
        let phase1_uri_set: HashSet<&str> = phase1_uris.iter().map(|uri| uri.as_str()).collect();
        let (phase2_work, resource_work) = if let Some(root) = workspace_root.clone() {
            let vendor_dir_paths = self.workspace.vendor_dir_paths.lock().clone();

            self.report_workspace_index_progress(progress, 3, "Scanning workspace files");
            let walk_start = std::time::Instant::now();
            let (php_files, resource_files) =
                crate::references::collect_workspace_index_files_gitignore(
                    &root,
                    &vendor_dir_paths,
                    &self.index_filters(),
                );
            tracing::info!(
                "ensure_workspace_indexed: Phase 2 disk walk found {} PHP and {} resource files in {:?}",
                php_files.len(),
                resource_files.len(),
                walk_start.elapsed()
            );

            let php_work = php_files
                .into_iter()
                .filter_map(|path| {
                    let uri = crate::util::path_to_uri(&path);
                    if existing_uris.contains(&uri) || phase1_uri_set.contains(uri.as_str()) {
                        None
                    } else {
                        Some((uri, path))
                    }
                })
                .collect();
            let resource_work = resource_files
                .into_iter()
                .filter_map(|path| {
                    let uri = crate::util::path_to_uri(&path);
                    (!existing_uris.contains(&uri)).then_some((uri, path))
                })
                .collect();
            (php_work, resource_work)
        } else {
            (Vec::new(), Vec::new())
        };

        let total_to_parse = phase1_uris.len() + phase2_work.len() + resource_work.len();
        let phase1_units: u64 = phase1_uris
            .iter()
            .map(|uri| self.index_progress_weight_for_uri(uri, None))
            .sum();
        let phase2_units: u64 = phase2_work
            .iter()
            .map(|(_, path)| index_progress_weight_for_path(path))
            .sum();
        let resource_units: u64 = resource_work
            .iter()
            .map(|(_, path)| index_progress_weight_for_path(path))
            .sum();
        let total_parse_units = phase1_units
            .saturating_add(phase2_units)
            .saturating_add(resource_units)
            .max(1);
        self.report_workspace_index_progress(
            progress,
            5,
            format!("Queued {total_to_parse} PHP files for indexing"),
        );

        if !phase1_uris.is_empty() {
            tracing::info!(
                "ensure_workspace_indexed: Phase 1 parsing {} files",
                phase1_uris.len()
            );
            self.parse_files_parallel_with_progress(
                phase1_uris
                    .iter()
                    .map(|uri| (uri.to_string(), None::<String>))
                    .collect(),
                Some(&|done_files, _phase_total, done_units, _phase_units| {
                    self.report_workspace_index_progress(
                        progress,
                        workspace_parse_percentage(done_units, total_parse_units),
                        format!("Parsing indexed files ({done_files}/{total_to_parse})"),
                    );
                }),
            );
        }

        if workspace_root.is_some() {
            self.report_workspace_index_progress(
                progress,
                workspace_parse_percentage(phase1_units, total_parse_units),
                format!(
                    "Indexed known files ({}/{total_to_parse})",
                    phase1_uris.len()
                ),
            );

            if !phase2_work.is_empty() {
                tracing::info!(
                    "ensure_workspace_indexed: Phase 2 parsing {} files",
                    phase2_work.len()
                );
                let parsed_before_phase2 = phase1_uris.len();
                let units_before_phase2 = phase1_units;
                self.parse_paths_parallel_with_progress(
                    &phase2_work,
                    Some(&|done_files, _phase_total, done_units, _phase_units| {
                        let total_done = parsed_before_phase2 + done_files;
                        let total_units_done = units_before_phase2.saturating_add(done_units);
                        self.report_workspace_index_progress(
                            progress,
                            workspace_parse_percentage(total_units_done, total_parse_units),
                            format!("Parsing workspace files ({total_done}/{total_to_parse})"),
                        );
                    }),
                );
            }
            if !resource_work.is_empty() {
                self.report_workspace_index_progress(
                    progress,
                    workspace_parse_percentage(
                        phase1_units.saturating_add(phase2_units),
                        total_parse_units,
                    ),
                    format!(
                        "Indexing resource references ({}/{total_to_parse})",
                        phase1_uris.len() + phase2_work.len()
                    ),
                );
                self.index_resource_paths_batch(&resource_work);
            }
            self.report_workspace_index_progress(progress, 99, "Finalizing workspace index");
            // Release pairs with the Acquire loads in
            // `reference_candidate_uris_for_keys` and `find_implementors`.
            self.workspace_indexed
                .store(true, std::sync::atomic::Ordering::Release);

            // Blade templates parsed before their controllers saw no
            // `view()` call sites; with the whole workspace indexed,
            // re-run call-site inference and re-parse the templates
            // whose inferred variable set changed.
            self.refresh_blade_injected_vars();
        }
        self.report_workspace_index_progress(progress, 100, "Workspace index ready");
        *self.workspace_index_status.lock() = None;
        tracing::info!("ensure_workspace_indexed: total time {:?}", start.elapsed());
    }

    /// Parse a batch of files in parallel using OS threads.
    ///
    /// Each entry is `(uri, optional_content)`.  When `content` is `None`,
    /// the file is loaded via [`get_file_content`].  Workers parse files into
    /// owned index updates, then a single merge publishes the whole batch.
    ///
    /// Uses [`std::thread::scope`] for structured concurrency so that all
    /// spawned threads are guaranteed to finish before this method returns.
    /// The thread count is capped at the number of available CPU cores.
    pub(crate) fn parse_files_parallel_with_progress(
        &self,
        files: Vec<(String, Option<String>)>,
        progress: Option<&(dyn Fn(usize, usize, u64, u64) + Sync)>,
    ) {
        if files.is_empty() {
            return;
        }
        let total = files.len();
        let parsed = AtomicUsize::new(0);
        let weights: Vec<u64> = files
            .iter()
            .map(|(uri, content)| self.index_progress_weight_for_uri(uri, content.as_deref()))
            .collect();
        let total_units = weights.iter().copied().sum::<u64>().max(1);
        let parsed_units = AtomicU64::new(0);

        // For very small batches, avoid thread overhead.
        if files.len() <= 2 {
            let mut results = Vec::with_capacity(files.len());
            for (idx, (uri, content)) in files.iter().enumerate() {
                let content = content.clone().or_else(|| self.get_file_content(uri));
                if let Some(content) = content {
                    results.push(self.parse_ast_index_update_for_index(uri, &content));
                }
                report_weighted_parse_progress(
                    progress,
                    &parsed,
                    &parsed_units,
                    weights[idx],
                    total,
                    total_units,
                );
            }
            report_weighted_merge_progress(progress, total, total_units);
            self.apply_ast_index_parse_results_batch(results);
            return;
        }

        let n_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(files.len());
        let next = AtomicUsize::new(0);
        let work_order = largest_first_work_order(&weights);

        let files_ref = &files;
        let weights_ref = &weights;
        let work_order_ref = &work_order;
        let mut results = std::thread::scope(|s| {
            let mut handles = Vec::with_capacity(n_threads);
            for _ in 0..n_threads {
                let parsed = &parsed;
                let parsed_units = &parsed_units;
                let next = &next;
                let files = files_ref;
                let weights = weights_ref;
                let work_order = work_order_ref;
                match std::thread::Builder::new()
                    .stack_size(crate::PARSE_WORKER_STACK_SIZE)
                    .spawn_scoped(s, move || {
                        let mut local_results = Vec::new();
                        loop {
                            let work_idx = next.fetch_add(1, Ordering::Relaxed);
                            let Some(&idx) = work_order.get(work_idx) else {
                                break;
                            };
                            let Some((uri, content)) = files.get(idx) else {
                                break;
                            };

                            let content = content.clone().or_else(|| self.get_file_content(uri));
                            if let Some(content) = content {
                                local_results.push((
                                    idx,
                                    self.parse_ast_index_update_for_index(uri, &content),
                                ));
                            }
                            report_weighted_parse_progress(
                                progress,
                                parsed,
                                parsed_units,
                                weights[idx],
                                total,
                                total_units,
                            );
                        }
                        local_results
                    }) {
                    Ok(handle) => handles.push(handle),
                    Err(e) => tracing::error!("failed to spawn parse thread: {e}"),
                }
            }

            handles
                .into_iter()
                .flat_map(|handle| {
                    handle.join().unwrap_or_else(|_| {
                        tracing::error!("parse thread panicked during workspace indexing");
                        Vec::new()
                    })
                })
                .collect::<Vec<_>>()
        });
        results.sort_by_key(|(idx, _)| *idx);
        report_weighted_merge_progress(progress, total, total_units);
        self.apply_ast_index_parse_results_batch(
            results.into_iter().map(|(_, result)| result).collect(),
        );
    }

    /// Parse a batch of files from disk paths in parallel.
    ///
    /// Each entry is `(uri, path)`.  The file is read from disk and parsed in
    /// a worker thread.  Work is pulled from a shared atomic counter so large
    /// files cannot leave one fixed chunk as the long tail.
    pub(crate) fn parse_paths_parallel_with_progress(
        &self,
        files: &[(String, PathBuf)],
        progress: Option<&(dyn Fn(usize, usize, u64, u64) + Sync)>,
    ) {
        if files.is_empty() {
            return;
        }
        let total = files.len();
        let parsed = AtomicUsize::new(0);
        let weights: Vec<u64> = files
            .iter()
            .map(|(_, path)| index_progress_weight_for_path(path))
            .collect();
        let total_units = weights.iter().copied().sum::<u64>().max(1);
        let parsed_units = AtomicU64::new(0);

        // For very small batches, avoid thread overhead.
        if files.len() <= 2 {
            let mut results = Vec::with_capacity(files.len());
            for (idx, (uri, path)) in files.iter().enumerate() {
                if let Ok(content) = std::fs::read_to_string(path) {
                    results.push(self.parse_ast_index_update_for_index(uri, &content));
                }
                report_weighted_parse_progress(
                    progress,
                    &parsed,
                    &parsed_units,
                    weights[idx],
                    total,
                    total_units,
                );
            }
            report_weighted_merge_progress(progress, total, total_units);
            self.apply_ast_index_parse_results_batch(results);
            return;
        }

        let n_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(files.len());
        let next = AtomicUsize::new(0);
        let work_order = largest_first_work_order(&weights);

        let weights_ref = &weights;
        let work_order_ref = &work_order;
        let mut results = std::thread::scope(|s| {
            let mut handles = Vec::with_capacity(n_threads);
            for _ in 0..n_threads {
                let parsed = &parsed;
                let parsed_units = &parsed_units;
                let next = &next;
                let weights = weights_ref;
                let work_order = work_order_ref;
                match std::thread::Builder::new()
                    .stack_size(crate::PARSE_WORKER_STACK_SIZE)
                    .spawn_scoped(s, move || {
                        let mut local_results = Vec::new();
                        loop {
                            let work_idx = next.fetch_add(1, Ordering::Relaxed);
                            let Some(&idx) = work_order.get(work_idx) else {
                                break;
                            };
                            let Some((uri, path)) = files.get(idx) else {
                                break;
                            };

                            if let Ok(content) = std::fs::read_to_string(path) {
                                local_results.push((
                                    idx,
                                    self.parse_ast_index_update_for_index(uri, &content),
                                ));
                            }
                            report_weighted_parse_progress(
                                progress,
                                parsed,
                                parsed_units,
                                weights[idx],
                                total,
                                total_units,
                            );
                        }
                        local_results
                    }) {
                    Ok(handle) => handles.push(handle),
                    Err(e) => tracing::error!("failed to spawn parse thread: {e}"),
                }
            }

            handles
                .into_iter()
                .flat_map(|handle| {
                    handle.join().unwrap_or_else(|_| {
                        tracing::error!("parse thread panicked during workspace indexing");
                        Vec::new()
                    })
                })
                .collect::<Vec<_>>()
        });
        results.sort_by_key(|(idx, _)| *idx);
        report_weighted_merge_progress(progress, total, total_units);
        self.apply_ast_index_parse_results_batch(
            results.into_iter().map(|(_, result)| result).collect(),
        );
    }

    pub(crate) fn index_progress_weight_for_uri(&self, uri: &str, content: Option<&str>) -> u64 {
        if let Some(content) = content {
            return (content.len() as u64).max(1);
        }
        if let Some(content) = self.open_files.read().get(uri) {
            return (content.len() as u64).max(1);
        }
        Url::parse(uri)
            .ok()
            .and_then(|url| url.to_file_path().ok())
            .map(|path| index_progress_weight_for_path(&path))
            .unwrap_or(1)
    }
}

pub(crate) fn workspace_parse_percentage(done: u64, total: u64) -> u32 {
    if total == 0 {
        return 95;
    }

    5 + ((done.saturating_mul(90) / total).min(90) as u32)
}

fn report_weighted_parse_progress(
    progress: Option<&(dyn Fn(usize, usize, u64, u64) + Sync)>,
    parsed: &AtomicUsize,
    parsed_units: &AtomicU64,
    weight: u64,
    total: usize,
    total_units: u64,
) {
    let done = parsed.fetch_add(1, Ordering::Relaxed) + 1;
    let done_units = parsed_units.fetch_add(weight, Ordering::Relaxed) + weight;
    let file_report_every = (total / 100).max(1);
    let unit_report_every = (total_units / 100).max(1);
    let crossed_unit_boundary =
        done_units == total_units || done_units % unit_report_every < weight.min(unit_report_every);

    if done == 1 || done == total || done.is_multiple_of(file_report_every) || crossed_unit_boundary
    {
        report_weighted_progress(progress, done, total, done_units, total_units);
    }
}

fn report_weighted_merge_progress(
    progress: Option<&(dyn Fn(usize, usize, u64, u64) + Sync)>,
    total: usize,
    total_units: u64,
) {
    report_weighted_progress(progress, total, total, total_units, total_units);
}

fn report_weighted_progress(
    progress: Option<&(dyn Fn(usize, usize, u64, u64) + Sync)>,
    done: usize,
    total: usize,
    done_units: u64,
    total_units: u64,
) {
    if let Some(progress) = progress {
        progress(done, total, done_units, total_units);
    }
}

pub(crate) fn largest_first_work_order(weights: &[u64]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..weights.len()).collect();
    order.sort_by_key(|&idx| std::cmp::Reverse(weights[idx]));
    order
}

fn index_progress_weight_for_path(path: &Path) -> u64 {
    path.metadata().map(|meta| meta.len()).unwrap_or(1).max(1)
}
