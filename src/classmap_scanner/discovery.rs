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
    scan_roots_full(
        &[workspace_root.to_path_buf()],
        &[],
        skip_dirs,
        filters,
        progress,
        followed,
    )
}

/// Full-scan `project_roots` and `library_roots` in one gitignore-aware
/// walk, tagging what each library root declares as library code.
///
/// The library roots are the editor's other workspace folders. They go
/// into the same [`walk_roots`] call as the project's own root rather
/// than a walk of their own, so the shared ancestor `.gitignore` files are
/// compiled once, and every root is claimed up front: a symlink in one
/// root pointing into another loses to that root's own walk instead of
/// indexing the tree a second time under the link's spelling. Each is
/// walked with every filter in force, the way the project's root is.
///
/// Without a project root (a Composer pipeline whose classmap comes from
/// its autoload mappings), list the workspace root in `skip_dirs` so a
/// link pointing back into it is still left to the pipeline that covers
/// it.
pub(crate) fn scan_roots_full(
    project_roots: &[PathBuf],
    library_roots: &[PathBuf],
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
    let roots: Vec<PathBuf> = project_roots.iter().chain(library_roots).cloned().collect();
    let php_files: Vec<(PathBuf, crate::ClassCompletionOrigin)> = walk_roots(&roots, &opts)
        .into_iter()
        .enumerate()
        .flat_map(|(index, files)| {
            let origin = if index < project_roots.len() {
                crate::ClassCompletionOrigin::Project
            } else {
                crate::ClassCompletionOrigin::VendorExplicit
            };
            files.into_iter().map(move |path| (path, origin))
        })
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

/// Scan the stub paths for symbols, in priority order.
///
/// A name two paths declare is kept from the earlier one, and within one
/// path from the first file in path order, so the result does not depend
/// on how the walk or the scan workers were scheduled. Files are listed
/// the way [`collect_include_files`] lists them. A directory holding
/// phpstorm-stubs' `PhpStormStubsMap.php` is read from the map instead of
/// being scanned.
///
/// A declaration its docblock marks `@removed` at or before `php_version`
/// is left out, as it is from the embedded stubs, so a later path's
/// declaration of the name is the one kept.
pub(crate) fn scan_stub_paths(
    paths: &[PathBuf],
    filters: &std::sync::Arc<IndexFilters>,
    php_version: crate::types::PhpVersion,
) -> WorkspaceScanResult {
    let origin = crate::ClassCompletionOrigin::VendorExplicit;
    let mut result = WorkspaceScanResult::default();
    for path in paths {
        let mut scan = match read_phpstorm_stubs_map(path, origin) {
            Some(scan) => scan,
            None => {
                let mut files = collect_include_files(std::slice::from_ref(path), filters);
                files.sort();
                let files: Vec<(PathBuf, crate::ClassCompletionOrigin)> =
                    files.into_iter().map(|file| (file, origin)).collect();
                scan_files_parallel_full(&files, None)
            }
        };
        drop_removed_declarations(&mut scan, php_version);
        result.absorb_missing(scan);
    }
    result
}

/// Remove from `scan` every declaration whose file marks it `@removed` at
/// or before `php_version`.
fn drop_removed_declarations(
    scan: &mut WorkspaceScanResult,
    php_version: crate::types::PhpVersion,
) {
    use crate::stubs::StubSymbolKind;

    let mut by_file: HashMap<&Path, Vec<(StubSymbolKind, &str)>> = HashMap::new();
    let indexes = [
        (StubSymbolKind::Class, &scan.classmap),
        (StubSymbolKind::Function, &scan.function_index),
        (StubSymbolKind::Constant, &scan.constant_index),
    ];
    for (kind, index) in indexes {
        for (name, path) in index {
            by_file
                .entry(path.as_path())
                .or_default()
                .push((kind, name.as_str()));
        }
    }
    let files: Vec<_> = by_file.into_iter().collect();
    let removed: Vec<(StubSymbolKind, String)> = crate::parallel::map_in_blocks(
        "drop_removed_declarations",
        &files,
        SCAN_BLOCK_FILES,
        |(path, names)| {
            let content = crate::util::read_php_source(path)?;
            let removed = crate::stubs::removed_stub_names(&content, php_version)?;
            Some(
                names
                    .iter()
                    .filter(|&&(kind, name)| removed.contains(kind, name))
                    .map(|&(kind, name)| (kind, name.to_owned()))
                    .collect::<Vec<_>>(),
            )
        },
    )
    .into_iter()
    .flatten()
    .collect();

    for (kind, name) in removed {
        let (index, origins) = match kind {
            StubSymbolKind::Class => (&mut scan.classmap, &mut scan.class_origins),
            StubSymbolKind::Function => (&mut scan.function_index, &mut scan.function_origins),
            StubSymbolKind::Constant => (&mut scan.constant_index, &mut scan.constant_origins),
        };
        index.remove(&name);
        origins.remove(&name);
    }
}

/// Whether `dir` is a phpstorm-stubs checkout, which the stub-path scan
/// reads from its `PhpStormStubsMap.php`.
pub(crate) fn is_phpstorm_stubs_checkout(dir: &Path) -> bool {
    dir.join(PHPSTORM_STUBS_MAP).is_file()
}

const PHPSTORM_STUBS_MAP: &str = "PhpStormStubsMap.php";

/// The symbols of the phpstorm-stubs checkout at `dir`, read from its
/// `PhpStormStubsMap.php`, or `None` when `dir` holds no such map.
fn read_phpstorm_stubs_map(
    dir: &Path,
    origin: crate::ClassCompletionOrigin,
) -> Option<WorkspaceScanResult> {
    let content = std::fs::read_to_string(dir.join(PHPSTORM_STUBS_MAP)).ok()?;
    let section = |name: &str| -> (
        HashMap<String, PathBuf>,
        HashMap<String, crate::ClassCompletionOrigin>,
    ) {
        crate::stub_map::parse_section(&content, name)
            .into_iter()
            .map(|(symbol, file)| ((symbol.clone(), dir.join(file)), (symbol, origin)))
            .unzip()
    };
    let (classmap, class_origins) = section("CLASSES");
    let (function_index, function_origins) = section("FUNCTIONS");
    let (constant_index, constant_origins) = section("CONSTANTS");
    Some(WorkspaceScanResult {
        classmap,
        class_origins,
        function_index,
        function_origins,
        constant_index,
        constant_origins,
        package_roots: Vec::new(),
    })
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
