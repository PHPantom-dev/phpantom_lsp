//! Composer support: reading `composer.json` and the files Composer
//! generates, and resolving class names to files through the project's
//! autoload rules.
//!
//! - [`psr4`]: the project's PSR-4 mappings, and resolving a class or a
//!   namespace to the file or directory they place it in (and back).
//! - [`installed`]: `vendor/composer/installed.json`, the installed packages
//!   and where each one lives on disk.
//! - [`autoload_files`]: the autoload maps Composer generates under
//!   `vendor/composer/`, and the files they pull in.
//! - [`project`]: what kind of project a `composer.json` describes (PHP
//!   version, Laravel, Drupal, permission packages).
//!
//! # PSR-4 Resolution
//!
//! Given a mapping like `"Klarna\\" => "src/Klarna/"`, a class name like
//! `Klarna\Customer` is resolved by:
//!   1. Stripping the matching prefix (`Klarna\`) from the class name
//!   2. Converting remaining namespace separators to directory separators
//!   3. Appending `.php`
//!   4. Prepending the mapped base directory
//!
//! Result: `<workspace>/src/Klarna/Customer.php`
//!
//! Composer JSON parsing is delegated to the [`mago_composer`] crate,
//! which provides typed Rust structs for the full `composer.json` schema.

use std::fs;
use std::path::{Path, PathBuf};

pub use mago_composer::ComposerPackage;

mod autoload_files;
mod installed;
mod project;
mod psr4;

pub use autoload_files::{
    detect_phar_references, discover_global_sibling_files, extract_require_once_paths,
    parse_autoload_classmap, parse_autoload_files, parse_autoload_namespaces,
    scan_composer_bootstrap_classes,
};
pub use installed::extract_path_repo_psr4_mappings;
pub(crate) use installed::{
    InstalledPackages, linked_package_roots, parse_installed_packages, path_repo_psr4_mappings,
    read_installed_packages, stub_package_roots,
};
pub(crate) use project::{
    detect_drupal_web_root, explicit_dependency_names, has_dependency, has_laravel_aware_phpstan,
    has_require_dev, has_runtime_permission_package, is_application_project, is_doctrine_project,
    is_laravel_application, is_laravel_project, is_symfony_project, is_twig_project,
};
pub use project::{detect_php_version, detect_php_version_from_package};
pub use psr4::{
    Psr4Mapping, ScanDirs, extract_psr4_mappings_from_package, extract_scan_dirs, normalise_path,
    resolve_class_path, resolve_namespace_from_path,
};
pub(crate) use psr4::{
    psr4_class_files, psr4_directories_for_namespace, psr4_directory_for_namespace,
    psr4_path_for_class,
};

/// Parse a `composer.json` file at the given workspace root and extract all
/// PSR-4 autoload mappings from both `autoload` and `autoload-dev` sections.
///
/// Only the project's own mappings are returned.  Vendor package mappings
/// (from `vendor/composer/autoload_psr4.php`) are deliberately not loaded —
/// the Composer classmap is the sole source of truth for vendor code.  If the
/// classmap is missing or stale, vendor classes will fail to resolve visibly
/// instead of being silently papered over by PSR-4, making the problem
/// obvious to the user (fix: run `composer dump-autoload`).
///
/// Returns `(mappings, vendor_dir)` where `vendor_dir` is the configured
/// `config.vendor-dir` (defaulting to `"vendor"`).  Both values come from
/// the same parse of `composer.json` so the file is only read once.
///
/// Returns an empty `Vec` and `"vendor"` if the file doesn't exist, can't
/// be read, or contains no PSR-4 mappings.
pub fn parse_composer_json(workspace_root: &Path) -> (Vec<Psr4Mapping>, String) {
    let package = match read_composer_package(workspace_root) {
        Some(p) => p,
        None => return (Vec::new(), "vendor".to_string()),
    };

    let vendor_dir = get_vendor_dir(&package);
    let mappings = extract_psr4_mappings_from_package(&package);

    (mappings, vendor_dir)
}

/// Read and parse the `composer.json` at the given workspace root into a
/// typed [`ComposerPackage`].
///
/// Returns `None` if the file does not exist, cannot be read, or fails
/// to parse.  Startup reads it through [`try_read_composer_package`]
/// instead, to tell the user why their autoload mappings are missing.
pub fn read_composer_package(workspace_root: &Path) -> Option<ComposerPackage> {
    try_read_composer_package(workspace_root).ok().flatten()
}

/// [`read_composer_package`] that reports a file it cannot read or parse.
/// `Ok(None)` means the workspace has no `composer.json`.
pub fn try_read_composer_package(
    workspace_root: &Path,
) -> Result<Option<ComposerPackage>, ComposerReadError> {
    let composer_path = workspace_root.join("composer.json");
    let content = match fs::read_to_string(&composer_path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(ComposerReadError {
                path: composer_path,
                reason: e.to_string(),
            });
        }
    };
    content
        .parse::<ComposerPackage>()
        .map(Some)
        .map_err(|e| ComposerReadError {
            path: composer_path,
            reason: e.to_string(),
        })
}

/// Read one of Composer's generated files, `None` when it is missing.  A
/// file that exists but cannot be read is logged rather than passed off
/// as missing.
pub(crate) fn read_composer_file(path: &Path) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(content) => Some(content),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            tracing::warn!("Failed to read {}: {e}", path.display());
            None
        }
    }
}

/// A Composer file that exists but could not be read or parsed.
#[derive(Debug)]
pub struct ComposerReadError {
    pub path: PathBuf,
    pub reason: String,
}

impl std::fmt::Display for ComposerReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "failed to read {}: {}", self.path.display(), self.reason)
    }
}

/// Read the configured vendor directory from a parsed [`ComposerPackage`].
///
/// Looks at `config.vendor-dir`; defaults to `"vendor"` when absent.
pub(crate) fn get_vendor_dir(package: &ComposerPackage) -> String {
    package
        .config
        .as_ref()
        .and_then(|c| c.vendor_dir.as_deref())
        .map(|s| s.trim_end_matches('/').to_string())
        .unwrap_or_else(|| "vendor".to_string())
}

/// Read the configured bin directory from a parsed [`ComposerPackage`].
///
/// Looks at `config.bin-dir`; defaults to `"<vendor-dir>/bin"` when absent,
/// where `<vendor-dir>` is itself resolved via [`get_vendor_dir`].
pub(crate) fn get_bin_dir(package: &ComposerPackage) -> String {
    if let Some(explicit) = package.config.as_ref().and_then(|c| c.bin_dir.as_deref()) {
        explicit.trim_end_matches('/').to_string()
    } else {
        format!("{}/bin", get_vendor_dir(package))
    }
}

/// Discover PHP subproject roots inside a workspace directory.
///
/// When the workspace root itself has no `composer.json`, this function
/// walks subdirectories looking for `composer.json` files using the
/// `ignore` crate for gitignore-aware walking.  Each directory that
/// contains a `composer.json` is returned as a subproject root paired
/// with its configured vendor directory name.  Once a `composer.json`
/// is found in a directory, its children are not descended into (they
/// belong to that project's source tree, not to a separate subproject).
///
/// The walk respects `.gitignore` at every level, so vendor directories
/// that are gitignored (the common case in monorepos) are automatically
/// skipped along with any other noise the user has excluded.  Hidden
/// directories (starting with `.`) are also skipped.
///
/// This enables monorepo support: a workspace like
///
/// ```text
/// monorepo/               ← workspace root (no composer.json)
/// ├── project-a/
/// │   ├── composer.json   ← discovered
/// │   ├── src/
/// │   └── vendor/
/// └── packages/
///     └── project-b/
///         ├── composer.json  ← discovered (depth 2)
///         └── src/
/// ```
///
/// returns `[(monorepo/project-a, "vendor"), (monorepo/packages/project-b, "vendor")]`.
pub fn discover_subproject_roots(workspace_root: &Path) -> Vec<(PathBuf, String)> {
    outermost_with_vendor_dirs(composer_json_dirs(workspace_root, None))
}

/// The Composer projects nested inside a workspace that has a
/// `composer.json` of its own: directories below `workspace_root` whose
/// `composer.json` has its own dependencies installed, paired with their
/// vendor directory name.
///
/// A nested project with its own `vendor/` is one the root's autoloader
/// knows nothing about, which is what makes it a project to index rather
/// than a package the root installs from a path repository or a fixture
/// a test suite keeps beside its sources: those have no `vendor/` of their
/// own.  `root_vendor` is the root project's vendor directory, which is
/// not walked: every installed package carries a `composer.json`.
pub fn discover_installed_subprojects(
    workspace_root: &Path,
    root_vendor: &Path,
) -> Vec<(PathBuf, String)> {
    let installed = composer_json_dirs(workspace_root, Some(root_vendor))
        .into_iter()
        .filter(|dir| {
            let vendor_dir = read_composer_package(dir)
                .map(|pkg| get_vendor_dir(&pkg))
                .unwrap_or_else(|| "vendor".to_string());
            dir.join(vendor_dir)
                .join("composer/installed.json")
                .is_file()
        })
        .collect();
    outermost_with_vendor_dirs(installed)
}

/// The directories below `workspace_root` holding a `composer.json`,
/// shallowest first, found by a gitignore-aware walk that skips hidden
/// directories and `skip`.
fn composer_json_dirs(workspace_root: &Path, skip: Option<&Path>) -> Vec<PathBuf> {
    use ignore::WalkBuilder;

    let skip = skip.map(Path::to_path_buf);
    let walker = WalkBuilder::new(workspace_root)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .hidden(true)
        .parents(true)
        .ignore(true)
        // Sort to get deterministic output
        .sort_by_file_name(|a, b| a.cmp(b))
        .filter_entry(move |entry| skip.as_deref() != Some(entry.path()))
        .build();

    let mut candidates: Vec<PathBuf> = Vec::new();
    for entry in walker.flatten() {
        let path = entry.path();
        // We only care about files named "composer.json"
        if !path.is_file() {
            continue;
        }
        if path.file_name().and_then(|n| n.to_str()) != Some("composer.json") {
            continue;
        }
        // The subproject root is the parent directory of composer.json
        if let Some(parent) = path.parent() {
            // Skip the workspace root itself — the caller already checked
            // that it has no composer.json (or has one and took the
            // single-project path).
            if parent == workspace_root {
                continue;
            }
            candidates.push(parent.to_path_buf());
        }
    }

    // Sort by path length so shallower paths come first, which is what
    // lets the caller keep only the outermost of two nested roots.
    candidates.sort_by_key(|p| p.as_os_str().len());
    candidates
}

/// Keep only the outermost of `candidates` (shallowest first): if both
/// `packages/foo` and `packages/foo/subdir` contain composer.json, the
/// inner one belongs to the outer project's source tree.  Each kept root
/// is paired with the vendor dir name its `composer.json` configures.
fn outermost_with_vendor_dirs(candidates: Vec<PathBuf>) -> Vec<(PathBuf, String)> {
    let mut roots: Vec<(PathBuf, String)> = Vec::new();
    for candidate in candidates {
        if roots.iter().any(|(root, _)| candidate.starts_with(root)) {
            continue;
        }
        let vendor_dir = read_composer_package(&candidate)
            .map(|pkg| get_vendor_dir(&pkg))
            .unwrap_or_else(|| "vendor".to_string());
        roots.push((candidate, vendor_dir));
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: parse a JSON string into a [`ComposerPackage`].
    fn pkg(json: &str) -> ComposerPackage {
        json.parse::<ComposerPackage>().unwrap()
    }

    // ── try_read_composer_package ───────────────────────────────────

    #[test]
    fn a_missing_composer_json_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(try_read_composer_package(dir.path()), Ok(None)));
    }

    #[test]
    fn a_malformed_composer_json_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("composer.json"), "{ \"require\": ").unwrap();
        let err = try_read_composer_package(dir.path()).unwrap_err();
        assert!(err.to_string().contains("composer.json"), "{err}");
        assert!(read_composer_package(dir.path()).is_none());
    }

    // ── get_vendor_dir ──────────────────────────────────────────────

    #[test]
    fn vendor_dir_default() {
        assert_eq!(get_vendor_dir(&pkg("{}")), "vendor");
    }

    #[test]
    fn vendor_dir_explicit() {
        let p = pkg(r#"{"config": {"vendor-dir": "lib"}}"#);
        assert_eq!(get_vendor_dir(&p), "lib");
    }

    #[test]
    fn vendor_dir_strips_trailing_slash() {
        let p = pkg(r#"{"config": {"vendor-dir": "lib/"}}"#);
        assert_eq!(get_vendor_dir(&p), "lib");
    }

    // ── get_bin_dir ─────────────────────────────────────────────────

    #[test]
    fn bin_dir_default_follows_vendor_dir() {
        assert_eq!(get_bin_dir(&pkg("{}")), "vendor/bin");
    }

    #[test]
    fn bin_dir_default_with_custom_vendor_dir() {
        let p = pkg(r#"{"config": {"vendor-dir": "lib"}}"#);
        assert_eq!(get_bin_dir(&p), "lib/bin");
    }

    #[test]
    fn bin_dir_explicit() {
        let p = pkg(r#"{"config": {"bin-dir": "bin"}}"#);
        assert_eq!(get_bin_dir(&p), "bin");
    }

    #[test]
    fn bin_dir_explicit_overrides_vendor_dir() {
        let p = pkg(r#"{"config": {"vendor-dir": "lib", "bin-dir": "tools/bin"}}"#);
        assert_eq!(get_bin_dir(&p), "tools/bin");
    }

    #[test]
    fn bin_dir_strips_trailing_slash() {
        let p = pkg(r#"{"config": {"bin-dir": "bin/"}}"#);
        assert_eq!(get_bin_dir(&p), "bin");
    }
}
