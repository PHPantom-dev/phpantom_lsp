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
    let mut php_files: Vec<(PathBuf, crate::ClassCompletionOrigin)> = Vec::new();
    let skip_paths = HashSet::new();
    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }
        collect_php_files(
            dir,
            vendor_dir_paths,
            &skip_paths,
            &mut php_files,
            crate::ClassCompletionOrigin::Project,
        );
    }
    let paths: Vec<PathBuf> = php_files.into_iter().map(|(p, _)| p).collect();
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
    // ── PSR-4 directories: collect (path, expected_fqn) pairs ───────
    let mut psr4_files: Vec<(PathBuf, String, crate::ClassCompletionOrigin)> = Vec::new();
    for (prefix, base_path) in psr4 {
        if !base_path.is_dir() {
            continue;
        }
        collect_psr4_php_files(
            base_path,
            prefix,
            vendor_dir_paths,
            skip_paths,
            &mut psr4_files,
            crate::ClassCompletionOrigin::Project,
        );
    }

    // ── Plain classmap directories ──────────────────────────────────
    let mut plain_files: Vec<(PathBuf, crate::ClassCompletionOrigin)> = Vec::new();
    for dir in classmap_dirs {
        if !dir.is_dir() {
            continue;
        }
        collect_php_files(
            dir,
            vendor_dir_paths,
            skip_paths,
            &mut plain_files,
            crate::ClassCompletionOrigin::Project,
        );
    }

    // ── Scan all files in parallel ──────────────────────────────────
    let psr4_pairs: Vec<(PathBuf, String)> =
        psr4_files.into_iter().map(|(p, s, _)| (p, s)).collect();
    let plain_paths: Vec<PathBuf> = plain_files.into_iter().map(|(p, _)| p).collect();
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

    let vendor_dir_paths: Vec<PathBuf> = vec![vendor_path.clone()];

    // The directory containing installed.json — install-path values
    // are relative to this directory.
    let composer_dir = vendor_path.join("composer");

    // Phase 1: collect all file paths from all packages (sequential
    // walk, but no file I/O beyond stat calls).
    let mut psr4_files: Vec<(PathBuf, String, crate::ClassCompletionOrigin)> = Vec::new();
    let mut plain_files: Vec<(PathBuf, crate::ClassCompletionOrigin)> = Vec::new();

    for package in packages {
        let origin = package
            .get("name")
            .and_then(|n| n.as_str())
            .map(|name| classify_package_origin(name, explicit_deps))
            .unwrap_or(crate::ClassCompletionOrigin::VendorTransitive);
        // Locate the package on disk.  Composer 2's installed.json
        // includes an `install-path` field that is relative to the
        // `vendor/composer/` directory.  This is the authoritative
        // location and handles path repositories, custom installers,
        // and any other layout that doesn't follow the default
        // `vendor/<name>/` convention.  Fall back to `vendor/<name>`
        // only when `install-path` is absent (Composer 1 format).
        let pkg_path =
            if let Some(install_path) = package.get("install-path").and_then(|p| p.as_str()) {
                composer_dir.join(install_path)
            } else if let Some(pkg_name) = package.get("name").and_then(|n| n.as_str()) {
                vendor_path.join(pkg_name)
            } else {
                continue;
            };

        let pkg_path = match pkg_path.canonicalize() {
            Ok(p) => p,
            Err(_) => {
                // Directory doesn't exist (package not installed yet).
                if !pkg_path.is_dir() {
                    continue;
                }
                pkg_path
            }
        };

        if !pkg_path.is_dir() {
            continue;
        }

        // Extract autoload section
        let Some(autoload) = package.get("autoload") else {
            continue;
        };

        // PSR-4 entries
        if let Some(psr4) = autoload.get("psr-4").and_then(|p| p.as_object()) {
            for (prefix, paths) in psr4 {
                let prefix = normalise_prefix(prefix);
                for dir_str in value_to_strings(paths) {
                    let dir = pkg_path.join(&dir_str);
                    if dir.is_dir() {
                        collect_psr4_php_files(
                            &dir,
                            &prefix,
                            &vendor_dir_paths,
                            skip_paths,
                            &mut psr4_files,
                            origin,
                        );
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
                        plain_files.push((file, origin));
                    }
                }
            }

            // When a files entry registers a custom autoloader via
            // spl_autoload_register, it will load classes from the
            // package at runtime. Since we can't execute that logic,
            // do a full scan of the package directory to discover all
            // classes it provides.
            if has_custom_autoloader {
                collect_php_files(
                    &pkg_path,
                    &vendor_dir_paths,
                    skip_paths,
                    &mut plain_files,
                    origin,
                );
            }
        }

        // Classmap entries
        if let Some(cm) = autoload.get("classmap").and_then(|c| c.as_array()) {
            for entry in cm {
                if let Some(dir_str) = entry.as_str() {
                    let dir = pkg_path.join(dir_str);
                    if dir.is_dir() {
                        collect_php_files(
                            &dir,
                            &vendor_dir_paths,
                            skip_paths,
                            &mut plain_files,
                            origin,
                        );
                    } else if dir.is_file()
                        && dir.extension().is_some_and(|ext| ext == "php")
                        && !skip_paths.contains(&dir)
                    {
                        plain_files.push((dir, origin));
                    }
                }
            }
        }
    }

    // Phase 2: scan all collected files in parallel
    let mut all_files: Vec<PathBuf> = psr4_files.iter().map(|(path, _, _)| path.clone()).collect();
    all_files.extend(plain_files.iter().map(|(path, _)| path.clone()));

    // The origin classification pass below re-reads every file, so it
    // counts as its own work units.
    progress_add_total(
        progress,
        all_files.len() + psr4_files.len() + plain_files.len(),
    );

    let mut result = scan_files_parallel_full(&all_files, progress);
    let mut class_origins = HashMap::new();
    let mut function_origins = HashMap::new();
    let mut constant_origins = HashMap::new();
    for (path, expected_fqn, origin) in psr4_files {
        progress_add_done(progress);
        if let Ok(content) = read_for_scan(&path) {
            for fqn in scan_content(&content) {
                if fqn == expected_fqn {
                    class_origins.entry(fqn).or_insert(origin);
                }
            }
        }
    }
    for (path, origin) in plain_files {
        progress_add_done(progress);
        let symbols = super::scan_file_full(&path);
        for fqn in symbols.classes {
            class_origins.entry(fqn).or_insert(origin);
        }
        for fqn in symbols.functions {
            function_origins.entry(fqn).or_insert(origin);
        }
        for name in symbols.constants {
            constant_origins.entry(name).or_insert(origin);
        }
    }
    result.class_origins = class_origins;
    result.function_origins = function_origins;
    result.constant_origins = constant_origins;
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
fn scan_files_parallel_full(
    files: &[PathBuf],
    progress: Option<&ScanProgress>,
) -> WorkspaceScanResult {
    if files.is_empty() {
        return WorkspaceScanResult::default();
    }

    // Small batches: sequential
    if files.len() <= 4 {
        let mut result = WorkspaceScanResult::default();
        for path in files {
            progress_add_done(progress);
            if let Ok(content) = read_for_scan(path) {
                let scan = super::find_symbols(&content);
                for fqcn in scan.classes {
                    let class_short_name = fqcn_short_name(&fqcn).to_owned();
                    result
                        .classmap
                        .entry(fqcn)
                        .and_modify(|existing| {
                            let existing_stem =
                                existing.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                            let new_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                            if existing_stem != class_short_name && new_stem == class_short_name {
                                *existing = path.clone();
                            }
                        })
                        .or_insert_with(|| path.clone());
                }
                for fqn in scan.functions {
                    result
                        .function_index
                        .entry(fqn)
                        .or_insert_with(|| path.clone());
                }
                for name in scan.constants {
                    result
                        .constant_index
                        .entry(name)
                        .or_insert_with(|| path.clone());
                }
            }
        }
        return result;
    }

    let n_threads = thread_count().min(files.len());
    let chunk_size = files.len().div_ceil(n_threads);

    let results: Vec<Vec<(ScanResult, PathBuf)>> = std::thread::scope(|s| {
        let handles: Vec<_> = files
            .chunks(chunk_size)
            .map(|chunk| {
                s.spawn(move || {
                    let mut local: Vec<(ScanResult, PathBuf)> = Vec::new();
                    for path in chunk {
                        progress_add_done(progress);
                        if let Ok(content) = read_for_scan(path) {
                            let scan = super::find_symbols(&content);
                            if !scan.classes.is_empty()
                                || !scan.functions.is_empty()
                                || !scan.constants.is_empty()
                            {
                                local.push((scan, path.clone()));
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
                    tracing::error!("PHPantom: thread panic in scan_files_parallel_full");
                    Vec::new()
                })
            })
            .collect()
    });

    let mut result = WorkspaceScanResult::default();
    for batch in results {
        for (scan, path) in batch {
            for fqcn in scan.classes {
                let class_short_name = fqcn_short_name(&fqcn).to_owned();
                result
                    .classmap
                    .entry(fqcn)
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
                        }
                    })
                    .or_insert_with(|| path.clone());
            }
            for fqn in scan.functions {
                result
                    .function_index
                    .entry(fqn)
                    .or_insert_with(|| path.clone());
            }
            for name in scan.constants {
                result
                    .constant_index
                    .entry(name)
                    .or_insert_with(|| path.clone());
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
    use ignore::WalkBuilder;

    let skip_dirs_owned = skip_dirs.clone();

    // Phase 1: collect file paths (single-threaded walk)
    let walker = WalkBuilder::new(workspace_root)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .hidden(true)
        .parents(true)
        .ignore(true)
        .filter_entry(move |entry| {
            if entry.file_type().is_some_and(|ft| ft.is_dir()) {
                let path = entry.path();
                // Skip directories in the skip set (monorepo subproject roots)
                if skip_dirs_owned.contains(path) {
                    return false;
                }
            }
            true
        })
        .build();

    let mut php_files: Vec<PathBuf> = Vec::new();
    for entry in walker.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "php") {
            php_files.push(path.to_path_buf());
        }
    }

    // Phase 2: scan files in parallel
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

    let mut php_files: Vec<PathBuf> = Vec::new();

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
            .parents(false)
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
                php_files.push(path.to_path_buf());
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

/// Normalise a PSR-4 prefix: ensure it ends with `\`.
fn normalise_prefix(prefix: &str) -> String {
    if prefix.is_empty() {
        String::new()
    } else if prefix.ends_with('\\') {
        prefix.to_string()
    } else {
        format!("{prefix}\\")
    }
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

/// Collect all `.php` file paths under a directory using gitignore-aware
/// walking.  Paths are appended to `out`.  No file content is read.
///
/// Uses the `ignore` crate's `WalkBuilder` to respect `.gitignore`
/// rules at every level.  Hidden directories are skipped automatically.
/// Directories whose absolute path is in `vendor_dir_paths` are also
/// skipped.  Individual files whose path appears in `skip_paths` are
/// excluded (used by the merged classmap + self-scan pipeline).
fn collect_php_files(
    dir: &Path,
    vendor_dir_paths: &[PathBuf],
    skip_paths: &HashSet<PathBuf>,
    out: &mut Vec<(PathBuf, crate::ClassCompletionOrigin)>,
    origin: crate::ClassCompletionOrigin,
) {
    use ignore::WalkBuilder;

    let vendor_paths: Vec<PathBuf> = vendor_dir_paths.to_vec();

    let walker = WalkBuilder::new(dir)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .hidden(true)
        .parents(true)
        .ignore(true)
        .filter_entry(move |entry| {
            if entry.file_type().is_some_and(|ft| ft.is_dir()) {
                let path = entry.path();
                if vendor_paths.iter().any(|vp| vp == path) {
                    return false;
                }
            }
            true
        })
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "php") {
            let owned = path.to_path_buf();
            if !skip_paths.contains(&owned) {
                out.push((owned, origin));
            }
        }
    }
}

/// Collect all `.php` file paths under a PSR-4 directory, computing the
/// expected FQN for each file from its relative path.  Paths and
/// expected FQNs are appended to `out`.  No file content is read.
///
/// Files whose path appears in `skip_paths` are excluded.
fn collect_psr4_php_files(
    base_path: &Path,
    namespace_prefix: &str,
    vendor_dir_paths: &[PathBuf],
    skip_paths: &HashSet<PathBuf>,
    out: &mut Vec<(PathBuf, String, crate::ClassCompletionOrigin)>,
    origin: crate::ClassCompletionOrigin,
) {
    use ignore::WalkBuilder;

    let vendor_paths: Vec<PathBuf> = vendor_dir_paths.to_vec();

    let walker = WalkBuilder::new(base_path)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .hidden(true)
        .parents(true)
        .ignore(true)
        .filter_entry(move |entry| {
            if entry.file_type().is_some_and(|ft| ft.is_dir()) {
                let path = entry.path();
                if vendor_paths.iter().any(|vp| vp == path) {
                    return false;
                }
            }
            true
        })
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "php") {
            let owned = path.to_path_buf();
            if skip_paths.contains(&owned) {
                continue;
            }
            // Compute expected FQN from the file path relative to the
            // PSR-4 base directory.
            let relative = match path.strip_prefix(base_path) {
                Ok(rel) => rel,
                Err(_) => continue,
            };
            let relative_str = relative.to_string_lossy();
            // Strip the `.php` extension
            let stem = match relative_str.strip_suffix(".php") {
                Some(s) => s,
                None => continue,
            };
            // Convert path separators to namespace separators
            let expected_fqn = format!("{}{}", namespace_prefix, stem.replace('/', "\\"));

            out.push((owned, expected_fqn, origin));
        }
    }
}

#[cfg(test)]
#[path = "discovery_tests.rs"]
mod tests;
