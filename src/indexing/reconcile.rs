//! Bringing the index back in line with changed file filters.
//!
//! `[indexing] exclude` and `extensions` can change mid-session, from a
//! `.phpantom.toml` edit or from the settings an editor forwards. The
//! recompiled filters govern every scan from that point on, but the index
//! was built under the old ones, so both directions are wrong until the
//! two are reconciled: a newly excluded tree keeps answering workspace
//! symbol search and completion, and a newly included one contributes
//! nothing.
//!
//! Eviction is answered from the indexes themselves and runs
//! immediately. Re-inclusion cannot be: nothing in memory records the
//! files a walk skipped, so it takes another walk. That walk is
//! debounced, since a settings edit reaches the server as a burst of
//! notifications and each one would otherwise queue a scan of its own.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tower_lsp::lsp_types::{FileChangeType, Url};

use crate::Backend;
use crate::classmap_scanner::IndexFilters;

/// How long the filters must hold still before the workspace is walked
/// again. Long enough to collapse a client that pushes its settings in
/// several notifications, short enough that a user who removed an
/// exclude to look something up does not sit and wait on it.
const REDISCOVERY_DEBOUNCE: Duration = Duration::from_millis(500);

impl Backend {
    /// Reconcile the symbol indexes with the file filters after a change.
    ///
    /// `previous` is the compiled filter set from before the change, so
    /// pass the value [`index_filters`](Backend::index_filters) returned
    /// *before* invalidating it. Cheap when nothing moved: the walk is
    /// only scheduled when a path or extension could have come back into
    /// scope, and a purely widened exclude list is answered by eviction
    /// alone.
    pub(crate) fn reconcile_index_for_filter_change(&self, previous: &Arc<IndexFilters>) {
        let current = self.index_filters();
        if Arc::ptr_eq(previous, &current) {
            return;
        }

        let evicted = self.evict_newly_filtered_symbols(previous, &current);
        if evicted > 0 {
            tracing::info!("PHPantom: file filters now exclude {evicted} indexed file(s)");
            // Whatever those files declared may be baked into a resolved
            // class (a parent, a trait, a mixin) or a cached completion
            // list, neither of which the index purge reaches.
            self.resolved_class_cache.write().clear();
            self.member_completion_cache.lock().clear();
        }

        if current.may_admit_more_than(previous) {
            self.schedule_filter_rediscovery();
        }
    }

    /// Drop every index entry describing a file the change just put out
    /// of scope, and report how many files were dropped.
    ///
    /// Needs no disk access: the candidates are the files the indexes
    /// already name. They are handed to
    /// [`reindex_files_batch`](Backend::reindex_files_batch) as deletions
    /// rather than purged here, so an excluded file leaves the index by
    /// exactly the same route a deleted one does and no index is missed.
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
        let mut filtered: HashMap<String, PathBuf> = HashMap::new();
        let note = |uri: &str, filtered: &mut HashMap<String, PathBuf>| {
            note_if_newly_filtered(uri, previous, current, filtered)
        };

        for uri in self.symbols.fqn_uri_index.read().values() {
            note(uri, &mut filtered);
        }
        for uri in self.symbols.uri_classes_index.read().keys() {
            note(uri, &mut filtered);
        }
        // A file whose only symbols are functions or constants is in
        // neither class index, and one that declares nothing at all is
        // still parsed and still holds a symbol map.
        for path in self.symbols.autoload_function_index.read().values() {
            note(&crate::util::path_to_uri(path), &mut filtered);
        }
        for path in self.symbols.autoload_constant_index.read().values() {
            note(&crate::util::path_to_uri(path), &mut filtered);
        }
        for uri in self.symbol_maps.read().keys() {
            note(uri, &mut filtered);
        }
        for uri in self.parsed_uris.read().iter() {
            note(uri, &mut filtered);
        }

        // A document open in the editor is served whatever the filters
        // say (`did_open` parses it outright), so dropping its symbol
        // map would break the file the user is looking at to honour a
        // setting about workspace discovery.
        if !filtered.is_empty() {
            let open = self.open_files.read();
            filtered.retain(|uri, _| !open.contains_key(uri));
        }
        if filtered.is_empty() {
            return 0;
        }

        let changes: Vec<(String, PathBuf, FileChangeType)> = filtered
            .into_iter()
            .map(|(uri, path)| (uri, path, FileChangeType::DELETED))
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
        tracing::info!("PHPantom: file filters changed, rediscovering workspace symbols");

        let progress_token = self.progress_create("phpantom/rediscovery").await;
        if let Some(ref tok) = progress_token {
            self.progress_begin(
                tok,
                "PHPantom: Reindexing",
                Some("File filters changed".to_string()),
            )
            .await;
        }
        let progress = crate::progress::ScanProgress::new();
        let poller = progress_token
            .as_ref()
            .map(|tok| self.spawn_progress_poller(tok.clone(), Arc::clone(&progress)));

        let composer_package = crate::composer::read_composer_package(&root);
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
        self.clear_class_not_found_cache();
        self.resolved_class_cache.write().clear();
        self.member_completion_cache.lock().clear();

        // Parse what the walk added, the way startup does; the pass
        // skips every file that already has a symbol map.
        self.start_full_background_index().await;
        self.request_diagnostic_refresh().await;
    }
}

/// Record `uri` in `filtered` when `current` keeps its file out of the
/// index and `previous` did not.
///
/// URIs that name no file on disk (embedded stubs, PHAR members) are
/// left alone: no file filter can speak about them.
fn note_if_newly_filtered(
    uri: &str,
    previous: &IndexFilters,
    current: &IndexFilters,
    filtered: &mut HashMap<String, PathBuf>,
) {
    if filtered.contains_key(uri) {
        return;
    }
    let Some(path) = Url::parse(uri).ok().and_then(|u| u.to_file_path().ok()) else {
        return;
    };
    if is_out_of_scope(current, &path) && !is_out_of_scope(previous, &path) {
        filtered.insert(uri.to_string(), path);
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

        let previous = backend.index_filters();
        backend.set_client_indexing_options(client_options(&["generated"]));
        backend.reconcile_index_for_filter_change(&previous);

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

        let previous = backend.index_filters();
        backend.set_client_indexing_options(config::ClientIndexingOptions::default());
        backend.reconcile_index_for_filter_change(&previous);

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

        let previous = backend.index_filters();
        backend.set_client_indexing_options(client_options(&["generated"]));
        backend.reconcile_index_for_filter_change(&previous);

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

        let previous = backend.index_filters();
        backend.set_client_indexing_options(client_options(&["generated", "build"]));
        backend.reconcile_index_for_filter_change(&previous);

        let index = backend.symbols.fqn_uri_index.read();
        assert!(index.get("C0").is_some());
        assert!(index.get("C1").is_some());
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
