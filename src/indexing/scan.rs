//! Vendor registration and the Composer-derived scans.
//!
//! These `impl Backend` methods register vendor directories, (re)build
//! the Composer-derived indexes, scan autoload files, and self-scan a
//! Composer project's autoload directories.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::path_aliases;
use crate::Backend;
use crate::classmap_scanner::{self, WorkspaceScanResult};
use crate::composer;
use crate::workspace_env::{ComposerProjectPaths, LibraryClaim, LibraryClaimant};

impl Backend {
    /// Register a vendor directory path and its URI prefix for
    /// vendor-file detection.
    pub(crate) fn add_vendor_dir(&self, vendor_path: &Path) {
        self.add_library_dir(LibraryClaimant::Composer, vendor_path);
    }

    /// Register a directory as library code on behalf of `claimant`.
    pub(super) fn add_library_dir(&self, claimant: LibraryClaimant, dir: &Path) {
        // Keep both filesystem spellings. Walkers normally yield the raw
        // workspace spelling, while Composer package discovery canonicalizes
        // its files; on macOS those can be `/var` and `/private/var` for the
        // same vendor tree. Caching both here keeps the hot lookup paths free
        // of filesystem calls.
        //
        // The URI prefixes serve the URI-level skip logic (diagnostics, find
        // references, rename), and keep both forms for the same reason, so
        // macOS `/tmp` vs `/private/tmp` style aliases do not leak vendor
        // files into workspace indexing.
        let aliases = path_aliases(dir);
        let uri_prefixes = aliases
            .iter()
            .map(|path| crate::util::dir_uri_prefix(path))
            .collect();
        self.register_vendor_location(claimant, aliases, uri_prefixes);
    }

    /// Record the spellings of a library location and the URI prefixes
    /// that mark a file under it, skipping any already registered.
    pub(super) fn register_vendor_location(
        &self,
        claimant: LibraryClaimant,
        paths: Vec<PathBuf>,
        uri_prefixes: Vec<String>,
    ) {
        let mut claims = self.workspace.library_claims.lock();
        let known = claims.iter().any(|claim| {
            claim.claimant == claimant && claim.paths == paths && claim.uri_prefixes == uri_prefixes
        });
        if !known {
            claims.push(LibraryClaim {
                claimant,
                paths: paths.clone(),
                uri_prefixes: uri_prefixes.clone(),
            });
        }
        push_new(&mut self.workspace.vendor_dir_paths.lock(), paths);
        push_new(&mut self.workspace.vendor_uri_prefixes.lock(), uri_prefixes);
    }

    /// Withdraw every location `claimant` registered, keeping the
    /// spellings another source still claims, and return the path
    /// spellings it had registered.
    pub(super) fn release_library_claimant(&self, claimant: &LibraryClaimant) -> Vec<PathBuf> {
        let mut claims = self.workspace.library_claims.lock();
        let (released, kept): (Vec<LibraryClaim>, Vec<LibraryClaim>) = std::mem::take(&mut *claims)
            .into_iter()
            .partition(|claim| claim.claimant == *claimant);
        *claims = kept;
        if released.is_empty() {
            return Vec::new();
        }

        let still_claimed_path =
            |path: &PathBuf| claims.iter().any(|claim| claim.paths.contains(path));
        let still_claimed_prefix = |prefix: &String| {
            claims
                .iter()
                .any(|claim| claim.uri_prefixes.contains(prefix))
        };
        let gone_paths: Vec<&PathBuf> = released
            .iter()
            .flat_map(|claim| &claim.paths)
            .filter(|path| !still_claimed_path(path))
            .collect();
        let gone_prefixes: Vec<&String> = released
            .iter()
            .flat_map(|claim| &claim.uri_prefixes)
            .filter(|prefix| !still_claimed_prefix(prefix))
            .collect();
        self.workspace
            .vendor_dir_paths
            .lock()
            .retain(|path| !gone_paths.contains(&path));
        self.workspace
            .vendor_uri_prefixes
            .lock()
            .retain(|prefix| !gone_prefixes.contains(&prefix));
        drop(claims);

        released.into_iter().flat_map(|claim| claim.paths).collect()
    }

    /// Rebuild the vendor-derived indexes after a `composer.json` /
    /// `composer.lock` change (e.g. a `composer install` or `update`).
    ///
    /// In a monorepo (no `composer.json` at `root`) only the subprojects
    /// holding one of `changed_files` are rescanned.  Every pass ends by
    /// clearing the resolved-class caches so stale vendor versions do not
    /// linger.  This is the synchronous body of
    /// [`did_change_watched_files`](Self::did_change_watched_files)'s
    /// composer branch, factored out so it can run on a blocking thread.
    pub(crate) fn rescan_composer_indexes(&self, root: &Path, changed_files: &[PathBuf]) {
        if let Some(pkg) = composer::read_composer_package(root) {
            self.rescan_composer_project(root, &pkg, false);
        }

        // The subprojects holding one of the changed files: a monorepo's,
        // or the ones nested below a root project with their own vendor.
        let root_aliases = path_aliases(root);
        let known: Vec<PathBuf> = self
            .workspace
            .composer_projects
            .read()
            .iter()
            .map(|project| project.root.clone())
            .filter(|project_root| !root_aliases.contains(project_root))
            .collect();
        let holds = |project_root: &Path, file: &Path| {
            path_aliases(project_root)
                .iter()
                .any(|alias| file.starts_with(alias))
        };
        for sub_root in known
            .iter()
            .filter(|sub_root| changed_files.iter().any(|file| holds(sub_root, file)))
        {
            if let Some(pkg) = composer::read_composer_package(sub_root) {
                self.rescan_composer_project(sub_root, &pkg, true);
            }
        }

        // A nested project whose dependencies were installed after startup
        // was not one discovery could find, so it is indexed now.
        let vendor_dirs = self.workspace.vendor_dir_paths.lock().clone();
        let mut fresh: Vec<PathBuf> = changed_files
            .iter()
            .filter_map(|file| file.parent())
            .filter(|dir| {
                !root_aliases.iter().any(|alias| dir == alias)
                    && !known.iter().any(|sub_root| holds(sub_root, dir))
                    && !vendor_dirs.iter().any(|vendor| dir.starts_with(vendor))
            })
            .map(Path::to_path_buf)
            .collect();
        fresh.sort();
        fresh.dedup();
        for sub_root in fresh {
            let Some(pkg) = composer::read_composer_package(&sub_root) else {
                continue;
            };
            let vendor_dir = composer::get_vendor_dir(&pkg);
            if sub_root
                .join(vendor_dir)
                .join("composer/installed.json")
                .is_file()
            {
                self.index_composer_project(&sub_root, Some(&pkg), true, &[], (0, 0), None);
            }
        }

        // After the rescans above, whose `replace_tree` over the vendor
        // directory also dropped the discovery entries of the stub
        // packages inside it, and which re-read which stub packages are
        // installed.
        self.refresh_stub_paths();

        // Clear all cached class info since vendor classes may have
        // changed versions.  The negative-cache clear comes last of the
        // three: it also retires the memoised class lookups, which must
        // happen after the index it memoises has been emptied.
        self.symbols.fqn_class_index.write().clear();
        // The runner-up declarations describe the vendor tree that was just
        // rescanned; keeping them would promote a copy of a class that the
        // update may have moved or removed.
        self.symbols.duplicate_classes.write().clear();
        self.symbols.gti_index.write().clear();
        self.symbols.gti_parents_index.write().clear();
        self.clear_not_found_caches();
        self.clear_resolved_class_cache();
        self.member_completion_cache.lock().clear();

        // Composer changes can introduce or remove functions/classes that
        // shadow Laravel's runtime helper and facade aliases. Only maps with
        // such candidates are revisited.
        self.refresh_all_published_laravel_candidates();
    }

    /// Re-read one Composer project's PSR-4 mappings, rebuild its vendor
    /// classmap and autoload function/constant indexes, and rescan its
    /// autoload files.
    fn rescan_composer_project(
        &self,
        project_root: &Path,
        pkg: &composer::ComposerPackage,
        subproject: bool,
    ) {
        // `vendor/` may not have existed during initialization (a fresh
        // clone before `composer install`), so registering it again here
        // is what lets the shared vendor filters cache both raw and
        // canonical spellings.
        let (vendor_dir, vendor_path) =
            self.register_composer_project_paths(project_root, Some(pkg), subproject);

        // Rebuild vendor classmap, tracking dependency provenance so
        // completion ranking stays accurate after a composer change.
        let explicit_deps = composer::explicit_dependency_names(pkg);
        let mut vendor_scan = classmap_scanner::scan_vendor_packages_with_skip(
            project_root,
            &vendor_dir,
            &HashSet::new(),
            &explicit_deps,
            &self.index_filters(),
            None,
            Some(self.followed_links()),
        );
        let vendor_package_roots = std::mem::take(&mut vendor_scan.package_roots);

        // Everything the old vendor tree contributed goes, so symbols
        // removed by a `composer update` no longer resolve.
        self.symbols
            .replace_tree(&path_aliases(&vendor_path), vendor_scan);

        self.set_project_package_roots(project_root, vendor_package_roots);

        // Rescan autoload files (they may have changed).
        self.scan_autoload_files(project_root, &vendor_dir, None);
    }

    /// Register where a Composer project keeps its code: the PSR-4
    /// mappings from its `composer.json` and from its path repositories,
    /// its vendor directory, and the stub packages it installed.
    ///
    /// A subproject's own mappings are made absolute; the workspace root's
    /// stay relative to it.  Returns the vendor directory both as
    /// `composer.json` spells it and as an absolute path.
    pub(super) fn register_composer_project_paths(
        &self,
        project_root: &Path,
        composer_json: Option<&composer::ComposerPackage>,
        subproject: bool,
    ) -> (String, PathBuf) {
        let (mut mappings, vendor_dir) = match composer_json {
            Some(pkg) => (
                composer::extract_psr4_mappings_from_package(pkg),
                composer::get_vendor_dir(pkg),
            ),
            None => (Vec::new(), "vendor".to_string()),
        };
        if subproject {
            for mapping in &mut mappings {
                let base = project_root.join(&mapping.base_path);
                mapping.base_path = composer::normalise_path(&base.to_string_lossy());
            }
        }

        // Cache the vendor dir path so cross-file scans can skip it
        // without re-reading composer.json on every request.
        let vendor_path = project_root.join(&vendor_dir);
        self.add_vendor_dir(&vendor_path);

        let installed = composer::read_installed_packages(project_root, &vendor_dir);
        // Include PSR-4 mappings from path-repository packages (local
        // packages symlinked into vendor/, e.g. internachi/modular modules).
        if let Some(installed) = &installed {
            mappings.extend(composer::path_repo_psr4_mappings(project_root, installed));
        }
        self.set_project_psr4_mappings(project_root, mappings);
        let stub_packages = installed
            .as_ref()
            .map(composer::stub_package_roots)
            .unwrap_or_default();
        let vendor_paths = if vendor_path.is_dir() {
            path_aliases(&vendor_path)
        } else {
            Vec::new()
        };
        {
            let mut projects = self.workspace.composer_projects.write();
            let project = composer_project_entry(&mut projects, project_root);
            project.stub_packages = stub_packages;
            project.vendor_paths = vendor_paths;
        }
        self.workspace.forget_project_scopes();

        (vendor_dir, vendor_path)
    }

    /// Replace the PSR-4 mappings `project_root` contributed.
    fn set_project_psr4_mappings(&self, project_root: &Path, mappings: Vec<composer::Psr4Mapping>) {
        let mut projects = self.workspace.composer_projects.write();
        composer_project_entry(&mut projects, project_root).psr4_mappings = mappings;
        let mut all: Vec<composer::Psr4Mapping> = projects
            .iter()
            .flat_map(|project| project.psr4_mappings.iter().cloned())
            .collect();
        // Longest-prefix-first, so path-repo namespaces are matched before
        // any shorter root prefix (e.g. an empty-prefix root fallback).
        all.sort_by_key(|m| std::cmp::Reverse(m.prefix.len()));
        *self.workspace.psr4_mappings.write() = all;
    }

    /// Replace the vendor package roots `project_root` contributed, which
    /// path-based origin lookups (functions, constants) classify lazily
    /// parsed symbols by.
    pub(super) fn set_project_package_roots(
        &self,
        project_root: &Path,
        package_roots: Vec<(PathBuf, crate::ClassCompletionOrigin, String)>,
    ) {
        let mut projects = self.workspace.composer_projects.write();
        composer_project_entry(&mut projects, project_root).package_roots = package_roots;
        *self.workspace.vendor_package_origin_roots.write() = projects
            .iter()
            .flat_map(|project| project.package_roots.iter().cloned())
            .collect();
    }

    /// Scan autoload files for a single project root and populate the
    /// autoload indices.  Returns the number of autoload file entries
    /// found.
    pub(crate) fn scan_autoload_files(
        &self,
        project_root: &Path,
        vendor_dir: &str,
        progress: Option<&crate::progress::ScanProgress>,
    ) -> usize {
        let autoload_files = composer::parse_autoload_files(project_root, vendor_dir);
        let autoload_count = autoload_files.len();

        // Some frameworks (e.g. CakePHP) ship global function aliases in a
        // `*_global.php` sibling that is loaded via the application
        // bootstrap rather than Composer's `files` autoload, so it never
        // appears in `autoload_files.php`. Seed those siblings too, so
        // globals like `__()`/`h()` are indexed instead of resolving to
        // "unknown function".
        let sibling_globals = composer::discover_global_sibling_files(&autoload_files);

        // Work queue + visited set for following require_once chains.
        let mut file_queue: Vec<PathBuf> = autoload_files;
        file_queue.extend(sibling_globals);
        let mut visited: HashSet<PathBuf> = HashSet::new();

        // Every queued file is popped exactly once, so counting queue
        // pushes as total and pops as done keeps the counters balanced
        // even as `require_once` chains grow the queue.
        if let Some(p) = progress {
            p.begin_phase(0.0, 0.4, "Scanning autoload files");
            p.add_total(file_queue.len() as u64);
        }

        while let Some(file_path) = file_queue.pop() {
            if let Some(p) = progress {
                p.add_done(1);
            }
            // Canonicalise to avoid revisiting the same file via
            // different relative paths.
            let canonical = file_path.canonicalize().unwrap_or(file_path);
            if !visited.insert(canonical.clone()) {
                continue;
            }

            if let Ok(content) = std::fs::read(&canonical) {
                // Lightweight byte-level scan: extract symbol names
                // without building a full AST.  The classes go into
                // `fqn_uri_index` so find_or_load_class can lazily parse
                // them later.
                let scan = classmap_scanner::find_symbols(&content);
                self.symbols
                    .merge_scan(WorkspaceScanResult::from_file(scan, &canonical));

                let content_str = String::from_utf8_lossy(&content);

                // ── Phar detection ──────────────────────────────────
                if let Some(file_dir) = canonical.parent() {
                    let phar_paths = composer::detect_phar_references(&content_str, file_dir);
                    for phar_path in phar_paths {
                        self.scan_phar_archive(&phar_path);
                    }
                }

                // Follow require_once statements to discover more files.
                let require_paths = composer::extract_require_once_paths(&content_str);
                if let Some(file_dir) = canonical.parent() {
                    for rel_path in require_paths {
                        let resolved = file_dir.join(&rel_path);
                        if resolved.is_file() {
                            if let Some(p) = progress {
                                p.add_total(1);
                            }
                            file_queue.push(resolved);
                        }
                    }
                }
            }
        }

        // Record the visited autoload file paths and eagerly parse them.
        //
        // The byte-level scan above only discovers symbols at brace
        // depth 0.  Functions guarded by `if (! function_exists(...))`
        // (common in Laravel and similar helper files) live at brace
        // depth > 0 and are missed.  Without a full parse they would only
        // be found by the last-resort fallback in `find_or_load_function`,
        // which blocks the first interactive request that needs such a
        // function while it serially parses every unparsed autoload file.
        //
        // Parsing them here in parallel moves that one-time cost to
        // startup.  The paths are still recorded so the fallback (which
        // skips already-parsed files) remains correct for anything that
        // slips through.
        let visited: Vec<PathBuf> = visited.into_iter().collect();
        {
            let mut paths = self.symbols.autoload_file_paths.write();
            // A rediscovery pass walks the same autoload chain again, so
            // record only what is new; the fallback lookups that read
            // this list would otherwise re-check the same file per pass.
            let known: HashSet<&PathBuf> = paths.iter().collect();
            let mut fresh: Vec<PathBuf> = visited
                .iter()
                .filter(|path| !known.contains(path))
                .cloned()
                .collect();
            paths.append(&mut fresh);
        }
        if let Some(p) = progress {
            p.begin_phase(0.4, 1.0, "Parsing autoload helpers");
        }
        self.preload_autoload_files_with_progress(&visited, progress);

        autoload_count
    }

    /// Build a workspace scan by self-scanning a Composer project's
    /// autoload directories (PSR-4 + classmap + vendor packages).
    ///
    /// Used by the merged classmap + self-scan pipeline and by the
    /// `"self"` / `"full"` indexing strategies.  The `project_root`
    /// is the directory containing `composer.json` (either the
    /// workspace root for single-project, or a subproject root for
    /// monorepo).
    ///
    /// `skip_paths` contains absolute file paths that should be
    /// excluded from scanning (typically the file paths already
    /// present in the Composer classmap).  Pass an empty set to
    /// scan everything.
    pub(crate) fn build_self_scan_composer(
        &self,
        project_root: &Path,
        vendor_dir: &str,
        preloaded_package: Option<&composer::ComposerPackage>,
        skip_paths: &HashSet<PathBuf>,
        progress: Option<&crate::progress::ScanProgress>,
    ) -> WorkspaceScanResult {
        // Use the pre-parsed package when available; only read from disk
        // as a fallback (e.g. monorepo subproject calls).
        let owned_package;
        let package = match preloaded_package {
            Some(p) => p,
            None => {
                owned_package = composer::read_composer_package(project_root);
                match owned_package.as_ref() {
                    Some(p) => p,
                    None => {
                        let skip_dirs = HashSet::new();
                        if let Some(p) = progress {
                            p.begin_phase(0.0, 1.0, "Scanning workspace files");
                        }
                        return classmap_scanner::scan_workspace_fallback_full(
                            project_root,
                            &skip_dirs,
                            &self.index_filters(),
                            progress,
                            Some(self.followed_links()),
                        );
                    }
                }
            }
        };

        let scan_dirs = composer::extract_scan_dirs(package);

        let psr4_dirs: Vec<(String, PathBuf)> = scan_dirs
            .psr4
            .iter()
            .map(|(prefix, dir)| (prefix.clone(), project_root.join(dir)))
            .collect();

        let classmap_dirs: Vec<PathBuf> = scan_dirs
            .classmap
            .iter()
            .map(|dir| project_root.join(dir))
            .collect();

        // Scan user source directories (classes only for PSR-4).
        // Project sources are a small slice of the file count; vendor
        // packages dominate, so they get most of the current scope.
        if let Some(p) = progress {
            p.begin_phase(0.0, 0.2, "Scanning project files");
        }
        let filters = self.index_filters();
        let vendor_dir_paths = vec![project_root.join(vendor_dir)];
        let classmap = classmap_scanner::scan_psr4_directories_with_skip(
            &psr4_dirs,
            &classmap_dirs,
            &vendor_dir_paths,
            skip_paths,
            &filters,
            progress,
            Some(self.followed_links()),
        );

        // Scan vendor packages from installed.json.
        if let Some(p) = progress {
            p.begin_phase(0.2, 1.0, "Scanning vendor packages");
        }
        let explicit_deps = crate::composer::explicit_dependency_names(package);
        let mut vendor_scan = classmap_scanner::scan_vendor_packages_with_skip(
            project_root,
            vendor_dir,
            skip_paths,
            &explicit_deps,
            &filters,
            progress,
            Some(self.followed_links()),
        );

        let mut result = WorkspaceScanResult {
            classmap,
            package_roots: std::mem::take(&mut vendor_scan.package_roots),
            ..Default::default()
        };

        result.absorb_missing(vendor_scan);

        result
    }
}

/// The record of the Composer project at `root`, added if absent.
fn composer_project_entry<'a>(
    projects: &'a mut Vec<ComposerProjectPaths>,
    root: &Path,
) -> &'a mut ComposerProjectPaths {
    let index = match projects.iter().position(|project| project.root == root) {
        Some(index) => index,
        None => {
            projects.push(ComposerProjectPaths {
                root: root.to_path_buf(),
                ..Default::default()
            });
            projects.len() - 1
        }
    };
    &mut projects[index]
}

/// Append each of `items` that `list` does not already hold.
fn push_new<T: PartialEq>(list: &mut Vec<T>, items: Vec<T>) {
    for item in items {
        if !list.contains(&item) {
            list.push(item);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn vendor_registration_caches_raw_and_canonical_path_spellings() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().expect("tempdir");
        let canonical_vendor = dir.path().join("packages");
        std::fs::create_dir(&canonical_vendor).expect("create package directory");
        let aliased_vendor = dir.path().join("vendor");
        symlink(&canonical_vendor, &aliased_vendor).expect("create vendor alias");

        let backend = Backend::new_test();
        backend.add_vendor_dir(&aliased_vendor);
        backend.add_vendor_dir(&aliased_vendor);

        let paths = backend.workspace.vendor_dir_paths.lock();
        assert_eq!(paths.len(), 2, "repeat registration must stay deduplicated");
        assert!(paths.contains(&aliased_vendor));
        assert!(paths.contains(&canonical_vendor.canonicalize().unwrap()));
    }

    #[test]
    fn composer_rescan_registers_a_vendor_directory_created_after_startup() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("composer.json"), "{}").expect("write composer.json");
        let canonical_vendor = dir.path().join("packages");
        std::fs::create_dir(&canonical_vendor).expect("create package directory");
        let aliased_vendor = dir.path().join("vendor");
        symlink(&canonical_vendor, &aliased_vendor).expect("create vendor alias");

        let backend = Backend::new_test();
        assert!(backend.workspace.vendor_dir_paths.lock().is_empty());
        backend.rescan_composer_indexes(dir.path(), &[]);

        let paths = backend.workspace.vendor_dir_paths.lock();
        assert!(paths.contains(&aliased_vendor));
        assert!(paths.contains(&canonical_vendor.canonicalize().unwrap()));
    }
}
