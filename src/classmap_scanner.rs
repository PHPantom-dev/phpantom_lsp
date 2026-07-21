//! Fast byte-level PHP symbol scanners for early-stage file discovery.
//!
//! This module provides two single-pass state machines that extract
//! symbol names from PHP source without a full AST parse:
//!
//! - **PSR-4 scanner** ([`find_classes`]) — extracts fully-qualified
//!   class, interface, trait, and enum names.  Used by the PSR-4
//!   directory walker to build a classmap when Composer's
//!   `autoload_classmap.php` is missing or incomplete.
//!
//! - **Full-scan** ([`find_symbols`]) — extracts classes *plus*
//!   standalone function names, `define()` constants, and top-level
//!   `const` declarations.  Used for non-Composer projects (no
//!   `composer.json`) and for Composer autoload files
//!   (`autoload_files.php` and their `require_once` chains) to
//!   populate name-to-path indices without a full AST parse.
//!
//! These scanners serve three indexing scenarios:
//!
//! 1. **Optimized Composer** — the Composer classmap is parsed
//!    directly (not by this module).  Functions and constants from
//!    `autoload_files.php` are discovered by the full-scan during
//!    initialization, populating `autoload_function_index`,
//!    `autoload_constant_index`, and `fqn_uri_index`.  Lazy
//!    `update_ast` on first access provides complete details.
//!
//! 2. **Composer self-scan** — the PSR-4 scanner builds a classmap
//!    from `composer.json`'s autoload directories.  Functions and
//!    constants from `autoload_files.php` are discovered by the
//!    full-scan, same as scenario 1.
//!
//! 3. **No Composer** — the full-scan walks all workspace files,
//!    populating the classmap, `autoload_function_index`, and
//!    `autoload_constant_index` in one pass.  Lazy `update_ast`
//!    on first access provides complete `FunctionInfo`/`DefineInfo`.
//!
//! The implementation is modelled after Composer's `PhpFileParser` /
//! `PhpFileCleaner` pipeline and Libretto's `FastScanner`.  Both
//! scanners handle:
//!
//! - `class`, `interface`, `trait`, and `enum` declarations
//! - `namespace` declarations (including braced and semicolon forms)
//! - Single-quoted and double-quoted strings (with escape handling)
//! - Heredoc and nowdoc literals
//! - Line comments (`//`, `#`) and block comments (`/* ... */`)
//! - PHP attributes (`#[...]`) — not confused with `#` comments
//! - Property/nullsafe access like `$node->class` (not treated as a
//!   class declaration)
//! - `SomeClass::class` constant access (not treated as a declaration)
//!
//! The full-scan additionally handles:
//!
//! - `function` declarations (top-level only, not methods or closures)
//! - `define('NAME', ...)` calls (constant name from first string arg)
//! - `const NAME = ...` at top level (not class constants)
//!
//! # Performance
//!
//! Both scanners use `memchr` for SIMD-accelerated keyword
//! pre-screening.  Files that contain none of the relevant keywords
//! are rejected in a single fast pass without entering the state
//! machine.

use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::path::{Path, PathBuf};

use memchr::{memchr, memmem};
use memmap2::Mmap;

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

// ─── File reading ────────────────────────────────────────────────────────────

/// Bytes of a file made available to the byte-level scanners.
///
/// [`read_for_scan`] prefers a memory-mapped view so the OS page cache
/// is shared without copying the file into the heap, falling back to a
/// heap read when mapping is not possible (empty files cannot be mapped,
/// and some filesystems do not support it).
pub(crate) enum FileBytes {
    /// A read-only memory map of the file's pages.
    Mapped(Mmap),
    /// A heap copy of the file's contents (mapping fallback).
    Owned(Vec<u8>),
}

impl Deref for FileBytes {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            FileBytes::Mapped(map) => map,
            FileBytes::Owned(bytes) => bytes,
        }
    }
}

/// Read a file's bytes for scanning, preferring a memory-mapped view.
///
/// The scanners read the returned bytes synchronously and drop them
/// before returning, so the map never outlives the scan.
pub(crate) fn read_for_scan(path: &Path) -> std::io::Result<FileBytes> {
    let file = std::fs::File::open(path)?;
    // SAFETY: The map is read synchronously and dropped before this
    // scan returns. A concurrent truncation could raise SIGBUS, but
    // index scanning does not run while the user is deleting files; the
    // heap-read fallback covers filesystems that reject mapping.
    match unsafe { Mmap::map(&file) } {
        Ok(map) => Ok(FileBytes::Mapped(map)),
        Err(_) => std::fs::read(path).map(FileBytes::Owned),
    }
}

// ─── Data structures ────────────────────────────────────────────────────────

/// All symbols discovered in a single PHP file by [`find_symbols`].
///
/// Contains fully-qualified names for classes, standalone functions,
/// and constants (`define()` and top-level `const`).
#[derive(Debug, Clone, Default)]
pub struct ScanResult {
    /// Fully-qualified class, interface, trait, and enum names.
    pub classes: Vec<String>,
    /// Fully-qualified standalone function names.
    pub functions: Vec<String>,
    /// Constant names from `define('NAME', ...)` and top-level `const NAME`.
    pub constants: Vec<String>,
}

/// Combined workspace scan results for classes, functions, and constants.
///
/// Returned by [`scan_workspace_fallback_full`] and consumed during
/// server initialization to populate the classmap and autoload indices.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceScanResult {
    /// FQN → file path for classes, interfaces, traits, and enums.
    pub classmap: HashMap<String, PathBuf>,
    /// FQN → completion origin tier.
    pub(crate) class_origins: HashMap<String, crate::ClassCompletionOrigin>,
    /// FQN → file path for standalone functions.
    pub function_index: HashMap<String, PathBuf>,
    /// FQN → completion origin tier for standalone functions.
    pub(crate) function_origins: HashMap<String, crate::ClassCompletionOrigin>,
    /// Constant name → file path for `define()` and top-level `const`.
    pub constant_index: HashMap<String, PathBuf>,
    /// Constant name → completion origin tier.
    pub(crate) constant_origins: HashMap<String, crate::ClassCompletionOrigin>,
}

// ─── Public API ─────────────────────────────────────────────────────────────

/// Scan a single PHP file and return the fully-qualified class names it
/// defines.
///
/// Returns an empty `Vec` when the file cannot be read, is empty, or
/// contains no class-like declarations.
pub fn scan_file(path: &Path) -> Vec<String> {
    let Ok(content) = read_for_scan(path) else {
        return Vec::new();
    };
    if content.is_empty() {
        return Vec::new();
    }
    find_classes(&content)
}

/// Scan already-loaded file content and return the fully-qualified class
/// names it defines.
///
/// This avoids a redundant `fs::read` when the caller already has the
/// bytes in memory (e.g. from a parallel batch read).
pub fn scan_content(content: &[u8]) -> Vec<String> {
    if content.is_empty() {
        return Vec::new();
    }
    find_classes(content)
}

/// Scan a single PHP file and return all discovered symbols (classes,
/// functions, and constants).
///
/// Returns an empty [`ScanResult`] when the file cannot be read or is
/// empty.
pub fn scan_file_full(path: &Path) -> ScanResult {
    let Ok(content) = read_for_scan(path) else {
        return ScanResult::default();
    };
    if content.is_empty() {
        return ScanResult::default();
    }
    find_symbols(&content)
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
        let symbols = scan_file_full(&path);
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
                let scan = find_symbols(&content);
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
                            let scan = find_symbols(&content);
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

// ─── Core scanner ───────────────────────────────────────────────────────────

/// The **full-scan**: a single-pass byte-level scanner that extracts
/// fully-qualified class, function, and constant names from PHP source
/// bytes.
///
/// This is the extended version of [`find_classes`] (the PSR-4 scanner)
/// that also recognises `function` declarations, `define()` calls, and
/// top-level `const` statements.  It is used for both non-Composer
/// projects (full workspace scan) and Composer autoload files
/// (`autoload_files.php` and their `require_once` chains).
pub fn find_symbols(content: &[u8]) -> ScanResult {
    // Quick rejection — if the file has none of the relevant keywords
    // we can bail immediately.
    if !has_any_keyword(content) {
        return ScanResult::default();
    }

    let mut result = ScanResult::default();
    let mut namespace = String::new();
    let len = content.len();
    let mut i = 0;

    // Brace depth tracking for top-level `const` detection.
    // Depth 0 = top-level, depth 1 = inside a class/namespace block.
    let mut brace_depth: u32 = 0;
    // Whether we are inside a braced namespace block.
    let mut in_braced_namespace = false;
    // The brace depth at which the current namespace was opened.
    // `const` declarations at this depth (or depth 0 outside braced
    // namespaces) are top-level.
    let mut namespace_brace_depth: u32 = 0;

    // State flags
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut in_single_string = false;
    let mut in_double_string = false;
    let mut in_heredoc = false;
    let mut heredoc_id: &[u8] = &[];

    while i < len {
        // ── Skip: line comment (memchr to newline) ──────────────────
        if in_line_comment {
            if let Some(pos) = memchr(b'\n', &content[i..]) {
                i += pos + 1;
            } else {
                break; // rest of file is a comment
            }
            in_line_comment = false;
            continue;
        }

        // ── Skip: block comment (memmem to "*/") ────────────────────
        if in_block_comment {
            if let Some(pos) = memmem::find(&content[i..], b"*/") {
                i += pos + 2;
                in_block_comment = false;
            } else {
                break; // unclosed block comment
            }
            continue;
        }

        // ── Skip: single-quoted string (memchr to '\'' or '\\') ────
        if in_single_string {
            match memchr2_single_string(&content[i..]) {
                Some((offset, b'\\')) => {
                    i += offset + 2; // skip escaped char
                }
                Some((offset, _)) => {
                    // Found closing quote
                    i += offset + 1;
                    in_single_string = false;
                }
                None => break, // unclosed string
            }
            continue;
        }

        // ── Skip: double-quoted string (memchr to '"' or '\\') ─────
        if in_double_string {
            match memchr2_double_string(&content[i..]) {
                Some((offset, b'\\')) => {
                    i += offset + 2; // skip escaped char
                }
                Some((offset, _)) => {
                    // Found closing quote
                    i += offset + 1;
                    in_double_string = false;
                }
                None => break, // unclosed string
            }
            continue;
        }

        // ── Skip: heredoc / nowdoc (memchr to newline) ──────────────
        if in_heredoc {
            let line_start = i;
            while i < len && (content[i] == b' ' || content[i] == b'\t') {
                i += 1;
            }
            if i + heredoc_id.len() <= len && &content[i..i + heredoc_id.len()] == heredoc_id {
                let after = i + heredoc_id.len();
                if after >= len
                    || content[after] == b';'
                    || content[after] == b'\n'
                    || content[after] == b'\r'
                    || content[after] == b','
                    || content[after] == b')'
                {
                    in_heredoc = false;
                    i = after;
                    continue;
                }
            }
            i = line_start;
            if let Some(pos) = memchr(b'\n', &content[i..]) {
                i += pos + 1;
            } else {
                break; // rest of file is inside heredoc
            }
            continue;
        }

        // ── Main code parsing ───────────────────────────────────────
        let b = content[i];

        // Braces for depth tracking
        if b == b'{' {
            brace_depth += 1;
            i += 1;
            continue;
        }
        if b == b'}' {
            brace_depth = brace_depth.saturating_sub(1);
            // Exiting a braced namespace block resets the namespace.
            if in_braced_namespace && brace_depth == namespace_brace_depth {
                in_braced_namespace = false;
                namespace.clear();
            }
            i += 1;
            continue;
        }

        // Comments
        if b == b'/' && i + 1 < len {
            if content[i + 1] == b'/' {
                in_line_comment = true;
                i += 2;
                continue;
            }
            if content[i + 1] == b'*' {
                in_block_comment = true;
                i += 2;
                continue;
            }
        }

        if b == b'#' {
            if i + 1 < len && content[i + 1] == b'[' {
                i += 1;
                continue;
            }
            in_line_comment = true;
            i += 1;
            continue;
        }

        // Strings
        if b == b'\'' {
            in_single_string = true;
            i += 1;
            continue;
        }
        if b == b'"' {
            in_double_string = true;
            i += 1;
            continue;
        }

        // Heredoc / nowdoc
        if b == b'<' && i + 2 < len && content[i + 1] == b'<' && content[i + 2] == b'<' {
            i += 3;
            while i < len && content[i] == b' ' {
                i += 1;
            }
            if i < len && (content[i] == b'\'' || content[i] == b'"') {
                i += 1;
            }
            let id_start = i;
            while i < len && (content[i].is_ascii_alphanumeric() || content[i] == b'_') {
                i += 1;
            }
            if i > id_start {
                heredoc_id = &content[id_start..i];
                in_heredoc = true;
                if i < len && (content[i] == b'\'' || content[i] == b'"') {
                    i += 1;
                }
                while i < len && content[i] != b'\n' {
                    i += 1;
                }
                if i < len {
                    i += 1;
                }
            }
            continue;
        }

        // ── Keyword detection ───────────────────────────────────────
        if is_keyword_boundary(content, i) {
            // namespace
            if b == b'n'
                && i + 9 <= len
                && &content[i..i + 9] == b"namespace"
                && (i + 9 >= len
                    || content[i + 9].is_ascii_whitespace()
                    || content[i + 9] == b';'
                    || content[i + 9] == b'{')
            {
                i += 9;
                while i < len && content[i].is_ascii_whitespace() {
                    i += 1;
                }

                let ns_start = i;
                while i < len {
                    let c = content[i];
                    if c.is_ascii_alphanumeric()
                        || c == b'_'
                        || c == b'\\'
                        || c.is_ascii_whitespace()
                    {
                        i += 1;
                    } else {
                        break;
                    }
                }
                namespace = content[ns_start..i]
                    .iter()
                    .filter(|&&c| !c.is_ascii_whitespace())
                    .map(|&c| c as char)
                    .collect();
                if !namespace.is_empty() && !namespace.ends_with('\\') {
                    namespace.push('\\');
                }

                // Check for braced namespace: `namespace Foo { ... }`
                while i < len && content[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i < len && content[i] == b'{' {
                    in_braced_namespace = true;
                    namespace_brace_depth = brace_depth;
                    brace_depth += 1;
                    i += 1;
                }
                continue;
            }

            // class
            if b == b'c'
                && i + 5 <= len
                && &content[i..i + 5] == b"class"
                && (i + 5 >= len || content[i + 5].is_ascii_whitespace())
            {
                i += 5;
                if let Some(name) = read_name(content, &mut i) {
                    result.classes.push(format!("{namespace}{name}"));
                }
                continue;
            }

            // interface
            if b == b'i'
                && i + 9 <= len
                && &content[i..i + 9] == b"interface"
                && (i + 9 >= len || content[i + 9].is_ascii_whitespace())
            {
                i += 9;
                if let Some(name) = read_name(content, &mut i) {
                    result.classes.push(format!("{namespace}{name}"));
                }
                continue;
            }

            // trait
            if b == b't'
                && i + 5 <= len
                && &content[i..i + 5] == b"trait"
                && (i + 5 >= len || content[i + 5].is_ascii_whitespace())
            {
                i += 5;
                if let Some(name) = read_name(content, &mut i) {
                    result.classes.push(format!("{namespace}{name}"));
                }
                continue;
            }

            // enum
            if b == b'e'
                && i + 4 <= len
                && &content[i..i + 4] == b"enum"
                && (i + 4 >= len || content[i + 4].is_ascii_whitespace())
            {
                i += 4;
                if let Some(name) = read_name(content, &mut i) {
                    result.classes.push(format!("{namespace}{name}"));
                }
                continue;
            }

            // function (standalone — not inside a class/trait/enum body)
            if b == b'f'
                && i + 8 <= len
                && &content[i..i + 8] == b"function"
                && (i + 8 >= len || content[i + 8].is_ascii_whitespace() || content[i + 8] == b'(')
            {
                // Skip `use function …;` import statements — these
                // are not function declarations.
                if is_preceded_by_use(content, i) {
                    i += 8;
                    // Advance past the rest of the `use function` line
                    // so we don't accidentally pick up names from it.
                    while i < len && content[i] != b';' && content[i] != b'\n' {
                        i += 1;
                    }
                    if i < len && content[i] == b';' {
                        i += 1;
                    }
                    continue;
                }

                // Only top-level functions: depth 0 (no braced ns) or
                // the namespace brace depth + 1 doesn't apply — we
                // want depth == 0 outside braced ns, or depth ==
                // namespace_brace_depth + 1 inside braced ns.
                let is_top_level = if in_braced_namespace {
                    brace_depth == namespace_brace_depth + 1
                } else {
                    brace_depth == 0
                };

                if is_top_level {
                    i += 8;
                    // Skip `function (` — that's a closure, not a named function.
                    let mut j = i;
                    while j < len && content[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if j < len && content[j] == b'(' {
                        // Anonymous function / closure — skip.
                        i = j;
                    } else if let Some(name) = read_name(content, &mut i) {
                        result.functions.push(format!("{namespace}{name}"));
                    }
                } else {
                    i += 8;
                }
                continue;
            }

            // define('NAME', ...)
            if b == b'd'
                && i + 6 <= len
                && &content[i..i + 6] == b"define"
                && (i + 6 < len && content[i + 6] == b'(')
            {
                i += 7; // skip `define(`
                // Skip whitespace
                while i < len && content[i].is_ascii_whitespace() {
                    i += 1;
                }
                // Read the constant name from the string argument.
                if let Some(name) = read_define_name(content, &mut i) {
                    result.constants.push(name.to_string());
                }
                continue;
            }

            // const NAME = ... (top-level only)
            if b == b'c'
                && i + 5 <= len
                && &content[i..i + 5] == b"const"
                && (i + 5 >= len || content[i + 5].is_ascii_whitespace())
            {
                // Skip `use const …;` import statements.
                if is_preceded_by_use(content, i) {
                    i += 5;
                    while i < len && content[i] != b';' && content[i] != b'\n' {
                        i += 1;
                    }
                    if i < len && content[i] == b';' {
                        i += 1;
                    }
                    continue;
                }

                let is_top_level = if in_braced_namespace {
                    brace_depth == namespace_brace_depth + 1
                } else {
                    brace_depth == 0
                };

                if is_top_level {
                    i += 5;
                    if let Some(name) = read_name(content, &mut i) {
                        // Top-level const names are FQN with namespace.
                        result.constants.push(format!("{namespace}{name}"));
                    }
                } else {
                    i += 5;
                }
                continue;
            }
        }

        i += 1;
    }

    result
}

/// The **PSR-4 scanner**: a single-pass byte-level scanner that
/// extracts fully-qualified class, interface, trait, and enum names
/// from PHP source bytes.
///
/// This is the classes-only scanner used by the PSR-4 directory walker
/// and vendor package scanner.  For a scanner that also extracts
/// functions and constants, see [`find_symbols`] (the full-scan).
///
/// Skips comments, strings, heredocs, and nowdocs inline without
/// allocating a separate "cleaned" buffer.
pub fn find_classes(content: &[u8]) -> Vec<String> {
    // Quick rejection — use SIMD to check if any class-like keywords exist
    if !has_class_keyword(content) {
        return Vec::new();
    }

    let mut classes = Vec::with_capacity(4);
    let mut namespace = String::new();
    let len = content.len();
    let mut i = 0;

    // State flags
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut in_single_string = false;
    let mut in_double_string = false;
    let mut in_heredoc = false;
    let mut heredoc_id: &[u8] = &[];

    while i < len {
        // ── Skip: line comment (memchr to newline) ──────────────────
        if in_line_comment {
            if let Some(pos) = memchr(b'\n', &content[i..]) {
                i += pos + 1;
            } else {
                break;
            }
            in_line_comment = false;
            continue;
        }

        // ── Skip: block comment (memmem to "*/") ────────────────────
        if in_block_comment {
            if let Some(pos) = memmem::find(&content[i..], b"*/") {
                i += pos + 2;
                in_block_comment = false;
            } else {
                break;
            }
            continue;
        }

        // ── Skip: single-quoted string (memchr to '\'' or '\\') ────
        if in_single_string {
            match memchr2_single_string(&content[i..]) {
                Some((offset, b'\\')) => {
                    i += offset + 2;
                }
                Some((offset, _)) => {
                    i += offset + 1;
                    in_single_string = false;
                }
                None => break,
            }
            continue;
        }

        // ── Skip: double-quoted string (memchr to '"' or '\\') ─────
        if in_double_string {
            match memchr2_double_string(&content[i..]) {
                Some((offset, b'\\')) => {
                    i += offset + 2;
                }
                Some((offset, _)) => {
                    i += offset + 1;
                    in_double_string = false;
                }
                None => break,
            }
            continue;
        }

        // ── Skip: heredoc / nowdoc (memchr to newline) ──────────────
        if in_heredoc {
            let line_start = i;
            // Skip leading whitespace (PHP 7.3+ flexible heredoc)
            while i < len && (content[i] == b' ' || content[i] == b'\t') {
                i += 1;
            }
            if i + heredoc_id.len() <= len && &content[i..i + heredoc_id.len()] == heredoc_id {
                let after = i + heredoc_id.len();
                if after >= len
                    || content[after] == b';'
                    || content[after] == b'\n'
                    || content[after] == b'\r'
                    || content[after] == b','
                    || content[after] == b')'
                {
                    in_heredoc = false;
                    i = after;
                    continue;
                }
            }
            // Skip to next line
            i = line_start;
            if let Some(pos) = memchr(b'\n', &content[i..]) {
                i += pos + 1;
            } else {
                break;
            }
            continue;
        }

        // ── Main code parsing ───────────────────────────────────────
        let b = content[i];

        // Comments: // and /* */
        if b == b'/' && i + 1 < len {
            if content[i + 1] == b'/' {
                in_line_comment = true;
                i += 2;
                continue;
            }
            if content[i + 1] == b'*' {
                in_block_comment = true;
                i += 2;
                continue;
            }
        }

        // Hash comments (but not PHP attributes #[...])
        if b == b'#' {
            if i + 1 < len && content[i + 1] == b'[' {
                // PHP attribute — skip past it (it's not a comment)
                i += 1;
                continue;
            }
            in_line_comment = true;
            i += 1;
            continue;
        }

        // Strings
        if b == b'\'' {
            in_single_string = true;
            i += 1;
            continue;
        }
        if b == b'"' {
            in_double_string = true;
            i += 1;
            continue;
        }

        // Heredoc / nowdoc: <<<
        if b == b'<' && i + 2 < len && content[i + 1] == b'<' && content[i + 2] == b'<' {
            i += 3;
            // Skip whitespace
            while i < len && content[i] == b' ' {
                i += 1;
            }
            // Skip optional quote (nowdoc uses single quotes)
            if i < len && (content[i] == b'\'' || content[i] == b'"') {
                i += 1;
            }
            let id_start = i;
            while i < len && (content[i].is_ascii_alphanumeric() || content[i] == b'_') {
                i += 1;
            }
            if i > id_start {
                heredoc_id = &content[id_start..i];
                in_heredoc = true;
                // Skip closing quote
                if i < len && (content[i] == b'\'' || content[i] == b'"') {
                    i += 1;
                }
                // Skip to newline
                while i < len && content[i] != b'\n' {
                    i += 1;
                }
                if i < len {
                    i += 1;
                }
            }
            continue;
        }

        // ── Keyword detection ───────────────────────────────────────
        // Only match at valid keyword boundaries to avoid matching
        // property accesses like `$node->class`.
        if is_keyword_boundary(content, i) {
            // namespace
            if b == b'n'
                && i + 9 <= len
                && &content[i..i + 9] == b"namespace"
                && (i + 9 >= len
                    || content[i + 9].is_ascii_whitespace()
                    || content[i + 9] == b';'
                    || content[i + 9] == b'{')
            {
                i += 9;
                while i < len && content[i].is_ascii_whitespace() {
                    i += 1;
                }

                // Check for braced namespace (e.g. `namespace Foo { ... }`)
                // vs. semicolon form. Either way, read the name.
                let ns_start = i;
                while i < len {
                    let c = content[i];
                    if c.is_ascii_alphanumeric()
                        || c == b'_'
                        || c == b'\\'
                        || c.is_ascii_whitespace()
                    {
                        i += 1;
                    } else {
                        break;
                    }
                }
                namespace = content[ns_start..i]
                    .iter()
                    .filter(|&&c| !c.is_ascii_whitespace())
                    .map(|&c| c as char)
                    .collect();
                if !namespace.is_empty() && !namespace.ends_with('\\') {
                    namespace.push('\\');
                }
                continue;
            }

            // class
            if b == b'c'
                && i + 5 <= len
                && &content[i..i + 5] == b"class"
                && (i + 5 >= len || content[i + 5].is_ascii_whitespace())
            {
                i += 5;
                if let Some(name) = read_name(content, &mut i) {
                    classes.push(format!("{namespace}{name}"));
                }
                continue;
            }

            // interface
            if b == b'i'
                && i + 9 <= len
                && &content[i..i + 9] == b"interface"
                && (i + 9 >= len || content[i + 9].is_ascii_whitespace())
            {
                i += 9;
                if let Some(name) = read_name(content, &mut i) {
                    classes.push(format!("{namespace}{name}"));
                }
                continue;
            }

            // trait
            if b == b't'
                && i + 5 <= len
                && &content[i..i + 5] == b"trait"
                && (i + 5 >= len || content[i + 5].is_ascii_whitespace())
            {
                i += 5;
                if let Some(name) = read_name(content, &mut i) {
                    classes.push(format!("{namespace}{name}"));
                }
                continue;
            }

            // enum
            if b == b'e'
                && i + 4 <= len
                && &content[i..i + 4] == b"enum"
                && (i + 4 >= len || content[i + 4].is_ascii_whitespace())
            {
                i += 4;
                if let Some(name) = read_name(content, &mut i) {
                    classes.push(format!("{namespace}{name}"));
                }
                continue;
            }
        }

        i += 1;
    }

    classes
}

// ─── Internal helpers ───────────────────────────────────────────────────────

/// SIMD-accelerated pre-screening: check whether the content contains
/// any of the class-like keywords.
#[inline]
fn has_class_keyword(content: &[u8]) -> bool {
    memmem::find(content, b"class").is_some()
        || memmem::find(content, b"interface").is_some()
        || memmem::find(content, b"trait").is_some()
        || memmem::find(content, b"enum").is_some()
}

/// SIMD-accelerated pre-screening: check whether the content contains
/// any keyword relevant to symbol extraction (classes, functions,
/// constants).
#[inline]
fn has_any_keyword(content: &[u8]) -> bool {
    memmem::find(content, b"class").is_some()
        || memmem::find(content, b"interface").is_some()
        || memmem::find(content, b"trait").is_some()
        || memmem::find(content, b"enum").is_some()
        || memmem::find(content, b"function").is_some()
        || memmem::find(content, b"define").is_some()
        || memmem::find(content, b"const").is_some()
}

/// Check if a character is a valid boundary (not part of an identifier).
#[inline]
fn is_boundary_char(c: u8) -> bool {
    !c.is_ascii_alphanumeric() && c != b'_' && c != b':' && c != b'$'
}

/// Find the next single-quote or backslash in a slice, returning the
/// offset and the byte found.  Uses `memchr` for SIMD acceleration.
#[inline]
fn memchr2_single_string(haystack: &[u8]) -> Option<(usize, u8)> {
    memchr::memchr2(b'\'', b'\\', haystack).map(|pos| (pos, haystack[pos]))
}

/// Find the next double-quote or backslash in a slice, returning the
/// offset and the byte found.  Uses `memchr` for SIMD acceleration.
#[inline]
fn memchr2_double_string(haystack: &[u8]) -> Option<(usize, u8)> {
    memchr::memchr2(b'"', b'\\', haystack).map(|pos| (pos, haystack[pos]))
}

/// Check whether the keyword at position `i` is preceded by `use `
/// (with optional whitespace), indicating a `use function` or `use const`
/// import statement rather than a declaration.
fn is_preceded_by_use(content: &[u8], i: usize) -> bool {
    if i < 4 {
        return false;
    }
    // Walk backwards over whitespace.
    let mut j = i - 1;
    while j > 0 && content[j].is_ascii_whitespace() {
        j -= 1;
    }
    // Check for `use` (the 'e' is at j, 'u' at j-2).
    if j >= 2 && &content[j - 2..=j] == b"use" {
        // Make sure `use` itself is at a keyword boundary (not part
        // of a longer identifier like `reuse`).
        if j - 2 == 0 || is_boundary_char(content[j - 3]) {
            return true;
        }
    }
    false
}

/// Check whether a keyword can start at this offset.
///
/// Rejects property accesses like `$node->class` and
/// `$node?->class` to avoid false positives.
#[inline]
fn is_keyword_boundary(content: &[u8], i: usize) -> bool {
    if i == 0 {
        return true;
    }

    let prev = content[i - 1];
    if !is_boundary_char(prev) {
        return false;
    }

    // Reject object/nullsafe property access: ->class, ?->class
    if prev == b'>' && i >= 2 {
        let prev2 = content[i - 2];
        if prev2 == b'-' || prev2 == b'?' {
            return false;
        }
    }

    true
}

/// Read the constant name from the first argument of a `define()` call.
///
/// Expects `i` to point at the first character after `define(` (with
/// optional whitespace already skipped).  Handles both single-quoted
/// and double-quoted string literals.  Returns the raw name string
/// (without quotes).
#[inline]
fn read_define_name<'a>(content: &'a [u8], i: &mut usize) -> Option<&'a str> {
    let len = content.len();
    if *i >= len {
        return None;
    }
    let quote = content[*i];
    if quote != b'\'' && quote != b'"' {
        return None;
    }
    *i += 1; // skip opening quote
    let start = *i;
    while *i < len && content[*i] != quote {
        if content[*i] == b'\\' && *i + 1 < len {
            let next = content[*i + 1];
            if next == quote || next == b'\\' {
                // Escaped quote or escaped backslash — the name
                // contains a real escape sequence, which is unusual
                // for constant names.  Bail out.
                return None;
            }
            // A bare backslash (e.g. namespace separator in
            // 'App\Config\DB_HOST') is literal in single-quoted
            // strings and safe to include.
        }
        *i += 1;
    }
    if *i >= len {
        return None;
    }
    let name = &content[start..*i];
    *i += 1; // skip closing quote
    std::str::from_utf8(name).ok()
}

/// Read a class/interface/trait/enum name after the keyword.
///
/// Skips whitespace, then reads an identifier.  Returns `None` for
/// keywords like `extends`/`implements` that can follow `class` in
/// anonymous class expressions (`new class extends Foo {}`).
#[inline]
fn read_name<'a>(content: &'a [u8], i: &mut usize) -> Option<&'a str> {
    let len = content.len();

    // Skip whitespace
    while *i < len && content[*i].is_ascii_whitespace() {
        *i += 1;
    }

    let start = *i;

    // Read identifier characters
    while *i < len {
        let c = content[*i];
        if c.is_ascii_alphanumeric() || c == b'_' {
            *i += 1;
        } else {
            break;
        }
    }

    if *i == start {
        return None;
    }

    let name = &content[start..*i];

    // Skip keywords that appear in anonymous class expressions
    if name == b"extends" || name == b"implements" {
        return None;
    }

    std::str::from_utf8(name).ok()
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

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "classmap_scanner_tests.rs"]
mod tests;
