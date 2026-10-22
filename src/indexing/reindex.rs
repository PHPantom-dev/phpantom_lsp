//! Re-scanning a batch of files that changed on disk into the discovery
//! indexes, shared by the watched-file handler and the filter
//! reconciliation.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use tower_lsp::lsp_types::FileChangeType;

use crate::Backend;
use crate::classmap_scanner::{ScanResult, WorkspaceScanResult};
use crate::types::ClassInfo;

impl Backend {
    /// Re-scan a batch of files from disk, refreshing their discovery-level
    /// index entries (FQN→URI, autoload functions/constants, globals).
    ///
    /// Used when files are created, changed, or deleted outside the editor
    /// (a git checkout, a `composer install`, an editor session resuming
    /// after idle).  Every index that references a changed file is purged
    /// first, or stale symbols linger: completion keeps suggesting a class
    /// whose file was removed, go-to-definition jumps into a deleted file,
    /// and so on.  Purging only some indexes (e.g. `fqn_uri_index` but not
    /// `gti_index`) leaves the symptom alive in whichever feature reads
    /// the index that was missed.
    ///
    /// The purge of each discovery index is done once for the whole batch
    /// rather than once per file.  Each of those maps must be scanned in
    /// full to drop a file's entries, so handling a flood of watched-file
    /// events one at a time would be O(files × index size); a branch switch
    /// can emit thousands of events at once.  Batching makes the purge
    /// O(index size) regardless of how many files changed.
    ///
    /// A given FQN→URI entry may have been stored under either of two URI
    /// conventions depending on how it was created (the classmap scan
    /// stores [`crate::util::path_to_uri`] of the discovered path, while
    /// `update_ast` stores the editor's URI string), so values are matched
    /// against both spellings.  The full
    /// [`ClassInfo`](crate::types::ClassInfo) is re-parsed lazily on next
    /// access.  Beyond the lightweight discovery indexes, only the files the
    /// workspace index covers are parsed again here, and only once that
    /// index exists, so their references keep counting.
    ///
    /// `changes` is `(editor URI string, file path, change type)`.
    ///
    /// Returns whether any *class declaration* was dropped, handed to a
    /// surviving file, or discovered — i.e. whether class resolution could
    /// now answer differently.  A batch of declaration-free files (a
    /// generated cache artifact, a routes or config file) returns `false`,
    /// and the caller can keep the resolved-class caches it would
    /// otherwise have to drop.
    pub(crate) fn reindex_files_batch(
        &self,
        changes: &[(String, PathBuf, FileChangeType)],
    ) -> bool {
        if changes.is_empty() {
            return false;
        }

        // Index values are stored under either the editor URI or the
        // canonical `file://` URI, so match both variants.
        let mut uri_set: HashSet<String> = HashSet::new();
        let mut path_set: HashSet<PathBuf> = HashSet::new();
        for (uri_str, path, _) in changes {
            uri_set.insert(uri_str.clone());
            uri_set.insert(crate::util::path_to_uri(path));
            path_set.insert(path.clone());
        }

        // Drop every class declaration sourced from a changed file in one
        // pass, collecting the affected FQNs so the dependent caches can be
        // evicted without re-scanning.  A name a purged file shared with
        // another declaration is promoted to that one rather than dropped,
        // so deleting one of two files declaring a class does not make the
        // class unresolvable.
        let crate::symbol_index::WithdrawnClasses {
            dropped: dropped_fqns,
            promoted: promoted_fqns,
        } = self
            .symbols
            .with_class_declarations(|decls| decls.withdraw_uris(&uri_set));
        // These FQNs no longer resolve, and the promoted ones resolve
        // elsewhere; retire the memoised lookups.
        self.symbols.note_class_lookup_change();
        let mut classes_changed = !dropped_fqns.is_empty() || !promoted_fqns.is_empty();
        let mut functions_retired = false;
        self.symbols.withdraw_inheritance(&dropped_fqns);
        if !promoted_fqns.is_empty() {
            // The indexes derived from the class index still describe the
            // purged declaration, so rebuild them from the survivor.
            let winners: Vec<Arc<ClassInfo>> = {
                let fci = self.symbols.fqn_class_index.read();
                promoted_fqns
                    .iter()
                    .filter_map(|fqn| fci.get(fqn).map(Arc::clone))
                    .collect()
            };
            self.symbols.withdraw_inheritance(&promoted_fqns);
            self.symbols.index_inheritance(&winners);
        }

        self.symbols
            .autoload_function_index
            .write()
            .retain(|_, v| !path_set.contains(v));
        self.symbols
            .autoload_constant_index
            .write()
            .retain(|_, v| !path_set.contains(v));
        self.withdraw_functions_for_uris(&uri_set);
        self.symbols
            .global_defines
            .write()
            .retain(|_, d| !uri_set.contains(d.file_uri.as_str()));

        // Per-URI keyed removals are cheap (no full scan).  Like the
        // retain-based purges above, clear both URI spellings: the editor
        // URI from the watcher event and the canonical `file://` URI the
        // background indexer stores.  Missing the canonical spelling would
        // leave a stale symbol map that also blocks re-parsing (the
        // workspace-index walk skips files that already have one).
        for (uri_str, path, change_type) in changes {
            let canonical_uri = crate::util::path_to_uri(path);
            let spellings = if canonical_uri == *uri_str {
                vec![uri_str.as_str()]
            } else {
                vec![uri_str.as_str(), canonical_uri.as_str()]
            };
            for &uri in &spellings {
                self.clear_file_maps(uri);
                self.symbols.uri_classes_index.write().remove(uri);
                self.parsed_uris.write().remove(uri);
                // The global_functions/global_defines entries for these URIs
                // were just retained out above; drop the per-URI tracking
                // record too so deleted files don't leave a stale entry
                // behind.  Created/changed files rebuild it when re-parsed
                // below.
                self.symbols.uri_globals_index.write().remove(uri);
                // A deleted file no longer declares the config keys it wrote
                // at runtime.  A file that was merely changed re-registers
                // them when it is re-parsed below.
                self.laravel.runtime_config_keys.write().remove(uri);
                // The Laravel registries a file fed (macros, storage drivers,
                // commands, morph aliases, gates, provider resources) are
                // refreshed only when the file is parsed, which a deleted
                // file never is again.
                if *change_type == FileChangeType::DELETED {
                    self.forget_laravel_file_contributions(uri);
                }
            }
            if *change_type == FileChangeType::DELETED {
                self.forget_workspace_diagnostics(&spellings);
            }
        }

        // Re-add current symbols for created/changed files.  Deleted files
        // keep their entries purged.
        for (uri_str, path, change_type) in changes {
            if !matches!(
                *change_type,
                FileChangeType::CREATED | FileChangeType::CHANGED
            ) {
                continue;
            }

            let classes = crate::classmap_scanner::scan_file(path);
            classes_changed |= !classes.is_empty();
            self.symbols.with_class_declarations(|decls| {
                for fqn in classes {
                    decls.note_discovered(&fqn, uri_str.clone());
                }
            });

            // Unlike the class caches the caller clears only when a class
            // declaration changed, the function negative cache is retired
            // for any created or changed file: the byte scan below misses
            // functions behind `function_exists` guards, which only the
            // lookup's autoload-file fallback finds, and that fallback
            // cannot run past a recorded miss.  Refilling the cache costs
            // one fallback pass per still-missing name.
            functions_retired = true;

            // The classes were noted above, under the editor's URI.
            let scan = ScanResult {
                classes: Vec::new(),
                ..crate::classmap_scanner::scan_file_full(path)
            };
            self.symbols
                .merge_scan(WorkspaceScanResult::from_file(scan, path));
        }

        self.symbols.prune_orphaned_origins();

        // Retired after the index inserts so a lookup that read the index
        // before them cannot record a miss that outlives the clear.
        if functions_retired {
            self.symbols.retire_function_misses();
        }

        // The refreshed discovery indexes may add or remove a namespace-local
        // `auth()` or a real global class that shadows a Laravel facade alias.
        // Re-evaluate only maps that recorded one of those dormant candidates.
        self.refresh_all_published_laravel_candidates();
        // The purge above took the files' symbol maps and reference-index
        // entries with it, and a completed workspace index is never walked
        // again to put them back, so the reference-count lenses would stop
        // counting what a changed file references and never see a created
        // one.  Re-parse them now.  An index that has not started yet picks
        // them up in its own walk, but one in flight may already be past
        // them.
        if self
            .workspace_index
            .indexed
            .load(std::sync::atomic::Ordering::Acquire)
            || self.workspace_index.lock.is_locked()
        {
            let reparse: Vec<(String, PathBuf)> = changes
                .iter()
                .filter(|(_, _, change_type)| {
                    matches!(
                        *change_type,
                        FileChangeType::CREATED | FileChangeType::CHANGED
                    )
                })
                .filter_map(|(_, path, _)| {
                    let uri = crate::util::path_to_uri(path);
                    self.workspace_index_path(&uri)?;
                    Some((uri, path.clone()))
                })
                .collect();
            self.parse_paths_parallel_with_progress(&reparse, None);
        }
        classes_changed
    }
}
