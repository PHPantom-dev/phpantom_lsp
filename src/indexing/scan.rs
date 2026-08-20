//! Autoload and vendor scanning.
//!
//! These `impl Backend` methods register vendor directories, (re)build
//! the Composer-derived indexes, scan autoload files and PHAR archives,
//! and populate the function/constant indices from a workspace scan.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::classify_class_origin;
use crate::Backend;
use crate::classmap_scanner::{self, WorkspaceScanResult};
use crate::composer;
use crate::phar;

/// Files claimed per work-stealing block when scanning a phar archive.
const PHAR_SCAN_BLOCK_FILES: usize = 32;

/// A class found inside a phar: its FQN, the `archive.phar!inner.php`
/// sentinel path, and the `phar://` URI to open it with.
type PharClass = (String, PathBuf, String);

/// A block of phar classes, tagged with the block index it was claimed
/// under so results can be merged back in manifest order.
type PharScannedBlock = (usize, Vec<PharClass>);

/// Build the vendor URI prefixes (raw and canonicalized) for a vendor
/// directory path, used to detect and skip vendor files.
fn vendor_uri_prefixes_for_path(vendor_path: &std::path::Path) -> Vec<String> {
    let mut prefixes = vec![format!("{}/", crate::util::path_to_uri(vendor_path))];
    if let Ok(canonical) = vendor_path.canonicalize() {
        prefixes.push(format!("{}/", crate::util::path_to_uri(&canonical)));
    }
    prefixes.sort();
    prefixes.dedup();
    prefixes
}

impl Backend {
    /// Register a vendor directory path and its URI prefix for
    /// vendor-file detection.
    pub(crate) fn add_vendor_dir(&self, vendor_path: &std::path::Path) {
        // Keep both filesystem spellings. Walkers normally yield the raw
        // workspace spelling, while Composer package discovery canonicalizes
        // its files; on macOS those can be `/var` and `/private/var` for the
        // same vendor tree. Caching both here keeps the hot lookup paths free
        // of filesystem calls.
        {
            let mut paths = self.workspace.vendor_dir_paths.lock();
            let mut insert = |path: PathBuf| {
                if !paths.contains(&path) {
                    paths.push(path);
                }
            };
            insert(vendor_path.to_path_buf());
            if let Ok(canonical) = vendor_path.canonicalize() {
                insert(canonical);
            }
        }
        // Store URI prefixes for URI-level skip logic (diagnostics, find
        // references, rename).  Keep both raw and canonical forms so macOS
        // `/tmp` vs `/private/tmp` style aliases do not leak vendor files into
        // workspace indexing.
        let new_prefixes = vendor_uri_prefixes_for_path(vendor_path);
        {
            let mut prefixes = self.workspace.vendor_uri_prefixes.lock();
            for prefix in new_prefixes {
                if !prefixes.contains(&prefix) {
                    prefixes.push(prefix);
                }
            }
        }
    }

    /// Rebuild the vendor-derived indexes after a `composer.json` /
    /// `composer.lock` change (e.g. a `composer install` or `update`).
    ///
    /// Re-reads PSR-4 mappings, rebuilds the vendor classmap and the
    /// autoload function/constant indexes, rescans autoload files, and
    /// clears the resolved-class caches so stale vendor versions do not
    /// linger.  This is the synchronous body of
    /// [`did_change_watched_files`](Self::did_change_watched_files)'s
    /// composer branch, factored out so it can run on a blocking thread.
    pub(crate) fn rescan_composer_indexes(&self, root: &std::path::Path) {
        if let Some(pkg) = composer::read_composer_package(root) {
            let mut mappings = composer::extract_psr4_mappings_from_package(&pkg);
            let vendor_dir = composer::get_vendor_dir(&pkg);
            mappings.extend(composer::extract_path_repo_psr4_mappings(root, &vendor_dir));
            // Keep the merged list longest-prefix-first so path-repo
            // namespaces are matched before any shorter root prefix.
            mappings.sort_by_key(|m| std::cmp::Reverse(m.prefix.len()));
            *self.workspace.psr4_mappings.write() = mappings;

            let vendor_path = root.join(&vendor_dir);
            // `vendor/` may not have existed during initialization (a fresh
            // clone before `composer install`). Register it again now so the
            // shared vendor filters cache both raw and canonical spellings.
            self.add_vendor_dir(&vendor_path);

            // Rebuild vendor classmap, tracking dependency provenance so
            // completion ranking stays accurate after a composer change.
            let explicit_deps = composer::explicit_dependency_names(&pkg);
            let mut vendor_scan = classmap_scanner::scan_vendor_packages_with_skip(
                root,
                &vendor_dir,
                &HashSet::new(),
                &explicit_deps,
                None,
                self.config().indexing.follow_links(),
            );
            // Package roots came out of the same `installed.json` parse
            // `scan_vendor_packages_with_skip` already did; no need to
            // re-read and re-parse the file a second time here.
            let vendor_package_roots = std::mem::take(&mut vendor_scan.package_roots);
            {
                let vendor_uri_prefixes = vendor_uri_prefixes_for_path(&vendor_path);

                // Remove old vendor entries and insert new ones.
                let mut idx = self.symbols.fqn_uri_index.write();
                let mut origins = self.symbols.fqn_origin_index.write();
                idx.retain(|_, v| {
                    !vendor_uri_prefixes
                        .iter()
                        .any(|prefix| v.starts_with(prefix.as_str()))
                });
                for (fqn, path) in vendor_scan.classmap {
                    let origin =
                        vendor_scan
                            .class_origins
                            .get(&fqn)
                            .copied()
                            .unwrap_or_else(|| {
                                classify_class_origin(&path, &vendor_path, &vendor_package_roots)
                            });
                    origins.insert(fqn.clone(), origin);
                    idx.insert(fqn, crate::util::path_to_uri(&path));
                }
            }
            {
                let mut fi = self.symbols.autoload_function_index.write();
                let mut origins = self.symbols.autoload_function_origin_index.write();
                // Purge functions that pointed into the old vendor tree
                // before re-inserting, so symbols removed by a
                // `composer update` no longer resolve.
                fi.retain(|_, v| !v.starts_with(&vendor_path));
                for (fqn, path) in vendor_scan.function_index {
                    let origin = vendor_scan
                        .function_origins
                        .get(&fqn)
                        .copied()
                        .unwrap_or(crate::ClassCompletionOrigin::Project);
                    origins.insert(fqn.clone(), origin);
                    fi.insert(fqn, path);
                }
            }
            {
                let mut ci = self.symbols.autoload_constant_index.write();
                let mut origins = self.symbols.autoload_constant_origin_index.write();
                // Same for constants from the old vendor tree.
                ci.retain(|_, v| !v.starts_with(&vendor_path));
                for (name, path) in vendor_scan.constant_index {
                    let origin = vendor_scan
                        .constant_origins
                        .get(&name)
                        .copied()
                        .unwrap_or(crate::ClassCompletionOrigin::Project);
                    origins.insert(name.clone(), origin);
                    ci.insert(name, path);
                }
            }

            // Refresh the cached package roots for path-based lookups.
            *self.workspace.vendor_package_origin_roots.write() = vendor_package_roots;

            // Rescan autoload files (they may have changed).
            self.scan_autoload_files(root, &vendor_dir, None);
        }

        // Clear all cached class info since vendor classes may have
        // changed versions.  The negative-cache clear comes last of the
        // three: it also retires the memoised class lookups, which must
        // happen after the index it memoises has been emptied.
        self.symbols.fqn_class_index.write().clear();
        // The runner-up declarations describe the vendor tree that was just
        // rescanned; keeping them would promote a copy of a class that the
        // update may have moved or removed.
        self.symbols.duplicate_classes.write().clear();
        self.symbols.method_store.write().clear();
        self.symbols.gti_index.write().clear();
        self.clear_class_not_found_cache();
        self.resolved_class_cache.write().clear();
        self.member_completion_cache.lock().clear();
    }

    /// Scan autoload files for a single project root and populate the
    /// autoload indices.  Returns the number of autoload file entries
    /// found.
    pub(crate) fn scan_autoload_files(
        &self,
        project_root: &std::path::Path,
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
                let uri = crate::util::path_to_uri(&canonical);

                // Lightweight byte-level scan: extract symbol names
                // without building a full AST.
                let scan = classmap_scanner::find_symbols(&content);

                {
                    let mut idx = self.symbols.autoload_function_index.write();
                    for fqn in &scan.functions {
                        idx.or_insert_with(fqn.as_str(), || canonical.clone());
                    }
                }

                {
                    let mut idx = self.symbols.autoload_constant_index.write();
                    for name in &scan.constants {
                        idx.entry(name.clone()).or_insert_with(|| canonical.clone());
                    }
                }

                // Populate fqn_uri_index so find_or_load_class can
                // lazily parse these classes later.
                {
                    let mut idx = self.symbols.fqn_uri_index.write();
                    for fqn in &scan.classes {
                        idx.or_insert_with(fqn.as_str(), || uri.clone());
                    }
                }

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
            paths.extend(visited.iter().cloned());
        }
        if let Some(p) = progress {
            p.begin_phase(0.4, 1.0, "Parsing autoload helpers");
        }
        self.preload_autoload_files_with_progress(&visited, progress);

        autoload_count
    }

    /// Parse a `.phar` archive and register its PHP classes in the
    /// fqn_uri_index for lazy loading.
    ///
    /// The phar's raw bytes are read from disk, parsed by
    /// [`phar::PharArchive`], and stored in
    /// [`phar_archives`](crate::Backend::phar_archives).  Each `.php`
    /// file inside the archive is scanned with the lightweight
    /// [`find_classes`](classmap_scanner::find_classes) byte scanner,
    /// and discovered classes are registered in:
    ///
    /// - `fqn_uri_index` — with a sentinel path like
    ///   `/path/to/phpstan.phar!src/Type/Type.php` (the `!` separator
    ///   tells [`parse_and_cache_file`](crate::Backend::parse_and_cache_file)
    ///   to extract content from the phar instead of reading from disk)
    ///   and a `phar://` URI for completions and workspace symbols
    fn scan_phar_archive(&self, phar_path: &Path) {
        // Avoid scanning the same phar twice.
        if self.phar_archives.read().contains_key(phar_path) {
            return;
        }

        // Map the archive rather than copying it onto the heap: the scan
        // below only touches the pages of the `.php` entries it reads, and
        // the map is dropped when this function returns.
        let data = match classmap_scanner::read_for_scan(phar_path) {
            Ok(d) => d,
            Err(_) => return,
        };
        let archive = match phar::PharArchive::parse(phar_path, &data) {
            Some(a) => a,
            None => {
                tracing::warn!("failed to parse phar archive: {}", phar_path.display());
                return;
            }
        };

        // Collect PHP file paths first so we can iterate while
        // holding the archive reference.
        let php_files: Vec<String> = archive
            .file_paths()
            .filter(|p| p.ends_with(".php"))
            .map(String::from)
            .collect();

        // A tool phar is large (phpstan's is tens of megabytes of PHP), so
        // the byte scan is spread over the cores.  Workers claim blocks
        // from a shared cursor and their results are merged in manifest
        // order, keeping the first-wins index inserts below identical to a
        // sequential scan.
        let mut scanned: Vec<PharScannedBlock> = {
            let next_block = AtomicUsize::new(0);
            let n_blocks = php_files.len().div_ceil(PHAR_SCAN_BLOCK_FILES);
            let n_threads = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
                .min(n_blocks.max(1));
            std::thread::scope(|s| {
                let handles: Vec<_> = (0..n_threads)
                    .map(|_| {
                        let next_block = &next_block;
                        let archive = &archive;
                        let data = &data;
                        let php_files = &php_files;
                        s.spawn(move || {
                            let mut out: Vec<PharScannedBlock> = Vec::new();
                            loop {
                                let b = next_block.fetch_add(1, Ordering::Relaxed);
                                if b >= n_blocks {
                                    break;
                                }
                                let start = b * PHAR_SCAN_BLOCK_FILES;
                                let end = (start + PHAR_SCAN_BLOCK_FILES).min(php_files.len());
                                let mut local: Vec<PharClass> = Vec::new();
                                for internal_path in &php_files[start..end] {
                                    let Some(range) = archive.file_range(internal_path) else {
                                        continue;
                                    };
                                    let Some(content) = data.get(range) else {
                                        continue;
                                    };
                                    for fqn in classmap_scanner::find_classes(content) {
                                        // Sentinel path: "archive.phar!internal/path.php"
                                        let sentinel = PathBuf::from(format!(
                                            "{}!{}",
                                            phar_path.display(),
                                            internal_path
                                        ));
                                        let phar_uri = format!(
                                            "phar://{}/{}",
                                            phar_path.display(),
                                            internal_path
                                        );
                                        local.push((fqn, sentinel, phar_uri));
                                    }
                                }
                                if !local.is_empty() {
                                    out.push((b, local));
                                }
                            }
                            out
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .flat_map(|h| {
                        h.join().unwrap_or_else(|_| {
                            tracing::error!("PHPantom: thread panic while scanning phar archive");
                            Vec::new()
                        })
                    })
                    .collect()
            })
        };
        scanned.sort_unstable_by_key(|(b, _)| *b);

        let mut classmap_entries: Vec<(String, PathBuf)> = Vec::new();
        let mut fqn_uri_entries: Vec<(String, String)> = Vec::new();
        for (_, batch) in scanned {
            for (fqn, sentinel, phar_uri) in batch {
                classmap_entries.push((fqn.clone(), sentinel));
                fqn_uri_entries.push((fqn, phar_uri));
            }
        }

        let class_count = classmap_entries.len();

        {
            let mut idx = self.symbols.fqn_uri_index.write();
            for (fqn, path) in classmap_entries {
                idx.or_insert_with(fqn, || crate::util::path_to_uri(&path));
            }
            for (fqn, uri) in fqn_uri_entries {
                idx.or_insert_with(fqn, || uri);
            }
        }

        // Clear the negative class cache so that classes previously
        // looked up (and cached as "not found") before the phar was
        // scanned can now be resolved.
        if class_count > 0 {
            self.clear_class_not_found_cache();
        }

        tracing::info!(
            "scanned phar {}: {} PHP files, {} classes",
            phar_path.display(),
            php_files.len(),
            class_count,
        );

        // Store the parsed archive for lazy content extraction.
        self.phar_archives
            .write()
            .insert(phar_path.to_owned(), archive);
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
        project_root: &std::path::Path,
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
                            progress,
                            self.config().indexing.follow_links(),
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
        let vendor_dir_paths = vec![project_root.join(vendor_dir)];
        let classmap = classmap_scanner::scan_psr4_directories_with_skip(
            &psr4_dirs,
            &classmap_dirs,
            &vendor_dir_paths,
            skip_paths,
            progress,
            self.config().indexing.follow_links(),
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
            progress,
            self.config().indexing.follow_links(),
        );

        let mut result = WorkspaceScanResult {
            classmap,
            package_roots: std::mem::take(&mut vendor_scan.package_roots),
            ..Default::default()
        };

        // Origins ride alongside the winning path: only record one for a
        // vendor entry when it actually wins the merge (i.e. the project
        // scan didn't already provide this FQN), so a symbol's recorded
        // origin always matches the file it resolves to.
        for (fqcn, path) in vendor_scan.classmap {
            if let std::collections::hash_map::Entry::Vacant(e) =
                result.classmap.entry(fqcn.clone())
            {
                e.insert(path);
                if let Some(&origin) = vendor_scan.class_origins.get(&fqcn) {
                    result.class_origins.insert(fqcn, origin);
                }
            }
        }
        for (fqn, path) in vendor_scan.function_index {
            if let std::collections::hash_map::Entry::Vacant(e) =
                result.function_index.entry(fqn.clone())
            {
                e.insert(path);
                if let Some(&origin) = vendor_scan.function_origins.get(&fqn) {
                    result.function_origins.insert(fqn, origin);
                }
            }
        }
        for (name, path) in vendor_scan.constant_index {
            if let std::collections::hash_map::Entry::Vacant(e) =
                result.constant_index.entry(name.clone())
            {
                e.insert(path);
                if let Some(&origin) = vendor_scan.constant_origins.get(&name) {
                    result.constant_origins.insert(name, origin);
                }
            }
        }

        result
    }

    /// Store the function and constant indices from a workspace scan
    /// into the backend's shared maps.
    ///
    /// Only has an effect for non-Composer projects (the "no
    /// `composer.json`" scenario) where the full-scan populates
    /// function and constant entries.  For Composer projects the scan
    /// result's function and constant indices are empty because those
    /// symbols are discovered via the `autoload_files.php` scan loop
    /// in `initialized()` instead.
    pub(crate) fn populate_autoload_indices(&self, scan: &WorkspaceScanResult) {
        if !scan.function_index.is_empty() {
            let mut idx = self.symbols.autoload_function_index.write();
            let mut origins = self.symbols.autoload_function_origin_index.write();
            for (fqn, path) in &scan.function_index {
                idx.or_insert_with(fqn.as_str(), || path.clone());
                let origin = scan
                    .function_origins
                    .get(fqn)
                    .copied()
                    .unwrap_or(crate::ClassCompletionOrigin::Project);
                origins.insert(fqn.clone(), origin);
            }
        }
        if !scan.constant_index.is_empty() {
            let mut idx = self.symbols.autoload_constant_index.write();
            let mut origins = self.symbols.autoload_constant_origin_index.write();
            for (name, path) in &scan.constant_index {
                idx.entry(name.clone()).or_insert_with(|| path.clone());
                let origin = scan
                    .constant_origins
                    .get(name)
                    .copied()
                    .unwrap_or(crate::ClassCompletionOrigin::Project);
                origins.insert(name.clone(), origin);
            }
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
        backend.rescan_composer_indexes(dir.path());

        let paths = backend.workspace.vendor_dir_paths.lock();
        assert!(paths.contains(&aliased_vendor));
        assert!(paths.contains(&canonical_vendor.canonicalize().unwrap()));
    }
}
