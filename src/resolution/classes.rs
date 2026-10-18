//! [`Backend::find_or_load_class`]: the class lookup pipeline, and the
//! linking a loaded class goes through before anything reads it.

use std::sync::Arc;

use crate::Backend;
use crate::class_loader_memo;
use crate::composer;
use crate::php_type::{PhpType, is_builtin_non_class_type};
use crate::types::{ClassInfo, FacadeAccessor};
use crate::util::short_name;

/// Whether [`Backend::link_loaded_class`] has anything to do for `class`.
pub(crate) fn needs_linking(class: &ClassInfo) -> bool {
    crate::type_engine::types::aliases::has_imported_type_aliases(class)
        || !class.unverified_doc_types.is_empty()
}

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
        if class_name == crate::virtual_members::laravel::CONFIGURED_DATE_CLASS_FQN {
            let configured = self.laravel_date_class.read().clone()?;
            let configured = configured
                .unwrap_or_else(|| crate::virtual_members::laravel::SUPPORT_CARBON_FQN.to_string());
            return self.find_or_load_class(&configured);
        }
        // A dot can never appear in a PHP identifier or namespace separator,
        // so a name containing one (a Laravel container key like
        // `demo.bakery` or `view.engine.resolver`) is never a class name.
        // `PhpType::parse` stops at the first character an identifier
        // cannot contain, so handing such a key to it would silently
        // truncate `demo.bakery` down to `demo` and let the ordinary
        // lookup phases match an unrelated class of that short name before
        // the alias table is ever consulted. Go there directly instead.
        if class_name.contains('.') {
            return self.resolve_laravel_alias(class_name);
        }
        // Defensively strip nullable prefix (`?Foo` → `Foo`) and generic
        // parameters (`Collection<int, User>` → `Collection`) so that
        // callers don't need to normalise before lookup.
        if let Some(cls) = self.find_or_load_class_typed(&PhpType::parse(class_name)) {
            return Some(cls);
        }
        // Fall back to Laravel's own alias tables (container string aliases and
        // global facade class aliases), parsed from the installed framework
        // source.  Only reached when every ordinary phase has missed, so a
        // real project class of the same name always wins, and non-class
        // strings like `blade.compiler` still resolve to their bound concrete
        // class.
        self.resolve_laravel_alias(class_name)
    }

    /// Like [`find_or_load_class`], but accepts a pre-parsed `PhpType`,
    /// avoiding the redundant `PhpType::parse()` call that the string
    /// overload performs internally.
    pub(crate) fn find_or_load_class_typed(&self, ty: &PhpType) -> Option<Arc<ClassInfo>> {
        let mut loaded = self.link_loaded_class(self.find_indexed_class(ty)?);
        // The refinements below stay outside the memo: each depends on an
        // index of its own (the configured auth model, registered macros,
        // many-to-many targets) that keeps changing as files are indexed.
        //
        // Refine the auth entry points' `user()` return type to the configured
        // model.  Gated on the cheap stored short name so the hot loader path
        // is untouched for every other class.
        if matches!(loaded.name.as_str(), "Guard" | "Request" | "Auth") {
            loaded = crate::virtual_members::laravel::patch_auth_user_class(self, loaded);
        }
        // Refine `disk()`/`drive()`/`cloud()`/`build()` from the abstract
        // `Filesystem`/`Cloud` contract to the concrete `FilesystemAdapter`
        // when no configured disk uses a custom `Storage::extend()` driver.
        if matches!(loaded.name.as_str(), "FilesystemManager" | "Storage") {
            loaded = crate::virtual_members::laravel::patch_storage_disk_type(self, loaded);
        }
        // A facade whose `getFacadeAccessor()` names a container binding
        // needs the alias table to reach the class it forwards to, and the
        // virtual member provider that does the forwarding reads that table
        // from the resolved-class cache rather than from here.  Building it
        // now, while the `Backend` is in hand, is what puts it there: this
        // runs before the facade is ever fully resolved, because full
        // resolution reloads the class through this loader first.
        if loaded
            .laravel()
            .is_some_and(|l| matches!(l.facade_accessor, Some(FacadeAccessor::Alias(_))))
        {
            self.ensure_laravel_aliases();
        }
        // Add any Laravel macros registered on this class.  Gated on a cheap
        // atomic so the hot loader path is untouched when no macros exist.
        loaded = self.inject_laravel_macros(loaded);
        // Add pivot accessors when this class is a many-to-many target.
        loaded = self.inject_laravel_pivot(loaded);
        Some(loaded)
    }

    /// The class `ty` names as indexed, before
    /// [`find_or_load_class_typed`](Self::find_or_load_class_typed) links
    /// it and refines it.
    pub(super) fn find_indexed_class(&self, ty: &PhpType) -> Option<Arc<ClassInfo>> {
        // Report the lookup to whoever is recording what a resolution
        // depends on, before the memo below can answer it without touching
        // the class index.  A name that finds nothing is reported too: it
        // gains a declaration as readily as an existing one changes.
        if let Some(base) = ty.base_name() {
            crate::resolution_deps::record(base);
        }
        // The name search is memoised per thread on the interned type
        // handle: the diagnostic pass asks for the same types millions of
        // times, and every miss costs two case-insensitive hash lookups
        // behind locks the whole worker pool shares.  Read the generation
        // first so that a change landing mid-search retires the answer.
        let generation = self.symbols.class_lookup_generation();
        match class_loader_memo::probe(self.symbols.id(), generation, ty) {
            Some(memoised) => memoised,
            None => {
                let found = ty
                    .base_name()
                    .and_then(|base| self.find_or_load_class_inner(base));
                class_loader_memo::store(self.symbols.id(), generation, ty, &found);
                found
            }
        }
    }

    /// `class` with what its parse left for loading done: the type aliases
    /// it imports linked into its members, and each docblock type that names
    /// a class the native hint does not kept only where it narrows the hint.
    ///
    /// Both read other classes: an imported alias is defined in another
    /// file, and whether a class narrows the hint takes its ancestors.  The
    /// result is cached against this very `Arc` together with the classes
    /// it read, so that editing any of them drops it (see `evict_fqn`).  A
    /// result that missed a class is only kept until the class index
    /// changes, since the class may be indexed later.
    pub(crate) fn link_loaded_class(&self, class: Arc<ClassInfo>) -> Arc<ClassInfo> {
        use crate::type_engine::types::{aliases, docblock_narrowing};
        if !needs_linking(&class) {
            return class;
        }
        // Read before the classes are, so that one indexed meanwhile
        // retires the result.
        let generation = self.symbols.class_lookup_generation();
        if let Some((linked, sources)) = self
            .resolved_class_cache
            .read()
            .get_linked_class(&class, generation)
        {
            // Whoever is recording what this lookup depends on depends on
            // the classes read too, cached or not.
            for source in sources {
                crate::resolution_deps::record(source);
            }
            return linked;
        }
        let sources = std::cell::RefCell::new(Vec::new());
        let missed = std::cell::Cell::new(false);
        // The classes read are loaded unlinked: linking only reads their
        // alias definitions and ancestors, and two classes may read each
        // other.
        let loader = |name: &str| {
            let name = name.strip_prefix('\\').unwrap_or(name);
            sources.borrow_mut().push(name.to_string());
            let found = self.find_indexed_class(&PhpType::parse(name));
            missed.set(missed.get() || found.is_none());
            found
        };
        let (linked, aliases_complete) = if aliases::has_imported_type_aliases(&class) {
            aliases::link_imported_type_aliases(&class, &loader)
        } else {
            (None, true)
        };
        let verified = docblock_narrowing::verify_class(linked.as_ref().unwrap_or(&class), &loader);
        let changed = verified.or(linked).map(Arc::new);
        let complete = aliases_complete && !missed.get();
        let mut sources = sources.into_inner();
        sources.sort_unstable();
        sources.dedup();
        self.resolved_class_cache.write().insert_linked_class(
            &class,
            changed.clone(),
            sources,
            (!complete).then_some(generation),
        );
        changed.unwrap_or(class)
    }

    /// Add Laravel macro methods registered on `class` (by FQN).
    ///
    /// A no-op unless the project registered at least one
    /// `Target::macro('name', closure)` on this class.  See
    /// [`laravel_macros`](crate::Backend::laravel_macros).
    fn inject_laravel_macros(&self, class: Arc<ClassInfo>) -> Arc<ClassInfo> {
        if !self
            .laravel_has_macros
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return class;
        }
        let index = self.laravel_macros.read();
        crate::virtual_members::laravel::inject_macros(&index, class)
    }

    /// Add Eloquent pivot accessors to `class` when it is the target of a
    /// many-to-many relationship somewhere in the project.
    ///
    /// The reverse index is rebuilt lazily when a pivot-bearing file has
    /// changed (`laravel_pivots_dirty`), then a cheap atomic gates the lock so
    /// the hot loader path is untouched when the project has no such relations.
    fn inject_laravel_pivot(&self, class: Arc<ClassInfo>) -> Arc<ClassInfo> {
        use std::sync::atomic::Ordering;
        if self.laravel_pivots_dirty.swap(false, Ordering::Relaxed) {
            self.rebuild_laravel_pivot_index();
        }
        if !self.laravel_has_pivots.load(Ordering::Relaxed) {
            return class;
        }
        let index = self.laravel_pivots.read();
        crate::virtual_members::laravel::inject_pivot(&index, class)
    }

    /// Mark the reverse pivot index stale when `content` (at `uri`) affects
    /// many-to-many relationships — either it now looks like it declares
    /// one, or it previously contributed one (an edit may have
    /// removed the last such relation). A no-op for unrelated edits.
    pub(crate) fn refresh_laravel_pivots(&self, uri: &str, content: &str) {
        use std::sync::atomic::Ordering;
        if self.laravel_pivots_dirty.load(Ordering::Relaxed) {
            return;
        }
        let has_m2m = crate::virtual_members::laravel::source_may_declare_pivot_relationship(
            content.as_bytes(),
        );
        if has_m2m || self.laravel_pivots.read().contributes(uri) {
            self.laravel_pivots_dirty.store(true, Ordering::Relaxed);
            // A pivot accessor is attached to the *target* model, which the
            // file declaring the relation never has to be looked up for, so a
            // cached receiver resolution records no dependency on this index.
            self.clear_resolved_member_files();
        }
    }

    /// Empty the resolved-class cache, and the caches whose entries were
    /// derived from it.
    ///
    /// The cached receiver resolutions a reference search leaves behind are
    /// kept across an edit by intersecting what each one consulted with what
    /// the edit changed, and the *unconsulted* half of that, a parent read
    /// out of this cache rather than loaded, is recovered from this cache's
    /// reverse-dependency graph.  Emptying the cache discards that graph, so
    /// there is no longer anything to recover it from and the resolutions go
    /// with it.  Prefer `evict_fqn` over this wherever the changed classes
    /// can be named.
    pub(crate) fn clear_resolved_class_cache(&self) {
        self.resolved_class_cache.write().clear();
        self.clear_resolved_member_files();
        // Whatever made every resolved class stale (a configuration
        // reload, a re-scan) makes every open file's slow diagnostics
        // stale too.
        self.diag.epoch.note_change();
    }

    /// Rebuild the reverse pivot index from every parsed class.
    ///
    /// Collects an `(uri, class)` snapshot, releases the index lock, then
    /// resolves related/pivot class names via the normal loader. A no-op-shaped
    /// empty index is stored for non-Laravel projects so the gate stays off.
    pub(crate) fn rebuild_laravel_pivot_index(&self) {
        let index = if self.resolved_class_cache.read().is_laravel() {
            let classes: Vec<(String, Arc<ClassInfo>)> = {
                let idx = self.symbols.uri_classes_index.read();
                idx.iter()
                    .flat_map(|(uri, list)| list.iter().map(move |c| (uri.clone(), Arc::clone(c))))
                    .collect()
            };
            let loader = |name: &str| self.find_or_load_class(name);
            crate::virtual_members::laravel::build_pivot_index(&classes, &loader)
        } else {
            crate::virtual_members::laravel::LaravelPivotIndex::default()
        };
        let non_empty = !index.is_empty();
        *self.laravel_pivots.write() = index;
        self.laravel_has_pivots
            .store(non_empty, std::sync::atomic::Ordering::Relaxed);
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
        if self
            .symbols
            .class_not_found_cache
            .read()
            .contains(class_name)
        {
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

        // ── Phase 0.5: built-in names resolve to the embedded stubs ──
        // A global-namespace class name that exists in the embedded
        // stub index is a PHP built-in (core or bundled extension).
        // Vendor code can only redeclare such a name inside a
        // conditional block: a polyfill mirroring the built-in for
        // older runtimes.  symfony/polyfill-php84, for example, ships
        // two `RoundingMode` declarations — an enum and a pre-enum
        // class whose "cases" are plain int constants.  Resolving the
        // name through the classmap (Phase 1) would pick whichever
        // polyfill file happened to be indexed first, which is
        // nondeterministic when several files declare the name and
        // wrong whenever a legacy variant wins (enum cases would
        // resolve to int constants).  The stub is the authoritative
        // definition of a built-in, so it wins over classmap entries.
        if expected_ns.is_none()
            && let Some(cls) = self.load_stub_class(last_segment)
        {
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
        let class_index_uri = self.symbols.fqn_uri_index.read().get(class_name).cloned();
        if let Some(file_uri) = class_index_uri
            && let Some(classes) = self.parse_and_cache_uri(&file_uri)
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
        let workspace_root = self.workspace.workspace_root.read().clone();
        if let Some(workspace_root) = workspace_root {
            let file_path = {
                let mappings = self.workspace.psr4_mappings.read();
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
            if let Some(classes) = self.parse_and_cache_stub(stub_content, &stub_uri, ver)
                && let Some(cls) = classes
                    .iter()
                    .find(|c| c.name.eq_ignore_ascii_case(last_segment))
            {
                return Some(Arc::clone(cls));
            }
        }

        // Cache the negative result so subsequent lookups for the same
        // unknown class skip the expensive multi-phase search.
        self.symbols
            .class_not_found_cache
            .write()
            .insert(class_name);
        None
    }

    /// Forget which class and function names failed to resolve, so names
    /// looked up before an index was populated are searched again.
    ///
    /// Callers reach for this after growing `fqn_uri_index` (a classmap
    /// scan, a phar scan) or when files changed on disk. It also retires
    /// the memoised lookups in
    /// [`class_loader_memo`](crate::class_loader_memo), which would
    /// otherwise keep answering from the negatives just cleared.
    pub(crate) fn clear_not_found_caches(&self) {
        self.symbols.class_not_found_cache.write().clear();
        self.symbols.function_not_found_cache.write().clear();
        self.symbols.note_class_lookup_change();
    }
}
