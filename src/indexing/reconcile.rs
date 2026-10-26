//! Bringing the index back in line with changed file filters.
//!
//! `[indexing] exclude` and `extensions` can change mid-session, from a
//! `.phpantom.toml` edit or from the settings an editor forwards. The
//! recompiled filters govern every scan from that point on, but the index
//! was built under the old ones, so both directions are wrong until the
//! two are reconciled: a newly excluded tree keeps answering workspace
//! symbol search and completion, and a newly included one contributes
//! nothing. A new `[indexing] include` entry is the same kind of change:
//! it names files the last walk never saw, and so is a folder added to
//! the editor's workspace. A folder removed from it leaves the index at
//! once.
//!
//! Eviction is answered from the indexes themselves and runs
//! immediately. Re-inclusion cannot be: nothing in memory records the
//! files a walk skipped, so it takes another walk. That walk is
//! debounced, since a settings edit reaches the server as a burst of
//! notifications and each one would otherwise queue a scan of its own.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tower_lsp_server::gen_lsp_types::FileChangeType;

use crate::Backend;
use crate::classmap_scanner::IndexFilters;

/// How long the filters must hold still before the workspace is walked
/// again. Long enough to collapse a client that pushes its settings in
/// several notifications, short enough that a user who removed an
/// exclude to look something up does not sit and wait on it.
const REDISCOVERY_DEBOUNCE: Duration = Duration::from_millis(500);

/// Everything that decides what a discovery walk finds, captured before a
/// settings change so the reconciliation can tell which way it moved.
///
/// Every entry point that can change one of these inputs compares a
/// snapshot rather than the inputs one by one, so a new input is added
/// here once instead of at each site.
pub(crate) struct DiscoveryInputs {
    filters: Arc<IndexFilters>,
    /// The `[indexing] include` entries as written.
    include: Vec<String>,
    /// The editor's other workspace folders, after the containment dedup
    /// in [`Backend::extra_roots`], so a folder that was hidden inside a
    /// removed parent counts as added.
    pub(super) extra_roots: Vec<PathBuf>,
    /// The stub paths in priority order, as
    /// [`Backend::stub_paths`] resolves them.
    stub_paths: Vec<PathBuf>,
}

impl DiscoveryInputs {
    /// Whether replacing `previous` with `self` can bring a file into the
    /// index that `previous` kept out, which takes another walk.
    pub(super) fn may_admit_more_than(&self, previous: &DiscoveryInputs) -> bool {
        self.filters.may_admit_more_than(&previous.filters)
            || self
                .include
                .iter()
                .any(|entry| !previous.include.contains(entry))
            || self
                .extra_roots
                .iter()
                .any(|root| !previous.extra_roots.contains(root))
            // A path that stops being a stub path is code the walks skipped
            // while it was one.
            || previous
                .stub_paths
                .iter()
                .any(|path| !self.stub_paths.contains(path))
    }
}

impl Backend {
    /// Snapshot the inputs of workspace discovery. Take it *before*
    /// applying a settings change and hand it to
    /// [`reconcile_index_for_input_change`](Backend::reconcile_index_for_input_change).
    pub(crate) fn discovery_inputs(&self) -> DiscoveryInputs {
        DiscoveryInputs {
            filters: self.index_filters(),
            include: self.config().indexing.include.unwrap_or_default(),
            extra_roots: self.extra_roots(),
            stub_paths: self.stub_paths(),
        }
    }

    /// Reconcile the symbol indexes with the discovery inputs after a
    /// change.
    ///
    /// Cheap when nothing moved: the walk is only scheduled when a path
    /// or extension could have come back into scope or a new `include`
    /// entry or workspace folder names files no walk has seen, and a
    /// purely widened exclude list is answered by eviction alone. A
    /// removed `include` entry is not evicted; its symbols stay until the
    /// next restart. A removed workspace folder is, since taking a folder
    /// out of a workspace is routine.
    pub(crate) fn reconcile_index_for_input_change(&self, previous: &DiscoveryInputs) {
        let current = self.discovery_inputs();
        let mut rediscover = current.may_admit_more_than(previous);

        let removed_roots: Vec<&PathBuf> = previous
            .extra_roots
            .iter()
            .filter(|root| !current.extra_roots.contains(root))
            .collect();
        if !removed_roots.is_empty() {
            let (evicted, still_covered) = self.retire_extra_roots(&removed_roots);
            tracing::info!(
                "PHPantom: {} workspace folder(s) removed, {evicted} indexed file(s) dropped",
                removed_roots.len()
            );
            self.clear_resolved_class_cache();
            self.member_completion_cache.lock().clear();
            rediscover |= still_covered;
        }

        if !Arc::ptr_eq(&previous.filters, &current.filters) {
            let evicted = self.evict_newly_filtered_symbols(&previous.filters, &current.filters);
            if evicted > 0 {
                tracing::info!("PHPantom: file filters now exclude {evicted} indexed file(s)");
                // Whatever those files declared may be baked into a
                // resolved class (a parent, a trait, a mixin) or a cached
                // completion list, neither of which the index purge
                // reaches.
                self.clear_resolved_class_cache();
                self.member_completion_cache.lock().clear();
            }
        }

        // The stub index is rebuilt whole rather than grown the way an
        // `include` entry is: a removed stub path that kept overriding the
        // declaration the user went back to would be worse than one that
        // merely kept filling gaps.  The order matters too, so a
        // reordering is a change.
        if current.stub_paths != previous.stub_paths {
            self.refresh_stub_paths();
        }

        if rediscover {
            self.schedule_filter_rediscovery();
        }
    }

    /// Drop every index entry describing a file the change just put out
    /// of scope, and report how many files were dropped.
    ///
    /// Needs no disk access: the candidates are the files the indexes
    /// already name.
    ///
    /// Only the *change* is applied. A file both filter sets exclude was
    /// put in the index by something the filters do not govern (a
    /// Composer classmap entry names its file without any walk reaching
    /// it), and evicting it would make an unrelated settings edit
    /// silently unresolve classes a restart would still resolve.
    fn evict_newly_filtered_symbols(
        &self,
        previous: &IndexFilters,
        current: &IndexFilters,
    ) -> usize {
        let filtered = self.indexed_files_matching(|path| {
            is_out_of_scope(current, path) && !is_out_of_scope(previous, path)
        });
        self.evict_indexed_files(filtered)
    }

    /// Every file on disk the indexes name whose path satisfies
    /// `matches`, keyed by the URI the index holds it under, leaving out
    /// the documents open in the editor.
    ///
    /// URIs that name no file on disk (embedded stubs, PHAR members) are
    /// never offered to `matches`: no setting about paths can speak about
    /// them.
    pub(super) fn indexed_files_matching(
        &self,
        matches: impl Fn(&Path) -> bool,
    ) -> HashMap<String, PathBuf> {
        self.indexed_files_where(|_| true, matches)
    }

    /// [`indexed_files_matching`](Self::indexed_files_matching) for the
    /// files under `trees`, each a directory or a single file.
    ///
    /// Answered from the URIs' prefixes, so the many URIs outside `trees`
    /// are rejected without being decoded into a path.
    pub(super) fn indexed_files_under(&self, trees: &[PathBuf]) -> HashMap<String, PathBuf> {
        let prefixes: Vec<(String, String)> = trees
            .iter()
            .map(|tree| {
                (
                    crate::util::path_to_uri(tree),
                    crate::util::dir_uri_prefix(tree),
                )
            })
            .collect();
        self.indexed_files_where(
            |uri| {
                prefixes
                    .iter()
                    .any(|(exact, prefix)| uri == exact || uri.starts_with(prefix.as_str()))
            },
            |path| trees.iter().any(|tree| path.starts_with(tree)),
        )
    }

    /// The indexed files whose URI passes `uri_may_match` and whose path
    /// then satisfies `matches`, leaving out the open documents.
    fn indexed_files_where(
        &self,
        uri_may_match: impl Fn(&str) -> bool,
        matches: impl Fn(&Path) -> bool,
    ) -> HashMap<String, PathBuf> {
        let mut found: HashMap<String, PathBuf> = HashMap::new();
        let mut note = |uri: &str| {
            if !uri_may_match(uri) || found.contains_key(uri) {
                return;
            }
            if let Some(path) = crate::util::uri_to_path(uri)
                && matches(&path)
            {
                found.insert(uri.to_string(), path);
            }
        };

        for uri in self.symbols.fqn_uri_index.read().values() {
            note(uri);
        }
        for uri in self.symbols.uri_classes_index.read().keys() {
            note(uri);
        }
        for uri in self.uris_with_symbol_maps() {
            note(&uri);
        }
        for uri in self.parsed_uris.read().iter() {
            note(uri);
        }
        // A file whose only symbols are functions or constants is in
        // neither class index, and one that declares nothing at all is
        // still parsed and still holds a symbol map.  These indexes hold
        // paths, so the path is tested before a URI is built for it.
        let mut note_path = |path: &Path| {
            if matches(path) {
                let uri = crate::util::path_to_uri(path);
                if uri_may_match(&uri) && !found.contains_key(&uri) {
                    found.insert(uri, path.to_path_buf());
                }
            }
        };
        for path in self.symbols.autoload_function_index.read().values() {
            note_path(path);
        }
        for path in self.symbols.autoload_constant_index.read().values() {
            note_path(path);
        }

        // A document open in the editor is served whatever the indexing
        // settings say (`did_open` parses it outright), so dropping its
        // symbol map would break the file the user is looking at to
        // honour a setting about workspace discovery.
        if !found.is_empty() {
            let open = self.open_files.read();
            found.retain(|uri, _| !open.contains_key(uri));
        }
        found
    }

    /// Drop `files` from every index and report how many there were.
    ///
    /// They are handed to [`reindex_files_batch`](Backend::reindex_files_batch)
    /// as deletions rather than purged here, so a file leaves the index by
    /// exactly the same route a deleted one does and no index is missed.
    pub(super) fn evict_indexed_files(&self, files: HashMap<String, PathBuf>) -> usize {
        if files.is_empty() {
            return 0;
        }
        let changes: Vec<(String, PathBuf, FileChangeType)> = files
            .into_iter()
            .map(|(uri, path)| (uri, path, FileChangeType::Deleted))
            .collect();
        self.reindex_files_batch(&changes);
        changes.len()
    }

    /// Queue the workspace walk a re-include needs, superseding any walk
    /// still waiting out its debounce.
    fn schedule_filter_rediscovery(&self) {
        // A headless backend (the test suite, the `analyse` and `fix`
        // pipelines) builds its index once with the filters already in
        // force, and has no client to report the walk's progress to.
        if self.client.is_none() || self.workspace.workspace_root.read().is_none() {
            return;
        }

        let generation = self
            .workspace
            .filter_rediscovery_generation
            .fetch_add(1, Ordering::AcqRel)
            + 1;
        let backend = self.clone_for_blocking();
        tokio::spawn(async move {
            tokio::time::sleep(REDISCOVERY_DEBOUNCE).await;
            if backend
                .workspace
                .filter_rediscovery_generation
                .load(Ordering::Acquire)
                != generation
            {
                return;
            }
            backend.rediscover_workspace().await;
        });
    }

    /// Walk the workspace again so files the widened filters admit are
    /// indexed, then retire what the narrower index had answered.
    ///
    /// Runs the same discovery pass startup does rather than a walk of
    /// the subtrees that changed: the pattern lists say which globs
    /// moved, not which directories they name, and a pass over an
    /// already-populated index only inserts what is missing.
    ///
    /// Awaited on the runtime, like the first pass in `initialized`.
    /// The scans it drives fan out to their own worker threads, so what
    /// waits here is one background task, not the message loop.
    pub(crate) async fn rediscover_workspace(&self) {
        let Some(root) = self.workspace.workspace_root.read().clone() else {
            return;
        };
        tracing::info!("PHPantom: discovery inputs changed, rediscovering workspace symbols");

        let progress_token = self.progress_create("phpantom/rediscovery").await;
        if let Some(ref tok) = progress_token {
            self.progress_begin(
                tok,
                "PHPantom: Reindexing",
                Some("Indexing settings changed".to_string()),
            )
            .await;
        }
        let progress = crate::progress::ScanProgress::new();
        let poller = progress_token
            .as_ref()
            .map(|tok| self.spawn_progress_poller(tok.clone(), Arc::clone(&progress)));

        let composer_package = match crate::composer::try_read_composer_package(&root) {
            Ok(package) => package,
            Err(e) => {
                self.log(
                    tower_lsp_server::gen_lsp_types::MessageType::Warning,
                    format!("{e}; PSR-4 mappings from it are ignored"),
                )
                .await;
                None
            }
        };
        self.discover_workspace_symbols(
            &root,
            self.php_version(),
            composer_package,
            Some(&progress),
        )
        .await;

        if let Some(poller) = poller {
            poller.finish().await;
        }
        if let Some(ref tok) = progress_token {
            let classmap_count = self.symbols.fqn_uri_index.read().len();
            self.progress_end(tok, Some(format!("Indexed {classmap_count} classes")))
                .await;
        }

        // Classes that were unresolvable a moment ago now exist, and a
        // class already resolved may have been missing a parent, trait,
        // or mixin the old filters hid.
        self.clear_not_found_caches();
        self.clear_resolved_class_cache();
        self.member_completion_cache.lock().clear();

        // Parse what the walk added, the way startup does; the pass
        // skips every file that already has a symbol map.
        self.start_full_background_index().await;
        self.request_diagnostic_refresh().await;
    }
}

/// Whether `filters` keep `path` out of workspace discovery, either by
/// excluding it or by no longer counting its extension as PHP source.
fn is_out_of_scope(filters: &IndexFilters, path: &std::path::Path) -> bool {
    filters.is_excluded_path(path, false) || !filters.is_php_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;

    /// A backend rooted at a temp workspace holding one indexed class
    /// per given relative path, discovered the way a workspace scan
    /// records them.
    fn backend_indexing(files: &[&str]) -> (Backend, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let backend = Backend::new_test();
        *backend.workspace.workspace_root.write() = Some(dir.path().to_path_buf());

        for (i, relative) in files.iter().enumerate() {
            let path = dir.path().join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, format!("<?php\nclass C{i} {{}}\n")).unwrap();
            let uri = crate::util::path_to_uri(&path);
            backend
                .symbols
                .with_class_declarations(|decls| decls.note_discovered(&format!("C{i}"), uri));
        }
        (backend, dir)
    }

    fn client_options(exclude: &[&str]) -> config::ClientIndexingOptions {
        config::ClientIndexingOptions {
            exclude: exclude.iter().map(|s| s.to_string()).collect(),
            extensions: Vec::new(),
        }
    }

    /// The complaint the reconciliation exists to answer: a tree the
    /// user just excluded keeps turning up in completion and workspace
    /// symbol search until the server restarts.
    #[test]
    fn a_newly_excluded_tree_leaves_the_index() {
        let (backend, _dir) = backend_indexing(&["src/Kept.php", "generated/Dropped.php"]);

        let previous = backend.discovery_inputs();
        backend.set_client_indexing_options(client_options(&["generated"]));
        backend.reconcile_index_for_input_change(&previous);

        let index = backend.symbols.fqn_uri_index.read();
        assert!(index.get("C0").is_some(), "an unfiltered class must stay");
        assert!(
            index.get("C1").is_none(),
            "a class under a newly excluded path must go"
        );
    }

    /// Dropping an extension is the same kind of change as excluding a
    /// path: the files stop being PHP source, so what they declared has
    /// to stop resolving.
    #[test]
    fn a_withdrawn_extension_evicts_its_files() {
        let (backend, _dir) = backend_indexing(&["src/Thing.module"]);
        backend.set_client_indexing_options(config::ClientIndexingOptions {
            exclude: Vec::new(),
            extensions: vec!["module".to_string()],
        });

        let previous = backend.discovery_inputs();
        backend.set_client_indexing_options(config::ClientIndexingOptions::default());
        backend.reconcile_index_for_input_change(&previous);

        assert!(backend.symbols.fqn_uri_index.read().get("C0").is_none());
    }

    /// Excluding a folder is about workspace discovery, not about the
    /// document in front of the user. Evicting an open file's maps would
    /// break hover and diagnostics in the file they are looking at.
    #[test]
    fn an_open_file_survives_being_excluded() {
        let (backend, dir) = backend_indexing(&["generated/Open.php"]);
        let uri = crate::util::path_to_uri(&dir.path().join("generated/Open.php"));
        backend
            .open_files
            .write()
            .insert(uri, Arc::new("<?php\nclass C0 {}\n".to_string()));

        let previous = backend.discovery_inputs();
        backend.set_client_indexing_options(client_options(&["generated"]));
        backend.reconcile_index_for_input_change(&previous);

        assert!(backend.symbols.fqn_uri_index.read().get("C0").is_some());
    }

    /// Only what the change put out of scope is evicted. A file both
    /// filter sets exclude is in the index because something the filters
    /// do not govern put it there (Composer's classmap names its file
    /// without any walk reaching it), so an unrelated settings edit must
    /// not quietly unresolve classes a restart would still resolve.
    #[test]
    fn a_file_excluded_before_and_after_is_left_alone() {
        let (backend, _dir) = backend_indexing(&["src/Kept.php", "generated/FromClassmap.php"]);
        backend.set_client_indexing_options(client_options(&["generated"]));

        let previous = backend.discovery_inputs();
        backend.set_client_indexing_options(client_options(&["generated", "build"]));
        backend.reconcile_index_for_input_change(&previous);

        let index = backend.symbols.fqn_uri_index.read();
        assert!(index.get("C0").is_some());
        assert!(index.get("C1").is_some());
    }

    /// A new `include` entry names files no walk has seen, so it needs a
    /// rescan even when the filters did not move. A dropped one does not:
    /// a walk cannot find less than the index already holds.
    #[test]
    fn a_new_include_entry_asks_for_a_walk() {
        let inputs = |include: &[&str]| DiscoveryInputs {
            filters: IndexFilters::empty(),
            include: include.iter().map(|s| s.to_string()).collect(),
            extra_roots: Vec::new(),
            stub_paths: Vec::new(),
        };

        assert!(inputs(&[".castor", "stub.php"]).may_admit_more_than(&inputs(&[".castor"])));
        assert!(!inputs(&[".castor"]).may_admit_more_than(&inputs(&[".castor", "stub.php"])));
        assert!(!inputs(&[".castor"]).may_admit_more_than(&inputs(&[".castor"])));
    }

    /// A path that stops being a stub path holds code the walks skipped
    /// while it was one. A new stub path takes no walk: the stub index
    /// scans it itself.
    #[test]
    fn a_dropped_stub_path_asks_for_a_walk() {
        let inputs = |stub_paths: &[&str]| DiscoveryInputs {
            filters: IndexFilters::empty(),
            include: Vec::new(),
            extra_roots: Vec::new(),
            stub_paths: stub_paths.iter().map(PathBuf::from).collect(),
        };

        assert!(inputs(&[]).may_admit_more_than(&inputs(&["/stubs"])));
        assert!(!inputs(&["/stubs"]).may_admit_more_than(&inputs(&[])));
    }

    /// Discovery re-run over a populated index has to find the files the
    /// widened filters admit, without disturbing the entries that were
    /// already there.
    #[tokio::test]
    async fn rediscovery_indexes_what_the_filters_now_admit() {
        let (backend, dir) = backend_indexing(&["src/Kept.php"]);
        std::fs::create_dir_all(dir.path().join("generated")).unwrap();
        std::fs::write(
            dir.path().join("generated/Returned.php"),
            "<?php\nclass Returned {}\n",
        )
        .unwrap();

        backend.rediscover_workspace().await;

        let index = backend.symbols.fqn_uri_index.read();
        assert!(index.get("C0").is_some(), "an indexed class must survive");
        assert!(
            index.get("Returned").is_some(),
            "the walk must pick up what was not indexed before"
        );
    }
}
