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
//! - [`discovery`] — directory walking, PSR-4/vendor package
//!   discovery, and parallel batch scanning built on top of the
//!   lexer.

use std::collections::HashMap;
use std::ops::Deref;
use std::path::{Path, PathBuf};

use memmap2::Mmap;

mod discovery;
mod lexer;

pub(crate) use discovery::vendor_package_roots;
pub use discovery::{
    scan_directories, scan_drupal_directories, scan_psr4_directories,
    scan_psr4_directories_with_skip, scan_vendor_packages, scan_vendor_packages_with_skip,
    scan_workspace_fallback, scan_workspace_fallback_full,
};
pub use lexer::{find_classes, find_symbols};

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
