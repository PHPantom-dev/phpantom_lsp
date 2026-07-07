/// Cross-file class and function resolution.
///
/// This module contains the heavyweight name-resolution logic that is
/// shared by the completion handler, definition resolver, and
/// named-argument resolution.  It was extracted from `util.rs` so that
/// module can focus on simple helper functions.
///
/// # Resolution pipeline
///
/// ## Class resolution ([`Backend::find_or_load_class`])
///
///   0. **Class index** — direct FQN → URI lookup (covers non-PSR-4 classes
///      and Composer classmap entries)
///   1. **uri_classes_index scan** — search all already-parsed files by short name,
///      with namespace verification when a qualified name is requested
///   2. **PSR-4 resolution** — convert namespace to file path and parse
///   3. **Embedded stubs** — built-in PHP classes/interfaces bundled in
///      the binary (e.g. `UnitEnum`, `BackedEnum`, `Iterator`)
///
/// ## Function resolution ([`Backend::find_or_load_function`])
///
///   1. **global_functions** — user code + already-cached stubs
///   2. **Embedded stubs** — built-in PHP functions from phpstorm-stubs
///
/// ## Name resolution ([`Backend::resolve_class_name`], [`Backend::resolve_function_name`])
///
///   These methods take a raw name as it appears in source code and resolve
///   it to a concrete `ClassInfo` or `FunctionInfo` using the file's `use`
///   statement mappings and namespace context.  They handle:
///
///   - Fully-qualified names (`\PDO`, `\Couchbase\Cluster`)
///   - Unqualified names resolved via the import table or current namespace
///   - Qualified names with alias expansion and namespace prefixing
use std::collections::HashMap;
use std::sync::Arc;

use std::path::Path;

use tower_lsp::lsp_types::Url;

use crate::Backend;
use crate::composer;
use crate::php_type::{PhpType, is_builtin_non_class_type};
use crate::types::{ClassInfo, FileContext, FunctionInfo, PhpVersion};
use crate::util::short_name;

impl Backend {
    /// Try to find a class by name across all cached files in the uri_classes_index,
    /// and if not found, attempt PSR-4 resolution to load the class from disk.
    ///
    /// The `class_name` can be:
    ///   - A simple name like `"Customer"`
    ///   - A namespace-qualified name like `"Klarna\\Customer"`
    ///   - A fully-qualified name like `"\\Klarna\\Customer"` (leading `\` is stripped)
    ///
    /// Returns a shared `Arc<ClassInfo>` if found, or `None`.
    pub(crate) fn find_or_load_class(&self, class_name: &str) -> Option<Arc<ClassInfo>> {
        // Defensively strip nullable prefix (`?Foo` → `Foo`) and generic
        // parameters (`Collection<int, User>` → `Collection`) so that
        // callers don't need to normalise before lookup.
        self.find_or_load_class_typed(&PhpType::parse(class_name))
    }

    /// Like [`find_or_load_class`], but accepts a pre-parsed `PhpType`,
    /// avoiding the redundant `PhpType::parse()` call that the string
    /// overload performs internally.
    pub(crate) fn find_or_load_class_typed(&self, ty: &PhpType) -> Option<Arc<ClassInfo>> {
        let base = ty.base_name()?;
        self.find_or_load_class_inner(base)
    }

    /// Shared implementation used by [`find_or_load_class`].
    /// `class_name` must already be normalised (no `?` prefix, no
    /// generic parameters).
    fn find_or_load_class_inner(&self, class_name: &str) -> Option<Arc<ClassInfo>> {
        // The class name stored in ClassInfo is just the short name (e.g. "Customer"),
        // so we match against the last segment of the namespace-qualified name.
        let last_segment = short_name(class_name);

        // ── Short-circuit: scalar/built-in type keywords are never classes ──
        // The name resolver or variable resolution pipeline sometimes
        // namespace-qualifies bare type keywords (e.g. `Tests\Feature\int`).
        // These can never resolve to a class, so bail out immediately to
        // avoid thousands of wasted lookups per analysis run.
        if is_builtin_non_class_type(last_segment) {
            return None;
        }

        // Extract the expected namespace prefix (if any).
        // For "Demo\\PDO" → expected_ns = Some("Demo")
        // For "PDO"       → expected_ns = None (global scope)
        let expected_ns: Option<&str> = if class_name.contains('\\') {
            Some(&class_name[..class_name.len() - last_segment.len() - 1])
        } else {
            None
        };

        // ── Negative cache: skip the full multi-phase search ──
        if self.class_not_found_cache.read().contains(class_name) {
            return None;
        }

        // ── Phase 0: Search all already-parsed files ────────────
        // O(1) lookup via `fqn_index` (populated by `update_ast` and
        // `parse_and_cache_content`), with a linear `uri_classes_index` fallback
        // for edge cases.  This is the fastest path — no disk I/O, no
        // parsing — so it runs before any file-based resolution.
        if let Some(cls) = self.find_class_in_uri_classes_index(class_name) {
            return Some(cls);
        }

        // ── Phase 1: Try the fqn_uri_index (FQN → URI) ───────────
        // The fqn_uri_index is populated by `scan_autoload_files` (Composer
        // `autoload_files.php` entries and their `require_once` chains),
        // by `update_ast` for every opened/changed file, and by the
        // workspace full-scan for non-Composer projects.  It covers
        // classes that don't follow PSR-4 conventions and aren't in the
        // Composer classmap — e.g. global-namespace classes like `Mockery`
        // that are loaded via Composer's `files` autoloading.
        let class_index_uri = self.fqn_uri_index.read().get(class_name).cloned();
        if let Some(file_uri) = class_index_uri
            && let Some(file_path) = Url::parse(&file_uri)
                .ok()
                .and_then(|u| u.to_file_path().ok())
            && let Some(classes) = self.parse_and_cache_file(&file_path)
            && let Some(cls) = classes
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case(last_segment))
        {
            return Some(Arc::clone(cls));
        }

        // ── Phase 2: Try PSR-4 resolution ──
        // PSR-4 mappings come exclusively from composer.json (user code).
        // Vendor code is covered by the class index (Phase 1).  If a
        // vendor class is missing from the class index, it fails visibly
        // rather than being silently resolved, making stale classmaps
        // obvious (fix: run `composer dump-autoload`).
        if let Some(workspace_root) = self.workspace_root.read().clone() {
            let file_path = {
                let mappings = self.psr4_mappings.read();
                composer::resolve_class_path(&mappings, &workspace_root, class_name)
            };
            if let Some(file_path) = file_path
                && let Some(classes) = self.parse_and_cache_file(&file_path)
                && let Some(cls) = classes
                    .iter()
                    .find(|c| c.name.eq_ignore_ascii_case(last_segment))
            {
                return Some(Arc::clone(cls));
            }
        }

        // ── Phase 3: Try embedded PHP stubs ──
        // Stubs are bundled in the binary for built-in classes/interfaces
        // (e.g. UnitEnum, BackedEnum, BcMath\Number).  Parse on first
        // access and cache in the uri_classes_index under a `phpantom-stub://` URI
        // so subsequent lookups hit Phase 1 and skip parsing entirely.
        //
        // Two lookup strategies:
        //
        //   a) **FQN lookup** — when the caller requests a namespaced
        //      name like `BcMath\Number`, look it up in the stub index
        //      by the full name.  Many PHP extensions define classes
        //      inside namespaces (Ds, BcMath, Random, Fiber, etc.).
        //
        //   b) **Short-name lookup** — when the caller requests an
        //      unqualified name like `PDO`, look it up by the short
        //      name.  This only fires when `expected_ns` is `None` to
        //      avoid `Demo\PDO` matching the global `PDO` stub.
        //
        // Strategy (a) is tried first because it is more specific.
        let stub_idx = self.stub_index.read();
        let stub_lookup = if expected_ns.is_some() {
            // Namespaced lookup — try the full FQN as a stub key.
            stub_idx.get_key_value(class_name)
        } else {
            // Global-namespace lookup — match by short name only.
            stub_idx.get_key_value(last_segment)
        };
        if let Some((canonical_name, &stub_content)) = stub_lookup {
            // Key the stub URI by the stub index's spelling so that
            // differently-cased lookups share one cache entry.
            let stub_uri = format!("phpantom-stub://{}", canonical_name);
            let ver = Some(self.php_version());
            if let Some(classes) =
                self.parse_and_cache_content_versioned(stub_content, &stub_uri, ver)
                && let Some(cls) = classes
                    .iter()
                    .find(|c| c.name.eq_ignore_ascii_case(last_segment))
            {
                return Some(Arc::clone(cls));
            }
        }

        // Cache the negative result so subsequent lookups for the same
        // unknown class skip the expensive multi-phase search.
        self.class_not_found_cache.write().insert(class_name);
        None
    }

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

        // Look up in the in-memory stub index.
        let stub_idx = self.stub_index.read();
        let stub_lookup = if class_name.contains('\\') {
            // Namespaced lookup (e.g. "BcMath\\Number").
            stub_idx.get_key_value(class_name)
        } else {
            // Global-namespace lookup (e.g. "PDO").
            stub_idx.get_key_value(last_segment)
        };

        if let Some((canonical_name, &content)) = stub_lookup {
            let stub_uri = format!("phpantom-stub://{}", canonical_name);
            let ver = Some(self.php_version());
            if let Some(classes) = self.parse_and_cache_content_versioned(content, &stub_uri, ver)
                && let Some(cls) = classes
                    .iter()
                    .find(|c| c.name.eq_ignore_ascii_case(last_segment))
            {
                return Some(Arc::clone(cls));
            }
        }

        None
    }

    /// Parse a PHP file from disk (or from a phar archive), cache the
    /// results, and return the extracted classes.
    ///
    /// Convenience wrapper around [`parse_and_cache_content`] that reads
    /// the file and derives a URI from the path.  Used by
    /// [`find_or_load_class`] (Phases 1.5 and 2) and by the
    /// go-to-implementation scanner.
    ///
    /// **Phar support:** when `file_path` contains a `!` separator
    /// (e.g. `/path/to/phpstan.phar!src/Type/Type.php`), the left side
    /// is the phar archive path and the right side is the internal file
    /// path.  The content is extracted from the in-memory
    /// [`phar_archives`](crate::Backend::phar_archives) cache instead
    /// of reading from disk.  The URI uses a `phar://` scheme so that
    /// go-to-definition can distinguish phar-sourced classes.
    pub(crate) fn parse_and_cache_file(&self, file_path: &Path) -> Option<Vec<Arc<ClassInfo>>> {
        let path_str = file_path.to_str().unwrap_or_default();

        // ── Phar path: "archive.phar!internal/path.php" ─────────
        if let Some(sep) = path_str.find('!') {
            let phar_path = Path::new(&path_str[..sep]);
            let internal_path = &path_str[sep + 1..];

            let uri = format!("phar://{}/{}", phar_path.display(), internal_path);

            // Deduplicate concurrent parses of the same phar entry.
            if !self.parse_inflight.lock().insert(uri.clone()) {
                return self.wait_for_cached_result(&uri);
            }
            let result = (|| {
                let archives = self.phar_archives.read();
                let archive = archives.get(phar_path)?;
                let bytes = archive.read_file(internal_path)?;
                let content = std::str::from_utf8(bytes).ok()?;
                self.parse_and_cache_content(content, &uri)
            })();
            self.parse_inflight.lock().remove(&uri);
            return result;
        }

        // ── Regular file path ───────────────────────────────────
        let uri = crate::util::path_to_uri(file_path);

        // Deduplicate concurrent parses of the same file.
        if !self.parse_inflight.lock().insert(uri.clone()) {
            return self.wait_for_cached_result(&uri);
        }
        let content = std::fs::read_to_string(file_path).ok();
        let result = content.and_then(|c| self.parse_and_cache_content(&c, &uri));
        self.parse_inflight.lock().remove(&uri);
        result
    }

    /// Spin-wait for another thread to finish parsing a file and return
    /// the cached result from `uri_classes_index`.
    fn wait_for_cached_result(&self, uri: &str) -> Option<Vec<Arc<ClassInfo>>> {
        for _ in 0..200 {
            // Check if parsing is complete (URI removed from inflight set).
            if !self.parse_inflight.lock().contains(uri) {
                return self.uri_classes_index.read().get(uri).cloned();
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        // Timeout: the other thread is still parsing.  Return whatever is
        // in uri_classes_index (may be stale or None).
        self.uri_classes_index.read().get(uri).cloned()
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
        let file_use_map = self.parse_use_statements(content);
        let file_namespace = self.parse_namespace(content);

        // Parse classes with per-class namespace tracking so that
        // multi-namespace files (e.g. PDO.php with both `namespace { }`
        // and `namespace Pdo { }`) resolve parent names correctly.
        let classes_with_ns = Self::parse_php_versioned_with_namespaces(content, php_version);

        // Group classes by their enclosing namespace and resolve parent
        // names once per group, mirroring the logic in `update_ast_inner`.
        let mut classes: Vec<ClassInfo> = Vec::with_capacity(classes_with_ns.len());
        let mut ns_groups: HashMap<Option<String>, Vec<usize>> = HashMap::new();
        for (i, (_cls, ns)) in classes_with_ns.iter().enumerate() {
            ns_groups.entry(ns.clone()).or_default().push(i);
        }

        // Flatten into a single Vec, preserving original order.
        for (cls, _) in &classes_with_ns {
            classes.push(cls.clone());
        }

        if ns_groups.len() <= 1 {
            // Single namespace (common case): resolve with file namespace.
            Self::resolve_parent_class_names(&mut classes, &file_use_map, &file_namespace);
        } else {
            // Multi-namespace file: resolve each group with its own
            // namespace context so that classes in `namespace { }` are
            // not polluted by a sibling `namespace Pdo { }` block.
            for (group_ns, indices) in &ns_groups {
                let mut group: Vec<ClassInfo> =
                    indices.iter().map(|&i| classes[i].clone()).collect();
                Self::resolve_parent_class_names(&mut group, &file_use_map, group_ns);
                for (j, &idx) in indices.iter().enumerate() {
                    classes[idx] = group[j].clone();
                }
            }
        }

        // Set the per-class file_namespace so that classes loaded via
        // PSR-4 / class index carry their namespace.  This mirrors the
        // same assignment done in `update_ast_inner` for files opened
        // through `did_open` / `did_change`.
        for (i, cls) in classes.iter_mut().enumerate() {
            if cls.file_namespace.is_none() {
                cls.file_namespace = classes_with_ns[i]
                    .1
                    .as_deref()
                    .or(file_namespace.as_deref())
                    .map(crate::atom::atom);
            }
        }

        // Apply class stub patches for phpstorm-stubs deficiencies
        // (e.g. ArrayIterator missing @template parameters).
        // Only patch classes loaded from stub URIs to avoid touching
        // user code.
        if uri.starts_with("phpantom-stub://") || uri.starts_with("phpantom-stub-fn://") {
            for class in &mut classes {
                crate::stub_patches::apply_class_stub_patches(class);
            }
        }

        // Wrap each ClassInfo in Arc before inserting into the maps.
        let arc_classes: Vec<Arc<ClassInfo>> = classes.into_iter().map(Arc::new).collect();

        // Check whether this URI already has been parsed before.
        // Used below to decide whether resolved-class cache eviction
        // is needed (only on re-parse, not first load).
        let was_already_parsed = self.parsed_uris.read().contains(uri);

        // Record that this URI has been parsed.
        self.parsed_uris.write().insert(uri.to_owned());

        // Store in uri_classes_index for wait_for_cached_result (concurrent
        // parse deduplication) and for consumers that iterate classes
        // by URI.  The memory cost is negligible (just Arc pointers).
        self.uri_classes_index
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
        {
            // Build the new entries outside the lock so the FQN-string
            // formatting and Arc clones don't serialize concurrent readers.
            // Only the brief insert below holds the `.write()` guards.
            let new_entries: Vec<(String, Arc<ClassInfo>)> = arc_classes
                .iter()
                .filter(|cls| !cls.name.starts_with("__anonymous@"))
                .map(|cls| (cls.fqn().to_string(), Arc::clone(cls)))
                .collect();

            let mut class_idx = self.fqn_uri_index.write();
            let mut fqn_idx = self.fqn_class_index.write();
            for (fqn, cls) in new_entries {
                class_idx.or_insert_with(fqn.as_str(), || uri.to_owned());
                fqn_idx.insert(fqn, cls);
            }
        }

        // Populate the method store from the freshly parsed classes.
        {
            let fqns: Vec<String> = arc_classes
                .iter()
                .filter(|c| !c.name.starts_with("__anonymous@"))
                .map(|c| c.fqn().to_string())
                .collect();
            self.evict_methods_for_fqns(&fqns);
        }
        self.populate_method_store(&arc_classes);
        self.populate_gti_index(&arc_classes);

        // Remove newly-discovered FQNs from the negative-result cache.
        {
            let nf_cache = self.class_not_found_cache.read();
            if !nf_cache.is_empty() {
                drop(nf_cache);
                let mut nf_cache = self.class_not_found_cache.write();
                for cls in &arc_classes {
                    if cls.name.starts_with("__anonymous@") {
                        continue;
                    }
                    let fqn = cls.fqn().to_string();
                    nf_cache.remove(&fqn);
                }
            }
        }

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
            for cls in &arc_classes {
                let fqn = cls.fqn();
                let _ = crate::virtual_members::evict_fqn(&mut cache, &fqn);
            }
        }

        Some(arc_classes)
    }

    /// Try to find a standalone function by name, checking user-defined
    /// functions first, then falling back to embedded PHP stubs.
    ///
    /// The lookup order is:
    ///   1. `global_functions` — functions from Composer autoload files and
    ///      opened/changed files.
    ///   2. `stub_function_index` — built-in PHP functions embedded from
    ///      phpstorm-stubs.  Parsed lazily on first access and cached in
    ///      `global_functions` under a `phpantom-stub-fn://` URI so
    ///      subsequent lookups hit step 1.
    ///
    /// `candidates` is a list of names to try (e.g. the bare name, the
    /// FQN via use-map, the namespace-qualified name).  The first match
    /// wins.
    pub fn find_or_load_function(&self, candidates: &[&str]) -> Option<FunctionInfo> {
        // ── Phase 1: Check global_functions (user code + already-cached stubs) ──
        {
            let fmap = self.global_functions.read();
            for &name in candidates {
                if let Some((_, info)) = fmap.get(name) {
                    return Some(info.clone());
                }
            }
        }

        // ── Phase 1.5: Check autoload_function_index (byte-level scan) ──
        // The lightweight `find_symbols` byte-level scan discovers
        // function names at startup without a full AST parse, for both
        // non-Composer projects (workspace scan) and Composer projects
        // (autoload_files.php scan).  When a candidate matches here, we
        // lazily call `update_ast` on the file to get a complete
        // `FunctionInfo` and cache it in global_functions so subsequent
        // lookups hit Phase 1.
        //
        // Note: the lazy parse is a full AST parse (`update_ast`), which
        // is the same cost as opening the file.  This is acceptable
        // because it only happens once per function, on first access.
        {
            let idx = self.autoload_function_index.read();
            for &name in candidates {
                if let Some(path) = idx.get(name) {
                    let path = path.clone();
                    drop(idx); // release read lock before parsing

                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let uri = crate::util::path_to_uri(&path);
                        self.update_ast(&uri, &content);

                        // Re-check global_functions after parsing.
                        let fmap = self.global_functions.read();
                        for &retry_name in candidates {
                            if let Some((_, info)) = fmap.get(retry_name) {
                                return Some(info.clone());
                            }
                        }
                    }
                    break; // Only try one file per lookup
                }
            }
        }

        // ── Phase 1.75: Last-resort lazy parse of known autoload files ──
        // The byte-level scanner misses functions wrapped in
        // `if (! function_exists(...))` guards (brace depth > 0).
        // These are common in Laravel helpers and similar packages.
        // As a safety net, lazily parse each known autoload file via
        // `update_ast` until the function is found.  Each file is
        // parsed at most once: subsequent lookups hit Phase 1
        // (`global_functions`).
        {
            let paths = self.autoload_file_paths.read().clone();
            for path in &paths {
                // Skip files that have already been fully parsed (their
                // functions are already in global_functions via Phase 1).
                let uri = crate::util::path_to_uri(path);
                if self.parsed_uris.read().contains(&uri) {
                    continue;
                }

                if let Ok(content) = std::fs::read_to_string(path) {
                    self.update_ast(&uri, &content);

                    let fmap = self.global_functions.read();
                    for &name in candidates {
                        if let Some((_, info)) = fmap.get(name) {
                            return Some(info.clone());
                        }
                    }
                }
            }
        }

        // ── Phase 2: Try embedded PHP stubs ──
        // The stub_function_index maps function names (including namespaced
        // ones like "Brotli\\compress") to the raw PHP source of the file
        // that defines them.  We parse the entire file, cache all discovered
        // functions in global_functions, and return the one we need.
        let stub_fn_idx = self.stub_function_index.read();
        for &name in candidates {
            if let Some(&stub_content) = stub_fn_idx.get(name) {
                let ver = Some(self.php_version());
                let mut functions = self.parse_functions_versioned(stub_content, ver);

                if functions.is_empty() {
                    continue;
                }

                // Apply stub patches for phpstorm-stubs deficiencies
                // (e.g. array_reduce returning `mixed` instead of a
                // template-based type).  See stub_patches.rs.
                for func in &mut functions {
                    crate::stub_patches::apply_function_stub_patches(func);
                }

                let stub_uri = format!("phpantom-stub-fn://{}", name);
                let mut result: Option<FunctionInfo> = None;

                {
                    let mut fmap = self.global_functions.write();
                    for func in &functions {
                        let fqn = if let Some(ref ns) = func.namespace {
                            format!("{}\\{}", ns, &func.name)
                        } else {
                            func.name.to_string()
                        };

                        // Check if this is the function we're looking for.
                        if result.is_none()
                            && (fqn.eq_ignore_ascii_case(name)
                                || func.name.eq_ignore_ascii_case(name))
                        {
                            result = Some(func.clone());
                        }

                        // Cache the FQN so future lookups hit Phase 1.
                        // No short-name fallback: `resolve_function_name`
                        // already builds namespace-qualified candidates.
                        fmap.or_insert_with(fqn, || (stub_uri.clone(), func.clone()));
                    }
                }

                // Also cache any classes defined in the same stub file so
                // that class lookups for types referenced by the function
                // (e.g. return types) can find them later.
                let mut classes = Self::parse_php_versioned(stub_content, ver);
                if !classes.is_empty() {
                    let empty_use_map = HashMap::new();
                    let stub_namespace = self.parse_namespace(stub_content);
                    Self::resolve_parent_class_names(&mut classes, &empty_use_map, &stub_namespace);
                    let class_uri = format!("phpantom-stub-fn://{}", name);
                    let arc_classes: Vec<Arc<ClassInfo>> =
                        classes.into_iter().map(Arc::new).collect();
                    self.uri_classes_index
                        .write()
                        .insert(class_uri, arc_classes);
                }

                if result.is_some() {
                    return result;
                }
            }
        }

        None
    }

    // ─── Shared Name Resolution ─────────────────────────────────────────────

    /// Resolve a class name using use-map, namespace, local classes, and
    /// cross-file / PSR-4 / stubs.
    ///
    /// This is the single canonical implementation of the "class_loader"
    /// logic used by the completion handler, definition resolver, and
    /// named-argument resolution.  It handles:
    ///
    ///   - Fully-qualified names (`\PDO`, `\Couchbase\Cluster`)
    ///   - Unqualified names resolved via the import table (`use` statements),
    ///     local class list, current namespace, or global scope
    ///   - Qualified names with alias expansion and namespace prefixing
    pub(crate) fn resolve_class_name(
        &self,
        name: &str,
        local_classes: &[Arc<ClassInfo>],
        file_use_map: &HashMap<String, String>,
        file_namespace: &Option<String>,
    ) -> Option<Arc<ClassInfo>> {
        // ── Fully qualified name (leading `\`) ──────────────
        if let Some(stripped) = name.strip_prefix('\\') {
            return self.find_or_load_class(stripped);
        }

        // ── Unqualified name (no `\` at all) ────────────────
        if !name.contains('\\') {
            // Check the import table first (`use` statements).
            if let Some(fqn) = file_use_map.get(name) {
                return self.find_or_load_class(fqn);
            }
            // Check local classes (same-file shortcut).
            // In multi-namespace files, prefer the class whose
            // file_namespace matches the current namespace context.
            let lookup = short_name(name);
            let ns_matches = |c: &ClassInfo| match (&c.file_namespace, file_namespace) {
                (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                (None, None) => true,
                _ => false,
            };
            let local_match = local_classes
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case(lookup) && ns_matches(c))
                .or_else(|| {
                    local_classes
                        .iter()
                        .find(|c| c.name.eq_ignore_ascii_case(lookup))
                });
            if let Some(cls) = local_match {
                return Some(Arc::clone(cls));
            }
            // In a namespace, try the namespace-qualified form first.
            // Per PHP semantics, class names do NOT fall back to global
            // scope (unlike functions/constants).  However, names that
            // arrive here may be already-resolved FQNs from ClassInfo
            // fields (e.g. `parent_class = "Exception"`) that happen to
            // be single-segment global names.  For those, the namespace-
            // qualified attempt will fail, so we fall back to a direct
            // lookup.  To preserve PHP semantics for user-typed code,
            // the namespace-qualified form is tried first and wins when
            // a same-named class exists in the current namespace.
            if let Some(ns) = file_namespace {
                let ns_qualified = format!("{}\\{}", ns, name);
                if let Some(cls) = self.find_or_load_class(&ns_qualified) {
                    return Some(cls);
                }
            }
            // Global scope: either no namespace context, or the
            // namespace-qualified lookup above did not find a match.
            return self.find_or_load_class(name);
        }

        // ── Qualified name (contains `\`, no leading `\`) ───
        // Check if the first segment is a use-map alias
        // (e.g. `OA\Endpoint` where `use Swagger\OpenAPI as OA;`
        // maps `OA` → `Swagger\OpenAPI`).  Expand to FQN.
        let first_segment = name.split('\\').next().unwrap_or(name);
        if let Some(fqn_prefix) = file_use_map.get(first_segment) {
            let rest = &name[first_segment.len()..];
            let expanded = format!("{}{}", fqn_prefix, rest);
            if let Some(cls) = self.find_or_load_class(&expanded) {
                return Some(cls);
            }
        }
        // Prepend current namespace (if any).
        if let Some(ns) = file_namespace {
            let ns_qualified = format!("{}\\{}", ns, name);
            if let Some(cls) = self.find_or_load_class(&ns_qualified) {
                return Some(cls);
            }
        }
        // Fall back to the name as-is.  Qualified names that
        // reach here are typically already-resolved FQNs from
        // the parser (parent classes, traits, mixins) that
        // were resolved by `resolve_parent_class_names` before
        // being stored.
        self.find_or_load_class(name)
    }

    /// Resolve a function name using use-map and namespace context.
    ///
    /// Builds a list of candidate names (exact name, use-map resolved,
    /// namespace-qualified) and tries each via `find_or_load_function`.
    ///
    /// This is the single canonical implementation of the "function_loader"
    /// logic used by both the completion handler and definition resolver.
    pub(crate) fn resolve_function_name(
        &self,
        name: &str,
        file_use_map: &HashMap<String, String>,
        file_namespace: &Option<String>,
    ) -> Option<FunctionInfo> {
        // Build candidate names to try: exact name, use-map
        // resolved name, and namespace-qualified name.
        let mut candidates: Vec<&str> = vec![name];

        let use_resolved: Option<String> = file_use_map.get(name).cloned();
        if let Some(ref fqn) = use_resolved {
            candidates.push(fqn.as_str());
        }

        let ns_qualified: Option<String> = file_namespace
            .as_ref()
            .map(|ns| format!("{}\\{}", ns, name));
        if let Some(ref nq) = ns_qualified {
            candidates.push(nq.as_str());
        }

        // Unified lookup: checks global_functions first, then
        // falls back to embedded PHP stubs (parsed lazily and
        // cached for future lookups).
        self.find_or_load_function(&candidates)
    }

    // ─── Loader Closure Factories ───────────────────────────────────────

    /// Return a class-loader closure bound to a [`FileContext`].
    ///
    /// This is the convenience wrapper for the common case where the
    /// caller already has a `FileContext`.  For situations that need a
    /// different class list (e.g. patched/effective classes after error
    /// recovery), use [`class_loader_with`](Self::class_loader_with).
    pub(crate) fn class_loader<'a>(
        &'a self,
        ctx: &'a FileContext,
    ) -> impl Fn(&str) -> Option<Arc<ClassInfo>> + 'a {
        self.class_loader_with(&ctx.classes, &ctx.use_map, &ctx.namespace)
    }

    /// Return a class-loader closure from individual file-context
    /// components.
    ///
    /// Useful when the class list differs from what is stored in a
    /// `FileContext` (e.g. after re-parsing patched content for error
    /// recovery).
    pub(crate) fn class_loader_with<'a>(
        &'a self,
        classes: &'a [Arc<ClassInfo>],
        use_map: &'a HashMap<String, String>,
        namespace: &'a Option<String>,
    ) -> impl Fn(&str) -> Option<Arc<ClassInfo>> + 'a {
        move |name: &str| {
            // For unqualified names (no `\`), check the use-map first.
            // A `use Illuminate\Support\Facades\Event;` import must
            // take priority over a global-namespace stub class named
            // `Event` (e.g. the PECL event extension).  Without this
            // check, `find_or_load_class("Event")` would find the stub
            // and short-circuit, never consulting the use-map.
            let stripped = name.strip_prefix('\\').unwrap_or(name);
            if !stripped.contains('\\')
                && let Some(fqn) = use_map.get(stripped)
                && let Some(cls) = self.find_or_load_class(fqn)
            {
                return Some(cls);
            }

            // Try a direct FQN lookup.  Names that arrive here are
            // often already-resolved FQNs (e.g. `parent_class`,
            // `used_traits`, `interfaces` — all canonicalised by
            // `resolve_parent_class_names`).  Passing a bare global
            // name like `Exception` through namespace-aware resolution
            // would incorrectly yield `Test\Exception` when a
            // same-named class exists in the current namespace.
            //
            // The direct lookup is cheap (hash-map hit in fqn_index)
            // and correct for FQNs.  For user-typed unqualified names
            // that should resolve via namespace context, the direct
            // lookup will miss (no global class with that name) and
            // we fall through to full resolution.
            if let Some(cls) = self.find_or_load_class(stripped) {
                return Some(cls);
            }
            // When the name is namespace-qualified (e.g. "App\IteratorAggregate")
            // and the direct lookup failed, try the short name as a global class.
            // This handles the case where resolve_parent_class_names prepended the
            // file namespace to an unqualified global class name.
            if stripped.contains('\\') {
                let short = crate::util::short_name(stripped);
                if let Some(cls) = self.find_or_load_class(short) {
                    return Some(cls);
                }
            }
            self.resolve_class_name(name, classes, use_map, namespace)
        }
    }

    /// Return a function-loader closure bound to a [`FileContext`].
    ///
    /// This is the convenience wrapper for the common case where the
    /// caller already has a `FileContext`.  For situations that need
    /// explicit use-map / namespace values, use
    /// [`function_loader_with`](Self::function_loader_with).
    pub(crate) fn function_loader<'a>(
        &'a self,
        ctx: &'a FileContext,
    ) -> impl Fn(&str) -> Option<FunctionInfo> + 'a {
        self.function_loader_with(&ctx.use_map, &ctx.namespace)
    }

    /// Return a function-loader closure from individual file-context
    /// components.
    ///
    /// Useful when the caller does not have a full `FileContext` or
    /// needs to use a different use-map / namespace.
    pub(crate) fn function_loader_with<'a>(
        &'a self,
        use_map: &'a HashMap<String, String>,
        namespace: &'a Option<String>,
    ) -> impl Fn(&str) -> Option<FunctionInfo> + 'a {
        move |name: &str| self.resolve_function_name(name, use_map, namespace)
    }

    /// Return a constant-value-loader closure.
    ///
    /// The returned closure looks up a global constant name and returns
    /// `Some(Some(value))` when found with a known value,
    /// `Some(None)` when found without a value, and `None` when not
    /// found.
    pub(crate) fn constant_loader(&self) -> impl Fn(&str) -> Option<Option<String>> + '_ {
        move |name: &str| self.lookup_global_constant(name)
    }
}

#[cfg(test)]
mod tests {
    //! PHP resolves class, function, and method names case-insensitively
    //! (B25).  These tests exercise each lookup phase with a casing that
    //! differs from the declaration.

    use crate::Backend;

    static STDCLASS_STUB: &str = "<?php class stdClass {}";
    static STRING_FUNCTIONS_STUB: &str = "<?php function strlen(string $string): int {}";

    #[test]
    fn class_lookup_ignores_case_for_parsed_classes() {
        let backend = Backend::new_test();
        backend.update_ast(
            "file:///user.php",
            "<?php namespace App\\Models; class User {}",
        );

        for name in [
            "App\\Models\\User",
            "app\\models\\user",
            "APP\\MODELS\\USER",
        ] {
            let cls = backend.find_or_load_class(name);
            assert!(cls.is_some(), "lookup for {name:?} should resolve");
            assert_eq!(cls.unwrap().name, "User", "declared spelling is kept");
        }
    }

    #[test]
    fn class_lookup_ignores_case_for_stubs() {
        let mut stubs = std::collections::HashMap::new();
        stubs.insert("stdClass", STDCLASS_STUB);
        let backend = Backend::new_test_with_stubs(stubs);

        for name in ["stdClass", "stdclass", "STDCLASS"] {
            let cls = backend.find_or_load_class(name);
            assert!(cls.is_some(), "lookup for {name:?} should resolve");
            assert_eq!(cls.unwrap().name, "stdClass");
        }
    }

    #[test]
    fn negative_cache_does_not_pin_per_casing() {
        let backend = Backend::new_test();

        // Miss with one casing populates the negative cache…
        assert!(backend.find_or_load_class("app\\foo").is_none());
        assert!(backend.class_not_found_cache.read().contains("APP\\FOO"));

        // …but once the class is parsed, every casing resolves again.
        backend.update_ast("file:///foo.php", "<?php namespace App; class Foo {}");
        assert!(backend.find_or_load_class("app\\foo").is_some());
        assert!(backend.find_or_load_class("APP\\FOO").is_some());
    }

    #[test]
    fn function_lookup_ignores_case_for_user_functions() {
        let backend = Backend::new_test();
        backend.update_ast("file:///helpers.php", "<?php function myHelper() {}");

        for name in ["myHelper", "myhelper", "MYHELPER"] {
            let func = backend.find_or_load_function(&[name]);
            assert!(func.is_some(), "lookup for {name:?} should resolve");
            assert_eq!(func.unwrap().name, "myHelper");
        }
    }

    #[test]
    fn function_lookup_ignores_case_for_stubs() {
        let mut function_stubs = std::collections::HashMap::new();
        function_stubs.insert("strlen", STRING_FUNCTIONS_STUB);
        let backend = Backend::new_test_with_all_stubs(
            std::collections::HashMap::new(),
            function_stubs,
            std::collections::HashMap::new(),
        );

        for name in ["strlen", "STRLEN", "StrLen"] {
            let func = backend.find_or_load_function(&[name]);
            assert!(func.is_some(), "lookup for {name:?} should resolve");
            assert_eq!(func.unwrap().name, "strlen");
        }
    }

    #[test]
    fn method_lookup_ignores_case() {
        let backend = Backend::new_test();
        backend.update_ast(
            "file:///cls.php",
            "<?php class Widget { public function getValue(): int { return 1; } }",
        );

        let cls = backend.find_or_load_class("Widget").expect("class");
        for name in ["getValue", "getvalue", "GETVALUE"] {
            assert!(cls.has_method(name), "has_method({name:?})");
            assert_eq!(cls.get_method(name).expect("get_method").name, "getValue");
            assert!(
                cls.get_method_arc(name).is_some(),
                "get_method_arc({name:?})"
            );
        }

        // The indexed path (post-`rebuild_method_index`) must agree.
        let mut indexed = crate::types::ClassInfo::clone(&cls);
        indexed.rebuild_method_index();
        assert!(indexed.has_method("GETVALUE"));
        assert_eq!(
            indexed.get_method("getvalue").expect("indexed").name,
            "getValue"
        );
    }
}
