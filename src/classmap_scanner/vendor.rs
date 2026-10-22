//! Scanning the packages Composer installed, as `installed.json` lists
//! them: where each one lives, the autoload sources it declares, and the
//! completion-origin tier its symbols get.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use memchr::memmem;

use super::discovery::{progress_add_total, scan_files_parallel_full};
use super::filters::{FollowedLinks, IndexFilters};
use super::walk::{WalkOptions, walk_roots};
use super::{WorkspaceScanResult, read_for_scan};
use crate::progress::ScanProgress;

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
        &IndexFilters::empty(),
        None,
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
    let Some(installed) = crate::composer::read_installed_packages(workspace_root, vendor_dir)
    else {
        return Vec::new();
    };
    let mut roots = Vec::new();
    for package in &installed.packages {
        let Some(pkg_path) = installed.package_root(package) else {
            continue;
        };
        let pkg_name = package
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("unknown/unknown");
        let origin = classify_package_origin(pkg_name, explicit_deps);
        roots.push((pkg_path, origin, pkg_name.to_string()));
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
    /// [`scan_psr4_directories_with_skip`](super::scan_psr4_directories_with_skip) it has no use for the class
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
    installed: &crate::composer::InstalledPackages,
    package: &serde_json::Value,
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
    let Some(pkg_path) = installed.package_root(package) else {
        return out;
    };

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
    filters: &std::sync::Arc<IndexFilters>,
    progress: Option<&ScanProgress>,
    followed: Option<&FollowedLinks>,
) -> WorkspaceScanResult {
    let vendor_path = workspace_root.join(vendor_dir);

    let Some(installed) = crate::composer::read_installed_packages(workspace_root, vendor_dir)
    else {
        return WorkspaceScanResult::default();
    };
    let packages = installed.packages.as_slice();

    // Phase 1: read every package's autoload section and resolve the
    // paths it declares, without walking any of them.  Packages are
    // independent so this fans out over all cores, and the results come
    // back in `installed.json` order so a duplicate FQN resolves to the
    // same file a sequential scan would have picked.
    let mut collected: Vec<PackageSources> =
        crate::parallel::map_in_blocks("vendor-packages", packages, 1, |package| {
            let sources = collect_package_sources(&installed, package, skip_paths, explicit_deps);
            (!sources.psr4.is_empty() || !sources.plain.is_empty() || sources.root.is_some())
                .then_some(sources)
        });

    let mut package_roots: Vec<(PathBuf, crate::ClassCompletionOrigin, String)> = collected
        .iter_mut()
        .filter_map(|sources| sources.root.take())
        .collect();
    // Match `vendor_package_roots`'s ordering: longest path first, so a
    // nested package's root wins a prefix match before its parent's.
    package_roots.sort_by_key(|(p, _, _)| std::cmp::Reverse(p.components().count()));

    // Phase 2: walk every collected directory in one parallel pass, so
    // the cores are shared across all packages instead of one thread per
    // package.  The roots are laid out PSR-4 first and classmap/`files`
    // second, matching the order phase 3 concatenates them in.
    let opts = WalkOptions::new(
        vec![vendor_path.clone()],
        skip_paths,
        std::sync::Arc::clone(filters),
        followed,
    );
    let mut roots: Vec<PathBuf> = Vec::new();
    for sources in &collected {
        roots.extend(sources.psr4.iter().cloned());
    }
    for sources in &collected {
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
    for sources in &collected {
        for _ in &sources.psr4 {
            let files = std::mem::take(&mut walked[next_root]);
            all_files.extend(files.into_iter().map(|path| (path, sources.origin)));
            next_root += 1;
        }
    }
    for sources in &collected {
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

#[cfg(test)]
#[path = "vendor_tests.rs"]
mod tests;
