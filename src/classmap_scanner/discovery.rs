//! Project, workspace, include-path and Drupal discovery, and the
//! parallel batch scanning built on top of the [`super::lexer`] fast path.
//!
//! This file turns the byte-lexer's single-file scans into
//! workspace-wide classmaps: it lists the files each scenario covers
//! (through [`super::walk`] where the gitignore rules apply) and fans the
//! reads out over the shared worker pool.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::filters::{FollowedLinks, IndexFilters};
use super::walk::{WalkOptions, walk_roots};
use super::{ScanResult, WorkspaceScanResult, read_for_scan, scan_content};
use crate::progress::ScanProgress;

/// Add discovered work units to the progress total, if reporting.
pub(super) fn progress_add_total(progress: Option<&ScanProgress>, n: usize) {
    if let Some(p) = progress {
        p.add_total(n as u64);
    }
}

/// Record one completed work unit, if reporting.
fn progress_add_done(progress: Option<&ScanProgress>) {
    if let Some(p) = progress {
        p.add_done(1);
    }
}

/// Files claimed per work-stealing block by the batch scanners.  Small
/// enough that a block of large files cannot hold up the tail of a scan,
/// large enough that the shared cursor is not the bottleneck.
const SCAN_BLOCK_FILES: usize = 32;

/// Build a classmap by scanning all `.php` files under the given
/// directories.
///
/// Each directory is walked recursively using the `ignore` crate for
/// gitignore-aware traversal.  Hidden directories (`.git`, `.idea`,
/// etc.) are skipped automatically.  Directories in `.gitignore` are
/// also skipped.  Any directory whose absolute path is in
/// `vendor_dir_paths` is explicitly skipped regardless of `.gitignore`.
///
/// File scanning is parallelised across CPU cores: the directory walk
/// collects file paths first, then files are read and scanned in
/// parallel batches on the shared worker pool ([`crate::parallel`]).
///
/// Returns a `HashMap<String, PathBuf>` mapping fully-qualified class
/// names to the absolute file path where they are defined.  When a
/// class name appears in multiple files, the first occurrence wins.
pub fn scan_directories(
    dirs: &[PathBuf],
    vendor_dir_paths: &[PathBuf],
    followed: Option<&FollowedLinks>,
) -> HashMap<String, PathBuf> {
    let skip_paths = HashSet::new();
    let opts = WalkOptions::new(
        vendor_dir_paths.to_vec(),
        &skip_paths,
        IndexFilters::empty(),
        followed,
    );
    let paths: Vec<PathBuf> = walk_roots(dirs, &opts).into_iter().flatten().collect();
    scan_files_parallel_classes(&paths, None)
}

/// Build a classmap by scanning all `.php` files under the given
/// directories, applying PSR-4 compliance filtering.
///
/// For each `(namespace_prefix, base_path)` pair the scanner walks
/// `base_path` recursively using the `ignore` crate for
/// gitignore-aware traversal, and only includes classes whose FQN
/// matches the PSR-4 mapping: the namespace prefix plus the relative
/// file path must equal the class name.
///
/// Entries from `classmap_dirs` are scanned without PSR-4 filtering
/// (equivalent to Composer's `autoload.classmap` entries).
///
/// File scanning is parallelised across CPU cores.
///
/// `vendor_dir_paths` contains absolute paths of all known vendor
/// directories.  Any directory whose absolute path matches one of
/// these is skipped.
pub fn scan_psr4_directories(
    psr4: &[(String, PathBuf)],
    classmap_dirs: &[PathBuf],
    vendor_dir_paths: &[PathBuf],
    followed: Option<&FollowedLinks>,
) -> HashMap<String, PathBuf> {
    scan_psr4_directories_with_skip(
        psr4,
        classmap_dirs,
        vendor_dir_paths,
        &HashSet::new(),
        &IndexFilters::empty(),
        None,
        followed,
    )
}

/// Like [`scan_psr4_directories`] but accepts a set of absolute file
/// paths to skip.  Files whose canonical path appears in `skip_paths`
/// are excluded from scanning.  This is used by the merged
/// classmap + self-scan pipeline to avoid re-scanning files that
/// the Composer classmap already covers.
pub fn scan_psr4_directories_with_skip(
    psr4: &[(String, PathBuf)],
    classmap_dirs: &[PathBuf],
    vendor_dir_paths: &[PathBuf],
    skip_paths: &HashSet<PathBuf>,
    filters: &std::sync::Arc<IndexFilters>,
    progress: Option<&ScanProgress>,
    followed: Option<&FollowedLinks>,
) -> HashMap<String, PathBuf> {
    // ── Walk the PSR-4 and classmap roots in one parallel pass ──────
    let opts = WalkOptions::new(
        vendor_dir_paths.to_vec(),
        skip_paths,
        std::sync::Arc::clone(filters),
        followed,
    );
    let mut roots: Vec<PathBuf> = psr4.iter().map(|(_, dir)| dir.clone()).collect();
    roots.extend(classmap_dirs.iter().cloned());
    let mut walked = walk_roots(&roots, &opts);
    let plain_paths: Vec<PathBuf> = walked.split_off(psr4.len()).into_iter().flatten().collect();

    // Each PSR-4 file is paired with the class name its mapping expects
    // it to declare, so the scan below can reject non-compliant classes.
    let psr4_pairs: Vec<(PathBuf, String)> = psr4
        .iter()
        .zip(walked)
        .flat_map(|((prefix, base_path), files)| {
            files.into_iter().filter_map(move |path| {
                let fqn = psr4_expected_fqn(base_path, prefix, &path)?;
                Some((path, fqn))
            })
        })
        .collect();

    // ── Scan all files in parallel ──────────────────────────────────
    progress_add_total(progress, psr4_pairs.len() + plain_paths.len());
    let mut classmap = scan_files_parallel_psr4(&psr4_pairs, progress);
    let plain_classmap = scan_files_parallel_classes(&plain_paths, progress);
    for (fqcn, path) in plain_classmap {
        classmap.entry(fqcn).or_insert(path);
    }

    classmap
}

/// Scan all `.php` files under the workspace root using the PSR-4
/// scanner (`find_classes`), excluding hidden directories, gitignored
/// directories, and vendor directories.
///
/// This is a classes-only fallback used when `composer.json` cannot be
/// parsed.  Prefer [`scan_workspace_fallback_full`] for the no-Composer
/// scenario so that functions and constants are also discovered.
///
/// `vendor_dir_paths` contains absolute paths of all known vendor
/// directories.  Pass a single-element slice with the vendor directory
/// for single-project workspaces.
pub fn scan_workspace_fallback(
    workspace_root: &Path,
    vendor_dir_paths: &[PathBuf],
    followed: Option<&FollowedLinks>,
) -> HashMap<String, PathBuf> {
    scan_directories(&[workspace_root.to_path_buf()], vendor_dir_paths, followed)
}

/// Scan `files` on the shared worker pool, calling `emit` on each to
/// append the `(fqcn, path)` pairs it declares, and merge the result into
/// one classmap where the first file to declare a name wins.
///
/// `name` names the workers, and the log line a panicking one leaves.
fn scan_files_parallel<T: Sync>(
    files: &[T],
    progress: Option<&ScanProgress>,
    name: &'static str,
    emit: impl Fn(&T, &mut Vec<(String, PathBuf)>) + Sync,
) -> HashMap<String, PathBuf> {
    let found = crate::parallel::map_in_blocks(name, files, SCAN_BLOCK_FILES, |file| {
        progress_add_done(progress);
        let mut declared = Vec::new();
        emit(file, &mut declared);
        (!declared.is_empty()).then_some(declared)
    });

    let mut classmap = HashMap::with_capacity(found.iter().map(Vec::len).sum());
    for (fqcn, path) in found.into_iter().flatten() {
        classmap.entry(fqcn).or_insert(path);
    }
    classmap
}

/// Scan a batch of files for the class names they declare.
fn scan_files_parallel_classes(
    files: &[PathBuf],
    progress: Option<&ScanProgress>,
) -> HashMap<String, PathBuf> {
    scan_files_parallel(
        files,
        progress,
        "scan_files_parallel_classes",
        |path, out| {
            if let Ok(content) = read_for_scan(path) {
                for fqcn in scan_content(&content) {
                    out.push((fqcn, path.clone()));
                }
            }
        },
    )
}

/// Scan a batch of files for class names with PSR-4 filtering.
///
/// Each entry is `(file_path, expected_fqn)`.  Only classes whose FQN
/// matches the expected FQN are included.
fn scan_files_parallel_psr4(
    files: &[(PathBuf, String)],
    progress: Option<&ScanProgress>,
) -> HashMap<String, PathBuf> {
    scan_files_parallel(
        files,
        progress,
        "scan_files_parallel_psr4",
        |(path, expected_fqn), out| {
            if let Ok(content) = read_for_scan(path) {
                for fqcn in scan_content(&content) {
                    if &fqcn == expected_fqn {
                        out.push((fqcn, path.clone()));
                    }
                }
            }
        },
    )
}

/// Merge one scanned file's symbols into `result`.
///
/// The first file to declare a name wins, except that a later file whose
/// stem matches the class's short name displaces one whose stem does not:
/// a package with conditional loading declares the same trait in
/// `ArraySubsetAsserts.php` and `ArraySubsetAssertsEmpty.php`, and PSR-4
/// says the first of those is the real one. The origin tier follows
/// whichever file the classmap ends up pointing at.
fn merge_scanned_file(
    result: &mut WorkspaceScanResult,
    scan: ScanResult,
    path: &Path,
    origin: crate::ClassCompletionOrigin,
) {
    for fqcn in scan.classes {
        let class_short_name = crate::util::short_name(&fqcn).to_owned();
        let mut origin_wins = false;
        result
            .classmap
            .entry(fqcn.clone())
            .and_modify(|existing| {
                let existing_stem = existing.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                let new_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                if existing_stem != class_short_name && new_stem == class_short_name {
                    *existing = path.to_path_buf();
                    origin_wins = true;
                }
            })
            .or_insert_with(|| {
                origin_wins = true;
                path.to_path_buf()
            });
        if origin_wins {
            result.class_origins.insert(fqcn, origin);
        }
    }
    for fqn in scan.functions {
        result
            .function_index
            .entry(fqn.clone())
            .or_insert_with(|| path.to_path_buf());
        result.function_origins.entry(fqn).or_insert(origin);
    }
    for name in scan.constants {
        result
            .constant_index
            .entry(name.clone())
            .or_insert_with(|| path.to_path_buf());
        result.constant_origins.entry(name).or_insert(origin);
    }
}

/// Scan a batch of files for all symbols (classes, functions, constants)
/// on the shared worker pool and return a [`WorkspaceScanResult`].
///
/// Each file carries its own completion-origin tier, which is attached to
/// any function or constant discovered in it. This lets callers that know
/// a file's package provenance up front (e.g. vendor package scanning)
/// classify symbols in the same read/scan pass instead of re-reading every
/// file afterwards just to determine its origin.
///
/// File sizes vary by orders of magnitude across a vendor tree, so the
/// workers claim [`SCAN_BLOCK_FILES`] at a time rather than an even share
/// each, and the results are merged in input order, the same as a
/// sequential scan.
pub(super) fn scan_files_parallel_full(
    files: &[(PathBuf, crate::ClassCompletionOrigin)],
    progress: Option<&ScanProgress>,
) -> WorkspaceScanResult {
    let scanned = crate::parallel::map_in_blocks(
        "scan_files_parallel_full",
        files,
        SCAN_BLOCK_FILES,
        |(path, origin)| {
            progress_add_done(progress);
            let content = read_for_scan(path).ok()?;
            let scan = super::find_symbols(&content);
            (!scan.classes.is_empty() || !scan.functions.is_empty() || !scan.constants.is_empty())
                .then_some((scan, path, *origin))
        },
    );

    let mut result = WorkspaceScanResult::default();
    for (scan, path, origin) in scanned {
        merge_scanned_file(&mut result, scan, path, origin);
    }
    result
}

/// Scan all `.php` files under the workspace root using the full-scan
/// (`find_symbols`) and return classes, functions, and constants in a
/// single pass.
///
/// This is the primary scanner for the "no `composer.json`" scenario.
/// It populates all three indices (classmap, function index, constant
/// index) so that non-Composer projects get cross-file resolution for
/// every symbol type.  Lazy `update_ast` on first access provides the
/// complete `FunctionInfo` / `DefineInfo` needed by hover, completion,
/// and go-to-definition.
///
/// Uses the `ignore` crate for gitignore-aware walking.  Hidden
/// directories (starting with `.`) are skipped automatically.
/// Directories whose absolute path is in `skip_dirs` are also skipped
/// (used by monorepo support to avoid double-scanning subproject
/// directories that were already processed by the Composer pipeline).
pub fn scan_workspace_fallback_full(
    workspace_root: &Path,
    skip_dirs: &HashSet<PathBuf>,
    filters: &std::sync::Arc<IndexFilters>,
    progress: Option<&ScanProgress>,
    followed: Option<&FollowedLinks>,
) -> WorkspaceScanResult {
    // Phase 1: collect file paths
    let skip_paths = HashSet::new();
    let opts = WalkOptions::new(
        skip_dirs.iter().cloned().collect(),
        &skip_paths,
        std::sync::Arc::clone(filters),
        followed,
    );
    let php_files: Vec<(PathBuf, crate::ClassCompletionOrigin)> =
        walk_roots(&[workspace_root.to_path_buf()], &opts)
            .into_iter()
            .flatten()
            .map(|path| (path, crate::ClassCompletionOrigin::Project))
            .collect();

    // Phase 2: scan files in parallel
    progress_add_total(progress, php_files.len());
    scan_files_parallel_full(&php_files, progress)
}

/// List the PHP files the `[indexing] include` paths name, bypassing
/// the gitignore and hidden-entry filters the workspace walk applies.
///
/// A named file is listed whatever its extension, since naming it is
/// the instruction. A named directory is walked recursively for files
/// `filters` accepts as PHP, and `[indexing] exclude` still prunes
/// entries below it (so a dependency tree inside an included directory
/// can be left out) but never the named path itself. `.git` is always
/// skipped.
pub(crate) fn collect_include_files(
    paths: &[PathBuf],
    filters: &std::sync::Arc<IndexFilters>,
) -> Vec<PathBuf> {
    use ignore::WalkBuilder;

    let mut files = Vec::new();
    for path in paths {
        if path.is_file() {
            files.push(path.clone());
            continue;
        }

        let filter_excludes = std::sync::Arc::clone(filters);
        let walker = WalkBuilder::new(path)
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .ignore(false)
            .parents(false)
            .hidden(false)
            .filter_entry(move |entry| {
                if entry.depth() == 0 {
                    return true;
                }
                let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
                if is_dir && entry.file_name() == ".git" {
                    return false;
                }
                !filter_excludes.is_excluded_entry(entry.path(), is_dir)
            })
            .build();
        for entry in walker.flatten() {
            if filters.is_php_file(entry.path()) && entry.path().is_file() {
                files.push(entry.into_path());
            }
        }
    }
    files
}

/// Scan the files [`collect_include_files`] lists for symbols, tagging
/// each with `origin`.
///
/// A path the normal walk already covered is scanned twice and loses
/// the first-wins merge, so callers need not deduplicate.
pub fn scan_include_paths(
    paths: &[PathBuf],
    origin: crate::ClassCompletionOrigin,
    filters: &std::sync::Arc<IndexFilters>,
    progress: Option<&ScanProgress>,
) -> WorkspaceScanResult {
    let php_files: Vec<(PathBuf, crate::ClassCompletionOrigin)> =
        collect_include_files(paths, filters)
            .into_iter()
            .map(|path| (path, origin))
            .collect();
    progress_add_total(progress, php_files.len());
    scan_files_parallel_full(&php_files, progress)
}

/// Scan Drupal-specific directories for PHP symbols, bypassing `.gitignore`.
///
/// Drupal projects typically exclude their web root directories
/// (`web/core`, `web/modules/contrib`, etc.) from version control via
/// `.gitignore` because those files are managed by Composer.  The normal
/// gitignore-aware walkers would therefore silently skip the most important
/// parts of the codebase.  This function walks with gitignore **disabled**
/// so that those directories are always indexed.
///
/// In addition to `.php` files, Drupal uses several other file extensions
/// for valid PHP source: `.module`, `.install`, `.theme`, `.profile`,
/// `.inc`, and `.engine`.  All are included by this scanner.
///
/// Test directories are indexed too: module tests extend base classes
/// that live in `core/tests/` (`Drupal\Tests\UnitTestCase`,
/// `Drupal\KernelTests\KernelTestBase`, …) and reference test modules
/// under `*/tests/modules/`, so skipping them by name leaves those
/// classes unresolvable. Projects that want a smaller index can trim it
/// with `[indexing] exclude`, which is honored here like everywhere
/// else; duplicate fixture classes are handled by the classmap's
/// first-wins merge.
pub fn scan_drupal_directories(
    web_root: &Path,
    filters: &std::sync::Arc<IndexFilters>,
    progress: Option<&ScanProgress>,
) -> WorkspaceScanResult {
    use ignore::WalkBuilder;

    let drupal_dirs = [
        "core",
        "modules/contrib",
        "modules/custom",
        "themes/contrib",
        "themes/custom",
        "profiles",
        "sites",
    ];

    let mut php_files: Vec<(PathBuf, crate::ClassCompletionOrigin)> = Vec::new();

    for rel in &drupal_dirs {
        let dir = web_root.join(rel);
        if !dir.exists() {
            continue;
        }

        let filter_excludes = std::sync::Arc::clone(filters);
        let walker = WalkBuilder::new(&dir)
            // Gitignore is intentionally disabled — Drupal's .gitignore
            // excludes web/core and web/modules/contrib which are the
            // most critical directories to index.
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .hidden(true) // still skip .git, .idea, etc.
            .parents(true)
            .ignore(false)
            .filter_entry(move |entry| {
                let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
                !filter_excludes.is_excluded_entry(entry.path(), is_dir)
            })
            .build();

        for entry in walker.flatten() {
            let path = entry.path();
            if path.is_file() && (is_drupal_php_file(path) || filters.is_php_file(path)) {
                php_files.push((path.to_path_buf(), crate::ClassCompletionOrigin::Project));
            }
        }
    }

    progress_add_total(progress, php_files.len());
    scan_files_parallel_full(&php_files, progress)
}

/// Return `true` for file extensions that Drupal treats as PHP source.
fn is_drupal_php_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("php" | "module" | "install" | "theme" | "profile" | "inc" | "engine")
    )
}

/// Derive the class name a PSR-4 mapping expects a file to declare, from
/// its path relative to the mapping's base directory.
fn psr4_expected_fqn(base_path: &Path, namespace_prefix: &str, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(base_path).ok()?;
    let relative_str = relative.to_string_lossy();
    let stem = relative_str.strip_suffix(".php")?;
    Some(format!("{}{}", namespace_prefix, stem.replace('/', "\\")))
}

#[cfg(test)]
#[path = "discovery_tests.rs"]
mod tests;
