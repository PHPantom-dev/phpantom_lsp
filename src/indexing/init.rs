//! Workspace initialization pipelines.
//!
//! Given a resolved workspace root, these `impl Backend` methods build
//! the initial symbol indexes for the three workspace shapes (single
//! Composer project, monorepo, and no-Composer).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tower_lsp::lsp_types::*;

use super::{classify_class_origin, path_aliases};
use crate::Backend;
use crate::classmap_scanner::{self, WorkspaceScanResult};
use crate::composer;
use crate::config::IndexingStrategy;

impl Backend {
    /// Build the symbol indexes for whichever shape the workspace at
    /// `root` turns out to have.
    ///
    /// `composer_package` is the already-parsed root `composer.json`, so
    /// the file is read once per pass rather than once per caller.
    ///
    /// Shared by the `initialized` handshake and by the rediscovery a
    /// mid-session filter change schedules, so the two produce the same
    /// index rather than the latter re-implementing a walk of its own.
    /// Every index write below is an insert-if-absent, which is what
    /// makes a second pass safe to run over a populated index.
    pub(crate) async fn discover_workspace_symbols(
        &self,
        root: &Path,
        php_version: crate::types::PhpVersion,
        composer_package: Option<composer::ComposerPackage>,
        progress: Option<&crate::progress::ScanProgress>,
    ) {
        if composer_package.is_some() {
            self.init_single_project(root, php_version, composer_package, progress)
                .await;
            return;
        }

        let subprojects = composer::discover_subproject_roots(root);
        if subprojects.is_empty() {
            self.init_no_composer(root, php_version, progress).await;
        } else {
            self.init_monorepo(root, &subprojects, php_version, progress)
                .await;
        }
    }

    /// Register where the project keeps its code: the PSR-4 mappings from
    /// the root `composer.json` and from its path repositories, and the
    /// vendor directory every workspace walk skips.
    ///
    /// Returns the vendor directory both as `composer.json` spells it and
    /// as an absolute path.  Split out from [`Self::init_single_project`]
    /// because the `format` command needs the project's source layout
    /// without paying for a class index it never consults.
    pub(crate) fn init_autoload_paths(
        &self,
        root: &Path,
        composer_json: Option<&composer::ComposerPackage>,
    ) -> (String, PathBuf) {
        self.register_composer_project_paths(root, composer_json, false)
    }

    /// Initialize a single-project workspace (root `composer.json` exists).
    ///
    /// This is the standard fast path: read PSR-4 mappings, build the
    /// classmap, scan autoload files.
    pub(crate) async fn init_single_project(
        &self,
        root: &Path,
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
        self.resolved_class_cache
            .write()
            .laravel_mut()
            .set_enabled(is_laravel);

        // A library's configuration is declared by whatever application
        // installs it, so the keys it reads cannot be judged.  An `artisan`
        // file settles it for an application whose `composer.json` says
        // neither way.
        self.set_is_application(
            composer_json
                .as_ref()
                .is_some_and(composer::is_application_project)
                || root.join("artisan").is_file(),
        );

        // A permission package answers authorization checks from the database,
        // so the abilities this project uses are not written in its source and
        // the unknown-ability diagnostic has nothing to judge them against.
        let runtime_permissions = composer_json
            .as_ref()
            .map(composer::has_runtime_permission_package)
            .unwrap_or(false);
        self.laravel
            .gates
            .write()
            .set_runtime_permission_package(runtime_permissions);

        let source_label =
            self.index_composer_project(root, composer_json.as_ref(), false, (15, 85), progress);

        self.finish_discovery(root, php_version, source_label, progress)
            .await;
    }

    /// Index one Composer project: its autoload paths, its classmap (built
    /// the way `[indexing] strategy` asks), its PSR-0 and Drupal
    /// directories, and its autoload files.
    ///
    /// `root` is the workspace root, or with `subproject` set one
    /// subproject of a monorepo.  The work reports progress within
    /// `percent_range`.  Returns where the classmap came from, for the
    /// discovery log.
    fn index_composer_project(
        &self,
        root: &Path,
        composer_json: Option<&composer::ComposerPackage>,
        subproject: bool,
        percent_range: (u32, u32),
        progress: Option<&crate::progress::ScanProgress>,
    ) -> &'static str {
        // A phase's position in `percent_range`, in seventieths of it.
        let (lo, hi) = percent_range;
        let at = |share: u32| lo + (hi - lo) * share / 70;

        let (vendor_dir, vendor_path) =
            self.register_composer_project_paths(root, composer_json, subproject);
        let vendor_paths = path_aliases(&vendor_path);

        // ── Build the classmap ──────────────────────────────────────
        let strategy = self.config().indexing.strategy();

        // The classmap build owns most of the range; the scan helpers
        // divide it into contiguous per-phase windows with per-file
        // counts.
        if let Some(p) = progress {
            p.set_scope(at(0), at(55), "Building class index");
        }

        let explicit_deps = composer_json
            .map(crate::composer::explicit_dependency_names)
            .unwrap_or_default();

        let (mut scan, source_label) = match strategy {
            IndexingStrategy::None => {
                let scan = WorkspaceScanResult {
                    classmap: composer::parse_autoload_classmap(root, &vendor_dir),
                    package_roots: classmap_scanner::vendor_package_roots(
                        root,
                        &vendor_dir,
                        &explicit_deps,
                    ),
                    ..Default::default()
                };
                (scan, "composer")
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
                let filters = self.index_filters();
                let mut scan = classmap_scanner::scan_workspace_fallback_full(
                    root,
                    &skip_dirs,
                    &filters,
                    progress,
                    Some(self.followed_links()),
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
                    &filters,
                    progress,
                    Some(self.followed_links()),
                );
                scan.package_roots = std::mem::take(&mut vendor_scan.package_roots);
                scan.absorb_missing(vendor_scan);
                (scan, "self-scan")
            }
            IndexingStrategy::Composer => {
                // ── Merged classmap + self-scan pipeline ─────────────
                // The Composer classmap wins every name it lists, so a
                // self-scan entry for one of them keeps neither its path
                // nor its origin.
                let composer_cm = composer::parse_autoload_classmap(root, &vendor_dir);
                let skip_paths: HashSet<PathBuf> = composer_cm.values().cloned().collect();
                let mut self_scan = self.build_self_scan_composer(
                    root,
                    &vendor_dir,
                    composer_json,
                    &skip_paths,
                    progress,
                );
                let mut scan = WorkspaceScanResult::from_classmap(composer_cm);
                scan.package_roots = std::mem::take(&mut self_scan.package_roots);
                scan.absorb_missing(self_scan);
                (scan, "composer+scan")
            }
        };

        // The classes no scan labelled (Composer's classmap, the project's
        // PSR-4 sources) are classified by where their file lives.
        let package_roots = std::mem::take(&mut scan.package_roots);
        for (fqn, path) in &scan.classmap {
            if !scan.class_origins.contains_key(fqn) {
                let origin = classify_class_origin(path, &vendor_paths, &package_roots);
                scan.class_origins.insert(fqn.clone(), origin);
            }
        }
        self.symbols.merge_scan(scan);
        self.set_project_package_roots(root, package_roots);

        // ── Drupal: scan web-root directories (gitignore bypassed) ──
        // Drupal's .gitignore excludes web/core, web/modules/contrib,
        // etc. because they are managed by Composer — but those paths
        // contain every base interface and hook definition that modules
        // depend on.  detect_drupal_web_root() returns None for
        // non-Drupal projects so this block is a no-op in that case.
        if let Some(pkg) = composer_json
            && let Some(drupal_web_root) = composer::detect_drupal_web_root(root, pkg)
        {
            if let Some(p) = progress {
                p.set_scope(at(55), at(59), "Scanning Drupal directories");
            }
            let drupal_result = classmap_scanner::scan_drupal_directories(
                &drupal_web_root,
                &self.index_filters(),
                progress,
            );
            let drupal_count = drupal_result.classmap.len()
                + drupal_result.function_index.len()
                + drupal_result.constant_index.len();
            self.symbols.merge_scan(drupal_result);
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
            self.symbols
                .merge_scan(WorkspaceScanResult::from_classmap(psr0_cm));
            tracing::info!("PSR-0: {} classes from autoload_namespaces.php", count);
        }

        // ── Composer's own bootstrap classes ────────────────────────
        // `Composer\Autoload\ClassLoader` and `Composer\InstalledVersions`
        // are `require`d by `vendor/composer/autoload_real.php` before any
        // autoloader exists, so no autoload map lists them and no package
        // scan reaches them.  Code that introspects its own autoloader
        // names them directly.
        let bootstrap_cm = composer::scan_composer_bootstrap_classes(root, &vendor_dir);
        if !bootstrap_cm.is_empty() {
            let count = bootstrap_cm.len();
            self.symbols
                .merge_scan(WorkspaceScanResult::from_classmap(bootstrap_cm));
            tracing::info!("Composer bootstrap: {count} classes from vendor/composer");
        }

        // ── Autoload files ──────────────────────────────────────────
        if let Some(p) = progress {
            p.set_scope(at(59), at(70), "Scanning autoload files");
        }

        self.scan_autoload_files(root, &vendor_dir, progress);

        source_label
    }

    /// Initialize a monorepo workspace (no root `composer.json`, but
    /// subprojects with their own `composer.json` were discovered).
    ///
    /// Each subproject is indexed the way a single Composer project is
    /// (see [`Self::index_composer_project`]).  After all subprojects
    /// are processed, a gitignore-aware full-scan picks up loose PHP files
    /// outside any subproject directory.
    pub(crate) async fn init_monorepo(
        &self,
        root: &Path,
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
        // One application among the subprojects makes the workspace's config
        // files the whole configuration; a workspace of libraries alone reads
        // keys that only the installing application declares.
        let mut any_application = false;

        for (sub_idx, (sub_root, _)) in subprojects.iter().enumerate() {
            // Each subproject owns an equal slice of the 10..80 range;
            // the loose-file scan gets 80..85.  Every report inside the
            // slice carries the subproject prefix.
            let sub_lo = 10 + (sub_idx as u32 * 70) / sub_count.max(1) as u32;
            let sub_hi = 10 + ((sub_idx as u32 + 1) * 70) / sub_count.max(1) as u32;
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
            }
            skip_dirs.insert(sub_root.clone());

            let composer_json = composer::read_composer_package(sub_root);
            if let Some(pkg) = &composer_json {
                any_laravel |= composer::is_laravel_project(pkg);
                any_runtime_permissions |= composer::has_runtime_permission_package(pkg);
                any_application |=
                    composer::is_application_project(pkg) || sub_root.join("artisan").is_file();
            }

            self.index_composer_project(
                sub_root,
                composer_json.as_ref(),
                true,
                (sub_lo, sub_hi),
                progress,
            );
        }

        self.resolved_class_cache
            .write()
            .laravel_mut()
            .set_enabled(any_laravel);
        self.set_is_application(any_application);
        self.laravel
            .gates
            .write()
            .set_runtime_permission_package(any_runtime_permissions);

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
            &self.index_filters(),
            progress,
            Some(self.followed_links()),
        );
        self.symbols.merge_scan(scan);

        self.finish_discovery(
            root,
            php_version,
            &format!("{} subprojects", subprojects.len()),
            progress,
        )
        .await;
    }

    /// Initialize a pure non-Composer workspace (no `composer.json`
    /// anywhere).  Full-scans all PHP files in the workspace.
    pub(crate) async fn init_no_composer(
        &self,
        root: &Path,
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
        self.resolved_class_cache
            .write()
            .laravel_mut()
            .set_enabled(false);

        let skip_dirs = HashSet::new();
        let scan = classmap_scanner::scan_workspace_fallback_full(
            root,
            &skip_dirs,
            &self.index_filters(),
            progress,
            Some(self.followed_links()),
        );
        self.symbols.merge_scan(scan);

        self.finish_discovery(root, php_version, "workspace scan", progress)
            .await;
    }

    /// End a discovery pass the way every pipeline does: index the
    /// `[indexing] include` paths, then log what the pass found, with
    /// `source` saying where it came from.
    async fn finish_discovery(
        &self,
        root: &Path,
        php_version: crate::types::PhpVersion,
        source: &str,
        progress: Option<&crate::progress::ScanProgress>,
    ) {
        // Runs for every pipeline and strategy, including `none`: an
        // explicit `include` entry is a direct instruction, not a fallback.
        self.scan_configured_include_paths(root, progress);

        let symbol_count = self.symbols.fqn_uri_index.read().len()
            + self.symbols.autoload_function_index.read().len()
            + self.symbols.autoload_constant_index.read().len();

        self.log(
            MessageType::INFO,
            format!(
                "PHPantom v{}: PHP {}, {} symbols from {}, stubs {}",
                self.version,
                php_version,
                symbol_count,
                source,
                crate::stubs::STUBS_VERSION
            ),
        )
        .await;
    }
}

#[cfg(test)]
#[path = "init_tests.rs"]
mod tests;
