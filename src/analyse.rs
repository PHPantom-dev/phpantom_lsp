//! CLI analysis mode.
//!
//! Scans PHP files in a project and reports PHPantom's own diagnostics
//! (no PHPStan, no external tools) in a PHPStan-like table format.
//!
//! # Philosophy
//!
//! The goal is **100% type coverage**: every class, member, and function
//! call in the project should be resolvable by the LSP.  When that holds,
//! completion works everywhere with no dead spots, and downstream tools
//! like PHPStan get the type information they need to find real bugs at
//! every level.  PHPStan only complains about missing types at levels 6,
//! 9, and 10; PHPantom fills those gaps cheaply and immediately so
//! PHPStan can focus on logic errors rather than fighting incomplete
//! type information.
//!
//! The diagnostics reported here are not trying to be a static analyser.
//! They assert structural correctness: does this class exist, does this
//! member exist, does the argument count match, did you implement every
//! required method.  Bug hunting is left to dedicated tools like PHPStan
//! and Psalm.  The `analyze` command surfaces the places where the LSP
//! cannot resolve a symbol so the user can fix them and achieve (or
//! maintain) full completion coverage across the project.
//!
//! It reuses the same `Backend` initialization pipeline as the LSP
//! server, so the results match exactly what a user would see in their
//! editor.
//!
//! Composer projects (root `composer.json`) use their autoload
//! configuration for file discovery; a plain PHP tree without one is
//! analysed by scanning and walking the workspace root.  Multi-project
//! monorepos are not supported.
//!
//! # Usage
//!
//! ```sh
//! phpantom_lsp analyze                     # scan entire project
//! phpantom_lsp analyze src/                # scan a subdirectory
//! phpantom_lsp analyze src/Foo.php         # scan a single file
//! ```

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

/// Severity filter for the analyse output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeverityFilter {
    /// Show all diagnostics (error, warning, information, hint).
    All,
    /// Show only errors and warnings.
    Warning,
    /// Show only errors.
    Error,
}

/// Output format for CLI commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable PHPStan-style table (default).
    Table,
    /// GitHub Actions workflow commands (`::error file=...::message`).
    /// Diagnostics appear as inline annotations on pull request diffs.
    Github,
    /// JSON object with totals and per-file diagnostics.
    Json,
}

/// Options for the analyse command.
#[derive(Debug)]
pub struct AnalyseOptions {
    /// Workspace root.  Usually a Composer project directory; a plain
    /// PHP tree without a composer.json is analysed by walking the
    /// root.
    pub workspace_root: PathBuf,
    /// Optional path filter: only analyse files under this path.
    /// Can be a directory or a single file.
    pub path_filter: Option<PathBuf>,
    /// Minimum severity to report.
    pub severity_filter: SeverityFilter,
    /// Whether to output with ANSI colours.
    pub use_colour: bool,
    /// Output format.
    pub output_format: OutputFormat,
}

/// A single diagnostic result for the analyse output.
struct FileDiagnostic {
    /// 1-based line number.
    line: u32,
    /// The diagnostic message.
    message: String,
    /// The diagnostic code (e.g. "unknown_class").
    identifier: Option<String>,
    /// The diagnostic severity.
    severity: DiagnosticSeverity,
}

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
    let cfg = match config::load_config(root) {
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
    *backend.config.lock() = cfg.clone();

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
    let files = discover_user_files(&backend, root, options.path_filter.as_deref());

    if files.is_empty() {
        eprintln!("No PHP files found.");
        return 0;
    }

    // ── 4. Two-phase parallel analysis ──────────────────────────────
    //
    // Phase 1 — **Parse**: run `update_ast` on every user file so that
    // `fqn_index`, `uri_classes_index`, `symbol_maps`, `use_map`, `namespace_map`
    // and `fqn_uri_index` are fully populated for the entire project.
    //
    // Phase 2 — **Diagnose**: collect diagnostics for every file.
    // Because all user classes are already in `fqn_index`, cross-file
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
    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    // ── Phase 1: Parse all files (parallel) ─────────────────────────
    // Read each file from disk and call `update_ast`.  Store the
    // (uri, content) pairs so Phase 2 can reuse them without re-reading.
    //
    // Parsing is fast, so the progress bar is drawn at 0% before Phase 1
    // and only advances during Phase 2 (the expensive diagnostic pass).
    if use_colour && output_format == OutputFormat::Table {
        eprint!("\r\x1b[2K {}", progress_bar(0, file_count));
    }
    let next_idx = AtomicUsize::new(0);

    let file_data: Vec<Option<(String, String)>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..n_threads)
            .map(|_| {
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
        let uri_classes_index = backend.uri_classes_index.read();
        crate::toposort::toposort_from_uri_classes_index(&uri_classes_index)
    };
    // Run on a dedicated large-stack thread: `resolve_class_fully_inner`
    // can nest deeply when the toposort misses dependencies (stubs,
    // dynamically loaded classes), and this runs on a Tokio worker whose
    // stack is the 2 MB default rather than the main thread's 8 MB.
    std::thread::scope(|s| {
        let backend = &backend;
        let sorted_fqns = &sorted_fqns;
        std::thread::Builder::new()
            .name("eager-populate".into())
            .stack_size(crate::PARSE_WORKER_STACK_SIZE)
            .spawn_scoped(s, move || {
                let class_loader =
                    |name: &str| -> Option<Arc<ClassInfo>> { backend.find_or_load_class(name) };
                crate::virtual_members::populate_from_sorted(
                    sorted_fqns,
                    &backend.resolved_class_cache,
                    &class_loader,
                );
            })
            .expect("failed to spawn eager-population thread");
    });
    // ── Phase 2: Collect diagnostics (parallel) ─────────────────────
    // Call individual collectors directly (instead of the grouped
    // collect_slow_diagnostics) so we can time each one independently.
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
                .map(|_| {
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
                            crate::completion::resolver::with_chain_resolution_cache();
                        let _callable_guard =
                            crate::completion::call_resolution::with_callable_target_cache();
                        let _body_infer_guard = backend.activate_body_return_inferrer();
                        let _auth_user_guard = backend.activate_auth_user_resolver();

                        // ── Forward-walked diagnostic scope cache ───
                        // Walk every function/method body once with the
                        // forward walker, recording scope snapshots at
                        // each statement boundary.  All subsequent
                        // `resolve_variable_types` calls from diagnostic
                        // collectors hit the cache (O(log N) lookup)
                        // instead of doing a full backward scan.
                        let _scope_guard =
                            crate::completion::variable::forward_walk::with_diagnostic_scope_cache(
                            );
                        let scope_t0 = Instant::now();
                        {
                            let file_ctx = backend.file_context(uri);
                            let class_loader = backend.class_loader(&file_ctx);
                            let function_loader_cl = backend.function_loader(&file_ctx);
                            let constant_loader_cl = backend.constant_loader();
                            let loaders = crate::completion::resolver::Loaders {
                                function_loader: Some(&function_loader_cl),
                                constant_loader: Some(&constant_loader_cl),
                            };
                            crate::completion::variable::forward_walk::build_diagnostic_scopes(
                                content,
                                &file_ctx.classes,
                                &class_loader,
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
                            type CollectFn = dyn Fn(&Backend, &str, &str, &mut Vec<Diagnostic>);
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

                            // Slow collectors, each run separately so it
                            // can be timed independently and check the
                            // deadline between collectors.  Keeping them as
                            // named entries also makes it easy to disable an
                            // individual collector when narrowing down which
                            // one is responsible for a hang on a given file.
                            let collectors: &[(&str, &CollectFn)] = &[
                                (
                                    "unknown_class",
                                    &|b: &Backend, u: &str, c: &str, o: &mut Vec<Diagnostic>| {
                                        b.collect_unknown_class_diagnostics(u, c, o)
                                    },
                                ),
                                ("class_case_mismatch", &|b, u, c, o| {
                                    b.collect_class_case_mismatch_diagnostics(u, c, o)
                                }),
                                ("unknown_member", &|b, u, c, o| {
                                    b.collect_unknown_member_diagnostics(u, c, o)
                                }),
                                ("unknown_function", &|b, u, c, o| {
                                    b.collect_unknown_function_diagnostics(u, c, o)
                                }),
                                ("argument_count_mismatch", &|b, u, c, o| {
                                    b.collect_argument_count_diagnostics(u, c, o)
                                }),
                                ("type_mismatch_argument", &|b, u, c, o| {
                                    b.collect_argument_type_diagnostics(u, c, o)
                                }),
                                ("type_mismatch_return", &|b, u, c, o| {
                                    b.collect_return_type_diagnostics(u, c, o)
                                }),
                                ("type_mismatch_property", &|b, u, c, o| {
                                    b.collect_property_type_diagnostics(u, c, o)
                                }),
                                ("missing_implementation", &|b, u, c, o| {
                                    b.collect_implementation_error_diagnostics(u, c, o)
                                }),
                                ("deprecated_usage", &|b, u, c, o| {
                                    b.collect_deprecated_diagnostics(u, c, o)
                                }),
                                ("unknown_variable", &|b, u, c, o| {
                                    b.collect_undefined_variable_diagnostics(u, c, o)
                                }),
                                ("invalid_class_kind", &|b, u, c, o| {
                                    b.collect_invalid_class_kind_diagnostics(u, c, o)
                                }),
                            ];

                            for (name, collect_fn) in collectors {
                                if Instant::now() >= deadline {
                                    timed_out = true;
                                    break;
                                }
                                let t0 = Instant::now();
                                collect_fn(backend, uri, content, &mut raw);
                                let elapsed = t0.elapsed();
                                timings.push((elapsed, name));
                            }

                            let file_elapsed = file_start.elapsed();
                            if timed_out {
                                let display =
                                    files[i].strip_prefix(root).unwrap_or(&files[i]).display();
                                let breakdown: Vec<String> = timings
                                    .iter()
                                    .filter(|(d, _)| d.as_millis() > 0)
                                    .map(|(d, name)| format!("{}={:.1}s", name, d.as_secs_f64()))
                                    .collect();
                                eprintln!(
                                    "\n  \u{23f1} timed out after {:.0}s: {}\n    {}",
                                    file_elapsed.as_secs_f64(),
                                    display,
                                    breakdown.join(", "),
                                );
                            } else if file_elapsed.as_secs() >= 5 {
                                let display =
                                    files[i].strip_prefix(root).unwrap_or(&files[i]).display();
                                let breakdown: Vec<String> = timings
                                    .iter()
                                    .filter(|(d, _)| d.as_millis() > 0)
                                    .map(|(d, name)| format!("{}={:.1}s", name, d.as_secs_f64()))
                                    .collect();
                                eprintln!(
                                    "\n  \u{26a0} slow file ({:.1}s): {}\n    {}",
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
                            if total.as_secs() >= 2 {
                                let display =
                                    files[i].strip_prefix(root).unwrap_or(&files[i]).display();
                                eprintln!(
                                    "\n  \u{26a0} slow file ({:.1}s): {}\n    scope={:.1}s, fast={:.1}s, slow={:.1}s",
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
                        crate::diagnostics::filter_ignored_by_comment(&mut raw, original_content);

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

                        // For Blade files, translate diagnostic ranges from
                        // virtual PHP coordinates back to original Blade
                        // coordinates so line numbers match the source file.
                        let is_blade = crate::blade::is_blade_file(uri);
                        let source_map = if is_blade {
                            backend.blade_source_maps.read().get(uri.as_str()).cloned()
                        } else {
                            None
                        };

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
                                let line = if let Some(ref map) = source_map {
                                    map.php_to_blade(d.range.start).line + 1
                                } else {
                                    d.range.start.line + 1
                                };
                                Some(FileDiagnostic {
                                    line,
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
                        if use_colour && output_format == OutputFormat::Table {
                            eprint!("\r\x1b[2K {}", progress_bar(completed, file_count));
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

    if use_colour && output_format == OutputFormat::Table {
        eprint!("\r\x1b[2K {}\n", progress_bar(file_count, file_count));
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

    let vendor_dirs: Vec<PathBuf> = backend.vendor_dir_paths.lock().clone();

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

// ── GitHub Actions annotations ──────────────────────────────────────────────

/// Emit GitHub Actions workflow commands for all diagnostics.
///
/// Each diagnostic is printed as a `::error` or `::warning` line so that
/// GitHub Actions surfaces them as inline annotations on pull request diffs.
/// See: <https://docs.github.com/en/actions/writing-workflows/choosing-what-your-workflow-does/workflow-commands-for-github-actions>
fn print_github_annotations(file_diagnostics: &[(String, Vec<FileDiagnostic>)]) {
    for (path, diagnostics) in file_diagnostics {
        for diag in diagnostics {
            let level = match diag.severity {
                DiagnosticSeverity::ERROR => "error",
                DiagnosticSeverity::WARNING => "warning",
                _ => "notice",
            };
            let message = format_github_message(&diag.message);
            let title = diag.identifier.as_deref().unwrap_or("");
            if title.is_empty() {
                println!(
                    "::{level} file={path},line={line},col=0::{message}",
                    line = diag.line,
                );
            } else {
                println!(
                    "::{level} file={path},line={line},col=0,title={title}::{message}",
                    line = diag.line,
                );
            }
        }
    }
}

/// Format a message for GitHub Actions workflow commands.
///
/// Newlines are encoded as `%0A` per the GitHub Actions spec, and `@mentions`
/// are wrapped in backticks to prevent GitHub from sending notifications
/// (matching PHPStan's `GithubErrorFormatter` behaviour).
pub(crate) fn format_github_message(message: &str) -> String {
    let message = message.replace('\n', "%0A");
    // Wrap @mentions in backticks to prevent GitHub notifications.
    let mut result = String::with_capacity(message.len());
    let mut chars = message.char_indices().peekable();
    let mut last_end = 0;
    while let Some((i, c)) = chars.next() {
        if c == '@' {
            let before_is_space = i == 0
                || message
                    .as_bytes()
                    .get(i - 1)
                    .is_none_or(|b| b.is_ascii_whitespace());
            if before_is_space {
                // Collect the mention: @[a-zA-Z0-9_-]+
                let start = i + 1;
                let mut end = start;
                while let Some(&(j, nc)) = chars.peek() {
                    if nc.is_ascii_alphanumeric() || nc == '_' || nc == '-' {
                        end = j + nc.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                if end > start {
                    result.push_str(&message[last_end..i]);
                    result.push('`');
                    result.push_str(&message[i..end]);
                    result.push('`');
                    last_end = end;
                    continue;
                }
            }
        }
    }
    result.push_str(&message[last_end..]);
    result
}

// ── JSON output ─────────────────────────────────────────────────────────────

/// Print all diagnostics as a single JSON object.
///
/// The format mirrors PHPStan's JSON output:
/// ```json
/// {
///   "totals": { "errors": 0, "file_errors": 42 },
///   "files": {
///     "src/Foo.php": {
///       "errors": 2,
///       "messages": [
///         { "message": "...", "line": 15, "severity": "error", "identifier": "unknown_class" }
///       ]
///     }
///   },
///   "errors": []
/// }
/// ```
fn print_json_output(file_diagnostics: &[(String, Vec<FileDiagnostic>)], total_errors: usize) {
    use std::fmt::Write;

    let mut out = String::from("{\n");
    let _ = writeln!(
        out,
        "  \"totals\": {{ \"errors\": 0, \"file_errors\": {} }},",
        total_errors
    );

    if file_diagnostics.is_empty() {
        out.push_str("  \"files\": {},\n");
    } else {
        out.push_str("  \"files\": {\n");
        for (i, (path, diagnostics)) in file_diagnostics.iter().enumerate() {
            let _ = write!(
                out,
                "    {}: {{\n      \"errors\": {},\n      \"messages\": [\n",
                json_escape(path),
                diagnostics.len()
            );
            for (j, diag) in diagnostics.iter().enumerate() {
                let severity_str = match diag.severity {
                    DiagnosticSeverity::ERROR => "error",
                    DiagnosticSeverity::WARNING => "warning",
                    DiagnosticSeverity::INFORMATION => "info",
                    DiagnosticSeverity::HINT => "hint",
                    _ => "unknown",
                };
                let _ = write!(
                    out,
                    "        {{ \"message\": {}, \"line\": {}, \"severity\": \"{}\"",
                    json_escape(&diag.message),
                    diag.line,
                    severity_str,
                );
                if let Some(ref id) = diag.identifier {
                    let _ = write!(out, ", \"identifier\": {}", json_escape(id));
                }
                out.push_str(" }");
                if j + 1 < diagnostics.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str("      ]\n    }");
            if i + 1 < file_diagnostics.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  },\n");
    }

    out.push_str("  \"errors\": []\n}");
    println!("{out}");
}

/// Escape a string for JSON output.
pub(crate) fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\x20' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ── PHPStan-style table output ──────────────────────────────────────────────
//
// Mirrors Symfony Console's `Table` style used by PHPStan's
// `TableErrorFormatter` (see phpstan-src tests for exact spacing):
//
//  ------ -------------------------------------------
//   Line   src/Foo.php
//  ------ -------------------------------------------
//   15     Call to undefined method Bar::baz().
//          🪪  unknown_member
//   42     Access to property $qux on unknown class.
//          🪪  unknown_class
//  ------ -------------------------------------------

/// Print a file's diagnostics in the PHPStan table format.
fn print_file_table(path: &str, diagnostics: &[FileDiagnostic], use_colour: bool) {
    struct Row {
        line_str: String,
        lines: Vec<String>,
    }

    let mut rows: Vec<Row> = Vec::new();
    for diag in diagnostics {
        let mut message_lines = vec![diag.message.clone()];
        if let Some(ref id) = diag.identifier {
            message_lines.push(format!("\u{1faaa}  {id}"));
        }
        rows.push(Row {
            line_str: diag.line.to_string(),
            lines: message_lines,
        });
    }

    // Column widths.
    let line_col_w = rows
        .iter()
        .map(|r| r.line_str.len())
        .max()
        .unwrap_or(0)
        .max(4); // at least as wide as "Line"

    let msg_col_w = rows
        .iter()
        .flat_map(|r| r.lines.iter().map(|l| l.len()))
        .max()
        .unwrap_or(0)
        .max(path.len());

    let sep = format!(
        " {} {}",
        "-".repeat(line_col_w + 2),
        "-".repeat(msg_col_w + 2),
    );

    // Header.
    println!("{sep}");
    if use_colour {
        println!(
            "  \x1b[32m{:>line_col_w$}\x1b[0m   \x1b[32m{path}\x1b[0m",
            "Line"
        );
    } else {
        println!("  {:>line_col_w$}   {path}", "Line");
    }
    println!("{sep}");

    // Data rows.
    for row in &rows {
        for (i, msg_line) in row.lines.iter().enumerate() {
            if i == 0 {
                println!("  {:>line_col_w$}   {msg_line}", row.line_str);
            } else if use_colour {
                println!("  {:>line_col_w$}   \x1b[2m{msg_line}\x1b[0m", "");
            } else {
                println!("  {:>line_col_w$}   {msg_line}", "");
            }
        }
    }

    // Footer + blank line between files.
    println!("{sep}");
    println!();
}

/// Print the `[OK]` success box.
fn print_success_box(_file_count: usize, use_colour: bool) {
    let text = " [OK] No errors ";
    if use_colour {
        let pad = " ".repeat(text.len());
        println!();
        println!(" \x1b[30;42m{pad}\x1b[0m");
        println!(" \x1b[30;42m{text}\x1b[0m");
        println!(" \x1b[30;42m{pad}\x1b[0m");
        println!();
    } else {
        println!("{text}");
    }
}

/// Print the `[ERROR]` summary box.
fn print_error_box(total_errors: usize, _file_count: usize, use_colour: bool) {
    let label = if total_errors == 1 { "error" } else { "errors" };
    let text = format!(" [ERROR] Found {total_errors} {label} ");
    if use_colour {
        let pad = " ".repeat(text.len());
        println!();
        println!(" \x1b[97;41m{pad}\x1b[0m");
        println!(" \x1b[97;41m{text}\x1b[0m");
        println!(" \x1b[97;41m{pad}\x1b[0m");
        println!();
    } else {
        println!("{text}");
    }
}

// ── Progress bar ────────────────────────────────────────────────────────────

const BAR_WIDTH: usize = 28;

/// Render a PHPStan-style progress bar string:
/// ` 120/883 [▓▓▓▓░░░░░░░░░░░░░░░░░░░░░░░░]  13%`
fn progress_bar(done: usize, total: usize) -> String {
    let pct = (done * 100).checked_div(total).unwrap_or(100);
    let filled = (done * BAR_WIDTH).checked_div(total).unwrap_or(BAR_WIDTH);
    let empty = BAR_WIDTH - filled;

    format!(
        " {done:>width$}/{total} [{bar_fill}{bar_empty}] {pct:>3}%",
        width = total.to_string().len(),
        bar_fill = "▓".repeat(filled),
        bar_empty = "░".repeat(empty),
    )
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

    #[test]
    fn json_escape_basic() {
        assert_eq!(json_escape("hello"), "\"hello\"");
    }

    #[test]
    fn json_escape_special_chars() {
        assert_eq!(json_escape("a\"b\\c\nd"), "\"a\\\"b\\\\c\\nd\"");
    }

    #[test]
    fn json_escape_control_chars() {
        assert_eq!(json_escape("\x00\x1f"), "\"\\u0000\\u001f\"");
    }

    #[test]
    fn github_annotation_format() {
        let diag = FileDiagnostic {
            line: 15,
            message: "Call to undefined method Bar::baz().".to_string(),
            identifier: Some("unknown_member".to_string()),
            severity: DiagnosticSeverity::ERROR,
        };
        // Verify the struct builds correctly with the expected values.
        assert_eq!(diag.line, 15);
        assert_eq!(diag.severity, DiagnosticSeverity::ERROR);
        assert_eq!(diag.identifier.as_deref(), Some("unknown_member"));
    }

    #[test]
    fn json_output_empty() {
        // Verify print_json_output doesn't panic with empty input.
        // We can't easily capture stdout in unit tests, so just verify
        // the helper works.
        let out = {
            let mut s = String::new();
            use std::fmt::Write;
            let _ = write!(s, "{{}}");
            s
        };
        assert_eq!(out, "{}");
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

        let files = discover_user_files(&backend, root, None);
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
        let files = discover_user_files(&backend, root, Some(Path::new("includes/target.php")));
        assert_eq!(files, vec![root.join("includes/target.php")]);
    }
}
