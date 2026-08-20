//! Workspace initialization pipelines.
//!
//! Given a resolved workspace root, these `impl Backend` methods build
//! the initial symbol indexes for the three workspace shapes (single
//! Composer project, monorepo, and no-Composer).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use tower_lsp::lsp_types::*;

use super::classify_class_origin;
use crate::Backend;
use crate::classmap_scanner;
use crate::composer;
use crate::config::IndexingStrategy;

impl Backend {
    /// Initialize a single-project workspace (root `composer.json` exists).
    ///
    /// This is the standard fast path: read PSR-4 mappings, build the
    /// classmap, scan autoload files.
    pub(crate) async fn init_single_project(
        &self,
        root: &std::path::Path,
        php_version: crate::types::PhpVersion,
        composer_json: Option<composer::ComposerPackage>,
        progress: Option<&crate::progress::ScanProgress>,
    ) {
        if let Some(p) = progress {
            p.set_percentage(10, "Reading composer.json");
        }

        // Classify the project so Laravel-specific resolution (Eloquent
        // members, config/view/route keys, contract bindings, patches) is
        // skipped when no Laravel/Illuminate dependency is present.
        let is_laravel = composer_json
            .as_ref()
            .map(composer::is_laravel_project)
            .unwrap_or(false);
        self.resolved_class_cache.write().set_laravel(is_laravel);

        // A permission package answers authorization checks from the database,
        // so the abilities this project uses are not written in its source and
        // the unknown-ability diagnostic has nothing to judge them against.
        let runtime_permissions = composer_json
            .as_ref()
            .map(composer::has_runtime_permission_package)
            .unwrap_or(false);
        self.laravel_gates
            .write()
            .set_runtime_permission_package(runtime_permissions);

        let (mappings, vendor_dir) = match &composer_json {
            Some(pkg) => {
                let mappings = composer::extract_psr4_mappings_from_package(pkg);
                let vendor_dir = composer::get_vendor_dir(pkg);
                (mappings, vendor_dir)
            }
            None => (Vec::new(), "vendor".to_string()),
        };

        // Cache the vendor dir path so cross-file scans can skip it
        // without re-reading composer.json on every request.
        let vendor_path = root.join(&vendor_dir);
        self.add_vendor_dir(&vendor_path);

        // Include PSR-4 mappings from path-repository packages (local
        // packages symlinked into vendor/, e.g. internachi/modular modules).
        let path_repo_mappings = composer::extract_path_repo_psr4_mappings(root, &vendor_dir);
        let mut all_mappings = mappings;
        all_mappings.extend(path_repo_mappings);
        // Keep the merged list longest-prefix-first so path-repo namespaces
        // are matched before any shorter root prefix (e.g. an empty-prefix
        // root fallback).
        all_mappings.sort_by_key(|m| std::cmp::Reverse(m.prefix.len()));
        *self.workspace.psr4_mappings.write() = all_mappings;

        // ── Build the classmap ──────────────────────────────────────
        let strategy = self.config().indexing.strategy();

        // The classmap build owns the 15..70 range of the bar; the
        // scan helpers divide it into contiguous per-phase windows
        // with per-file counts.
        if let Some(p) = progress {
            p.set_scope(15, 70, "Building class index");
        }

        let explicit_deps = composer_json
            .as_ref()
            .map(crate::composer::explicit_dependency_names)
            .unwrap_or_default();

        let (classmap, source_label, class_origins, package_roots) = match strategy {
            IndexingStrategy::None => {
                let cm = composer::parse_autoload_classmap(root, &vendor_dir);
                let roots =
                    classmap_scanner::vendor_package_roots(root, &vendor_dir, &explicit_deps);
                (cm, "composer", HashMap::new(), roots)
            }
            IndexingStrategy::SelfScan | IndexingStrategy::Full => {
                // "self" strategy: scan every PHP file under the
                // workspace root (ignoring .gitignore, hidden dirs,
                // etc.) to discover all classes, functions, and
                // constants — regardless of whether they appear in
                // composer.json's autoload sections.
                //
                // Explicitly skip the vendor directory so it is never
                // walked even when it is not in .gitignore.  Vendor
                // packages are scanned separately via installed.json
                // so that third-party classes are still indexed.
                let mut skip_dirs = HashSet::new();
                skip_dirs.insert(vendor_path.clone());
                if let Some(p) = progress {
                    p.begin_phase(0.0, 0.3, "Scanning workspace files");
                }
                let mut scan = classmap_scanner::scan_workspace_fallback_full(
                    root,
                    &skip_dirs,
                    progress,
                    self.config().indexing.follow_links(),
                );

                // Merge vendor packages (excluded from the workspace
                // walk above, scanned separately here).
                if let Some(p) = progress {
                    p.begin_phase(0.3, 1.0, "Scanning vendor packages");
                }
                let mut vendor_scan = classmap_scanner::scan_vendor_packages_with_skip(
                    root,
                    &vendor_dir,
                    &HashSet::new(),
                    &explicit_deps,
                    progress,
                    self.config().indexing.follow_links(),
                );
                let package_roots = std::mem::take(&mut vendor_scan.package_roots);

                // Origins ride alongside the winning path: only record
                // one for a vendor entry when it actually wins the merge
                // (the workspace scan didn't already provide this FQN),
                // so a symbol's recorded origin always matches the file
                // it resolves to.
                for (fqcn, path) in vendor_scan.classmap {
                    if let std::collections::hash_map::Entry::Vacant(e) =
                        scan.classmap.entry(fqcn.clone())
                    {
                        e.insert(path);
                        if let Some(&origin) = vendor_scan.class_origins.get(&fqcn) {
                            scan.class_origins.insert(fqcn, origin);
                        }
                    }
                }
                for (fqn, path) in vendor_scan.function_index {
                    if let std::collections::hash_map::Entry::Vacant(e) =
                        scan.function_index.entry(fqn.clone())
                    {
                        e.insert(path);
                        if let Some(&origin) = vendor_scan.function_origins.get(&fqn) {
                            scan.function_origins.insert(fqn, origin);
                        }
                    }
                }
                for (name, path) in vendor_scan.constant_index {
                    if let std::collections::hash_map::Entry::Vacant(e) =
                        scan.constant_index.entry(name.clone())
                    {
                        e.insert(path);
                        if let Some(&origin) = vendor_scan.constant_origins.get(&name) {
                            scan.constant_origins.insert(name, origin);
                        }
                    }
                }

                self.populate_autoload_indices(&scan);
                (
                    scan.classmap,
                    "self-scan",
                    scan.class_origins,
                    package_roots,
                )
            }
            IndexingStrategy::Composer => {
                // ── Merged classmap + self-scan pipeline ─────────────
                let composer_cm = composer::parse_autoload_classmap(root, &vendor_dir);
                let skip_paths: HashSet<PathBuf> = composer_cm.values().cloned().collect();
                let scan = self.build_self_scan_composer(
                    root,
                    &vendor_dir,
                    composer_json.as_ref(),
                    &skip_paths,
                    progress,
                );
                self.populate_autoload_indices(&scan);
                let mut class_origins = scan.class_origins;
                let mut merged = composer_cm;
                for (fqcn, path) in scan.classmap {
                    match merged.entry(fqcn.clone()) {
                        std::collections::hash_map::Entry::Vacant(e) => {
                            e.insert(path);
                        }
                        std::collections::hash_map::Entry::Occupied(_) => {
                            // The composer classmap already provides this
                            // FQN and wins the merge below, so classify
                            // its origin from the path prefix like the
                            // rest of the composer-only entries instead
                            // of keeping the scan's (losing) origin.
                            class_origins.remove(&fqcn);
                        }
                    }
                }
                (merged, "composer+scan", class_origins, scan.package_roots)
            }
        };

        let class_entries: Vec<(String, PathBuf)> = classmap.into_iter().collect();
        let symbol_count = class_entries.len();
        {
            let mut idx = self.symbols.fqn_uri_index.write();
            let mut origins = self.symbols.fqn_origin_index.write();
            origins.clear();
            for (fqn, path) in class_entries {
                let origin = class_origins
                    .get(&fqn)
                    .copied()
                    .unwrap_or_else(|| classify_class_origin(&path, &vendor_path, &package_roots));
                origins.insert(fqn.clone(), origin);
                idx.or_insert_with(fqn, || crate::util::path_to_uri(&path));
            }
        }
        // Cache the package roots so path-based origin lookups
        // (functions, constants) can classify lazily parsed symbols.
        *self.workspace.vendor_package_origin_roots.write() = package_roots;

        // ── Drupal: scan web-root directories (gitignore bypassed) ──
        // Drupal's .gitignore excludes web/core, web/modules/contrib,
        // etc. because they are managed by Composer — but those paths
        // contain every base interface and hook definition that modules
        // depend on.  detect_drupal_web_root() returns None for
        // non-Drupal projects so this block is a no-op in that case.
        if let Some(ref pkg) = composer_json
            && let Some(drupal_web_root) = composer::detect_drupal_web_root(root, pkg)
        {
            if let Some(p) = progress {
                p.set_scope(70, 74, "Scanning Drupal directories");
            }
            let drupal_result =
                classmap_scanner::scan_drupal_directories(&drupal_web_root, progress);
            let drupal_count = drupal_result.classmap.len()
                + drupal_result.function_index.len()
                + drupal_result.constant_index.len();
            {
                let mut idx = self.symbols.fqn_uri_index.write();
                for (fqn, path) in drupal_result.classmap {
                    idx.or_insert_with(fqn, || crate::util::path_to_uri(&path));
                }
            }
            {
                let mut fi = self.symbols.autoload_function_index.write();
                for (fqn, path) in drupal_result.function_index {
                    fi.or_insert_with(fqn, || path);
                }
            }
            {
                let mut ci = self.symbols.autoload_constant_index.write();
                for (name, path) in drupal_result.constant_index {
                    ci.entry(name).or_insert(path);
                }
            }
            tracing::info!(
                "PHPantom: Drupal web root {:?}, {} symbols indexed",
                drupal_web_root,
                drupal_count
            );
        }

        // ── PSR-0 (legacy) classmap ─────────────────────────────────
        // Packages that declare `autoload.psr-0` in their composer.json
        // (e.g. HTMLPurifier) are listed in `autoload_namespaces.php`.
        // Scan the listed directories and merge discovered classes into
        // the classmap so they are resolvable via `find_or_load_class`.
        let psr0_cm = composer::parse_autoload_namespaces(root, &vendor_dir);
        if !psr0_cm.is_empty() {
            let count = psr0_cm.len();
            let mut idx = self.symbols.fqn_uri_index.write();
            for (fqn, path) in psr0_cm {
                idx.or_insert_with(fqn, || crate::util::path_to_uri(&path));
            }
            tracing::info!("PSR-0: {} classes from autoload_namespaces.php", count);
        }

        // ── Autoload files ──────────────────────────────────────────
        if let Some(p) = progress {
            p.set_scope(74, 85, "Scanning autoload files");
        }

        self.scan_autoload_files(root, &vendor_dir, progress);

        let symbol_count = symbol_count
            + self.symbols.autoload_function_index.read().len()
            + self.symbols.autoload_constant_index.read().len();

        self.log(
            MessageType::INFO,
            format!(
                "PHPantom v{}: PHP {}, {} symbols from {}, stubs {}",
                self.version,
                php_version,
                symbol_count,
                source_label,
                crate::stubs::STUBS_VERSION
            ),
        )
        .await;

        // When symlink-following is enabled, tell the user it is active so
        // a configuration mistake (or a deliberate `git pull` into a linked
        // framework) is diagnosable from the log alone.
        if self.config().indexing.follow_links() && has_interior_symlink(root) {
            self.log(
                MessageType::INFO,
                "PHPantom: symlink-following enabled — indexing directories reached through workspace symlinks. Changes inside a linked target on disk do not trigger a re-index; reload the window after a `git pull` or similar.".to_string(),
            )
            .await;
        }
    }

    /// Initialize a monorepo workspace (no root `composer.json`, but
    /// subprojects with their own `composer.json` were discovered).
    ///
    /// Each subproject is processed through the Composer pipeline (PSR-4,
    /// classmap, autoload files, vendor packages).  After all subprojects
    /// are processed, a gitignore-aware full-scan picks up loose PHP files
    /// outside any subproject directory.
    pub(crate) async fn init_monorepo(
        &self,
        root: &std::path::Path,
        subprojects: &[(PathBuf, String)],
        php_version: crate::types::PhpVersion,
        progress: Option<&crate::progress::ScanProgress>,
    ) {
        // Log the discovered subprojects.
        let sub_list: Vec<String> = subprojects
            .iter()
            .filter_map(|(p, _)| {
                p.strip_prefix(root)
                    .ok()
                    .map(|r| format!("  {}", r.display()))
            })
            .collect();
        self.log(
            MessageType::INFO,
            format!(
                "PHPantom: No root composer.json. Found {} Composer project(s):\n{}",
                subprojects.len(),
                sub_list.join("\n")
            ),
        )
        .await;

        // Collect subproject root paths for the skip set.
        let mut skip_dirs: HashSet<PathBuf> = HashSet::new();
        let sub_count = subprojects.len();

        // The workspace is treated as Laravel when any subproject depends on
        // Laravel/Illuminate, so Laravel-specific resolution runs there while
        // pure non-Laravel workspaces skip it.
        let mut any_laravel = false;
        // Likewise for a runtime-permission dependency: one subproject
        // authorizing from the database opens the ability space workspace-wide,
        // since the gate index that judges abilities is shared.
        let mut any_runtime_permissions = false;

        for (sub_idx, (sub_root, vendor_dir)) in subprojects.iter().enumerate() {
            // Each subproject owns an equal slice of the 10..80 range;
            // the loose-file scan gets 80..85.  Every report inside the
            // slice carries the subproject prefix, and the scan helpers
            // divide the slice into per-phase windows with file counts.
            let sub_lo = 10 + (sub_idx as u32 * 70) / sub_count.max(1) as u32;
            let sub_hi = 10 + ((sub_idx as u32 + 1) * 70) / sub_count.max(1) as u32;
            // The autoload byte-scan is a small fraction of a
            // subproject's work; the classmap build dominates.
            let sub_mid = sub_lo + (sub_hi - sub_lo) / 5;
            if let Some(p) = progress {
                let label = sub_root
                    .strip_prefix(root)
                    .unwrap_or(sub_root)
                    .display()
                    .to_string();
                p.set_label_prefix(format!(
                    "Subproject {} / {}: {}",
                    sub_idx + 1,
                    sub_count,
                    label
                ));
                p.set_scope(sub_lo, sub_mid, "Scanning autoload files");
            }
            skip_dirs.insert(sub_root.clone());

            if (!any_laravel || !any_runtime_permissions)
                && let Some(pkg) = composer::read_composer_package(sub_root)
            {
                any_laravel |= composer::is_laravel_project(&pkg);
                any_runtime_permissions |= composer::has_runtime_permission_package(&pkg);
            }

            // ── PSR-4 mappings ──────────────────────────────────────
            let (mappings, _) = composer::parse_composer_json(sub_root);

            // Resolve base_path values to absolute paths so that
            // resolve_class_path works regardless of workspace_root.
            let abs_mappings: Vec<composer::Psr4Mapping> = mappings
                .into_iter()
                .map(|m| {
                    let abs_base = sub_root.join(&m.base_path).to_string_lossy().to_string();
                    composer::Psr4Mapping {
                        prefix: m.prefix,
                        base_path: composer::normalise_path(&abs_base),
                    }
                })
                .collect();
            {
                let mut psr4 = self.workspace.psr4_mappings.write();
                psr4.extend(abs_mappings);
            }

            // ── Vendor dir tracking ─────────────────────────────────
            let vendor_path = sub_root.join(vendor_dir);
            self.add_vendor_dir(&vendor_path);

            // ── Autoload files ──────────────────────────────────────
            self.scan_autoload_files(sub_root, vendor_dir, progress);

            // ── Merged classmap + self-scan ──────────────────────────
            // Load the subproject's Composer classmap as a skip set,
            // then self-scan its PSR-4 directories and vendor packages
            // for anything the classmap missed.
            let mut sub_cm = composer::parse_autoload_classmap(sub_root, vendor_dir);
            // Merge PSR-0 classes for this subproject.
            let psr0_cm = composer::parse_autoload_namespaces(sub_root, vendor_dir);
            for (fqn, path) in psr0_cm {
                sub_cm.entry(fqn).or_insert(path);
            }
            let sub_skip: HashSet<PathBuf> = sub_cm.values().cloned().collect();
            if let Some(p) = progress {
                p.set_scope(sub_mid, sub_hi, "Building class index");
            }
            let scan =
                self.build_self_scan_composer(sub_root, vendor_dir, None, &sub_skip, progress);
            self.populate_autoload_indices(&scan);
            {
                let mut idx = self.symbols.fqn_uri_index.write();
                for (fqcn, path) in sub_cm {
                    idx.or_insert_with(fqcn, || crate::util::path_to_uri(&path));
                }
                for (fqcn, path) in scan.classmap {
                    idx.or_insert_with(fqcn, || crate::util::path_to_uri(&path));
                }
            }
        }

        self.resolved_class_cache.write().set_laravel(any_laravel);
        self.laravel_gates
            .write()
            .set_runtime_permission_package(any_runtime_permissions);

        // Re-sort PSR-4 mappings by prefix length descending so
        // longest-prefix-first matching works.
        {
            let mut psr4 = self.workspace.psr4_mappings.write();
            psr4.sort_by_key(|b| std::cmp::Reverse(b.prefix.len()));
        }

        // ── Full-scan loose files ───────────────────────────────────
        // Walk the workspace for PHP files outside any subproject
        // directory, using gitignore-aware walking.
        if let Some(p) = progress {
            p.set_label_prefix("");
            p.set_scope(80, 85, "Scanning loose PHP files");
        }

        let scan = classmap_scanner::scan_workspace_fallback_full(
            root,
            &skip_dirs,
            progress,
            self.config().indexing.follow_links(),
        );
        self.populate_autoload_indices(&scan);
        {
            let mut idx = self.symbols.fqn_uri_index.write();
            for (fqcn, path) in scan.classmap {
                idx.or_insert_with(fqcn, || crate::util::path_to_uri(&path));
            }
        }

        let symbol_count = self.symbols.fqn_uri_index.read().len()
            + self.symbols.autoload_function_index.read().len()
            + self.symbols.autoload_constant_index.read().len();

        self.log(
            MessageType::INFO,
            format!(
                "PHPantom v{}: PHP {}, {} symbols from {} subprojects, stubs {}",
                self.version,
                php_version,
                symbol_count,
                subprojects.len(),
                crate::stubs::STUBS_VERSION
            ),
        )
        .await;
    }

    /// Initialize a pure non-Composer workspace (no `composer.json`
    /// anywhere).  Full-scans all PHP files in the workspace.
    pub(crate) async fn init_no_composer(
        &self,
        root: &std::path::Path,
        php_version: crate::types::PhpVersion,
        progress: Option<&crate::progress::ScanProgress>,
    ) {
        self.log(
            MessageType::INFO,
            "PHPantom: No composer.json found. Scanning workspace for PHP classes.".to_string(),
        )
        .await;

        if let Some(p) = progress {
            p.set_scope(10, 85, "Scanning workspace for PHP files");
        }

        // No composer.json means no Laravel/Illuminate dependency, so
        // Laravel-specific resolution is disabled.
        self.resolved_class_cache.write().set_laravel(false);

        let skip_dirs = HashSet::new();
        let scan = classmap_scanner::scan_workspace_fallback_full(
            root,
            &skip_dirs,
            progress,
            self.config().indexing.follow_links(),
        );
        self.populate_autoload_indices(&scan);

        let symbol_count = scan.classmap.len();
        {
            let mut idx = self.symbols.fqn_uri_index.write();
            for (fqn, path) in scan.classmap {
                idx.or_insert_with(fqn, || crate::util::path_to_uri(&path));
            }
        }

        let symbol_count = symbol_count
            + self.symbols.autoload_function_index.read().len()
            + self.symbols.autoload_constant_index.read().len();

        self.log(
            MessageType::INFO,
            format!(
                "PHPantom v{}: PHP {}, {} symbols from workspace scan, stubs {}",
                self.version,
                php_version,
                symbol_count,
                crate::stubs::STUBS_VERSION
            ),
        )
        .await;
    }
}

/// Return `true` when any entry directly under `root` is a symlink to a
/// directory.  Used only to decide whether the "symlink-following
/// enabled" startup notice is worth printing; the walks themselves do
/// the real work.
fn has_interior_symlink(root: &std::path::Path) -> bool {
    let Ok(entries) = std::fs::read_dir(root) else {
        return false;
    };
    entries.flatten().any(|e| {
        e.file_type().is_ok_and(|ft| ft.is_symlink())
            && std::fs::metadata(e.path()).is_ok_and(|m| m.is_dir())
    })
}
