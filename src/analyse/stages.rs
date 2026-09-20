//! The stages of a command-line run over a whole project.
//!
//! `analyze` and `fix` both open the project, discover the user files
//! they cover, and report on what they found.  The stages the two share
//! live here so neither can drift from the other in what it opens or
//! which files it picks up, next to the ones only `analyze` performs.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::Backend;
use crate::config::Config;
use crate::types::ClassInfo;

use super::output::{
    dispatch_report, print_error_box, print_file_table, print_github_annotations,
    print_json_output, print_success_box,
};
use super::{FileDiagnostic, OutputFormat};

// ── Shared with `fix` ───────────────────────────────────────────────────────

/// Note on stderr that `root` holds no `composer.json`, spelling out
/// what the command does instead in `consequence`.
///
/// A missing `composer.json` is not an error: plain PHP trees (a
/// WordPress site, a legacy codebase) work fine — classes are indexed by
/// scanning the tree and files are discovered by walking the root.  The
/// note exists so a mistyped `--project-root` does not silently work on
/// the wrong directory as a bare tree.
pub(crate) fn note_plain_php_project(root: &Path, consequence: &str) {
    if !root.join("composer.json").is_file() {
        eprintln!(
            "Note: no composer.json found in {} - {consequence}",
            root.display()
        );
    }
}

/// A project indexed on a headless `Backend`, with the user files the
/// command works on already discovered.
pub(crate) struct OpenedProject {
    /// The backend the project was indexed on.
    pub(crate) backend: Backend,
    /// The user files to work on, in path order.
    pub(crate) files: Vec<PathBuf>,
}

/// Index the project under `root` on a headless `Backend` (no LSP
/// client, so the log and progress calls are no-ops) and discover the
/// user files `path_filters` selects.
///
/// `None` when the project holds no PHP file to work on, which is
/// reported on stderr: a run with nothing to do is not a failure.
pub(crate) async fn open_project(
    root: &Path,
    cfg: Config,
    path_filters: &[PathBuf],
    follow_links: bool,
) -> Option<OpenedProject> {
    let backend = Backend::new_headless();
    super::open_headless_project(&backend, root, cfg).await;

    let files = super::discover_user_files(&backend, root, path_filters, follow_links);
    if files.is_empty() {
        eprintln!("No PHP files found.");
        return None;
    }

    Some(OpenedProject { backend, files })
}

// ── `analyze` only ──────────────────────────────────────────────────────────

/// The parsed user files, and what each half of the index build cost.
pub(super) struct IndexedProject {
    /// Each file's URI and content, at the same index as the file list;
    /// `None` for a file that could not be read.
    pub(super) file_data: Vec<Option<(String, String)>>,
    /// How long parsing took.
    pub(super) parse_elapsed: Duration,
    /// How long the eager class population took.
    pub(super) populate_elapsed: Duration,
}

/// Parse every user file and resolve every class the project knows, so
/// the diagnostic pass starts from a fully populated index.
///
/// Parsing first means `fqn_class_index`, `uri_classes_index`,
/// `symbol_maps`, `file_imports`, `file_namespaces` and `fqn_uri_index`
/// cover the whole project before a single diagnostic runs, so a
/// cross-file reference resolves through an O(1) hash lookup instead of
/// falling through to `fqn_uri_index` / PSR-4 lazy loading (which takes
/// write locks and serialises threads).  It also keeps the diagnostic
/// pass from ever calling `parse_and_cache_file` for another *user*
/// file, which was the main source of write-lock contention behind the
/// "stuck at 99 %" stall.
///
/// Resolving every class afterwards, in topological (dependency-first)
/// order, pre-populates the `resolved_class_cache` so the diagnostic
/// pass finds every dependency of a type it resolves already cached,
/// which is what keeps `resolve_class_fully_inner` out of the unbounded
/// mutual recursion that used to overflow the stack.
///
/// The `(uri, content)` pairs come back so the diagnostic pass can reuse
/// them without re-reading.  With `trace`, each file is named on stderr
/// as it is parsed.
pub(super) fn index_project(
    backend: &Backend,
    root: &Path,
    files: &[PathBuf],
    trace: bool,
) -> IndexedProject {
    let parse_t0 = Instant::now();
    let file_data = super::parse_user_files(backend, root, files, trace);
    let parse_elapsed = parse_t0.elapsed();
    let populate_t0 = Instant::now();

    super::discover_laravel_resources(backend);

    // The toposorted FQN list is snapshotted while holding the
    // uri_classes_index read lock, then the lock is dropped before
    // resolving: resolution may call find_or_load_class, which takes
    // write locks on uri_classes_index.
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

    // Blade templates parsed before their controllers saw no `view()`
    // call sites.  With every user file parsed, re-run call-site
    // inference and re-parse the templates whose inferred set changed, so
    // the diagnostic pass sees them with injected variables in scope.
    backend.refresh_blade_injected_vars();

    IndexedProject {
        file_data,
        parse_elapsed,
        populate_elapsed: populate_t0.elapsed(),
    }
}

/// Render `diagnostics` in the requested format and return the process
/// exit code: `0` when the project is clean, `1` when it is not.
pub(super) fn report(
    diagnostics: &[(String, Vec<FileDiagnostic>)],
    file_count: usize,
    output_format: OutputFormat,
    use_colour: bool,
) -> i32 {
    if diagnostics.is_empty() {
        dispatch_report(
            output_format,
            || print_success_box(" [OK] No errors ", use_colour),
            || {}, // no output on success
            || print_json_output(&[], 0),
        );
        return 0;
    }

    let total_errors: usize = diagnostics.iter().map(|(_, diags)| diags.len()).sum();

    dispatch_report(
        output_format,
        || {
            for (path, file_diagnostics) in diagnostics {
                print_file_table(path, file_diagnostics, use_colour);
            }
            print_error_box(total_errors, file_count, use_colour);
        },
        || print_github_annotations(diagnostics),
        || print_json_output(diagnostics, total_errors),
    );

    1
}
