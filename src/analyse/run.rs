//! The `analyze` command driver and file discovery.
//!
//! Runs the same `Backend` indexing pipeline as the LSP server across
//! a whole project, collects diagnostics in parallel, and hands the
//! results to the `output` module for rendering.

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::Backend;

use super::diagnose::{DiagnosticPass, collect_diagnostics};
use super::output::progress_bar;
use super::stages::{OpenedProject, index_project, note_plain_php_project, open_project, report};
use super::{AnalyseOptions, OutputFormat};

/// Run the analyse command and return the process exit code.
///
/// Returns `0` when no diagnostics are found, `1` when diagnostics exist.
pub async fn run(options: AnalyseOptions) -> i32 {
    let root = &options.workspace_root;

    note_plain_php_project(root, "analysing as a plain PHP project.");

    let cfg = super::load_config_or_default(root, options.global_config.as_deref());
    let ignore_rules =
        crate::diagnostics::ignore_rules::compile_ignore_rules(&cfg.diagnostics.ignore);
    let follow_links = cfg.indexing.follow_links();

    let Some(OpenedProject { backend, files }) =
        open_project(root, cfg, &options.path_filters, follow_links).await
    else {
        return 0;
    };

    let file_count = files.len();
    let debug = options.debug;
    let verbosity = options.verbosity;
    // Per-file lines and the `\r`-rewritten progress bar would clobber
    // each other, so --debug replaces the bar entirely.
    let show_progress =
        options.use_colour && options.output_format == OutputFormat::Table && !debug;
    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    // Parsing is fast, so the progress bar is drawn at 0% before the
    // index build and only advances during the diagnostic pass.
    if show_progress {
        eprint!("\r\x1b[2K {}", progress_bar(0, file_count, ""));
    }
    let indexed = index_project(&backend, root, &files, debug && verbosity >= 2);

    let diagnose_t0 = Instant::now();
    let mut all_file_diagnostics = collect_diagnostics(DiagnosticPass {
        backend: &backend,
        root,
        files: &files,
        file_data: &indexed.file_data,
        ignore_rules: &ignore_rules,
        severity_filter: options.severity_filter,
        debug,
        verbosity,
        show_progress,
        n_threads,
    });

    if show_progress {
        eprint!("\r\x1b[2K {}\n", progress_bar(file_count, file_count, ""));
    }
    if verbosity >= 1 {
        // Every phase is listed, including the class population between
        // parsing and diagnostics: on a large project it can outweigh
        // both, and a summary that omits it leaves the bulk of the run
        // unaccounted for.
        eprintln!(
            " parse: {:.1}s, populate: {:.1}s, diagnose: {:.1}s, files: {}, threads: {}",
            indexed.parse_elapsed.as_secs_f64(),
            indexed.populate_elapsed.as_secs_f64(),
            diagnose_t0.elapsed().as_secs_f64(),
            file_count,
            n_threads,
        );
    }

    #[cfg(feature = "mem-audit")]
    if std::env::var_os("PHPANTOM_MEM_AUDIT").is_some() {
        let runner_bytes: usize = indexed
            .file_data
            .iter()
            .flatten()
            .map(|(uri, content)| uri.capacity() + content.capacity())
            .sum();
        crate::mem_audit::report(&backend, runner_bytes);
    }

    // Sort by path so output order is deterministic.
    all_file_diagnostics.sort_by(|a, b| a.0.cmp(&b.0));

    report(
        &all_file_diagnostics,
        file_count,
        options.output_format,
        options.use_colour,
    )
}

// ── File discovery ──────────────────────────────────────────────────────────

/// Discover user PHP files to analyse.
///
/// Walks each PSR-4 source directory from `composer.json` (these only
/// cover the project's own code, not vendor).  When `path_filters` is
/// non-empty the results are cropped to those files and directories.
pub(crate) fn discover_user_files(
    backend: &Backend,
    workspace_root: &Path,
    path_filters: &[PathBuf],
    follow_links: bool,
) -> Vec<PathBuf> {
    // Resolve the path filters to absolute paths, and split them into the
    // directories that need walking and the files that are taken as given.
    let (filter_dirs, filter_files): (Vec<PathBuf>, Vec<PathBuf>) = path_filters
        .iter()
        .map(|f| {
            if f.is_relative() {
                workspace_root.join(f)
            } else {
                f.to_path_buf()
            }
        })
        .partition(|p| p.is_dir());

    // `[indexing] extensions` files are PHP source, so they are analysed
    // like `.php`; `[indexing] exclude` prunes the default project walk
    // below but never a path the user named outright.
    let filters = backend.index_filters();

    let mut files: Vec<PathBuf> = filter_files
        .into_iter()
        .filter(|p| filters.is_php_file(p))
        .collect();

    // Every filter named a file, so there is nothing left to walk.
    if !path_filters.is_empty() && filter_dirs.is_empty() {
        files.sort();
        files.dedup();
        return files;
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

    // A directory filter that points outside every PSR-4 source directory
    // (e.g. into vendor/) is walked directly instead of being skipped.
    // This matches PHPStan behaviour: the default scan covers only user
    // code, but an explicit override scans whatever you point it at.
    let (psr4_filters, external_filters): (Vec<&Path>, Vec<&Path>) =
        filter_dirs.iter().map(PathBuf::as_path).partition(|fp| {
            source_dirs
                .iter()
                .any(|d| d.starts_with(fp) || fp.starts_with(d))
        });

    // Walk the project's own source tree when no filter was given, or when
    // at least one filter lands inside it.
    if filter_dirs.is_empty() || !psr4_filters.is_empty() {
        for dir in &source_dirs {
            // Skip source directories that no active filter overlaps.
            if !psr4_filters.is_empty()
                && !psr4_filters
                    .iter()
                    .any(|fp| dir.starts_with(fp) || fp.starts_with(dir))
            {
                continue;
            }

            collect_php_files(dir, &vendor_dirs, &psr4_filters, &filters, follow_links, &mut files);
        }
    }

    // The user explicitly targeted these paths, so no vendor exclusion and
    // no cropping beyond the walked directory itself.  `[indexing] exclude`
    // still applies: it declares what is not the project's code at all, the
    // way PHPStan's `excludePaths` holds for a path named on its command
    // line too.  Naming a file outright bypasses it (those never reach this
    // walk), which is the escape hatch for analysing an excluded path.
    for dir in &external_filters {
        collect_php_files(dir, &[], &[], &filters, follow_links, &mut files);
    }

    files.sort();
    files.dedup();
    files
}

/// Walk `dir` for PHP files, skipping anything under `skip_vendor` and
/// keeping only files under one of the `crop` paths (all of them when
/// `crop` is empty).
///
/// `filters` decides which extensions count as PHP source and which
/// paths `[indexing] exclude` prunes.
fn collect_php_files(
    dir: &Path,
    skip_vendor: &[PathBuf],
    crop: &[&Path],
    filters: &std::sync::Arc<crate::classmap_scanner::IndexFilters>,
    follow_links: bool,
    out: &mut Vec<PathBuf>,
) {
    use ignore::WalkBuilder;

    let skip_vendor = skip_vendor.to_vec();
    let filter_excludes = std::sync::Arc::clone(filters);
    let walker = WalkBuilder::new(dir)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .hidden(true)
        .parents(true)
        .ignore(true)
        .follow_links(follow_links)
        .filter_entry(move |entry| {
            let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
            if is_dir
                && !skip_vendor.is_empty()
                && let Ok(canonical) = entry.path().canonicalize()
                && skip_vendor.iter().any(|v| canonical.starts_with(v))
            {
                return false;
            }
            !filter_excludes.is_excluded_entry(entry.path(), is_dir)
        })
        .build();

    for entry in walker.flatten() {
        let path = entry.into_path();
        if !path.is_file() || !filters.is_php_file(&path) {
            continue;
        }

        if !crop.is_empty() && !crop.iter().any(|fp| path.starts_with(fp)) {
            continue;
        }

        out.push(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let files = discover_user_files(&backend, root, &[], false);
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

    /// `[indexing] exclude` prunes the project scan, and `[indexing]
    /// extensions` brings non-`.php` sources into it, so `analyze`
    /// reports on the same set of files the indexer holds. Excludes
    /// hold for a directory named on the command line as well; naming
    /// a file outright is the escape hatch.
    #[test]
    fn discover_user_files_honors_the_indexing_filters() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let root = dir.path();
        std::fs::write(root.join("index.php"), "<?php\n").unwrap();
        std::fs::write(root.join("hooks.module"), "<?php\n").unwrap();
        std::fs::create_dir_all(root.join("generated")).unwrap();
        std::fs::write(root.join("generated/Stub.php"), "<?php\n").unwrap();

        let backend = Backend::new_headless();
        *backend.workspace_root().write() = Some(root.to_path_buf());
        let mut cfg = crate::config::Config::default();
        cfg.indexing.exclude = Some(vec!["generated".to_string()]);
        cfg.indexing.extensions = Some(vec!["module".to_string()]);
        backend.set_config(cfg);

        let names = |files: &[PathBuf]| -> Vec<String> {
            files
                .iter()
                .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().into_owned())
                .collect()
        };

        let scanned = names(&discover_user_files(&backend, root, &[], false));
        assert!(scanned.contains(&"index.php".to_string()), "{scanned:?}");
        assert!(
            scanned.contains(&"hooks.module".to_string()),
            "a configured extension is analysed like .php: {scanned:?}"
        );
        assert!(
            !scanned.iter().any(|n| n.starts_with("generated")),
            "an excluded directory must be pruned: {scanned:?}"
        );

        // Naming the excluded directory does not re-enable it...
        let targeted = names(&discover_user_files(
            &backend,
            root,
            &[root.join("generated")],
            false,
        ));
        assert!(
            targeted.is_empty(),
            "an exclude holds for a directory named on the command line: {targeted:?}"
        );

        // ...but naming the file itself does.
        let by_file = names(&discover_user_files(
            &backend,
            root,
            &[root.join("generated/Stub.php")],
            false,
        ));
        assert_eq!(by_file, vec!["generated/Stub.php".to_string()]);
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

        let files = discover_user_files(&backend, &real_root, &[], false);
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
        let files = discover_user_files(&backend, root, &[PathBuf::from("includes/target.php")], false);
        assert_eq!(files, vec![root.join("includes/target.php")]);
    }

    /// Several filters are unioned, mixing files and directories, and
    /// everything they do not cover stays out.
    #[test]
    fn discover_user_files_unions_multiple_filters() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("app/Models")).unwrap();
        std::fs::create_dir_all(root.join("lib")).unwrap();
        std::fs::create_dir_all(root.join("tests")).unwrap();
        std::fs::write(root.join("app/Models/User.php"), "<?php\n").unwrap();
        std::fs::write(root.join("lib/Helper.php"), "<?php\n").unwrap();
        std::fs::write(root.join("lib/Other.php"), "<?php\n").unwrap();
        std::fs::write(root.join("tests/UserTest.php"), "<?php\n").unwrap();

        let backend = Backend::new_headless();
        let files = discover_user_files(
            &backend,
            root,
            &[
                PathBuf::from("app"),
                PathBuf::from("lib/Helper.php"),
                PathBuf::from("tests"),
            ],
            false,
        );

        assert_eq!(
            files,
            vec![
                root.join("app/Models/User.php"),
                root.join("lib/Helper.php"),
                root.join("tests/UserTest.php"),
            ]
        );
    }

    /// The same file named twice, and a file that also sits inside a
    /// named directory, are each reported once.
    #[test]
    fn discover_user_files_dedupes_overlapping_filters() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/A.php"), "<?php\n").unwrap();
        std::fs::write(root.join("src/B.php"), "<?php\n").unwrap();

        let backend = Backend::new_headless();
        let files = discover_user_files(
            &backend,
            root,
            &[
                PathBuf::from("src"),
                PathBuf::from("src/A.php"),
                PathBuf::from("src/A.php"),
            ],
            false,
        );

        assert_eq!(files, vec![root.join("src/A.php"), root.join("src/B.php")]);
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
        let files = discover_user_files(&backend, &root, &[], true);
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
        let files = discover_user_files(&backend, &root, &[], false);
        assert!(
            !files.iter().any(|p| p.ends_with("Hidden.php")),
            "interior symlink must not be followed by default: {files:?}"
        );
    }
}
