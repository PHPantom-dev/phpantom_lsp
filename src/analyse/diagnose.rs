//! The parallel diagnostic pass of the `analyze` command.
//!
//! Every user file has been parsed and every class it can see resolved
//! by the time this runs, so a worker only ever hits cached lookups for
//! cross-file references: no worker takes the write locks that lazy
//! PSR-4 loading would, and none of them serialise on each other.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(debug_assertions)]
use std::time::Duration;
use std::time::Instant;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::diagnostics::ignore_rules::CompiledIgnoreRule;
use crate::parser::with_parse_cache;
use crate::type_engine::resolver::LendsLoaders;
use crate::virtual_members::with_active_resolved_class_cache;

use super::output::progress_bar;
use super::{FileDiagnostic, SeverityFilter};

/// Everything one diagnostic pass works on.
pub(super) struct DiagnosticPass<'a> {
    /// The backend holding the indexed project.
    pub(super) backend: &'a Backend,
    /// Workspace root, used to shorten the paths that are reported.
    pub(super) root: &'a Path,
    /// The user files to diagnose.
    pub(super) files: &'a [PathBuf],
    /// Each file's URI and content from the parse phase, at the same
    /// index as `files`; `None` for a file that could not be read.
    pub(super) file_data: &'a [Option<(String, String)>],
    /// Compiled `[[diagnostics.ignore]]` rules.
    pub(super) ignore_rules: &'a [CompiledIgnoreRule],
    /// Minimum severity to keep.
    pub(super) severity_filter: SeverityFilter,
    /// Name every file as it starts and finishes.
    pub(super) debug: bool,
    /// Verbosity level: 0 = normal, 1 = -v, 2 = -vv, 3+ = -vvv.
    pub(super) verbosity: u8,
    /// Whether the progress bar is being drawn.
    pub(super) show_progress: bool,
    /// How many workers to run.
    pub(super) n_threads: usize,
}

/// Collect the diagnostics for every file in the pass, in parallel, and
/// return them per file in worker-completion order.
///
/// Individual collectors are called directly (instead of the grouped
/// `collect_slow_diagnostics`) so each one can be timed independently.
pub(super) fn collect_diagnostics(pass: DiagnosticPass<'_>) -> Vec<(String, Vec<FileDiagnostic>)> {
    let DiagnosticPass {
        backend,
        root,
        files,
        file_data,
        ignore_rules,
        severity_filter,
        debug,
        verbosity,
        show_progress,
        n_threads,
    } = pass;
    let file_count = files.len();

    let done_count = AtomicUsize::new(0);

    crate::parallel::map_indexed_with_threads(
        "diag-worker",
        file_count,
        Some(n_threads),
        |worker, i| {
                        let (uri, original_content) = match &file_data[i] {
                            Some(pair) => (&pair.0, &pair.1),
                            None => return None, // file that failed to read
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

                        // A Blade template is analysed as the virtual PHP it
                        // lowers to, which `update_ast` produced in Phase 1;
                        // every other file is analysed as itself.
                        let analysable = backend.analysable_content_or(uri, original_content);
                        let content: &str = &analysable;

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
                            let owned_loaders = backend.diagnostic_loaders(&file_ctx);
                            crate::type_engine::variable::forward_walk::build_diagnostic_scopes(
                                content,
                                &file_ctx.classes,
                                &class_loader,
                                Some(backend),
                                owned_loaders.loaders(),
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
                                    column: d.range.start.character,
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
                            eprint!("\r\x1b[2K {}", progress_bar(completed, file_count, ""));
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

                        if filtered.is_empty() {
                            return None;
                        }
                        filtered.sort_by(|a, b| {
                            a.line
                                .cmp(&b.line)
                                .then(a.column.cmp(&b.column))
                                .then(a.identifier.cmp(&b.identifier))
                                .then(a.message.cmp(&b.message))
                        });
                        let display_path = files[i]
                            .strip_prefix(root)
                            .unwrap_or(&files[i])
                            .to_string_lossy()
                            .to_string();
                        Some((display_path, filtered))
        },
    )
    .into_iter()
    .map(|(_, result)| result)
    .collect()
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
}
