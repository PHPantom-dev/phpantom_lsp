//! Workspace initialization pipelines.
//!
//! Given a resolved workspace root, these `impl Backend` methods build
//! the initial symbol indexes for the three workspace shapes (single
//! Composer project, monorepo, and no-Composer).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tower_lsp_server::gen_lsp_types::*;

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
        let was_drupal = self.is_drupal_project();
        if composer_package.is_some() {
            self.init_single_project(root, php_version, composer_package, progress)
                .await;
        } else {
            let (subprojects, drupal_tree) = composer::discover_workspace_projects(root);
            if subprojects.is_empty() {
                self.init_no_composer(root, drupal_tree.as_deref(), php_version, progress)
                    .await;
            } else {
                self.init_monorepo(
                    root,
                    &subprojects,
                    drupal_tree.as_deref(),
                    php_version,
                    progress,
                )
                .await;
            }
        }
        // The editor opens files while the walk runs, and a file read
        // before the walk found a Drupal site was read without Drupal's
        // names in it.
        if !was_drupal && self.is_drupal_project() {
            self.reparse_open_files();
        }
    }

    /// Read every open file again from its buffer, as opening it does.
    fn reparse_open_files(&self) {
        let open: Vec<(String, std::sync::Arc<String>)> = self
            .open_files
            .read()
            .iter()
            .map(|(uri, text)| (uri.clone(), std::sync::Arc::clone(text)))
            .collect();
        for (uri, text) in open {
            if self.is_resource_uri(&uri) {
                self.update_resource_symbol_index(&uri, &text);
            } else {
                self.update_ast(&uri, &text);
            }
            self.schedule_diagnostics(uri);
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
        let (vendor_dir, vendor_path, _) =
            self.register_composer_project_paths(root, composer_json, false);
        (vendor_dir, vendor_path)
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

        // Composer projects nested below this one with their own installed
        // dependencies, which the root's autoloader knows nothing about.
        // They are indexed the way a monorepo's subprojects are, and
        // classify the workspace alongside the root the same way too.
        // The root's bin namespaces are not among them: they install the
        // root's own tools, and are indexed as part of it.
        let root_vendor = root.join(
            composer_json
                .as_ref()
                .map(composer::get_vendor_dir)
                .unwrap_or_else(|| "vendor".to_string()),
        );
        let skip: Vec<PathBuf> = std::iter::once(root_vendor)
            .chain(
                composer_json
                    .as_ref()
                    .and_then(|pkg| composer::bin_target_directory(root, pkg)),
            )
            .collect();
        let subprojects: Vec<(PathBuf, Option<composer::ComposerPackage>)> =
            composer::discover_installed_subprojects(root, &skip)
                .into_iter()
                .map(|(sub_root, _)| {
                    let package = composer::read_composer_package(&sub_root);
                    (sub_root, package)
                })
                .collect();
        let bin_namespaces = composer_json
            .as_ref()
            .map(|pkg| composer::bin_namespaces(root, pkg))
            .unwrap_or_default();
        let packages = || {
            composer_json
                .iter()
                .chain(subprojects.iter().filter_map(|(_, pkg)| pkg.as_ref()))
        };
        // The frameworks a bin namespace installs are the root's tools'
        // too, so they classify the workspace the way the root's own
        // dependencies do.
        let tool_packages = || {
            packages().chain(
                bin_namespaces
                    .iter()
                    .filter_map(|namespace| namespace.package.as_ref()),
            )
        };

        // Classify the project so Laravel-specific resolution (Eloquent
        // members, config/view/route keys, contract bindings, patches) is
        // skipped when no Laravel/Illuminate dependency is present.
        let is_laravel = tool_packages().any(composer::is_laravel_project);
        self.resolved_class_cache
            .write()
            .laravel_mut()
            .set_enabled(is_laravel);

        // A library's configuration is declared by whatever application
        // installs it, so the keys it reads cannot be judged.  An `artisan`
        // file settles it for an application whose `composer.json` says
        // neither way.
        self.set_is_application(
            packages().any(composer::is_application_project)
                || std::iter::once(root)
                    .chain(subprojects.iter().map(|(sub_root, _)| sub_root.as_path()))
                    .any(|dir| dir.join("artisan").is_file()),
        );

        self.set_is_doctrine(tool_packages().any(composer::is_doctrine_project));
        self.set_is_symfony(tool_packages().any(composer::is_symfony_project));
        self.set_is_twig(tool_packages().any(composer::is_twig_project));
        self.set_is_yii(tool_packages().any(composer::is_yii_project));
        self.set_is_tempest(tool_packages().any(composer::is_tempest_project));
        // PHPUnit is often one of the tools a bin namespace installs.
        self.set_phpunit_major(
            std::iter::once(root)
                .chain(subprojects.iter().map(|(sub_root, _)| sub_root.as_path()))
                .chain(
                    bin_namespaces
                        .iter()
                        .map(|namespace| namespace.root.as_path()),
                )
                .filter_map(composer::locked_phpunit_major)
                .min(),
        );
        self.clear_drupal_web_roots();

        // A permission package answers authorization checks from the database,
        // so the abilities this project uses are not written in its source and
        // the unknown-ability diagnostic has nothing to judge them against.
        let runtime_permissions = packages().any(composer::has_runtime_permission_package);
        self.laravel
            .gates
            .write()
            .set_runtime_permission_package(runtime_permissions);

        // The workspace folders beside this one join the walk of the
        // project's root when the strategy walks it, and are walked on
        // their own at the end otherwise.
        let extra_roots = self.claim_extra_roots();
        let walks_root = matches!(
            self.config().indexing.strategy(),
            IndexingStrategy::SelfScan | IndexingStrategy::Full
        );
        let (walked, unwalked) = if walks_root {
            (extra_roots.as_slice(), &[][..])
        } else {
            (&[][..], extra_roots.as_slice())
        };

        let root_hi = if subprojects.is_empty() { 85 } else { 50 };
        let source_label = self.index_composer_project(
            root,
            composer_json.as_ref(),
            false,
            walked,
            (15, root_hi),
            progress,
        );

        // The subprojects share what is left of the range, each reporting
        // under its own label the way a monorepo's do.
        let sub_count = subprojects.len() as u32;
        for (sub_idx, (sub_root, package)) in subprojects.iter().enumerate() {
            let sub_lo = root_hi + (sub_idx as u32 * (85 - root_hi)) / sub_count;
            let sub_hi = root_hi + ((sub_idx as u32 + 1) * (85 - root_hi)) / sub_count;
            if let Some(p) = progress {
                let label = sub_root.strip_prefix(root).unwrap_or(sub_root).display();
                p.set_label_prefix(format!("Subproject {label}"));
            }
            self.index_composer_project(
                sub_root,
                package.as_ref(),
                true,
                &[],
                (sub_lo, sub_hi),
                progress,
            );
        }
        if !subprojects.is_empty()
            && let Some(p) = progress
        {
            p.set_label_prefix("");
        }

        self.finish_discovery(root, php_version, source_label, unwalked, progress)
            .await;
    }

    /// Index one Composer project: its autoload paths, its classmap (built
    /// the way `[indexing] strategy` asks), its PSR-0 and Drupal
    /// directories, and its autoload files.
    ///
    /// `root` is the workspace root, or with `subproject` set one
    /// subproject of a monorepo.  `library_roots` join the walk of `root`
    /// when the strategy walks it.  The work reports progress within
    /// `percent_range`.  Returns where the classmap came from, for the
    /// discovery log.
    pub(super) fn index_composer_project(
        &self,
        root: &Path,
        composer_json: Option<&composer::ComposerPackage>,
        subproject: bool,
        library_roots: &[PathBuf],
        percent_range: (u32, u32),
        progress: Option<&crate::progress::ScanProgress>,
    ) -> &'static str {
        // A phase's position in `percent_range`, in seventieths of it.
        let (lo, hi) = percent_range;
        let at = |share: u32| lo + (hi - lo) * share / 70;

        let (vendor_dir, vendor_path, bin_namespaces) =
            self.register_composer_project_paths(root, composer_json, subproject);
        let mut vendor_paths = path_aliases(&vendor_path);

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

        // Detected before the filters are read, so the walks below see
        // Drupal's `.module`, `.install`, … files as PHP too.
        let drupal_web_root =
            composer_json.and_then(|pkg| composer::detect_drupal_web_root(root, pkg));
        if drupal_web_root.is_some() {
            self.mark_drupal_project();
        }

        // Composer's class lists name files without any walk reaching
        // them, so `exclude` is applied to them here, where they merge.
        let filters = self.index_filters();
        let composer_classmap = || {
            let mut classmap = composer::parse_autoload_classmap(root, &vendor_dir);
            filters.retain_unexcluded(&mut classmap);
            classmap
        };

        let (mut scan, source_label) = match strategy {
            IndexingStrategy::None => {
                let scan = WorkspaceScanResult {
                    classmap: composer_classmap(),
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
                skip_dirs.extend(
                    bin_namespaces
                        .iter()
                        .map(composer::BinNamespace::vendor_path),
                );
                if let Some(p) = progress {
                    p.begin_phase(0.0, 0.3, "Scanning workspace files");
                }
                let mut scan = classmap_scanner::scan_roots_full(
                    &[root.to_path_buf()],
                    library_roots,
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
                let composer_cm = composer_classmap();
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

        // The tools the project's bin namespaces install, behind the
        // project's own vendor.
        for namespace in &bin_namespaces {
            let mut namespace_scan = self.scan_bin_namespace(namespace, strategy, &filters);
            scan.package_roots.append(&mut namespace_scan.package_roots);
            scan.absorb_missing(namespace_scan);
            vendor_paths.extend(path_aliases(&namespace.vendor_path()));
        }

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

        // detect_drupal_web_root() returns None for non-Drupal projects,
        // so this block is a no-op in that case.
        if let Some(drupal_web_root) = drupal_web_root {
            if let Some(p) = progress {
                p.set_scope(at(55), at(59), "Scanning Drupal directories");
            }
            self.index_drupal_web_root(&drupal_web_root, &filters, progress);
        }

        // ── PSR-0 (legacy) classmap ─────────────────────────────────
        // Packages that declare `autoload.psr-0` in their composer.json
        // (e.g. HTMLPurifier) are listed in `autoload_namespaces.php`.
        // Scan the listed directories and merge discovered classes into
        // the classmap so they are resolvable via `find_or_load_class`.
        let mut psr0_cm = composer::parse_autoload_namespaces(root, &vendor_dir);
        for namespace in &bin_namespaces {
            for (fqn, path) in
                composer::parse_autoload_namespaces(&namespace.root, &namespace.vendor_dir)
            {
                psr0_cm.entry(fqn).or_insert(path);
            }
        }
        filters.retain_unexcluded(&mut psr0_cm);
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
        let mut bootstrap_cm = composer::scan_composer_bootstrap_classes(root, &vendor_dir);
        filters.retain_unexcluded(&mut bootstrap_cm);
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
        for namespace in &bin_namespaces {
            self.scan_autoload_files(&namespace.root, &namespace.vendor_dir, None);
        }

        source_label
    }

    /// Index the Drupal web root at `web_root`, walked with `.gitignore`
    /// bypassed: a site's `.gitignore` leaves out core and the contributed
    /// extensions Composer installs, which hold every base interface and
    /// hook definition the site's own modules depend on.  Records the web
    /// root and the declaration files, hook files and service providers
    /// the walk finds.
    ///
    /// The caller marks the project as Drupal (see
    /// [`Self::mark_drupal_project`]) before compiling `filters`, or the
    /// project's other walks read no `.module`, `.install`, … file as PHP.
    fn index_drupal_web_root(
        &self,
        web_root: &Path,
        filters: &std::sync::Arc<classmap_scanner::IndexFilters>,
        progress: Option<&crate::progress::ScanProgress>,
    ) {
        self.note_drupal_web_root(web_root);
        let (drupal_result, declaration_files) =
            classmap_scanner::scan_drupal_directories(web_root, filters, progress);
        self.note_drupal_declaration_files(declaration_files.declarations);
        self.note_drupal_service_providers(declaration_files.service_providers);
        self.note_drupal_config_files(
            declaration_files.default_config,
            declaration_files.config_schemas,
        );
        self.note_drupal_templates(declaration_files.templates);
        let drupal_count = drupal_result.classmap.len()
            + drupal_result.function_index.len()
            + drupal_result.constant_index.len();
        self.symbols.merge_scan(drupal_result);
        tracing::info!(
            "PHPantom: Drupal web root {:?}, {} symbols indexed",
            web_root,
            drupal_count
        );
    }

    /// The classes, functions and constants the vendor directory of one
    /// bin namespace holds, read the way `[indexing] strategy` reads the
    /// project's own vendor directory, with their package roots.
    fn scan_bin_namespace(
        &self,
        namespace: &composer::BinNamespace,
        strategy: IndexingStrategy,
        filters: &std::sync::Arc<classmap_scanner::IndexFilters>,
    ) -> WorkspaceScanResult {
        let explicit_deps = namespace
            .package
            .as_ref()
            .map(composer::explicit_dependency_names)
            .unwrap_or_default();
        let composer_classmap = || {
            let mut classmap =
                composer::parse_autoload_classmap(&namespace.root, &namespace.vendor_dir);
            filters.retain_unexcluded(&mut classmap);
            classmap
        };
        let scan_packages = |skip: &HashSet<PathBuf>| {
            classmap_scanner::scan_vendor_packages_with_skip(
                &namespace.root,
                &namespace.vendor_dir,
                skip,
                &explicit_deps,
                filters,
                None,
                Some(self.followed_links()),
            )
        };
        match strategy {
            IndexingStrategy::None => WorkspaceScanResult {
                classmap: composer_classmap(),
                package_roots: classmap_scanner::vendor_package_roots(
                    &namespace.root,
                    &namespace.vendor_dir,
                    &explicit_deps,
                ),
                ..Default::default()
            },
            IndexingStrategy::SelfScan | IndexingStrategy::Full => scan_packages(&HashSet::new()),
            IndexingStrategy::Composer => {
                let classmap = composer_classmap();
                let skip: HashSet<PathBuf> = classmap.values().cloned().collect();
                let mut packages = scan_packages(&skip);
                let mut scan = WorkspaceScanResult::from_classmap(classmap);
                scan.package_roots = std::mem::take(&mut packages.package_roots);
                scan.absorb_missing(packages);
                scan
            }
        }
    }

    /// Initialize a monorepo workspace (no root `composer.json`, but
    /// subprojects with their own `composer.json` were discovered).
    ///
    /// Each subproject is indexed the way a single Composer project is
    /// (see [`Self::index_composer_project`]).  After all subprojects
    /// are processed, a gitignore-aware full-scan picks up loose PHP files
    /// outside any subproject directory.  A Drupal tree no subproject owns,
    /// with its web root at `drupal_tree`, is indexed beside them the way
    /// [`Self::init_no_composer`] indexes one.
    pub(crate) async fn init_monorepo(
        &self,
        root: &Path,
        subprojects: &[(PathBuf, String)],
        drupal_tree: Option<&Path>,
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
            MessageType::Info,
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
        // Doctrine in any subproject turns its lenses on workspace-wide.
        let mut any_doctrine = false;
        let mut any_symfony = false;
        // Drupal's templates are Twig, as `drupal/core` makes them in a
        // Composer project.
        let mut any_twig = drupal_tree.is_some();
        let mut any_yii = false;
        let mut any_tempest = false;
        let mut phpunit_major: Option<u8> = None;
        self.clear_drupal_web_roots();
        // Marked before any walk compiles the filters, so every walk sees
        // Drupal's `.module`, `.install`, … files as PHP.
        if drupal_tree.is_some() {
            self.mark_drupal_project();
        }

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
                any_runtime_permissions |= composer::has_runtime_permission_package(pkg);
                // A subproject's bin namespaces install its tools, which
                // classify it as its own dependencies do.
                let bin_namespaces = composer::bin_namespaces(sub_root, pkg);
                for tool_pkg in std::iter::once(pkg)
                    .chain(bin_namespaces.iter().filter_map(|ns| ns.package.as_ref()))
                {
                    any_laravel |= composer::is_laravel_project(tool_pkg);
                    any_doctrine |= composer::is_doctrine_project(tool_pkg);
                    any_symfony |= composer::is_symfony_project(tool_pkg);
                    any_twig |= composer::is_twig_project(tool_pkg);
                    any_yii |= composer::is_yii_project(tool_pkg);
                    any_tempest |= composer::is_tempest_project(tool_pkg);
                }
                for major in std::iter::once(sub_root.as_path())
                    .chain(bin_namespaces.iter().map(|ns| ns.root.as_path()))
                    .filter_map(composer::locked_phpunit_major)
                {
                    phpunit_major =
                        Some(phpunit_major.map_or(major, |lowest: u8| lowest.min(major)));
                }
                any_application |=
                    composer::is_application_project(pkg) || sub_root.join("artisan").is_file();
            }

            self.index_composer_project(
                sub_root,
                composer_json.as_ref(),
                true,
                &[],
                (sub_lo, sub_hi),
                progress,
            );
        }

        self.resolved_class_cache
            .write()
            .laravel_mut()
            .set_enabled(any_laravel);
        self.set_is_application(any_application);
        self.set_is_doctrine(any_doctrine);
        self.set_is_symfony(any_symfony);
        self.set_is_twig(any_twig);
        self.set_is_yii(any_yii);
        self.set_is_tempest(any_tempest);
        self.set_phpunit_major(phpunit_major);
        self.laravel
            .gates
            .write()
            .set_runtime_permission_package(any_runtime_permissions);

        // ── Full-scan loose files ───────────────────────────────────
        // Walk the workspace for PHP files outside any subproject
        // directory, using gitignore-aware walking.
        let walk_hi = if drupal_tree.is_some() { 83 } else { 85 };
        if let Some(p) = progress {
            p.set_label_prefix("");
            p.set_scope(80, walk_hi, "Scanning loose PHP files");
        }

        skip_dirs.extend(drupal_tree.map(drupal_core_dir));
        let filters = self.index_filters();
        let extra_roots = self.claim_extra_roots();
        let scan = classmap_scanner::scan_roots_full(
            &[root.to_path_buf()],
            &extra_roots,
            &skip_dirs,
            &filters,
            progress,
            Some(self.followed_links()),
        );
        self.symbols.merge_scan(scan);

        if let Some(web_root) = drupal_tree {
            if let Some(p) = progress {
                p.set_scope(walk_hi, 85, "Scanning Drupal directories");
            }
            self.index_drupal_web_root(web_root, &filters, progress);
        }

        self.finish_discovery(
            root,
            php_version,
            &format!("{} subprojects", subprojects.len()),
            &[],
            progress,
        )
        .await;
    }

    /// Initialize a non-Composer workspace (no root `composer.json`, and
    /// no subproject to index on its own).  Full-scans all PHP files in the
    /// workspace, and when the workspace is a Drupal tree with its web root
    /// at `drupal_web_root`, indexes that web root the way a Composer
    /// project's is.
    pub(crate) async fn init_no_composer(
        &self,
        root: &Path,
        drupal_web_root: Option<&Path>,
        php_version: crate::types::PhpVersion,
        progress: Option<&crate::progress::ScanProgress>,
    ) {
        let message = if drupal_web_root.is_some() {
            "PHPantom: No composer.json found. Scanning the Drupal site for PHP classes."
        } else {
            "PHPantom: No composer.json found. Scanning workspace for PHP classes."
        };
        self.log(MessageType::Info, message.to_string()).await;

        let walk_hi = if drupal_web_root.is_some() { 75 } else { 85 };
        if let Some(p) = progress {
            p.set_scope(10, walk_hi, "Scanning workspace for PHP files");
        }

        // No composer.json means no Laravel/Illuminate dependency, so
        // Laravel-specific resolution is disabled.
        self.resolved_class_cache
            .write()
            .laravel_mut()
            .set_enabled(false);

        // Marked before the filters are read, as for a Composer project,
        // so the walk sees Drupal's `.module`, `.install`, … files as PHP.
        // Drupal's templates are Twig, as `drupal/core` makes them in a
        // Composer project.
        if drupal_web_root.is_some() {
            self.clear_drupal_web_roots();
            self.mark_drupal_project();
            self.set_is_twig(true);
        }
        let filters = self.index_filters();

        let extra_roots = self.claim_extra_roots();
        let skip_dirs: HashSet<PathBuf> =
            drupal_web_root.map(drupal_core_dir).into_iter().collect();
        let scan = classmap_scanner::scan_roots_full(
            &[root.to_path_buf()],
            &extra_roots,
            &skip_dirs,
            &filters,
            progress,
            Some(self.followed_links()),
        );
        self.symbols.merge_scan(scan);

        let source = match drupal_web_root {
            Some(web_root) => {
                if let Some(p) = progress {
                    p.set_scope(walk_hi, 85, "Scanning Drupal directories");
                }
                self.index_drupal_web_root(web_root, &filters, progress);
                "Drupal site scan"
            }
            None => "workspace scan",
        };

        self.finish_discovery(root, php_version, source, &[], progress)
            .await;
    }

    /// End a discovery pass the way every pipeline does: walk the
    /// workspace folders the pipeline's own walk did not take in
    /// (`unwalked_roots`), index the `[indexing] include` paths and the
    /// stub paths, then log what the pass found, with `source` saying
    /// where it came from.
    async fn finish_discovery(
        &self,
        root: &Path,
        php_version: crate::types::PhpVersion,
        source: &str,
        unwalked_roots: &[PathBuf],
        progress: Option<&crate::progress::ScanProgress>,
    ) {
        // Like an `include` entry, a workspace folder is walked whatever
        // the strategy: the editor named it, so it is not a fallback.
        if !unwalked_roots.is_empty() {
            let scan = classmap_scanner::scan_roots_full(
                &[],
                unwalked_roots,
                &HashSet::from([root.to_path_buf()]),
                &self.index_filters(),
                progress,
                Some(self.followed_links()),
            );
            self.symbols.merge_scan(scan);
        }

        // Runs for every pipeline and strategy, including `none`: an
        // explicit `include` entry is a direct instruction, not a fallback.
        self.scan_configured_include_paths(root, progress);

        // Last, so the stub entries it merges are what the discovery
        // indexes hold for a stub file the walks above also found.
        self.refresh_stub_paths();

        let symbol_count = self.symbols.fqn_uri_index.read().len()
            + self.symbols.autoload_function_index.read().len()
            + self.symbols.autoload_constant_index.read().len();

        self.log(
            MessageType::Info,
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

/// The `core/` folder of the Drupal web root at `web_root`, which the
/// workspace walk skips because the Drupal scan walks it anyway, so core
/// is parsed once.  The extension folders stay in the walk: it follows
/// symlinks, such as a custom module linked in from elsewhere, and the
/// Drupal scan does not.
fn drupal_core_dir(web_root: &Path) -> PathBuf {
    web_root.join("core")
}

#[cfg(test)]
#[path = "init_tests.rs"]
mod tests;
