//! [`Backend::find_or_load_class`]: the class lookup pipeline, and the
//! linking a loaded class goes through before anything reads it.

use std::sync::Arc;

use crate::Backend;
use crate::class_loader_memo;
use crate::class_lookup::Fqn;
use crate::composer;
use crate::php_type::{PhpType, is_builtin_non_class_type};
use crate::resolution::{ProjectScope, project_scope};
use crate::types::ClassInfo;
use crate::util::short_name;

/// What a framework adds to class lookup.
///
/// The loader runs every registered hook on every lookup, so one with
/// nothing to do for a name or a class must return at once.
pub(crate) struct ClassLoadHooks {
    /// A name the framework alone gives a meaning (a container key, a
    /// configured stand-in for a class): `Some` settles the lookup, with
    /// the class the name stands for if there is one.
    pub(crate) claim_name: fn(&Backend, &str) -> Option<Option<Arc<ClassInfo>>>,
    /// The class for a name every ordinary phase missed.
    pub(crate) fallback: fn(&Backend, &str) -> Option<Arc<ClassInfo>>,
    /// A loaded class with what the framework's project facts add to it.
    ///
    /// Memoised per class, so it must depend on nothing but the class and
    /// the facts [`refine_generation`](Self::refine_generation) stamps.
    pub(crate) refine: fn(&Backend, Arc<ClassInfo>) -> Arc<ClassInfo>,
    /// A stamp that moves whenever `refine` could make something different
    /// of the same class.  Brings any index `refine` reads that is due a
    /// rebuild up to date first, so that the stamp describes what it will
    /// read.
    pub(crate) refine_generation: fn(&Backend) -> u64,
}

/// The frameworks whose facts the loader applies, in order.
const CLASS_LOAD_HOOKS: &[ClassLoadHooks] = &[crate::backend::laravel::CLASS_LOAD_HOOKS];

/// Whether [`Backend::link_loaded_class`] has anything to do for `class`.
pub(crate) fn needs_linking(class: &ClassInfo) -> bool {
    crate::type_engine::type_ops::aliases::has_imported_type_aliases(class)
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
        for hooks in CLASS_LOAD_HOOKS {
            if let Some(claimed) = (hooks.claim_name)(self, class_name) {
                return claimed;
            }
        }
        // Defensively strip nullable prefix (`?Foo` → `Foo`) and generic
        // parameters (`Collection<int, User>` → `Collection`) so that
        // callers don't need to normalise before lookup.
        if let Some(cls) = self.find_or_load_class_typed(&PhpType::parse(class_name)) {
            return Some(cls);
        }
        // Only reached when every ordinary phase has missed, so a real
        // project class of the same name always wins.
        CLASS_LOAD_HOOKS
            .iter()
            .find_map(|hooks| (hooks.fallback)(self, class_name))
    }

    /// Like [`find_or_load_class`], but accepts a pre-parsed `PhpType`,
    /// avoiding the redundant `PhpType::parse()` call that the string
    /// overload performs internally.
    pub(crate) fn find_or_load_class_typed(&self, ty: &PhpType) -> Option<Arc<ClassInfo>> {
        let loaded = self.link_loaded_class(self.find_indexed_class(ty)?);
        Some(self.refine_loaded_class(loaded))
    }

    /// `loaded` with every framework hook's refinements applied.
    ///
    /// Memoised per thread on the class itself: most lookups refine nothing,
    /// yet checking is not free, and one that does refine copies the whole
    /// class.  A repeat hands back the same `Arc`.
    fn refine_loaded_class(&self, loaded: Arc<ClassInfo>) -> Arc<ClassInfo> {
        // Read before the hooks run, so that a change landing meanwhile
        // retires the answer.
        let generation = CLASS_LOAD_HOOKS.iter().fold(0u64, |sum, hooks| {
            sum.wrapping_add((hooks.refine_generation)(self))
        });
        let owner = self.symbols.id();
        if let Some(refined) = class_loader_memo::probe_refined(owner, generation, &loaded) {
            return refined;
        }
        let refined = CLASS_LOAD_HOOKS
            .iter()
            .fold(Arc::clone(&loaded), |class, hooks| {
                (hooks.refine)(self, class)
            });
        class_loader_memo::store_refined(owner, generation, &loaded, &refined);
        refined
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
        // The same name can find a different class in another Composer
        // project, so the answer is memoised for the active project.
        let generation = self.symbols.class_lookup_generation();
        let owner = self.symbols.id() ^ (u64::from(project_scope::active_id()) << 48);
        match class_loader_memo::probe(owner, generation, ty) {
            Some(memoised) => memoised,
            None => {
                let found = ty
                    .base_name()
                    .and_then(|base| self.find_or_load_class_inner(base));
                class_loader_memo::store(owner, generation, ty, &found);
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
        use crate::type_engine::type_ops::{aliases, docblock_narrowing};
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
        let loader = |name: Fqn<'_>| {
            sources.borrow_mut().push(name.to_string());
            let found = self.find_indexed_class(&PhpType::parse(&name));
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

        // ── Stub paths: a declaration there wins over every other ──
        if let Some(cls) = self.load_external_stub_class(class_name) {
            return Some(cls);
        }

        // ── Another Composer project's dependency ───────────────────
        // The index holds one declaration per name for the whole
        // workspace.  When that one lives in the vendor directory of a
        // project other than the one asking, the asking project only has
        // the class if it installed a copy of its own.  Not a miss worth
        // remembering: the other project does have the class.
        let scope = project_scope::active();
        if let Some(scope) = &scope {
            let indexed = self.symbols.fqn_uri_index.read().get(class_name).cloned();
            if let Some(uri) = indexed
                && !scope.sees(&uri)
            {
                return self.find_project_copy(class_name, last_segment, scope);
            }
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
        // The mappings are every project's, so a file another project's
        // mapping places the class in is skipped, and the miss is not
        // remembered for the projects that do have it.
        let mut hidden = false;
        let workspace_root = self.workspace.workspace_root.read().clone();
        if let Some(workspace_root) = workspace_root {
            let file_path = {
                let mappings = self.workspace.psr4_mappings.read();
                composer::psr4_class_files(&mappings, &workspace_root, class_name).find(|path| {
                    let seen = scope
                        .as_ref()
                        .is_none_or(|scope| scope.sees(&crate::util::path_to_uri(path)));
                    hidden |= !seen;
                    seen
                })
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
        // access and cache in the uri_classes_index under a stub URI so
        // subsequent lookups hit Phase 0 and skip parsing entirely.  Phase
        // 0.5 already covered global names; this is where a namespaced
        // extension class (Ds, BcMath, Random, …) is found.
        if let Some(cls) = self.load_stub_class(class_name) {
            return Some(cls);
        }
        if hidden {
            return None;
        }

        // Cache the negative result so subsequent lookups for the same
        // unknown class skip the expensive multi-phase search.
        self.symbols
            .class_not_found_cache
            .write()
            .insert(class_name);
        None
    }

    /// The declaration of `class_name` that `scope` sees, other than the
    /// one the index holds: one in the project's own vendor directory
    /// first, else one outside every vendor directory.
    fn find_project_copy(
        &self,
        class_name: &str,
        last_segment: &str,
        scope: &ProjectScope,
    ) -> Option<Arc<ClassInfo>> {
        for uri in scope.visible_alternates(self.symbols.alternate_class_uris(class_name)) {
            let parsed = self
                .symbols
                .duplicate_classes
                .read()
                .get(class_name)
                .and_then(|decls| decls.get(&uri).cloned());
            if parsed.is_some() {
                return parsed;
            }
            if let Some(classes) = self.parse_and_cache_uri(&uri)
                && let Some(cls) = classes
                    .iter()
                    .find(|c| c.name.eq_ignore_ascii_case(last_segment))
            {
                return Some(Arc::clone(cls));
            }
        }
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
