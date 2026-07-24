//! Find References (`textDocument/references`).
//!
//! When the user invokes "Find All References" on a symbol, the LSP
//! collects every occurrence of that symbol across the project.
//!
//! **Same-file references** are answered from the precomputed
//! [`SymbolMap`] — we iterate all spans and collect those that match
//! the symbol under the cursor.
//!
//! **Cross-file references** iterate every `SymbolMap` stored in
//! `self.symbol_maps` (one per opened / parsed file).  For files that
//! are in the workspace but have not been opened yet, we lazily parse
//! them on demand (via the fqn_uri_index, PSR-4, and workspace scan).
//!
//! **Variable references** (including `$this`) are strictly scoped to
//! the enclosing function / method / closure body within the current
//! file.
//!
//! **Member references** (methods, properties, constants) are filtered
//! by the class hierarchy of the target member.  When the user triggers
//! "Find References" on `MyClass::save()`, only accesses where the
//! subject resolves to a class in the same inheritance tree are returned.
//! Accesses on unrelated classes that happen to have a member with the
//! same name are excluded.
//!
//! The per-symbol-kind finders live in sibling submodules
//! ([`dispatch`], [`variables`], [`classes`], [`members`],
//! [`functions`]); this module retains the shared symbol-map snapshot
//! helpers, the workspace-indexing pipeline, and the free helpers those
//! finders share.

mod classes;
mod dispatch;
mod functions;
mod members;
mod variables;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use tower_lsp::lsp_types::{Location, Position, Range, Url};

use crate::Backend;
use crate::reference_index::ReferenceIndexKey;
use crate::symbol_map::SymbolMap;
use crate::util::strip_fqn_prefix;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReferenceSearchMode {
    References,
    Rename,
}

impl ReferenceSearchMode {
    fn include_declaring_interfaces(self) -> bool {
        matches!(self, ReferenceSearchMode::References)
    }
}

impl Backend {
    /// Snapshot all symbol maps for user (non-vendor, non-stub) files.
    ///
    /// Ensures the workspace is indexed first, then returns a cloned
    /// snapshot of every symbol map whose URI does not fall under the
    /// vendor directory or the internal stub scheme.  All four cross-file
    /// reference scanners use this to restrict results to user code.
    pub(crate) fn user_file_symbol_maps(&self) -> Vec<(String, Arc<SymbolMap>)> {
        self.ensure_workspace_indexed_for_request();
        self.user_file_symbol_maps_matching(None)
    }

    pub(crate) fn user_file_symbol_maps_for_reference_keys(
        &self,
        keys: &[ReferenceIndexKey],
    ) -> Vec<(String, Arc<SymbolMap>)> {
        self.ensure_workspace_indexed_for_request();
        let candidate_uris = self.reference_candidate_uris_for_keys(keys);
        self.user_file_symbol_maps_matching(candidate_uris.as_ref())
    }

    fn user_file_symbol_maps_matching(
        &self,
        candidate_uris: Option<&HashSet<String>>,
    ) -> Vec<(String, Arc<SymbolMap>)> {
        let vendor_prefixes = self.vendor_uri_prefixes.lock().clone();

        let maps = self.symbol_maps.read();
        maps.iter()
            .filter(|(uri, _)| {
                candidate_uris.is_none_or(|uris| uris.contains(uri.as_str()))
                    && !uri.starts_with("phpantom-stub://")
                    && !uri.starts_with("phpantom-stub-fn://")
                    && !vendor_prefixes.iter().any(|p| uri.starts_with(p.as_str()))
            })
            .map(|(uri, map)| (uri.clone(), Arc::clone(map)))
            .collect()
    }

    pub(super) fn reference_file_content(&self, uri: &str) -> Option<String> {
        if self.is_blade_file(uri)
            && let Some(content) = self.blade_virtual_content.read().get(uri)
        {
            return Some(content.clone());
        }
        self.get_file_content(uri)
    }

    pub(super) fn reference_file_content_arc(&self, uri: &str) -> Option<Arc<String>> {
        if self.is_blade_file(uri)
            && let Some(content) = self.blade_virtual_content.read().get(uri)
        {
            return Some(Arc::new(content.clone()));
        }
        self.get_file_content_arc(uri)
    }

    /// Ensure all workspace PHP files have been parsed and have symbol maps.
    ///
    /// This lazily parses files that are in the workspace directory but
    /// have not been opened or indexed yet.  It also covers files known
    /// via the fqn_uri_index.  The vendor directory (read from
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

    /// Enter the per-file scan window (80..100) of the current
    /// request's progress bar and register `total` files to scan.
    /// No-op when no progress sink is attached.
    pub(crate) fn begin_request_scan_window(&self, total: usize, label: &str) {
        if let Some(state) = self.request_progress.as_deref() {
            state.set_scope(80, 100, label);
            state.add_total(total as u64);
        }
    }

    /// Record one scanned file in the current request's progress bar.
    pub(crate) fn request_scan_file_done(&self) {
        if let Some(state) = self.request_progress.as_deref() {
            state.add_done(1);
        }
    }

    pub(crate) fn ensure_workspace_indexed_with_progress(
        &self,
        progress: Option<&(dyn Fn(u32, String) + Sync)>,
    ) {
        let _workspace_index_guard = self.workspace_index_lock.lock();
        let start = std::time::Instant::now();
        report_workspace_index_progress(progress, 1, "Preparing workspace index");
        // Collect URIs that already have symbol maps.
        let existing_uris: HashSet<String> = self.symbol_maps.read().keys().cloned().collect();

        // Build the vendor URI prefixes so we can skip vendor files in
        // Phase 1 (fqn_uri_index may contain vendor URIs from prior
        // resolution, but we only need symbol maps for user files).
        let vendor_prefixes = self.vendor_uri_prefixes.lock().clone();

        // ── Phase 1: fqn_uri_index files (user only) ─────────────────────
        let index_uris: Vec<String> = self.fqn_uri_index.read().values().cloned().collect();

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
        // Even after the initial scan, repeat the walk so newly-created PHP
        // files that are not open in the editor can still be discovered.
        // The existing-URI filter below keeps this cheap by parsing only files
        // that are not already in `symbol_maps`.
        let workspace_root = self.workspace_root.read().clone();
        let phase1_uri_set: HashSet<&str> = phase1_uris.iter().map(|uri| uri.as_str()).collect();
        let phase2_work = if let Some(root) = workspace_root.clone() {
            let vendor_dir_paths = self.vendor_dir_paths.lock().clone();

            report_workspace_index_progress(progress, 3, "Scanning workspace files");
            let walk_start = std::time::Instant::now();
            let php_files = collect_php_files_gitignore(&root, &vendor_dir_paths);
            tracing::info!(
                "ensure_workspace_indexed: Phase 2 disk walk found {} PHP files in {:?}",
                php_files.len(),
                walk_start.elapsed()
            );

            php_files
                .into_iter()
                .filter_map(|path| {
                    let uri = crate::util::path_to_uri(&path);
                    if existing_uris.contains(&uri) || phase1_uri_set.contains(uri.as_str()) {
                        None
                    } else {
                        Some((uri, path))
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        let total_to_parse = phase1_uris.len() + phase2_work.len();
        let phase1_units: u64 = phase1_uris
            .iter()
            .map(|uri| self.index_progress_weight_for_uri(uri, None))
            .sum();
        let phase2_units: u64 = phase2_work
            .iter()
            .map(|(_, path)| index_progress_weight_for_path(path))
            .sum();
        let total_parse_units = phase1_units.saturating_add(phase2_units).max(1);
        report_workspace_index_progress(
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
                    report_workspace_index_progress(
                        progress,
                        workspace_parse_percentage(done_units, total_parse_units),
                        format!("Parsing indexed files ({done_files}/{total_to_parse})"),
                    );
                }),
            );
        }

        if workspace_root.is_some() {
            report_workspace_index_progress(
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
                        report_workspace_index_progress(
                            progress,
                            workspace_parse_percentage(total_units_done, total_parse_units),
                            format!("Parsing workspace files ({total_done}/{total_to_parse})"),
                        );
                    }),
                );
            }
            report_workspace_index_progress(progress, 99, "Finalizing workspace index");
            // Release pairs with the Acquire loads in
            // `reference_candidate_uris_for_keys` and `find_implementors`.
            self.workspace_indexed
                .store(true, std::sync::atomic::Ordering::Release);
        }
        report_workspace_index_progress(progress, 100, "Workspace index ready");
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
    fn parse_files_parallel_with_progress(
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

    fn index_progress_weight_for_uri(&self, uri: &str, content: Option<&str>) -> u64 {
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

/// Normalise a class FQN: strip leading `\` if present.
pub(super) fn normalize_fqn(fqn: &str) -> String {
    strip_fqn_prefix(fqn).to_string()
}

pub(super) fn static_call_root(expr: &crate::subject_expr::SubjectExpr) -> Option<(&str, &str)> {
    match expr {
        crate::subject_expr::SubjectExpr::CallExpr { callee, .. } => static_call_root(callee),
        crate::subject_expr::SubjectExpr::MethodCall { base, .. } => static_call_root(base),
        crate::subject_expr::SubjectExpr::StaticMethodCall { class, method } => {
            Some((class.as_str(), method.as_str()))
        }
        _ => None,
    }
}

pub(super) fn unresolved_member_subject_matches_scope(
    subject_text: &str,
    scope: &HashSet<String>,
) -> bool {
    let Some(subject_name) = unresolved_member_subject_name(subject_text) else {
        return false;
    };
    let subject_key = normalized_member_subject_key(&subject_name);
    if subject_key.is_empty() {
        return false;
    }

    scope.iter().any(|fqn| {
        member_scope_name_keys(crate::util::short_name(fqn))
            .into_iter()
            .any(|key| key == subject_key)
    })
}

fn unresolved_member_subject_name(subject_text: &str) -> Option<String> {
    match crate::subject_expr::SubjectExpr::parse(subject_text) {
        crate::subject_expr::SubjectExpr::Variable(name) => {
            Some(name.trim_start_matches('$').to_string())
        }
        crate::subject_expr::SubjectExpr::PropertyChain { property, .. } => Some(property),
        _ => None,
    }
}

fn member_scope_name_keys(short_name: &str) -> Vec<String> {
    let mut names = vec![short_name.to_string()];
    for suffix in ["Repository", "Gateway"] {
        if let Some(stem) = short_name.strip_suffix(suffix) {
            names.push(format!("{stem}{suffix}"));
            if suffix == "Repository" {
                names.push(format!("{stem}Repo"));
            }
        }
    }

    names
        .into_iter()
        .map(|name| normalized_member_subject_key(&name))
        .filter(|name| !name.is_empty())
        .collect()
}

fn normalized_member_subject_key(name: &str) -> String {
    name.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

pub(super) fn is_laravel_builder_static_entrypoint(method_name: &str) -> bool {
    matches!(
        method_name.to_ascii_lowercase().as_str(),
        "query"
            | "newquery"
            | "where"
            | "wherein"
            | "wherenull"
            | "wherenotnull"
            | "orderby"
            | "select"
            | "with"
            | "without"
            | "latest"
            | "oldest"
    )
}

/// Whether a member name is the PHP constructor (`__construct`).
///
/// PHP method names are case-insensitive, so `__CONSTRUCT` matches too.
pub(super) fn is_constructor_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("__construct")
}

/// Check whether a resolved class name matches the target FQN.
///
/// Two names match if their fully-qualified forms are equal, or if both
/// are unqualified and their short names match.
pub(super) fn class_names_match(resolved: &str, target: &str, target_short: &str) -> bool {
    if resolved == target {
        return true;
    }
    // When neither name is qualified, compare short names.
    if !resolved.contains('\\') && !target.contains('\\') {
        return resolved == target_short;
    }
    // When the resolved name is unqualified but the target is
    // namespace-qualified, the resolved name might be a short-name
    // reference to the target class (e.g. `Request` referencing
    // `Illuminate\Http\Request` via a `use` import that was not
    // tracked in the resolved-names map).  Accept the match only
    // when the short names agree.
    //
    // The reverse (resolved is qualified, target is unqualified) is
    // NOT accepted: `App\Helper` is a different class from a global
    // `Helper`, so matching by short name alone would produce false
    // positives.
    if !resolved.contains('\\') && target.contains('\\') {
        return resolved == target_short;
    }
    false
}

pub(super) fn class_candidate_keys(target: &str, target_short: &str) -> Vec<ReferenceIndexKey> {
    symbol_candidate_names(target, target_short)
        .into_iter()
        .map(ReferenceIndexKey::Class)
        .collect()
}

pub(super) fn function_candidate_keys(target: &str, target_short: &str) -> Vec<ReferenceIndexKey> {
    symbol_candidate_names(target, target_short)
        .into_iter()
        .map(ReferenceIndexKey::Function)
        .collect()
}

fn symbol_candidate_names(target: &str, target_short: &str) -> Vec<String> {
    let mut keys = vec![
        strip_fqn_prefix(target).to_string(),
        strip_fqn_prefix(target_short).to_string(),
    ];
    keys.sort();
    keys.dedup();
    keys
}

pub(super) fn member_candidate_keys(
    target_member: &str,
    target_is_static: bool,
    hierarchy: Option<&HashSet<String>>,
) -> Vec<ReferenceIndexKey> {
    let mut keys = vec![ReferenceIndexKey::Member {
        name: target_member.to_string(),
        is_static: target_is_static,
    }];
    if hierarchy.is_some() {
        keys.push(ReferenceIndexKey::Member {
            name: target_member.to_string(),
            is_static: !target_is_static,
        });
    }
    keys
}

fn report_workspace_index_progress(
    progress: Option<&(dyn Fn(u32, String) + Sync)>,
    percentage: u32,
    message: impl Into<String>,
) {
    if let Some(progress) = progress {
        progress(percentage.min(100), message.into());
    }
}

fn workspace_parse_percentage(done: u64, total: u64) -> u32 {
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

fn largest_first_work_order(weights: &[u64]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..weights.len()).collect();
    order.sort_by_key(|&idx| std::cmp::Reverse(weights[idx]));
    order
}

fn index_progress_weight_for_path(path: &Path) -> u64 {
    path.metadata().map(|meta| meta.len()).unwrap_or(1).max(1)
}

/// Recursively collect all `.php` files under a workspace root,
/// respecting `.gitignore` rules (including nested and global
/// gitignore files).
///
/// Used by Find References which walks the entire workspace root.
/// Unlike `classmap_scanner`'s PSR-4 walkers, this uses the `ignore`
/// crate's [`ignore::WalkBuilder`] so that generated/cached directories
/// listed in `.gitignore` (e.g. `storage/framework/views/`,
/// `var/cache/`, `node_modules/`) are automatically skipped.
///
/// All known vendor directories are always skipped regardless of
/// `.gitignore` content, since some projects commit their vendor
/// directory.  `vendor_dir_paths` contains absolute paths of all
/// known vendor directories (one per subproject in monorepo mode).
///
/// Hidden files and directories are skipped by default (handled by
/// the `ignore` crate).
pub(crate) fn collect_php_files_gitignore(
    root: &Path,
    vendor_dir_paths: &[PathBuf],
) -> Vec<PathBuf> {
    use ignore::WalkBuilder;

    let mut result = Vec::new();
    let vendor_paths_owned: Vec<PathBuf> = vendor_dir_paths.to_vec();

    let walker = WalkBuilder::new(root)
        // Respect .gitignore, .git/info/exclude, global gitignore
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        // Skip hidden files/dirs (.git, .idea, etc.)
        .hidden(true)
        // Read parent .gitignore files
        .parents(true)
        // Also respect .ignore files (ripgrep convention)
        .ignore(true)
        // Always skip vendor directories, even if not gitignored
        .filter_entry(move |entry| {
            if entry.file_type().is_some_and(|ft| ft.is_dir()) {
                let path = entry.path();
                if vendor_paths_owned.iter().any(|vp| vp == path) {
                    return false;
                }
            }
            true
        })
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "php") {
            result.push(path.to_path_buf());
        }
    }

    result
}

/// Push a location only if it is not already present (deduplication).
pub(crate) fn push_unique_location(
    locations: &mut Vec<Location>,
    uri: &Url,
    start: Position,
    end: Position,
) {
    let already_present = locations.iter().any(|l| {
        l.uri == *uri
            && l.range.start.line == start.line
            && l.range.start.character == start.character
    });
    if !already_present {
        locations.push(Location {
            uri: uri.clone(),
            range: Range { start, end },
        });
    }
}

#[cfg(test)]
mod tests;
