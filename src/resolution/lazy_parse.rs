//! Parsing a file, stub, or phar entry the first time a lookup needs one
//! of its classes, and caching what it declares.

use std::path::Path;
use std::sync::Arc;

use crate::Backend;
use crate::types::{ClassInfo, PhpVersion};
use crate::util::short_name;

use super::inflight::InflightGuard;

impl Backend {
    /// Try to load a class from the embedded stub index only.
    ///
    /// This is the in-memory-only counterpart of [`find_or_load_class`].
    /// It checks the `uri_classes_index` first (O(1) via the FQN index), and if
    /// the class hasn't been parsed yet, looks it up in the in-memory
    /// `stub_index`.  When found there, it parses and caches the stub
    /// under a `phpantom-stub://` URI so subsequent lookups are free.
    ///
    /// **No disk I/O is performed.**  Classes that live on disk (class index,
    /// PSR-4, vendor) are not resolved — callers that need those should
    /// use [`find_or_load_class`] instead.
    pub(crate) fn load_stub_class(&self, class_name: &str) -> Option<Arc<ClassInfo>> {
        let last_segment = short_name(class_name);

        // Fast path: already parsed and cached.
        if let Some(cls) = self.find_class_in_uri_classes_index(class_name) {
            return Some(cls);
        }

        // Look up in the in-memory stub index: a namespaced name (e.g.
        // "BcMath\\Number") by its FQN, a global one (e.g. "PDO") by its
        // short name, so `Demo\PDO` never matches the global `PDO` stub.
        // The stub URI is keyed by the index's spelling so that
        // differently-cased lookups share one cache entry, and the guard
        // is released before the parse instead of being held across it.
        let key = if class_name.contains('\\') {
            class_name
        } else {
            last_segment
        };
        let (stub_uri, content) = {
            let stub_idx = self.stub_index.read();
            let (canonical_name, &content) = stub_idx.get_key_value(key)?;
            (
                format!("{}{}", crate::stubs::STUB_URI_PREFIX, canonical_name),
                content,
            )
        };

        let ver = Some(self.php_version());
        let classes = self.parse_and_cache_stub(content, &stub_uri, ver)?;
        classes
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(last_segment))
            .map(Arc::clone)
    }

    /// Parse the file a `fqn_uri_index` URI names, cache the results, and
    /// return the extracted classes.
    ///
    /// Besides `file://` URIs this accepts the `phar://` URIs that
    /// [`scan_phar_archive`](crate::Backend::scan_phar_archive) registers
    /// and that a parse of a phar entry caches its classes under, reading
    /// the entry out of [`phar_archives`](crate::Backend::phar_archives).
    pub(crate) fn parse_and_cache_uri(&self, uri: &str) -> Option<Vec<Arc<ClassInfo>>> {
        if uri.starts_with("phar://") {
            return self.parse_and_cache_phar_entry(uri);
        }
        let file_path = crate::util::uri_to_path(uri)?;
        self.parse_and_cache_file(&file_path)
    }

    /// Parse the phar entry at `uri` (see [`crate::util::phar_entry_uri`]).
    fn parse_and_cache_phar_entry(&self, uri: &str) -> Option<Vec<Arc<ClassInfo>>> {
        // Deduplicate concurrent parses of the same phar entry.
        if !self.parse_inflight.try_claim(uri) {
            return self.wait_for_cached_result(uri);
        }
        // Remove the inflight entry even if the work below unwinds.
        let _guard = InflightGuard {
            inflight: &self.parse_inflight,
            uri: uri.to_owned(),
        };
        // Read the entry before parsing so the archive map is not locked
        // for the duration of a parse.
        let bytes = {
            let entry = uri.strip_prefix("phar://")?;
            let archives = self.phar_archives.read();
            archives.iter().find_map(|(phar_path, archive)| {
                let internal_path = entry.strip_prefix(phar_path.to_str()?)?.strip_prefix('/')?;
                archive.read_file(internal_path)
            })?
        };
        let content = std::str::from_utf8(&bytes).ok()?;
        self.parse_and_cache_content(content, uri)
    }

    /// Parse a PHP file from disk, cache the results, and return the
    /// extracted classes.
    ///
    /// Convenience wrapper around [`parse_and_cache_content`] that reads
    /// the file and derives a URI from the path.  Used by
    /// [`find_or_load_class`] (Phases 1 and 2) and by the
    /// go-to-implementation scanner.
    pub(crate) fn parse_and_cache_file(&self, file_path: &Path) -> Option<Vec<Arc<ClassInfo>>> {
        let uri = crate::util::path_to_uri(file_path);

        // Deduplicate concurrent parses of the same file.
        if !self.parse_inflight.try_claim(&uri) {
            return self.wait_for_cached_result(&uri);
        }
        // Remove the inflight entry even if the work below unwinds.
        let _guard = InflightGuard {
            inflight: &self.parse_inflight,
            uri: uri.clone(),
        };
        let content = crate::util::read_php_source(file_path);
        content.and_then(|c| self.parse_and_cache_content(&c, &uri))
    }

    /// Parse an embedded stub under its `phpantom-stub://` URI, sharing
    /// the work with any thread that is already parsing the same stub.
    ///
    /// Stubs bypass [`parse_and_cache_file`] (they have no on-disk path),
    /// so they need their own claim on `parse_inflight`.  Without one, two
    /// workers that both miss the same stub parse it twice, and whichever
    /// finishes second sees the URI already in `parsed_uris` and takes the
    /// re-parse branch: it evicts every resolved class that depends on the
    /// stub's classes, discarding work the other workers had already
    /// finished.  Which dependants survived then came down to thread
    /// timing, and the discarded ones were re-resolved later against a
    /// different cache state, so the same file could report different
    /// diagnostics from one run to the next.
    pub(super) fn parse_and_cache_stub(
        &self,
        content: &'static str,
        uri: &str,
        php_version: Option<PhpVersion>,
    ) -> Option<Vec<Arc<ClassInfo>>> {
        if !self.parse_inflight.try_claim(uri) {
            return self.wait_for_cached_result(uri);
        }
        let _guard = InflightGuard {
            inflight: &self.parse_inflight,
            uri: uri.to_owned(),
        };
        self.parse_and_cache_content_versioned(content, uri, php_version)
    }

    /// Block until another thread finishes parsing a file, then return
    /// the cached result from `uri_classes_index`.
    ///
    /// The wait must not give up while the parse is still running:
    /// returning early makes the caller conclude the class does not
    /// exist, and that conclusion is cached in `class_not_found_cache`
    /// — one slow parse under heavy thread contention would permanently
    /// poison resolution of every class in the file for the rest of the
    /// process (nondeterministic "type could not be resolved"
    /// diagnostics in full-project runs).  The wait is bounded by the
    /// owning thread's single parse, which never parses another file
    /// and releases its claim via RAII even on panic; see
    /// [`ParseInflight::wait_until_released`] for the abnormal-hang
    /// escape hatch.
    fn wait_for_cached_result(&self, uri: &str) -> Option<Vec<Arc<ClassInfo>>> {
        self.parse_inflight.wait_until_released(uri);
        self.symbols.uri_classes_index.read().get(uri).cloned()
    }

    /// Parse PHP source text, cache the results in
    /// `uri_classes_index`/`use_map`/`namespace_map`, and return the extracted
    /// classes.
    ///
    /// This is the single canonical implementation of the "parse → cache"
    /// pipeline.  All code paths that need to parse PHP content and store
    /// the results (file-based resolution, stub resolution, implementation
    /// scanning) funnel through here so the caching logic stays consistent.
    pub(crate) fn parse_and_cache_content(
        &self,
        content: &str,
        uri: &str,
    ) -> Option<Vec<Arc<ClassInfo>>> {
        self.parse_and_cache_content_versioned(content, uri, None)
    }

    /// Version-aware variant of [`parse_and_cache_content`].
    ///
    /// When `php_version` is `Some`, elements annotated with
    /// `#[PhpStormStubsElementAvailable]` whose version range excludes
    /// the target version are filtered out during extraction.  Used when
    /// parsing phpstorm-stubs so that only the correct variant of each
    /// function, method, or parameter is presented.
    ///
    /// # Consistency model
    ///
    /// The five maps (`uri_classes_index`, `use_map`, `namespace_map`, `fqn_index`,
    /// `resolved_class_cache`) are written sequentially, not under a
    /// single lock.  A concurrent reader could briefly observe a state
    /// where some maps reflect the new parse while others still hold
    /// stale data for the same URI.  This is acceptable because:
    ///
    /// - All writes complete within microseconds.
    /// - Every consumer clones the data it needs from each map
    ///   independently and does not rely on cross-map atomicity.
    /// - An audit of all read sites (completion, diagnostics, hover,
    ///   definition, references, highlighting) confirmed that none
    ///   requires a consistent snapshot across multiple maps.
    ///
    /// If a future change adds a reader that checks two of these maps
    /// for consistency within the same request, the writes here must
    /// be batched under a single coordination mechanism.
    pub(crate) fn parse_and_cache_content_versioned(
        &self,
        content: &str,
        uri: &str,
        php_version: Option<PhpVersion>,
    ) -> Option<Vec<Arc<ClassInfo>>> {
        // The three helpers below each parse `content`; share one AST.
        let _parse_guard = crate::parser::with_parse_cache(content);
        let file_use_map = self.parse_use_statements(content);
        let constant_imports = self.parse_symbol_imports(content).constants;
        let file_namespace = self.parse_namespace(content);

        // Parse classes with per-class namespace tracking so that
        // multi-namespace files (e.g. PDO.php with both `namespace { }`
        // and `namespace Pdo { }`) resolve parent names correctly.
        let (mut classes_with_ns, blocks) = Self::parse_php_classes_by_block(content, php_version);

        // Resolve parent names against each class's own namespace block
        // (namespace and imports), mirroring the logic in `update_ast_inner`,
        // so that classes in `namespace { }` are not polluted by a sibling
        // `namespace Pdo { }` block.
        Self::resolve_parent_class_names_by_block(
            &mut classes_with_ns,
            &blocks,
            &file_use_map,
            &constant_imports,
            &file_namespace,
        );
        let single_namespace = {
            let mut namespaces = classes_with_ns.iter().map(|(_, ns, _)| ns);
            let first = namespaces.next();
            namespaces.all(|ns| Some(ns) == first)
        };
        let mut classes: Vec<ClassInfo> = Vec::with_capacity(classes_with_ns.len());
        let mut class_namespaces: Vec<Option<String>> = Vec::with_capacity(classes_with_ns.len());
        for (cls, ns, _) in classes_with_ns {
            classes.push(cls);
            class_namespaces.push(ns);
        }

        // Set the per-class file_namespace so that classes loaded via
        // PSR-4 / class index carry their namespace.  This mirrors the
        // same assignment done in `update_ast_inner` for files opened
        // through `did_open` / `did_change`.
        //
        // In a file with several namespace blocks the per-class namespace is
        // authoritative, including when it is `None`: a class in an anonymous
        // `namespace { }` block (as in the bundled `PDO/PDO.php`, which pairs
        // one with a `namespace Pdo { }` block) is global, and falling back to
        // the file-level namespace would label it `Pdo\PDO`.  The fallback is
        // only meaningful for single-namespace files, where a missing
        // per-class value means the namespace simply was not tracked.
        for (i, cls) in classes.iter_mut().enumerate() {
            if cls.file_namespace.is_none() {
                let class_ns = class_namespaces[i].as_deref();
                cls.file_namespace = if single_namespace {
                    class_ns.or(file_namespace.as_deref())
                } else {
                    class_ns
                }
                .map(crate::atom::atom);
            }
            cls.cache_fqn_in_uri(uri);
        }

        // Apply class stub patches for phpstorm-stubs deficiencies
        // (e.g. ArrayIterator missing @template parameters).
        // Only patch classes loaded from stub URIs to avoid touching
        // user code.
        if crate::stubs::is_embedded_stub_uri(uri) {
            for class in &mut classes {
                crate::stub_patches::apply_class_stub_patches(class);
            }
        } else {
            // A handful of third-party classes make a promise their own
            // docblocks leave undeclared.  Keyed by FQN, so everything
            // else passes through on one string compare.
            for class in &mut classes {
                crate::stub_patches::apply_third_party_class_patches(class);
            }
        }

        let arc_classes: Vec<Arc<ClassInfo>> = crate::types::into_arcs(classes);

        // Used below to decide whether resolved-class cache eviction
        // is needed (only on re-parse, not first load).
        let was_already_parsed = self.parsed_uris.read().contains(uri);

        // When re-parsing a previously loaded URI, capture the prior FQN
        // set (still available in uri_classes_index before the overwrite
        // below) so stale entries for renamed or removed classes can be
        // evicted.  Mirrors `update_ast_inner`, which computes `old_fqns`
        // and evicts fqn_class_index, fqn_uri_index, and gti_index.
        // Without this, a class deleted or renamed on re-parse (vendor
        // change, phar refresh, re-open after did_close) leaves ghost
        // entries that keep resolving from stale state and pollute
        // find_implementors / type hierarchy.
        let old_fqns: Vec<String> = if was_already_parsed {
            self.symbols
                .uri_classes_index
                .read()
                .get(uri)
                .map(|v| {
                    v.iter()
                        .filter(|c| !c.name.starts_with("__anonymous@"))
                        .map(|c| c.fqn().to_string())
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        self.parsed_uris.write().insert(uri.to_owned());

        // Store in uri_classes_index for wait_for_cached_result (concurrent
        // parse deduplication) and for consumers that iterate classes
        // by URI.  The memory cost is negligible (just Arc pointers).
        self.symbols
            .uri_classes_index
            .write()
            .insert(uri.to_owned(), arc_classes.clone());
        // NOTE: use_map and namespace_map are intentionally NOT stored
        // for lazily-loaded files (vendor, stubs, PSR-4).  These maps
        // are only needed for files open in the editor (populated by
        // update_ast_inner).  Skipping them reduces memory usage across
        // thousands of vendor files.

        // Populate the fqn_index so that `find_class_in_uri_classes_index` can
        // resolve these classes via O(1) hash lookup.  Also populate
        // fqn_uri_index (FQN → URI) so that go-to-definition can locate
        // the source file even after the uri_classes_index entry is
        // cleared by didClose.
        //
        // The declarations go through the same bookkeeping the editor path
        // (`apply_ast_index_updates_batch`) uses, so a class name two files
        // declare settles on the same one of them whichever path indexed
        // it, and the two indexes cannot end up describing different files.
        let (winning_classes, reowned_fqns) = {
            // Build the new entries outside the lock so the FQN-string
            // formatting and Arc clones don't serialize concurrent readers.
            // Only the brief update below holds the `.write()` guards.
            let new_entries: Vec<(String, Arc<ClassInfo>)> = arc_classes
                .iter()
                .filter(|cls| !cls.name.starts_with("__anonymous@"))
                .map(|cls| (cls.fqn().to_string(), Arc::clone(cls)))
                .collect();

            self.symbols.with_class_declarations(|decls| {
                // The method store and the reverse-inheritance index are
                // keyed by fqn, so they must be repopulated from whichever
                // declaration won rather than from this file's copy of a
                // duplicated name.
                let mut winners: Vec<Arc<ClassInfo>> = Vec::with_capacity(arc_classes.len());

                // On re-parse, withdraw the classes this file no longer
                // defines, so a renamed or deleted class stops resolving
                // from the old ClassInfo.  A name another file also
                // declares is handed to that file instead of vanishing.
                for old_fqn in &old_fqns {
                    if new_entries.iter().any(|(fqn, _)| fqn == old_fqn) {
                        continue;
                    }
                    decls.withdraw(old_fqn, uri);
                    if let Some(promoted) = decls.winner(old_fqn) {
                        winners.push(Arc::clone(promoted));
                    }
                }

                let mut reowned: Vec<String> = Vec::new();
                for (fqn, cls) in &new_entries {
                    let declared = decls.declare(fqn, uri, cls);
                    if declared.reowned {
                        reowned.push(fqn.clone());
                    }
                    if declared.won {
                        winners.push(Arc::clone(cls));
                    } else if let Some(other) = decls.winner(fqn) {
                        winners.push(Arc::clone(other));
                    }
                }

                winners.extend(
                    arc_classes
                        .iter()
                        .filter(|cls| cls.name.starts_with("__anonymous@"))
                        .map(Arc::clone),
                );
                (winners, reowned)
            })
        };
        // On re-parse, evict the gti_index entries for the classes this
        // file previously defined before re-populating.  Evicting the *old*
        // FQN set (rather than the new one) clears stale
        // reverse-inheritance edges of classes that were deleted or
        // renamed, so find_implementors / type hierarchy stop serving
        // children that no longer extend a parent.  For a first-time load
        // `old_fqns` is empty and the call early-returns.
        self.symbols.withdraw_inheritance(&old_fqns);
        // Same for a name this parse took over from another file: those
        // entries describe the declaration that just lost.
        self.symbols.withdraw_inheritance(&reowned_fqns);
        self.symbols.index_inheritance(&winning_classes);

        // Remove newly-discovered FQNs from the negative-result cache.
        {
            let nf_cache = self.symbols.class_not_found_cache.read();
            if !nf_cache.is_empty() {
                drop(nf_cache);
                let mut nf_cache = self.symbols.class_not_found_cache.write();
                for cls in &arc_classes {
                    if cls.name.starts_with("__anonymous@") {
                        continue;
                    }
                    let fqn = cls.fqn().to_string();
                    nf_cache.remove(&fqn);
                }
            }
        }

        // Retire memoised lookups now that the index has these classes, has
        // dropped any the file no longer declares, and has un-cached their
        // negatives.  It has to come after all three, or a lookup that read
        // the new generation could still see a negative about to be cleared
        // and memoise it as fresh.
        self.symbols.note_class_lookup_change();

        // Selectively invalidate the resolved-class cache for the
        // classes defined in this file.
        //
        // This function is only reached when the class was NOT found
        // in uri_classes_index (find_class_in_uri_classes_index / fqn_index returned None).
        // That means the class has never been parsed — so it cannot
        // have a direct entry in the resolved-class cache.
        //
        // Dependents (e.g. a child class resolved before this parent
        // was available) *could* hold stale entries, but the transitive
        // evict_fqn scan is O(cache_size) per class and is called for
        // every vendor class loaded from class index / PSR-4 / stubs.
        // With thousands of classes this becomes O(N²) and dominates
        // total analysis time.
        //
        // Instead, only evict when the URI was already present in
        // uri_classes_index (i.e. a re-parse of a previously loaded file, which
        // can happen in the LSP editing path).  For first-time loads
        // the cost/benefit is strongly negative.
        if was_already_parsed {
            let mut cache = self.resolved_class_cache.write();
            let mut new_fqns: std::collections::HashSet<String> =
                std::collections::HashSet::with_capacity(arc_classes.len());
            for cls in &arc_classes {
                let fqn = cls.fqn();
                new_fqns.insert(fqn.to_string());
                let _ = crate::virtual_members::evict_fqn(&mut cache, &fqn);
            }
            // Also evict classes this file previously defined but no longer
            // does (renames / removals) so they stop resolving from a stale
            // resolved-class cache entry.
            for old_fqn in &old_fqns {
                if !new_fqns.contains(old_fqn) {
                    let _ = crate::virtual_members::evict_fqn(&mut cache, old_fqn);
                }
            }
            drop(cache);
            // A first load only adds what a lookup would have found on disk
            // anyway, but a re-parse can change what open files resolve to.
            self.diag.epoch.note_change();
        }

        Some(arc_classes)
    }
}
