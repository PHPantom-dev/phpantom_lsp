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
//!
//! # Module layout
//!
//! - [`lexer`] — the SIMD byte-lexer fast path ([`find_classes`],
//!   [`find_symbols`], and the state-machine helpers they share).
//! - [`discovery`] — PSR-4, workspace, include-path and Drupal
//!   discovery, and the parallel batch scanning built on top of the
//!   lexer.
//! - [`vendor`] — the packages `installed.json` lists, and the autoload
//!   sources each one declares.
//! - [`walk`] — the `.gitignore`-aware directory walks.
//! - [`filters`] — the compiled `[indexing]` filters every walk applies.

use std::collections::HashMap;
use std::ops::Deref;
use std::path::{Path, PathBuf};

use memmap2::Mmap;

mod discovery;
mod filters;
mod lexer;
mod vendor;
mod walk;

pub(crate) use discovery::collect_include_files;
pub use discovery::{
    scan_directories, scan_drupal_directories, scan_include_paths, scan_psr4_directories,
    scan_psr4_directories_with_skip, scan_workspace_fallback, scan_workspace_fallback_full,
};
pub use filters::{FollowedLinks, IndexFilters, LinkClaims, workspace_walk_builder};
pub use lexer::{find_classes, find_symbols};
pub(crate) use vendor::vendor_package_roots;
pub use vendor::{scan_vendor_packages, scan_vendor_packages_with_skip};
pub(crate) use walk::{
    collect_php_files_gitignore, collect_workspace_index_files_gitignore,
    visit_workspace_files_gitignore,
};

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
    use std::io::Read;

    let file = std::fs::File::open(path)?;
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if len >= MMAP_MIN_BYTES {
        // SAFETY: The map is read synchronously and dropped before this
        // scan returns. A concurrent truncation could raise SIGBUS, but
        // index scanning does not run while the user is deleting files; the
        // heap-read fallback covers filesystems that reject mapping.
        if let Ok(map) = unsafe { Mmap::map(&file) } {
            return Ok(FileBytes::Mapped(map));
        }
    }
    let mut buf = Vec::with_capacity(len as usize);
    (&file).read_to_end(&mut buf)?;
    Ok(FileBytes::Owned(buf))
}

/// Below this size a file is copied to the heap instead of mapped.
const MMAP_MIN_BYTES: u64 = 256 * 1024;

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
    /// FQN → completion origin tier for classes, interfaces, traits, and
    /// enums. Populated from the package a file was collected under, so
    /// no second pass over the classmap is needed to classify origins.
    pub(crate) class_origins: HashMap<String, crate::ClassCompletionOrigin>,
    /// FQN → file path for standalone functions.
    pub function_index: HashMap<String, PathBuf>,
    /// FQN → completion origin tier for standalone functions.
    pub(crate) function_origins: HashMap<String, crate::ClassCompletionOrigin>,
    /// Constant name → file path for `define()` and top-level `const`.
    pub constant_index: HashMap<String, PathBuf>,
    /// Constant name → completion origin tier.
    pub(crate) constant_origins: HashMap<String, crate::ClassCompletionOrigin>,
    /// Vendor package roots discovered while parsing `installed.json`
    /// during this scan (path, origin, package name), sorted
    /// longest-path-first for prefix-match origin lookups. Empty for
    /// scans that never touch a vendor tree.
    pub(crate) package_roots: Vec<(PathBuf, crate::ClassCompletionOrigin, String)>,
}

impl WorkspaceScanResult {
    /// A result holding only `classmap`, with no origins.
    pub(crate) fn from_classmap(classmap: HashMap<String, PathBuf>) -> Self {
        Self {
            classmap,
            ..Default::default()
        }
    }

    /// The symbols one scanned file declares, all at `path`, with no
    /// origins.
    pub(crate) fn from_file(scan: ScanResult, path: &Path) -> Self {
        let at_path = |names: Vec<String>| {
            names
                .into_iter()
                .map(|name| (name, path.to_path_buf()))
                .collect()
        };
        Self {
            classmap: at_path(scan.classes),
            function_index: at_path(scan.functions),
            constant_index: at_path(scan.constants),
            ..Default::default()
        }
    }

    /// Fold `other`'s classes, functions and constants into `self`,
    /// keeping `self`'s entry when both define the same name.
    ///
    /// Origins ride alongside the winning path: one is only recorded for
    /// an entry of `other` that actually wins, so a symbol's recorded
    /// origin always matches the file it resolves to. `other`'s
    /// `package_roots` are dropped.
    pub(crate) fn absorb_missing(&mut self, other: WorkspaceScanResult) {
        fn absorb(
            into: &mut HashMap<String, PathBuf>,
            into_origins: &mut HashMap<String, crate::ClassCompletionOrigin>,
            from: HashMap<String, PathBuf>,
            from_origins: &HashMap<String, crate::ClassCompletionOrigin>,
        ) {
            for (name, path) in from {
                if let std::collections::hash_map::Entry::Vacant(e) = into.entry(name) {
                    if let Some(&origin) = from_origins.get(e.key()) {
                        into_origins.insert(e.key().clone(), origin);
                    }
                    e.insert(path);
                }
            }
        }
        absorb(
            &mut self.classmap,
            &mut self.class_origins,
            other.classmap,
            &other.class_origins,
        );
        absorb(
            &mut self.function_index,
            &mut self.function_origins,
            other.function_index,
            &other.function_origins,
        );
        absorb(
            &mut self.constant_index,
            &mut self.constant_origins,
            other.constant_index,
            &other.constant_origins,
        );
    }
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
