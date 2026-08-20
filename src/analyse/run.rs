//! The `analyze` command driver and file discovery.
//!
//! Runs the same `Backend` indexing pipeline as the LSP server across
//! a whole project, collects diagnostics in parallel, and hands the
//! results to the `output` module for rendering.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(debug_assertions)]
use std::time::Duration;
use std::time::Instant;

use tower_lsp::lsp_types::*;

use crate::parser::with_parse_cache;
use crate::virtual_members::with_active_resolved_class_cache;

use crate::Backend;
use crate::composer;
use crate::config;
use crate::types::ClassInfo;

use super::output::{
    print_error_box, print_file_table, print_github_annotations, print_json_output,
    print_success_box, progress_bar,
};
use super::{AnalyseOptions, FileDiagnostic, OutputFormat, SeverityFilter};

/// Run the analyse command and return the process exit code.
///
/// Returns `0` when no diagnostics are found, `1` when diagnostics exist.
pub async fn run(options: AnalyseOptions) -> i32 {
    let root = &options.workspace_root;

    // A missing composer.json is not an error: plain PHP trees (a
    // WordPress site, a legacy codebase) analyse fine — classes are
    // indexed by scanning the tree and files are discovered by walking
    // the root.  Note it on stderr so a mistyped --project-root does
    // not silently analyse the wrong directory as a bare tree.
    if !root.join("composer.json").is_file() {
        eprintln!(
            "Note: no composer.json found in {} — analysing as a plain PHP project.",
            root.display()
        );
    }

    // ── 1. Load config ──────────────────────────────────────────────
    let cfg = match config::load_config_from(root, options.global_config.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Warning: failed to load .phpantom.toml: {e}");
            config::Config::default()
        }
    };

    let ignore_rules =
        crate::diagnostics::ignore_rules::compile_ignore_rules(&cfg.diagnostics.ignore);

    // ── 2. Index project ────────────────────────────────────────────
    // Create a headless Backend (no LSP client) and run the same init
    // pipeline as the LSP server.  With client=None the log/progress
    // calls are no-ops.
    let backend = Backend::new_headless();
    *backend.workspace_root().write() = Some(root.to_path_buf());
    *backend.workspace.config.lock() = cfg.clone();

    let composer_package = composer::read_composer_package(root);

    let php_version = cfg
        .php
        .version
        .as_deref()
        .and_then(crate::types::PhpVersion::from_composer_constraint)
        .unwrap_or_else(|| {
            composer_package
                .as_ref()
                .and_then(composer::detect_php_version_from_package)
                .unwrap_or_default()
        });
    backend.set_php_version(php_version);

    backend
        .init_single_project(root, php_version, composer_package, None)
        .await;
    // ── 3. Locate user files (via PSR-4) and crop to path ───────────
    let files = discover_user_files(
        &backend,
        root,
        options.path_filter.as_deref(),
        cfg.indexing.follow_links(),
    );

    if files.is_empty() {
        eprintln!("No PHP files found.");
        return 0;
    }

    // ── 4. Two-phase parallel analysis ──────────────────────────────
    //
    // Phase 1 — **Parse**: run `update_ast` on every user file so that
    // `fqn_class_index`, `uri_classes_index`, `symbol_maps`, `file_imports`,
    // `file_namespaces` and `fqn_uri_index` are fully populated for the
    // entire project.
    //
    // Phase 2 — **Diagnose**: collect diagnostics for every file.
    // Because all user classes are already in `fqn_class_index`, cross-file
    // references resolve via an O(1) hash lookup instead of falling
    // through to fqn_uri_index / PSR-4 lazy loading (which takes write
    // locks and serialises threads).
    //
    // Splitting the work this way also means the diagnostic phase
    // never triggers `parse_and_cache_file` for other *user* files,
    // eliminating the main source of write-lock contention that
    // previously caused the "stuck at 99 %" stall.

    let file_count = files.len();
    let severity_filter = options.severity_filter;
    let use_colour = options.use_colour;
    let output_format = options.output_format;
    let debug = options.debug;
    let verbosity = options.verbosity;
    // Per-file lines and the `\r`-rewritten progress bar would clobber
    // each other, so --debug replaces the bar entirely.
    let show_progress = use_colour && output_format == OutputFormat::Table && !debug;
    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    // ── Phase 1: Parse all files (parallel) ─────────────────────────
    // Read each file from disk and call `update_ast`.  Store the
    // (uri, content) pairs so Phase 2 can reuse them without re-reading.
    //
    // Parsing is fast, so the progress bar is drawn at 0% before Phase 1
    // and only advances during Phase 2 (the expensive diagnostic pass).
    if show_progress {
        eprint!("\r\x1b[2K {}", progress_bar(0, file_count));
    }
    let parse_t0 = Instant::now();
    let next_idx = AtomicUsize::new(0);

    let file_data: Vec<Option<(String, String)>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..n_threads)
            .map(|worker| {
                let backend = &backend;
                let next_idx = &next_idx;
                let files = &files;
                std::thread::Builder::new()
                    .name("index-worker".into())
                    .stack_size(crate::PARSE_WORKER_STACK_SIZE)
                    .spawn_scoped(s, move || {
                        let mut entries: Vec<(usize, String, String)> = Vec::new();
                        loop {
                            let i = next_idx.fetch_add(1, Ordering::Relaxed);
                            if i >= file_count {
                                break;
                            }

                            let file_path = &files[i];
                            if debug && verbosity >= 2 {
                                let display =
                                    file_path.strip_prefix(root).unwrap_or(file_path).display();
                                eprintln!("[w{worker:02}] parse {display}");
                            }
                            let content = match std::fs::read_to_string(file_path) {
                                Ok(c) => c,
                                Err(_) => continue,
                            };

                            let uri = crate::util::path_to_uri(file_path);
                            backend.update_ast(&uri, &content);
                            entries.push((i, uri, content));
                        }
                        entries
                    })
                    .expect("failed to spawn index-worker thread")
            })
            .collect();

        // Collect into an indexed vec so Phase 2 can iterate in the
        // same order as `files`.
        let mut indexed: Vec<Option<(String, String)>> = (0..file_count).map(|_| None).collect();
        for handle in handles {
            for (i, uri, content) in handle.join().unwrap_or_default() {
                indexed[i] = Some((uri, content));
            }
        }
        indexed
    });
    let parse_elapsed = parse_t0.elapsed();
    let populate_t0 = Instant::now();

    // ── Discover the configured Laravel date class ──────────────────
    // The `now()`/`today()` helpers and the Date facade / DateFactory
    // resolve to the class selected by `Date::use()` (defaulting to
    // `Illuminate\Support\Carbon`).  Discovery reads project service
    // providers, so it must run after Phase 1 has parsed every user file.
    // The LSP does the equivalent in its `initialized` handler; without
    // this call the helpers would resolve to nothing here, producing
    // false-positive return-type diagnostics.
    if backend.resolved_class_cache.read().is_laravel() {
        backend.build_laravel_date_class();
        // Discover config files, view/translation directories, and route
        // files registered by service providers so that config(), view(),
        // trans(), and route() string keys resolve the same way they do in
        // the LSP (which builds these in its `initialized` handler).
        backend.build_provider_resources();
        // Discover the Eloquent morph map so alias strings are validated the
        // same way here as in the LSP.
        backend.build_laravel_morph_map_index();
        // Discover the gate abilities and policy map so authorization strings
        // are validated the same way here as in the LSP.
        backend.build_laravel_gate_index();
        // Scan the whole FQN → URI index for Artisan commands and for macro
        // registrations.  `update_ast` only refreshes these from the files it
        // parses, which here is the project's own source, so without a full
        // scan the indexes hold no vendor entries and every framework command
        // name and vendor-registered macro reads as unknown.
        backend.build_laravel_command_index();
        backend.build_laravel_macro_index();
    }

    // ── Phase 1.5: Eager class population ───────────────────────────
    // Pre-populate the resolved_class_cache by resolving every known
    // class in topological (dependency-first) order.  This ensures
    // that when Phase 2 resolves types, all dependencies are already
    // cached — eliminating the unbounded mutual recursion in
    // resolve_class_fully_inner that previously caused stack overflow.
    //
    // We snapshot the toposorted FQN list while holding the uri_classes_index
    // read lock, then drop the lock before resolving.  Resolution may
    // call find_or_load_class which takes write locks on uri_classes_index.
    let sorted_fqns = {
        let uri_classes_index = backend.symbols.uri_classes_index.read();
        crate::toposort::toposort_from_uri_classes_index(&uri_classes_index)
    };
    // `populate_from_sorted` fans the list out over its own large-stack
    // workers, so this needs no wrapper thread of its own.
    let class_loader = |name: &str| -> Option<Arc<ClassInfo>> { backend.find_or_load_class(name) };
    crate::virtual_members::populate_from_sorted(
        &sorted_fqns,
        &backend.resolved_class_cache,
        &class_loader,
    );

    // Blade templates parsed in Phase 1 before their controllers saw no
    // `view()` call sites.  With every user file parsed, re-run call-site
    // inference and re-parse the templates whose inferred set changed, so
    // Phase 2 diagnoses them with injected variables in scope.
    backend.refresh_blade_injected_vars();
    let populate_elapsed = populate_t0.elapsed();

    // ── Phase 2: Collect diagnostics (parallel) ─────────────────────
    // Call individual collectors directly (instead of the grouped
    // collect_slow_diagnostics) so we can time each one independently.
    let diagnose_t0 = Instant::now();
    let next_idx = AtomicUsize::new(0);
    let done_count = AtomicUsize::new(0);

    // Phase 2 diagnostic threads need large stacks because the forward
    // walker + type resolution pipeline can nest deeply on files with
    // many class hierarchies and virtual members.  Spawned threads get a
    // 2 MB stack by default (only the main thread gets the 8 MB OS
    // default), so set it explicitly.
    let mut all_file_diagnostics: Vec<(String, Vec<FileDiagnostic>)> = std::thread::scope(|s| {
        let handles: Vec<_> =
            (0..n_threads)
                .map(|worker| {
                    let backend = &backend;
                    let next_idx = &next_idx;
                    let done_count = &done_count;
                    let files = &files;
                    let file_data = &file_data;
                    let ignore_rules = &ignore_rules;
                    std::thread::Builder::new()
                    .name("diag-worker".into())
                    .stack_size(crate::PARSE_WORKER_STACK_SIZE)
                    .spawn_scoped(s, move || {
                    let mut results: Vec<(String, Vec<FileDiagnostic>)> = Vec::new();
                    loop {
                        let i = next_idx.fetch_add(1, Ordering::Relaxed);
                        if i >= file_count {
                            break;
                        }
                        let (uri, original_content) = match &file_data[i] {
                            Some(pair) => (&pair.0, &pair.1),
                            None => continue, // file that failed to read
                        };

                        // Announce the file when it *starts* so that on a
                        // hang the started-but-not-done lines are exactly
                        // the in-flight files.
                        if debug {
                            let display =
                                files[i].strip_prefix(root).unwrap_or(&files[i]).display();
                            if verbosity >= 2 {
                                eprintln!("[w{worker:02}] {display}");
                            } else {
                                eprintln!(" {display}");
                            }
                        }
                        let file_t0 = Instant::now();

                        // For Blade files, use the preprocessed virtual PHP
                        // content instead of the raw Blade template.  The
                        // virtual content was produced by `update_ast` in
                        // Phase 1 and stored in `blade_virtual_content`.
                        let blade_content;
                        let content = if crate::blade::is_blade_file(uri) {
                            if let Some(vc) = backend.blade_virtual_content.read().get(uri.as_str()) {
                                blade_content = vc.clone();
                                &blade_content
                            } else {
                                original_content
                            }
                        } else {
                            original_content
                        };

                        // Activate ONE parse cache for the entire file so
                        // all collectors share the same parsed AST.  Each
                        // collector's own `with_parse_cache` call becomes
                        // a no-op (nested guard).
                        let _parse_guard = with_parse_cache(content);
                        let _cache_guard =
                            with_active_resolved_class_cache(&backend.resolved_class_cache);
                        let _chain_guard =
                            crate::type_engine::resolver::with_chain_resolution_cache();
                        let _resolver_guard = crate::type_engine::call_resolution::activate_type_engine_caches();

                        // ── Forward-walked diagnostic scope cache ───
                        // Walk every function/method body once with the
                        // forward walker, recording scope snapshots at
                        // each statement boundary.  All subsequent
                        // `resolve_variable_types` calls from diagnostic
                        // collectors hit the cache (O(log N) lookup)
                        // instead of doing a full backward scan.
                        let _scope_guard =
                            crate::type_engine::variable::forward_walk::with_diagnostic_scope_cache(
                            );
                        let scope_t0 = Instant::now();
                        {
                            let file_ctx = backend.file_context(uri);
                            let class_loader = backend.class_loader(&file_ctx);
                            let function_loader_cl = backend.function_loader(&file_ctx);
                            let constant_loader_cl = backend.constant_loader(&file_ctx);
                            let config_resolver = |key: &str| backend.resolve_config_type(key);
                            let trans_resolver = |key: &str| backend.resolve_trans_type(key);
                            let loaders = crate::type_engine::resolver::Loaders {
                                function_loader: Some(&function_loader_cl),
                                constant_loader: Some(&constant_loader_cl),
                                config_resolver: Some(&config_resolver),
                                trans_resolver: Some(&trans_resolver),
                            };
                            crate::type_engine::variable::forward_walk::build_diagnostic_scopes(
                                content,
                                &file_ctx.classes,
                                &class_loader,
                                Some(backend),
                                loaders,
                                Some(&backend.resolved_class_cache),
                            );
                        }
                        let scope_elapsed = scope_t0.elapsed();

                        let mut raw = Vec::new();

                        // In debug builds, time each collector and warn
                        // about slow files.  In release builds, just call
                        // the collectors directly.
                        #[cfg(debug_assertions)]
                        {
                            const FILE_TIMEOUT: Duration = Duration::from_secs(60);
                            let file_start = Instant::now();
                            let deadline = file_start + FILE_TIMEOUT;
                            let mut timings = Vec::new();
                            let mut timed_out = false;
                            // Record scope-build time (it ran before file_start).
                            timings.push((scope_elapsed, "scope"));

                            // Fast diagnostics always run (cheap).
                            timings.push({
                                let t0 = Instant::now();
                                backend.collect_fast_diagnostics(uri, content, &mut raw);
                                (t0.elapsed(), "fast")
                            });

                            // Slow collectors, timed one by one with the
                            // deadline checked between them, so a hang on a
                            // given file can be attributed to a single
                            // collector.  The list of collectors lives in
                            // `collect_slow_diagnostics` so this path always
                            // runs exactly what the LSP runs.
                            backend.collect_slow_diagnostics_observed(
                                uri,
                                content,
                                &mut raw,
                                Some(&mut |name, elapsed| {
                                    timings.push((elapsed, name));
                                    if Instant::now() >= deadline {
                                        timed_out = true;
                                        false
                                    } else {
                                        true
                                    }
                                }),
                            );

                            let file_elapsed = file_start.elapsed();
                            // The leading newline escapes the `\r`-rewritten
                            // progress-bar line; without the bar it would
                            // just leave blank lines.
                            let nl = if show_progress { "\n" } else { "" };
                            if timed_out {
                                let display =
                                    files[i].strip_prefix(root).unwrap_or(&files[i]).display();
                                let breakdown: Vec<String> = timings
                                    .iter()
                                    .filter(|(d, _)| d.as_millis() > 0)
                                    .map(|(d, name)| format!("{}={:.1}s", name, d.as_secs_f64()))
                                    .collect();
                                eprintln!(
                                    "{nl}  \u{23f1} timed out after {:.0}s: {}\n    {}",
                                    file_elapsed.as_secs_f64(),
                                    display,
                                    breakdown.join(", "),
                                );
                            } else if debug && file_elapsed.as_secs() >= 5 {
                                let display =
                                    files[i].strip_prefix(root).unwrap_or(&files[i]).display();
                                let breakdown: Vec<String> = timings
                                    .iter()
                                    .filter(|(d, _)| d.as_millis() > 0)
                                    .map(|(d, name)| format!("{}={:.1}s", name, d.as_secs_f64()))
                                    .collect();
                                eprintln!(
                                    "{nl}  \u{26a0} slow file ({:.1}s): {}\n    {}",
                                    file_elapsed.as_secs_f64(),
                                    display,
                                    breakdown.join(", "),
                                );
                            }
                        }

                        #[cfg(not(debug_assertions))]
                        {
                            let diag_t0 = Instant::now();
                            backend.collect_fast_diagnostics(uri, content, &mut raw);
                            let fast_elapsed = diag_t0.elapsed();
                            let slow_t0 = Instant::now();
                            backend.collect_slow_diagnostics(uri, content, &mut raw);
                            let slow_elapsed = slow_t0.elapsed();
                            let total = scope_elapsed + fast_elapsed + slow_elapsed;
                            if debug && total.as_secs() >= 2 {
                                let display =
                                    files[i].strip_prefix(root).unwrap_or(&files[i]).display();
                                eprintln!(
                                    "  \u{26a0} slow file ({:.1}s): {}\n    scope={:.1}s, fast={:.1}s, slow={:.1}s",
                                    total.as_secs_f64(),
                                    display,
                                    scope_elapsed.as_secs_f64(),
                                    fast_elapsed.as_secs_f64(),
                                    slow_elapsed.as_secs_f64(),
                                );
                            }
                        }

                        // ── Apply @phpantom-ignore comment suppression ─────
                        // Use original_content (not virtual PHP) because
                        // diagnostic line numbers have already been translated
                        // back to original file coordinates.
                        crate::diagnostics::suppression::filter_ignored_by_comment(
                            &mut raw,
                            original_content,
                        );

                        // ── Apply [[diagnostics.ignore]] config rules ──────
                        if !ignore_rules.is_empty() {
                            let relative_path = files[i]
                                .strip_prefix(root)
                                .unwrap_or(&files[i])
                                .to_string_lossy()
                                .replace('\\', "/");
                            crate::diagnostics::ignore_rules::filter_ignored_by_config(
                                &mut raw,
                                &relative_path,
                                ignore_rules,
                            );
                        }

                        // Diagnostic ranges are already in original-file
                        // coordinates: every collector builds its range through
                        // `Backend::offset_range_to_lsp_range`, which maps a
                        // Blade file's virtual-PHP range back through the source
                        // map. Translating again here would shift every Blade
                        // diagnostic up by `blade::PROLOGUE_LINES`.
                        let mut filtered: Vec<FileDiagnostic> = raw
                            .into_iter()
                            .filter_map(|d| {
                                let sev = d.severity.unwrap_or(DiagnosticSeverity::WARNING);
                                if !passes_severity_filter(sev, severity_filter) {
                                    return None;
                                }
                                let identifier = match &d.code {
                                    Some(NumberOrString::String(s)) => Some(s.clone()),
                                    _ => None,
                                };
                                Some(FileDiagnostic {
                                    line: d.range.start.line + 1,
                                    message: d.message,
                                    identifier,
                                    severity: sev,
                                })
                            })
                            .collect();

                        // Update progress bar after the file is fully
                        // processed so the count reflects completed work,
                        // not work that has merely been started.
                        let completed = done_count.fetch_add(1, Ordering::Relaxed) + 1;
                        if show_progress {
                            eprint!("\r\x1b[2K {}", progress_bar(completed, file_count));
                        }
                        if debug && verbosity >= 1 {
                            let display =
                                files[i].strip_prefix(root).unwrap_or(&files[i]).display();
                            let secs = file_t0.elapsed().as_secs_f64();
                            let prefix = if verbosity >= 2 {
                                format!("[w{worker:02}] ")
                            } else {
                                " ".to_string()
                            };
                            // Only read /proc at -vvv: `rss_bytes()` is a
                            // file read per file analyzed, so it must not
                            // run just to be discarded at -v/-vv.
                            match if verbosity >= 3 { rss_bytes() } else { None } {
                                Some(rss) => eprintln!(
                                    "{prefix}done {display} ({secs:.2}s, rss {} MB)",
                                    rss / (1024 * 1024),
                                ),
                                None => eprintln!("{prefix}done {display} ({secs:.2}s)"),
                            }
                        }

                        if !filtered.is_empty() {
                            filtered.sort_by_key(|d| d.line);
                            let display_path = files[i]
                                .strip_prefix(root)
                                .unwrap_or(&files[i])
                                .to_string_lossy()
                                .to_string();
                            results.push((display_path, filtered));
                        }
                    }
                    results
                })
                })
                .collect();

        let mut merged: Vec<(String, Vec<FileDiagnostic>)> = Vec::new();
        for handle in handles {
            merged.extend(
                handle
                    .expect("diagnostic worker thread spawn failed")
                    .join()
                    .unwrap_or_default(),
            );
        }
        merged
    });

    if show_progress {
        eprint!("\r\x1b[2K {}\n", progress_bar(file_count, file_count));
    }
    if verbosity >= 1 {
        // Every phase is listed, including the class population between
        // parsing and diagnostics: on a large project it can outweigh
        // both, and a summary that omits it leaves the bulk of the run
        // unaccounted for.
        eprintln!(
            " parse: {:.1}s, populate: {:.1}s, diagnose: {:.1}s, files: {}, threads: {}",
            parse_elapsed.as_secs_f64(),
            populate_elapsed.as_secs_f64(),
            diagnose_t0.elapsed().as_secs_f64(),
            file_count,
            n_threads,
        );
    }

    #[cfg(feature = "mem-audit")]
    if std::env::var_os("PHPANTOM_MEM_AUDIT").is_some() {
        let runner_bytes: usize = file_data
            .iter()
            .flatten()
            .map(|(uri, content)| uri.capacity() + content.capacity())
            .sum();
        crate::mem_audit::report(&backend, runner_bytes);
    }

    // Sort by path so output order is deterministic.
    all_file_diagnostics.sort_by(|a, b| a.0.cmp(&b.0));

    let total_errors: usize = all_file_diagnostics
        .iter()
        .map(|(_, diags)| diags.len())
        .sum();

    // ── 5. Render output ────────────────────────────────────────────
    if all_file_diagnostics.is_empty() {
        match output_format {
            OutputFormat::Table => print_success_box(file_count, options.use_colour),
            OutputFormat::Github => {} // no output on success
            OutputFormat::Json => print_json_output(&[], 0),
        }
        return 0;
    }

    match output_format {
        OutputFormat::Table => {
            // When running in GitHub Actions, also emit annotations
            // alongside the table (same behaviour as PHPStan).
            if std::env::var("GITHUB_ACTIONS").is_ok() {
                print_github_annotations(&all_file_diagnostics);
            }
            for (path, diagnostics) in &all_file_diagnostics {
                print_file_table(path, diagnostics, options.use_colour);
            }
            print_error_box(total_errors, file_count, options.use_colour);
        }
        OutputFormat::Github => {
            print_github_annotations(&all_file_diagnostics);
        }
        OutputFormat::Json => {
            print_json_output(&all_file_diagnostics, total_errors);
        }
    }

    1
}

// ── File discovery ──────────────────────────────────────────────────────────

/// Discover user PHP files to analyse.
///
/// Walks each PSR-4 source directory from `composer.json` (these only
/// cover the project's own code, not vendor).  When `path_filter` is
/// provided the results are cropped to that file or directory.
pub(crate) fn discover_user_files(
    backend: &Backend,
    workspace_root: &Path,
    path_filter: Option<&Path>,
    follow_links: bool,
) -> Vec<PathBuf> {
    use ignore::WalkBuilder;

    // Resolve the path filter to an absolute path.
    let abs_filter = path_filter.map(|f| {
        if f.is_relative() {
            workspace_root.join(f)
        } else {
            f.to_path_buf()
        }
    });

    // Single-file short circuit.
    if let Some(ref resolved) = abs_filter
        && resolved.is_file()
    {
        return if resolved.extension().is_some_and(|ext| ext == "php") {
            vec![resolved.clone()]
        } else {
            Vec::new()
        };
    }

    // Collect the PSR-4 source directories as absolute paths.
    let psr4 = backend.psr4_mappings().read().clone();
    let mut source_dirs: Vec<PathBuf> = psr4
        .iter()
        .map(|m| {
            let p = Path::new(&m.base_path);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                workspace_root.join(p)
            }
        })
        .filter(|p| p.is_dir())
        .collect();

    // Projects without PSR-4 mappings (no composer.json at all, or a
    // classmap/files-only autoload section) still need a user-file
    // set: walk the workspace root itself, the same tree the
    // self-scan class indexing covers.  The walker below still
    // honours ignore files and skips vendor directories.
    if source_dirs.is_empty() {
        source_dirs.push(workspace_root.to_path_buf());
    }

    // Also scan Laravel Blade view directories (from config/view.php
    // or the conventional resources/views fallback).
    for view_dir in crate::blade::discover_view_paths(workspace_root) {
        source_dirs.push(view_dir);
    }

    source_dirs.sort();
    source_dirs.dedup();

    // The walker compares canonical entry paths below.  Canonicalize the
    // registered roots once as well so path aliases such as macOS's `/var`
    // -> `/private/var` do not let vendor files through.
    let vendor_dirs = backend.workspace.vendor_dir_paths.lock().clone();
    let mut vendor_dirs: Vec<PathBuf> = vendor_dirs
        .into_iter()
        .map(|path| path.canonicalize().unwrap_or(path))
        .collect();
    vendor_dirs.sort_unstable();
    vendor_dirs.dedup();

    // When an explicit path filter points outside all PSR-4 source
    // directories (e.g. into vendor/), walk the filter path directly
    // instead of skipping it.  This matches PHPStan behaviour: the
    // default scan covers only user code, but an explicit override
    // scans whatever you point it at.
    let filter_overlaps_psr4 = abs_filter.as_ref().is_none_or(|fp| {
        source_dirs
            .iter()
            .any(|d| d.starts_with(fp) || fp.starts_with(d))
    });

    let dirs_to_walk: Vec<&Path> = if filter_overlaps_psr4 {
        source_dirs.iter().map(|p| p.as_path()).collect()
    } else {
        // The filter path doesn't overlap any PSR-4 dir — walk it
        // directly (no vendor exclusion since the user explicitly
        // asked for this path).
        vec![abs_filter.as_deref().unwrap()]
    };

    let mut files: Vec<PathBuf> = Vec::new();

    for dir in &dirs_to_walk {
        // If a directory filter is active and doesn't overlap with
        // this source dir, skip entirely.
        if let Some(ref fp) = abs_filter
            && fp.is_dir()
            && !dir.starts_with(fp)
            && !fp.starts_with(dir)
        {
            continue;
        }

        let skip_vendor = if filter_overlaps_psr4 {
            vendor_dirs.clone()
        } else {
            // User explicitly targeted this path — don't skip vendor
            // subdirectories within it.
            Vec::new()
        };
        let walker = WalkBuilder::new(dir)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .hidden(true)
            .parents(true)
            .ignore(true)
            .follow_links(follow_links)
            .filter_entry(move |entry| {
                if entry.file_type().is_some_and(|ft| ft.is_dir())
                    && !skip_vendor.is_empty()
                    && let Ok(canonical) = entry.path().canonicalize()
                    && skip_vendor.iter().any(|v| canonical.starts_with(v))
                {
                    return false;
                }
                true
            })
            .build();

        for entry in walker.flatten() {
            let path = entry.into_path();
            if !path.is_file() || path.extension().is_none_or(|ext| ext != "php") {
                continue;
            }

            // Crop to the filter directory.
            if let Some(ref fp) = abs_filter
                && fp.is_dir()
                && !path.starts_with(fp)
            {
                continue;
            }

            files.push(path);
        }
    }

    files.sort();
    files.dedup();
    files
}

/// Current process resident-set size in bytes, for the -vvv per-file
/// completion lines. Linux-only; other platforms report no rss.
#[cfg(target_os = "linux")]
fn rss_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}

#[cfg(not(target_os = "linux"))]
fn rss_bytes() -> Option<u64> {
    None
}

// ── Severity helpers ────────────────────────────────────────────────────────

fn passes_severity_filter(severity: DiagnosticSeverity, filter: SeverityFilter) -> bool {
    match filter {
        SeverityFilter::All => true,
        SeverityFilter::Warning => {
            matches!(
                severity,
                DiagnosticSeverity::ERROR | DiagnosticSeverity::WARNING
            )
        }
        SeverityFilter::Error => severity == DiagnosticSeverity::ERROR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_filter_all_passes_everything() {
        assert!(passes_severity_filter(
            DiagnosticSeverity::ERROR,
            SeverityFilter::All
        ));
        assert!(passes_severity_filter(
            DiagnosticSeverity::WARNING,
            SeverityFilter::All
        ));
        assert!(passes_severity_filter(
            DiagnosticSeverity::INFORMATION,
            SeverityFilter::All
        ));
        assert!(passes_severity_filter(
            DiagnosticSeverity::HINT,
            SeverityFilter::All
        ));
    }

    #[test]
    fn severity_filter_warning_blocks_info_and_hint() {
        assert!(passes_severity_filter(
            DiagnosticSeverity::ERROR,
            SeverityFilter::Warning
        ));
        assert!(passes_severity_filter(
            DiagnosticSeverity::WARNING,
            SeverityFilter::Warning
        ));
        assert!(!passes_severity_filter(
            DiagnosticSeverity::INFORMATION,
            SeverityFilter::Warning
        ));
        assert!(!passes_severity_filter(
            DiagnosticSeverity::HINT,
            SeverityFilter::Warning
        ));
    }

    #[test]
    fn severity_filter_error_only() {
        assert!(passes_severity_filter(
            DiagnosticSeverity::ERROR,
            SeverityFilter::Error
        ));
        assert!(!passes_severity_filter(
            DiagnosticSeverity::WARNING,
            SeverityFilter::Error
        ));
        assert!(!passes_severity_filter(
            DiagnosticSeverity::INFORMATION,
            SeverityFilter::Error
        ));
        assert!(!passes_severity_filter(
            DiagnosticSeverity::HINT,
            SeverityFilter::Error
        ));
    }

    /// Without PSR-4 mappings (no composer.json, or a classmap-only
    /// autoload), file discovery falls back to walking the workspace
    /// root, still skipping registered vendor directories.
    #[test]
    fn discover_user_files_walks_root_without_psr4() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let root = dir.path();
        std::fs::write(root.join("index.php"), "<?php\n").unwrap();
        std::fs::create_dir_all(root.join("includes")).unwrap();
        std::fs::write(root.join("includes/helper.php"), "<?php\n").unwrap();
        std::fs::write(root.join("readme.txt"), "not php\n").unwrap();
        std::fs::create_dir_all(root.join("vendor/lib")).unwrap();
        std::fs::write(root.join("vendor/lib/dep.php"), "<?php\n").unwrap();

        let backend = Backend::new_headless();
        backend.add_vendor_dir(&root.join("vendor"));

        let files = discover_user_files(&backend, root, None, false);
        let names: Vec<String> = files
            .iter()
            .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"index.php".to_string()), "{names:?}");
        assert!(
            names.contains(&"includes/helper.php".to_string()),
            "{names:?}"
        );
        assert!(
            !names.iter().any(|n| n.starts_with("vendor")),
            "vendor files must be skipped: {names:?}"
        );
        assert!(
            !names.contains(&"readme.txt".to_string()),
            "non-PHP files must be skipped: {names:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn discover_user_files_normalizes_aliased_vendor_roots() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let real_root = dir.path().join("real-project");
        let linked_root = dir.path().join("linked-project");
        std::fs::create_dir_all(real_root.join("app")).unwrap();
        std::fs::create_dir_all(real_root.join("vendor/pkg")).unwrap();
        std::fs::write(real_root.join("app/Main.php"), "<?php\n").unwrap();
        std::fs::write(real_root.join("vendor/pkg/Dep.php"), "<?php\n").unwrap();
        symlink(&real_root, &linked_root).expect("failed to create workspace alias");

        let backend = Backend::new_headless();
        backend
            .workspace
            .vendor_dir_paths
            .lock()
            .push(linked_root.join("vendor"));

        let files = discover_user_files(&backend, &real_root, None, false);
        assert!(files.contains(&real_root.join("app/Main.php")), "{files:?}");
        assert!(
            !files
                .iter()
                .any(|path| path.starts_with(real_root.join("vendor"))),
            "vendor files must be skipped across path aliases: {files:?}"
        );
    }

    /// A single-file path filter returns exactly that file even when
    /// the project has no PSR-4 mappings.
    #[test]
    fn discover_user_files_single_file_filter_without_psr4() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("includes")).unwrap();
        std::fs::write(root.join("includes/target.php"), "<?php\n").unwrap();
        std::fs::write(root.join("other.php"), "<?php\n").unwrap();

        let backend = Backend::new_headless();
        let files = discover_user_files(&backend, root, Some(Path::new("includes/target.php")), false);
        assert_eq!(files, vec![root.join("includes/target.php")]);
    }

    #[test]
    fn discover_user_files_follows_interior_symlink_when_enabled() {
        // CLI analyse's user-file walker keeps the same follow-links
        // contract as the workspace walkers (issue #383).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("ws");
        let real = dir.path().join("real");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("Hidden.php"), "<?php\n").unwrap();

        let link = root.join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&real, &link).unwrap();

        let backend = Backend::new_headless();
        let files = discover_user_files(&backend, &root, None, true);
        let linked = files
            .iter()
            .find(|p| p.ends_with("Hidden.php"))
            .unwrap_or_else(|| panic!("linked file must be indexed: {files:?}"));
        assert!(
            linked.starts_with(&link),
            "paths must keep the symlink spelling: {linked:?} vs {link:?}"
        );
    }

    #[test]
    fn discover_user_files_does_not_follow_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("ws");
        let real = dir.path().join("real");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("Hidden.php"), "<?php\n").unwrap();

        let link = root.join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&real, &link).unwrap();

        let backend = Backend::new_headless();
        let files = discover_user_files(&backend, &root, None, false);
        assert!(
            !files.iter().any(|p| p.ends_with("Hidden.php")),
            "interior symlink must not be followed by default: {files:?}"
        );
    }
}
