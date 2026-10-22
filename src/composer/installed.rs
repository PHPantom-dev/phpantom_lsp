//! `vendor/composer/installed.json`: the packages Composer installed and
//! where each one lives on disk.

use std::path::{Path, PathBuf};

use super::psr4::{Psr4Mapping, normalise_prefix};

/// The package list from `vendor/composer/installed.json`, together with
/// the directories its paths are relative to.
pub(crate) struct InstalledPackages {
    /// Every entry of the package list, in `installed.json` order.
    pub(crate) packages: Vec<serde_json::Value>,
    /// `<vendor>/composer`, the directory each `install-path` is relative
    /// to.
    composer_dir: PathBuf,
    /// `<vendor>`, where a package without an `install-path` lives under
    /// its own name.
    vendor_path: PathBuf,
}

impl InstalledPackages {
    /// Where `package` is installed: its `install-path`, which Composer 2
    /// writes relative to `vendor/composer` and which also covers path
    /// repositories and custom installers, else `vendor/<name>`, the
    /// Composer 1 layout.  Canonicalized, so a symlinked package resolves
    /// to the directory it points at.
    ///
    /// `None` when the entry names neither, or nothing is installed there
    /// (a package `composer install` has not fetched yet).
    pub(crate) fn package_root(&self, package: &serde_json::Value) -> Option<PathBuf> {
        let root = match package.get("install-path").and_then(|p| p.as_str()) {
            Some(install_path) => self.composer_dir.join(install_path),
            None => self.vendor_path.join(package.get("name")?.as_str()?),
        };
        let root = root.canonicalize().unwrap_or(root);
        root.is_dir().then_some(root)
    }
}

/// Parse the package list out of `installed.json` text.
///
/// The file has two formats: Composer 1 writes a top-level array of
/// packages, Composer 2 writes `{ "packages": [...] }`. Returns an empty
/// list for anything else, and logs unparseable JSON.
pub(crate) fn parse_installed_packages(content: &str) -> Vec<serde_json::Value> {
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(serde_json::Value::Array(packages)) => packages,
        Ok(serde_json::Value::Object(mut root)) => match root.remove("packages") {
            Some(serde_json::Value::Array(packages)) => packages,
            _ => Vec::new(),
        },
        Ok(_) => Vec::new(),
        Err(e) => {
            tracing::warn!("Failed to parse Composer's installed.json: {e}");
            Vec::new()
        }
    }
}

/// Read and parse `<workspace_root>/<vendor_dir>/composer/installed.json`.
///
/// Returns `None` when the file is missing or unreadable; see
/// [`parse_installed_packages`] for the formats it accepts.
pub(crate) fn read_installed_packages(
    workspace_root: &Path,
    vendor_dir: &str,
) -> Option<InstalledPackages> {
    let vendor_path = workspace_root.join(vendor_dir);
    let composer_dir = vendor_path.join("composer");
    let content = super::read_composer_file(&composer_dir.join("installed.json"))?;
    Some(InstalledPackages {
        packages: parse_installed_packages(&content),
        composer_dir,
        vendor_path,
    })
}

/// Extract PSR-4 mappings from path-repository packages in `installed.json`.
///
/// Path-repository packages (Composer `"type": "path"` repositories) are
/// local packages symlinked into `vendor/`.  They are project code that
/// happens to be organized as Composer packages — e.g. the `internachi/modular`
/// pattern where each module lives under `app-modules/` or similar and has
/// its own `composer.json` with PSR-4 mappings.
///
/// A package is included only when **all three** conditions are met:
///
/// 1. `dist.type == "path"` in `installed.json`.
/// 2. `transport-options.symlink == true` — the package is symlinked into
///    vendor, not copied.  A copied path-repo is effectively a snapshot
///    and should be treated as a regular vendor package.
/// 3. The canonicalized (symlink-resolved) package root is a subdirectory
///    of the workspace root but is **not** inside `vendor/`.  A symlink
///    pointing outside the project (e.g. a shared library on disk) is not
///    project code; neither is a package whose canonical path resolves
///    back inside `vendor/` — that is an ordinary vendored dependency,
///    already covered by the vendor classmap and service-provider scan,
///    and must not be walked by the diagnostics pass as user source.
///
/// Their `autoload.psr-4` entries and `install-path` fields are combined
/// to produce absolute directory mappings that can be appended to the
/// project's own `psr4_mappings`.
///
/// Returns an empty `Vec` if `installed.json` does not exist, cannot be
/// parsed, or contains no qualifying path-repository packages.
pub fn extract_path_repo_psr4_mappings(
    workspace_root: &Path,
    vendor_dir: &str,
) -> Vec<Psr4Mapping> {
    let Some(installed) = read_installed_packages(workspace_root, vendor_dir) else {
        return Vec::new();
    };

    let canonical_root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let canonical_vendor = installed
        .vendor_path
        .canonicalize()
        .unwrap_or_else(|_| installed.vendor_path.clone());
    let mut mappings = Vec::new();

    for package in &installed.packages {
        // Condition 1: dist.type must be "path".
        let is_path_repo = package
            .get("dist")
            .and_then(|d| d.get("type"))
            .and_then(|t| t.as_str())
            == Some("path");
        if !is_path_repo {
            continue;
        }

        // Condition 2: must be symlinked, not copied.
        let is_symlinked = package
            .get("transport-options")
            .and_then(|t| t.get("symlink"))
            .and_then(|s| s.as_bool())
            == Some(true);
        if !is_symlinked {
            continue;
        }

        // Canonical, so the symlink leads to the real location.
        let Some(pkg_root) = installed.package_root(package) else {
            continue;
        };

        // Condition 3: the resolved path must be inside the workspace but
        // NOT inside the vendor directory.  Path repositories are project
        // code only when they are symlinked *out* of vendor (e.g. the
        // internachi/modular pattern, where modules live under
        // `app-modules/` and are symlinked into vendor).  A package whose
        // canonical path resolves back inside `vendor/` — because the
        // symlink target is itself in vendor, or the symlink was
        // materialised into a real copy — is an ordinary vendored
        // dependency.  It is already covered by the vendor classmap for
        // resolution and by the service-provider scan for macros, so
        // adding its PSR-4 mapping here would only cause the diagnostics
        // pass to walk and analyse the whole dependency as if it were the
        // user's own source.
        if !pkg_root.starts_with(&canonical_root) || pkg_root.starts_with(&canonical_vendor) {
            continue;
        }

        let Some(psr4) = package
            .get("autoload")
            .and_then(|a| a.get("psr-4"))
            .and_then(|p| p.as_object())
        else {
            continue;
        };

        for (prefix, paths) in psr4 {
            // The prefix should end with `\` per PSR-4 convention.
            let normalised = normalise_prefix(prefix);

            // paths can be a string or an array of strings.
            let dirs: Vec<&str> = if let Some(s) = paths.as_str() {
                vec![s]
            } else if let Some(arr) = paths.as_array() {
                arr.iter().filter_map(|v| v.as_str()).collect()
            } else {
                continue;
            };

            for dir in dirs {
                let abs_path = pkg_root.join(dir);
                if abs_path.is_dir() {
                    mappings.push(Psr4Mapping {
                        prefix: normalised.clone(),
                        base_path: abs_path.to_string_lossy().into_owned(),
                    });
                }
            }
        }
    }

    // Sort by prefix length descending (longest-prefix-first matching).
    mappings.sort_by_key(|m| std::cmp::Reverse(m.prefix.len()));

    mappings
}
