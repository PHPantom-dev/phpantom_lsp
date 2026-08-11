//! Directory walking, PSR-4/vendor package discovery, and parallel
//! batch scanning built on top of the [`super::lexer`] fast path.
//!
//! This file turns the byte-lexer's single-file scans into
//! workspace-wide classmaps: walking directories (gitignore-aware or
//! not, depending on the scenario), reading Composer's
//! `installed.json` to locate vendor packages, and fanning file reads
//! out across CPU cores.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use memchr::memmem;

use super::{ScanResult, WorkspaceScanResult, read_for_scan, scan_content};
use crate::progress::ScanProgress;

/// Add discovered work units to the progress total, if reporting.
fn progress_add_total(progress: Option<&ScanProgress>, n: usize) {
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

/// One file's scan result together with the path and origin it came from.
type ScannedFile = (ScanResult, PathBuf, crate::ClassCompletionOrigin);

/// A block of scanned files, tagged with the block index it was claimed
/// under so results can be merged back in input order.
type ScannedBlock = (usize, Vec<ScannedFile>);

/// Files claimed per work-stealing block by the batch scanners.  Small
/// enough that a block of large files cannot hold up the tail of a scan,
/// large enough that the shared cursor is not the bottleneck.
const SCAN_BLOCK_FILES: usize = 32;

/// Return the number of available CPU cores, capped at a sensible
/// default.  Used to size parallel scanning batches.
fn thread_count() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

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
/// parallel batches using [`std::thread::scope`].
///
/// Returns a `HashMap<String, PathBuf>` mapping fully-qualified class
/// names to the absolute file path where they are defined.  When a
/// class name appears in multiple files, the first occurrence wins.
pub fn scan_directories(
    dirs: &[PathBuf],
    vendor_dir_paths: &[PathBuf],
) -> HashMap<String, PathBuf> {
    let skip_paths = HashSet::new();
    let opts = WalkOptions::new(vendor_dir_paths.to_vec(), &skip_paths);
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
) -> HashMap<String, PathBuf> {
    scan_psr4_directories_with_skip(psr4, classmap_dirs, vendor_dir_paths, &HashSet::new(), None)
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
    progress: Option<&ScanProgress>,
) -> HashMap<String, PathBuf> {
    // ── Walk the PSR-4 and classmap roots in one parallel pass ──────
    let opts = WalkOptions::new(vendor_dir_paths.to_vec(), skip_paths);
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

/// Build a classmap from `installed.json` vendor package metadata.
///
/// Reads `<vendor_path>/composer/installed.json` and scans each
/// package's autoload directories.  Supports PSR-4 and classmap
/// entries.
pub fn scan_vendor_packages(workspace_root: &Path, vendor_dir: &str) -> WorkspaceScanResult {
    scan_vendor_packages_with_skip(
        workspace_root,
        vendor_dir,
        &HashSet::new(),
        &HashSet::new(),
        None,
    )
}

/// Classify a Composer package name into its completion origin.
///
/// Symfony polyfill packages (`symfony/polyfill-*`) backport PHP core
/// classes and extension functions (e.g. `symfony/polyfill-php83`
/// ships `\Override`), so they are treated as core stubs and sort and
/// display like built-in PHP symbols. Everything else is an explicit
/// dependency when it appears in the root `composer.json`, or a
/// transitive dependency otherwise.
pub(crate) fn classify_package_origin(
    pkg_name: &str,
    explicit_deps: &HashSet<String>,
) -> crate::ClassCompletionOrigin {
    if pkg_name.starts_with("symfony/polyfill-") {
        crate::ClassCompletionOrigin::CoreStub
    } else if explicit_deps.contains(pkg_name) {
        crate::ClassCompletionOrigin::VendorExplicit
    } else {
        crate::ClassCompletionOrigin::VendorTransitive
    }
}

pub(crate) fn vendor_package_roots(
    workspace_root: &Path,
    vendor_dir: &str,
    explicit_deps: &HashSet<String>,
) -> Vec<(PathBuf, crate::ClassCompletionOrigin, String)> {
    let vendor_path = workspace_root.join(vendor_dir);
    let installed_path = vendor_path.join("composer").join("installed.json");
    let Ok(content) = std::fs::read_to_string(&installed_path) else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Vec::new();
    };
    let packages = if let Some(arr) = json.as_array() {
        arr.as_slice()
    } else if let Some(pkgs) = json.get("packages").and_then(|p| p.as_array()) {
        pkgs.as_slice()
    } else {
        return Vec::new();
    };
    let composer_dir = vendor_path.join("composer");
    let mut roots = Vec::new();
    for package in packages {
        let pkg_name = package
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("unknown/unknown");
        let origin = classify_package_origin(pkg_name, explicit_deps);
        let pkg_path =
            if let Some(install_path) = package.get("install-path").and_then(|p| p.as_str()) {
                composer_dir.join(install_path)
            } else {
                vendor_path.join(pkg_name)
            };
        let pkg_path = pkg_path.canonicalize().unwrap_or(pkg_path);
        if pkg_path.is_dir() {
            roots.push((pkg_path, origin, pkg_name.to_string()));
        }
    }
    roots.sort_by_key(|(p, _, _)| std::cmp::Reverse(p.components().count()));
    roots
}

/// One `autoload.files` / `autoload.classmap` entry of a package, before
/// any directory is walked.
enum PlainSource {
    /// A file Composer names directly.
    File(PathBuf),
    /// A directory whose whole tree contributes classes.
    Dir(PathBuf),
}

/// The autoload sources a single Composer package contributes to a vendor
/// scan, as paths only: walking them is deferred to a single
/// [`walk_roots`] call over every package's roots at once.
#[derive(Default)]
struct PackageSources {
    /// The package's `autoload.psr-4` base directories.
    ///
    /// The namespace prefix each maps to is not kept: a vendor scan reads
    /// every file it finds without PSR-4 compliance filtering, so unlike
    /// [`scan_psr4_directories_with_skip`] it has no use for the class
    /// name the mapping expects.
    psr4: Vec<PathBuf>,
    /// `autoload.files` and `autoload.classmap` entries in declaration
    /// order, plus the package's own tree when a `files` entry registers
    /// an autoloader of its own.
    plain: Vec<PlainSource>,
    /// The completion-origin tier every symbol from this package gets.
    origin: crate::ClassCompletionOrigin,
    /// The package's own root (path, origin, package name), resolved once
    /// here so `vendor_package_roots` doesn't need to re-read and
    /// re-parse `installed.json` to get the same information.
    root: Option<(PathBuf, crate::ClassCompletionOrigin, String)>,
}

/// Read one `installed.json` entry and return the autoload sources it
/// exposes, without walking any of them.
///
/// Returns an empty result when the package is not installed, has no
/// `autoload` section, or declares no PHP sources.
fn collect_package_sources(
    package: &serde_json::Value,
    composer_dir: &Path,
    vendor_path: &Path,
    skip_paths: &HashSet<PathBuf>,
    explicit_deps: &HashSet<String>,
) -> PackageSources {
    let pkg_name = package.get("name").and_then(|n| n.as_str());
    let origin = pkg_name
        .map(|name| classify_package_origin(name, explicit_deps))
        .unwrap_or(crate::ClassCompletionOrigin::VendorTransitive);
    let mut out = PackageSources {
        origin,
        ..Default::default()
    };
    // Locate the package on disk.  Composer 2's installed.json
    // includes an `install-path` field that is relative to the
    // `vendor/composer/` directory.  This is the authoritative
    // location and handles path repositories, custom installers,
    // and any other layout that doesn't follow the default
    // `vendor/<name>/` convention.  Fall back to `vendor/<name>`
    // only when `install-path` is absent (Composer 1 format).
    let pkg_path = if let Some(install_path) = package.get("install-path").and_then(|p| p.as_str())
    {
        composer_dir.join(install_path)
    } else if let Some(pkg_name) = pkg_name {
        vendor_path.join(pkg_name)
    } else {
        return out;
    };

    let pkg_path = match pkg_path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            // Directory doesn't exist (package not installed yet).
            if !pkg_path.is_dir() {
                return out;
            }
            pkg_path
        }
    };

    if !pkg_path.is_dir() {
        return out;
    }

    out.root = Some((
        pkg_path.clone(),
        origin,
        pkg_name.unwrap_or("unknown/unknown").to_string(),
    ));

    let Some(autoload) = package.get("autoload") else {
        return out;
    };

    if let Some(psr4) = autoload.get("psr-4").and_then(|p| p.as_object()) {
        for paths in psr4.values() {
            for dir_str in value_to_strings(paths) {
                let dir = pkg_path.join(&dir_str);
                if dir.is_dir() {
                    out.psr4.push(dir);
                }
            }
        }
    }

    // Files entries (individual PHP files that are always loaded)
    if let Some(files) = autoload.get("files").and_then(|f| f.as_array()) {
        let mut has_custom_autoloader = false;
        for entry in files {
            if let Some(file_str) = entry.as_str() {
                let file = pkg_path.join(file_str);
                if file.is_file()
                    && file.extension().is_some_and(|ext| ext == "php")
                    && !skip_paths.contains(&file)
                {
                    // Check if this file registers a custom autoloader.
                    if !has_custom_autoloader
                        && let Ok(content) = read_for_scan(&file)
                        && memmem::find(&content, b"spl_autoload_register").is_some()
                    {
                        has_custom_autoloader = true;
                    }
                    out.plain.push(PlainSource::File(file));
                }
            }
        }

        // When a files entry registers a custom autoloader via
        // spl_autoload_register, it will load classes from the
        // package at runtime. Since we can't execute that logic,
        // do a full scan of the package directory to discover all
        // classes it provides.
        if has_custom_autoloader {
            out.plain.push(PlainSource::Dir(pkg_path.clone()));
        }
    }

    if let Some(cm) = autoload.get("classmap").and_then(|c| c.as_array()) {
        for entry in cm {
            if let Some(dir_str) = entry.as_str() {
                let dir = pkg_path.join(dir_str);
                if dir.is_dir() {
                    out.plain.push(PlainSource::Dir(dir));
                } else if dir.is_file()
                    && dir.extension().is_some_and(|ext| ext == "php")
                    && !skip_paths.contains(&dir)
                {
                    out.plain.push(PlainSource::File(dir));
                }
            }
        }
    }

    out
}

/// Like [`scan_vendor_packages`] but accepts a set of absolute file
/// paths to skip.  Files whose path appears in `skip_paths` are
/// excluded from scanning.
pub fn scan_vendor_packages_with_skip(
    workspace_root: &Path,
    vendor_dir: &str,
    skip_paths: &HashSet<PathBuf>,
    explicit_deps: &HashSet<String>,
    progress: Option<&ScanProgress>,
) -> WorkspaceScanResult {
    let vendor_path = workspace_root.join(vendor_dir);
    let installed_path = vendor_path.join("composer").join("installed.json");

    let Ok(content) = std::fs::read_to_string(&installed_path) else {
        return WorkspaceScanResult::default();
    };

    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
        return WorkspaceScanResult::default();
    };

    // installed.json has two formats:
    //   Composer 1: top-level array of packages
    //   Composer 2: { "packages": [...] }
    let packages = if let Some(arr) = json.as_array() {
        arr.as_slice()
    } else if let Some(pkgs) = json.get("packages").and_then(|p| p.as_array()) {
        pkgs.as_slice()
    } else {
        return WorkspaceScanResult::default();
    };

    // The directory containing installed.json — install-path values
    // are relative to this directory.
    let composer_dir = vendor_path.join("composer");

    // Phase 1: read every package's autoload section and resolve the
    // paths it declares, without walking any of them.  Packages are
    // independent so this fans out over all cores; the per-package
    // results are put back in `installed.json` order below so a duplicate
    // FQN resolves to the same file a sequential scan would have picked.
    let mut collected: Vec<(usize, PackageSources)> = {
        let next_pkg = std::sync::atomic::AtomicUsize::new(0);
        let n_threads = thread_count().min(packages.len().max(1));
        std::thread::scope(|s| {
            let handles: Vec<_> = (0..n_threads)
                .map(|_| {
                    let next_pkg = &next_pkg;
                    let composer_dir = &composer_dir;
                    let vendor_path = &vendor_path;
                    s.spawn(move || {
                        let mut out: Vec<(usize, PackageSources)> = Vec::new();
                        loop {
                            let i = next_pkg.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some(package) = packages.get(i) else {
                                break;
                            };
                            let sources = collect_package_sources(
                                package,
                                composer_dir,
                                vendor_path,
                                skip_paths,
                                explicit_deps,
                            );
                            if !sources.psr4.is_empty()
                                || !sources.plain.is_empty()
                                || sources.root.is_some()
                            {
                                out.push((i, sources));
                            }
                        }
                        out
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| {
                    h.join().unwrap_or_else(|_| {
                        tracing::error!("PHPantom: thread panic in vendor package collection");
                        Vec::new()
                    })
                })
                .collect()
        })
    };
    collected.sort_unstable_by_key(|(i, _)| *i);

    let mut package_roots: Vec<(PathBuf, crate::ClassCompletionOrigin, String)> = collected
        .iter_mut()
        .filter_map(|(_, sources)| sources.root.take())
        .collect();
    // Match `vendor_package_roots`'s ordering: longest path first, so a
    // nested package's root wins a prefix match before its parent's.
    package_roots.sort_by_key(|(p, _, _)| std::cmp::Reverse(p.components().count()));

    // Phase 2: walk every collected directory in one parallel pass, so
    // the cores are shared across all packages instead of one thread per
    // package.  The roots are laid out PSR-4 first and classmap/`files`
    // second, matching the order phase 3 concatenates them in.
    let opts = WalkOptions::new(vec![vendor_path.clone()], skip_paths);
    let mut roots: Vec<PathBuf> = Vec::new();
    for (_, sources) in &collected {
        roots.extend(sources.psr4.iter().cloned());
    }
    for (_, sources) in &collected {
        for source in &sources.plain {
            if let PlainSource::Dir(dir) = source {
                roots.push(dir.clone());
            }
        }
    }
    let mut walked = walk_roots(&roots, &opts);

    // Phase 3: concatenate in `installed.json` order.  Each file's origin
    // is already known from the package it was declared by, so it travels
    // alongside the path and gets attached to the functions and constants
    // discovered in the same read, instead of re-reading every file in a
    // second pass just to classify it.
    let mut all_files: Vec<(PathBuf, crate::ClassCompletionOrigin)> =
        Vec::with_capacity(walked.iter().map(Vec::len).sum());
    let mut next_root = 0;
    for (_, sources) in &collected {
        for _ in &sources.psr4 {
            let files = std::mem::take(&mut walked[next_root]);
            all_files.extend(files.into_iter().map(|path| (path, sources.origin)));
            next_root += 1;
        }
    }
    for (_, sources) in &collected {
        for source in &sources.plain {
            match source {
                PlainSource::File(file) => all_files.push((file.clone(), sources.origin)),
                PlainSource::Dir(_) => {
                    let files = std::mem::take(&mut walked[next_root]);
                    all_files.extend(files.into_iter().map(|path| (path, sources.origin)));
                    next_root += 1;
                }
            }
        }
    }

    progress_add_total(progress, all_files.len());

    let mut result = scan_files_parallel_full(&all_files, progress);
    result.package_roots = package_roots;
    result
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
) -> HashMap<String, PathBuf> {
    scan_directories(&[workspace_root.to_path_buf()], vendor_dir_paths)
}

/// Scan a batch of files for class names in parallel and return a classmap.
///
/// Uses [`std::thread::scope`] with one thread per CPU core.  Small
/// batches (≤ 4 files) are processed sequentially to avoid thread
/// overhead.
fn scan_files_parallel_classes(
    files: &[PathBuf],
    progress: Option<&ScanProgress>,
) -> HashMap<String, PathBuf> {
    if files.is_empty() {
        return HashMap::new();
    }

    // Small batches: sequential
    if files.len() <= 4 {
        let mut classmap = HashMap::new();
        for path in files {
            progress_add_done(progress);
            if let Ok(content) = read_for_scan(path) {
                for fqcn in scan_content(&content) {
                    classmap.entry(fqcn).or_insert_with(|| path.clone());
                }
            }
        }
        return classmap;
    }

    let n_threads = thread_count().min(files.len());
    let chunk_size = files.len().div_ceil(n_threads);

    let results: Vec<Vec<(String, PathBuf)>> = std::thread::scope(|s| {
        let handles: Vec<_> = files
            .chunks(chunk_size)
            .map(|chunk| {
                s.spawn(move || {
                    let mut local: Vec<(String, PathBuf)> = Vec::new();
                    for path in chunk {
                        progress_add_done(progress);
                        if let Ok(content) = read_for_scan(path) {
                            for fqcn in scan_content(&content) {
                                local.push((fqcn, path.clone()));
                            }
                        }
                    }
                    local
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join().unwrap_or_else(|_| {
                    tracing::error!("PHPantom: thread panic in scan_files_parallel_classes");
                    Vec::new()
                })
            })
            .collect()
    });

    let total: usize = results.iter().map(|v| v.len()).sum();
    let mut classmap = HashMap::with_capacity(total);
    for batch in results {
        for (fqcn, path) in batch {
            classmap.entry(fqcn).or_insert(path);
        }
    }
    classmap
}

/// Scan a batch of files for class names with PSR-4 filtering in
/// parallel.
///
/// Each entry is `(file_path, expected_fqn)`.  Only classes whose FQN
/// matches the expected FQN are included.
fn scan_files_parallel_psr4(
    files: &[(PathBuf, String)],
    progress: Option<&ScanProgress>,
) -> HashMap<String, PathBuf> {
    if files.is_empty() {
        return HashMap::new();
    }

    // Small batches: sequential
    if files.len() <= 4 {
        let mut classmap = HashMap::new();
        for (path, expected_fqn) in files {
            progress_add_done(progress);
            if let Ok(content) = read_for_scan(path) {
                for fqcn in scan_content(&content) {
                    if &fqcn == expected_fqn {
                        classmap.entry(fqcn).or_insert_with(|| path.clone());
                    }
                }
            }
        }
        return classmap;
    }

    let n_threads = thread_count().min(files.len());
    let chunk_size = files.len().div_ceil(n_threads);

    let results: Vec<Vec<(String, PathBuf)>> = std::thread::scope(|s| {
        let handles: Vec<_> = files
            .chunks(chunk_size)
            .map(|chunk| {
                s.spawn(move || {
                    let mut local: Vec<(String, PathBuf)> = Vec::new();
                    for (path, expected_fqn) in chunk {
                        progress_add_done(progress);
                        if let Ok(content) = read_for_scan(path) {
                            for fqcn in scan_content(&content) {
                                if &fqcn == expected_fqn {
                                    local.push((fqcn, path.clone()));
                                }
                            }
                        }
                    }
                    local
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join().unwrap_or_else(|_| {
                    tracing::error!("PHPantom: thread panic in scan_files_parallel_psr4");
                    Vec::new()
                })
            })
            .collect()
    });

    let total: usize = results.iter().map(|v| v.len()).sum();
    let mut classmap = HashMap::with_capacity(total);
    for batch in results {
        for (fqcn, path) in batch {
            classmap.entry(fqcn).or_insert(path);
        }
    }
    classmap
}

/// Scan a batch of files for all symbols (classes, functions, constants)
/// in parallel and return a [`WorkspaceScanResult`].
///
/// Each file carries its own completion-origin tier, which is attached to
/// any function or constant discovered in it. This lets callers that know
/// a file's package provenance up front (e.g. vendor package scanning)
/// classify symbols in the same read/scan pass instead of re-reading every
/// file afterwards just to determine its origin.
fn scan_files_parallel_full(
    files: &[(PathBuf, crate::ClassCompletionOrigin)],
    progress: Option<&ScanProgress>,
) -> WorkspaceScanResult {
    if files.is_empty() {
        return WorkspaceScanResult::default();
    }

    // Small batches: sequential
    if files.len() <= 4 {
        let mut result = WorkspaceScanResult::default();
        for (path, origin) in files {
            progress_add_done(progress);
            if let Ok(content) = read_for_scan(path) {
                let scan = super::find_symbols(&content);
                for fqcn in scan.classes {
                    let class_short_name = fqcn_short_name(&fqcn).to_owned();
                    let mut origin_wins = false;
                    result
                        .classmap
                        .entry(fqcn.clone())
                        .and_modify(|existing| {
                            let existing_stem =
                                existing.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                            let new_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                            if existing_stem != class_short_name && new_stem == class_short_name {
                                *existing = path.clone();
                                origin_wins = true;
                            }
                        })
                        .or_insert_with(|| {
                            origin_wins = true;
                            path.clone()
                        });
                    if origin_wins {
                        result.class_origins.insert(fqcn, *origin);
                    }
                }
                for fqn in scan.functions {
                    result
                        .function_index
                        .entry(fqn.clone())
                        .or_insert_with(|| path.clone());
                    result.function_origins.entry(fqn).or_insert(*origin);
                }
                for name in scan.constants {
                    result
                        .constant_index
                        .entry(name.clone())
                        .or_insert_with(|| path.clone());
                    result.constant_origins.entry(name).or_insert(*origin);
                }
            }
        }
        return result;
    }

    let n_threads = thread_count().min(files.len());

    // Workers claim blocks from a shared cursor rather than taking a
    // fixed slice each: file sizes vary by orders of magnitude across a
    // vendor tree, so an even split by count leaves most workers idle
    // behind whichever slice drew the large files.  Blocks are merged in
    // index order below, so the result is the same as a sequential scan.
    let n_blocks = files.len().div_ceil(SCAN_BLOCK_FILES);
    let next_block = std::sync::atomic::AtomicUsize::new(0);
    let mut results: Vec<ScannedBlock> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..n_threads)
            .map(|_| {
                let next_block = &next_block;
                s.spawn(move || {
                    let mut out: Vec<ScannedBlock> = Vec::new();
                    loop {
                        let b = next_block.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if b >= n_blocks {
                            break;
                        }
                        let start = b * SCAN_BLOCK_FILES;
                        let end = (start + SCAN_BLOCK_FILES).min(files.len());
                        let mut local: Vec<ScannedFile> = Vec::new();
                        for (path, origin) in &files[start..end] {
                            progress_add_done(progress);
                            if let Ok(content) = read_for_scan(path) {
                                let scan = super::find_symbols(&content);
                                if !scan.classes.is_empty()
                                    || !scan.functions.is_empty()
                                    || !scan.constants.is_empty()
                                {
                                    local.push((scan, path.clone(), *origin));
                                }
                            }
                        }
                        if !local.is_empty() {
                            out.push((b, local));
                        }
                    }
                    out
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| {
                h.join().unwrap_or_else(|_| {
                    tracing::error!("PHPantom: thread panic in scan_files_parallel_full");
                    Vec::new()
                })
            })
            .collect()
    });
    results.sort_unstable_by_key(|(b, _)| *b);

    let mut result = WorkspaceScanResult::default();
    for (_, batch) in results {
        for (scan, path, origin) in batch {
            for fqcn in scan.classes {
                let class_short_name = fqcn_short_name(&fqcn).to_owned();
                let mut origin_wins = false;
                result
                    .classmap
                    .entry(fqcn.clone())
                    .and_modify(|existing| {
                        // When two files declare the same FQN, prefer the one
                        // whose filename matches the class's short name (PSR-4
                        // convention). This handles packages with conditional
                        // loading (e.g. ArraySubsetAsserts.php vs
                        // ArraySubsetAssertsEmpty.php both defining the same
                        // trait name).
                        let existing_stem =
                            existing.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                        let new_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                        if existing_stem != class_short_name && new_stem == class_short_name {
                            *existing = path.clone();
                            origin_wins = true;
                        }
                    })
                    .or_insert_with(|| {
                        origin_wins = true;
                        path.clone()
                    });
                if origin_wins {
                    result.class_origins.insert(fqcn, origin);
                }
            }
            for fqn in scan.functions {
                result
                    .function_index
                    .entry(fqn.clone())
                    .or_insert_with(|| path.clone());
                result.function_origins.entry(fqn).or_insert(origin);
            }
            for name in scan.constants {
                result
                    .constant_index
                    .entry(name.clone())
                    .or_insert_with(|| path.clone());
                result.constant_origins.entry(name).or_insert(origin);
            }
        }
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
    progress: Option<&ScanProgress>,
) -> WorkspaceScanResult {
    // Phase 1: collect file paths
    let skip_paths = HashSet::new();
    let opts = WalkOptions::new(skip_dirs.iter().cloned().collect(), &skip_paths);
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

/// Scan the files and directories named by `[indexing] include`,
/// bypassing both the gitignore and the hidden-entry filters.
//////
/// Directories are walked recursively; files are scanned directly. A
/// path the normal walk already covered is simply scanned twice and
/// loses the first-wins merge, so callers need not deduplicate.
pub fn scan_include_paths(
    paths: &[PathBuf],
    progress: Option<&ScanProgress>,
) -> WorkspaceScanResult {
    use ignore::WalkBuilder;

    let mut php_files: Vec<(PathBuf, crate::ClassCompletionOrigin)> = Vec::new();
    for path in paths {
        if path.is_file() {
            if is_php_file(path) {
                php_files.push((path.clone(), crate::ClassCompletionOrigin::Project));
            }
            continue;
        }

        let walker = WalkBuilder::new(path)
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .ignore(false)
            .parents(false)
            .hidden(false)
            .build();
        for entry in walker.flatten() {
            let entry_path = entry.path();
            if is_php_file(entry_path)
                && (entry.file_type().is_some_and(|ft| ft.is_file()) || entry_path.is_file())
            {
                php_files.push((
                    entry_path.to_path_buf(),
                    crate::ClassCompletionOrigin::Project,
                ));
            }
        }
    }

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
/// Test directories (`tests/` and `Tests/`) are excluded by name to avoid
/// indexing duplicate class definitions from unit-test fixtures.
pub fn scan_drupal_directories(
    web_root: &Path,
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
            .filter_entry(|entry| {
                if entry.file_type().is_some_and(|ft| ft.is_dir()) {
                    let name = entry.file_name().to_str().unwrap_or("");
                    // Exclude test directories (both conventional casings)
                    if name == "tests" || name == "Tests" {
                        return false;
                    }
                }
                true
            })
            .build();

        for entry in walker.flatten() {
            let path = entry.path();
            if path.is_file() && is_drupal_php_file(path) {
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

/// Extract the short (unqualified) class name from a fully-qualified name.
///
/// For example, `"DMS\\PHPUnitExtensions\\ArraySubset\\ArraySubsetAsserts"`
/// yields `"ArraySubsetAsserts"`.
fn fqcn_short_name(fqcn: &str) -> &str {
    fqcn.rsplit('\\').next().unwrap_or(fqcn)
}

/// Extract string values from a JSON value that is either a single
/// string or an array of strings.
fn value_to_strings(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::String(s) => vec![s.clone()],
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        _ => Vec::new(),
    }
}

/// Return `true` for a file the PHP scanners should read.
fn is_php_file(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "php")
}

/// What a [`walk_roots`] call leaves out.
struct WalkOptions<'a> {
    /// Directories that must never be entered: vendor trees scanned
    /// through `installed.json` instead, and monorepo subproject roots
    /// another pipeline already covered.  Shared behind an `Arc` because
    /// `ignore` requires the `filter_entry` closure to own its captures.
    skip_dirs: std::sync::Arc<Vec<PathBuf>>,
    /// Absolute file paths to leave out of the result, typically the ones
    /// Composer's generated classmap already covers.
    skip_paths: &'a HashSet<PathBuf>,
}

impl<'a> WalkOptions<'a> {
    fn new(skip_dirs: Vec<PathBuf>, skip_paths: &'a HashSet<PathBuf>) -> Self {
        Self {
            skip_dirs: std::sync::Arc::new(skip_dirs),
            skip_paths,
        }
    }
}

/// Walk a set of directory roots across all CPU cores and return the
/// matching files of each.
///
/// All the roots go into a *single* `ignore` walk, which matters twice
/// over.  `ignore` then reads the global gitignore and compiles each
/// shared ancestor `.gitignore` once for the whole set instead of once
/// per root: a vendor tree has thousands of autoload roots sharing the
/// same ancestors, and that setup cost used to be most of the scan's CPU
/// time.  And its worker pool splits the work by *directory* over a
/// work-stealing deque, so one enormous package (`aws/aws-sdk-php` has
/// 2900 files under a single PSR-4 root) no longer runs on one core while
/// the rest of the scan waits for it.
///
/// The result has one entry per input root, in root order, so callers
/// keep control over the concatenation order their duplicate-symbol
/// tie-break depends on.  Roots that are not directories yield an empty
/// entry, and a file below two overlapping roots is reported for both.
///
/// Each root's files are sorted by path.  A parallel walk reaches files
/// in whatever order the workers happen to get to them, and a first-wins
/// classmap must not depend on that; sorting also makes the result
/// reproducible across runs, which the readdir order it replaces was not.
fn walk_roots(roots: &[PathBuf], opts: &WalkOptions) -> Vec<Vec<PathBuf>> {
    use ignore::{WalkBuilder, WalkState};

    // Two PSR-4 prefixes can map to the same directory, so the walk is
    // over the distinct roots; the attribution below still hands the
    // files to every root that named them.
    let mut roots_by_path: HashMap<&Path, Vec<usize>> = HashMap::new();
    let mut builder: Option<WalkBuilder> = None;
    for (index, dir) in roots.iter().enumerate() {
        if !dir.is_dir() {
            continue;
        }
        match roots_by_path.entry(dir.as_path()) {
            std::collections::hash_map::Entry::Occupied(mut e) => e.get_mut().push(index),
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(vec![index]);
                match &mut builder {
                    Some(builder) => {
                        builder.add(dir);
                    }
                    None => builder = Some(WalkBuilder::new(dir)),
                }
            }
        }
    }

    let mut out: Vec<Vec<PathBuf>> = vec![Vec::new(); roots.len()];
    let Some(mut builder) = builder else {
        return out;
    };

    let skip_dirs = std::sync::Arc::clone(&opts.skip_dirs);
    builder
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .hidden(true)
        .parents(true)
        .ignore(true)
        .threads(thread_count())
        .filter_entry(move |entry| {
            !(entry.file_type().is_some_and(|ft| ft.is_dir())
                && skip_dirs.iter().any(|dir| dir == entry.path()))
        });

    let (tx, rx) = std::sync::mpsc::channel::<(usize, PathBuf)>();
    let skip_paths = opts.skip_paths;
    let roots_by_path = &roots_by_path;
    builder.build_parallel().run(|| {
        let tx = tx.clone();
        Box::new(move |entry| {
            let Ok(entry) = entry else {
                return WalkState::Continue;
            };
            let path = entry.path();
            let file_type = entry.file_type();
            if file_type.is_some_and(|ft| ft.is_dir())
                || !is_php_file(path)
                || skip_paths.contains(path)
                // `ignore` reports a symlink's own type, so confirm the
                // target is a regular file before indexing it.  The tests
                // above keep this stat off the common path.
                || !(file_type.is_some_and(|ft| ft.is_file()) || path.is_file())
            {
                return WalkState::Continue;
            }
            // The root this entry came from is its `depth`-th ancestor.
            // Overlapping roots (Laravel maps both `src/Illuminate` and
            // `src/Illuminate/Collections`) are walked once each, so
            // attributing by depth gives every root exactly the files its
            // own walk produced.
            if let Some(root) = path.ancestors().nth(entry.depth())
                && let Some(indices) = roots_by_path.get(root)
            {
                for &index in indices {
                    let _ = tx.send((index, path.to_path_buf()));
                }
            }
            WalkState::Continue
        })
    });
    drop(tx);

    for (index, path) in rx {
        out[index].push(path);
    }
    for files in &mut out {
        files.sort_unstable();
    }
    out
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
