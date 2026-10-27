//! The workspace symbol indexes, grouped out of `Backend`.
//!
//! These are the class, function, and constant lookup tables that answer
//! "where is symbol X defined?" across the whole workspace. All fields are
//! `Arc`-wrapped, so `#[derive(Clone)]` shares them with a cloned `Backend`
//! (the same semantics the per-request clone had as individual fields).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;

use crate::ci_map::{CiMap, CiSet};
use crate::classmap_scanner::WorkspaceScanResult;
use crate::types::{ClassInfo, DefineInfo, FunctionInfo};
use crate::{ClassCompletionOrigin, UriGlobals};

/// Every declaration of the names that more than one file declares, keyed
/// by name and then by declaring URI.
pub(crate) type DuplicateDeclarations<V> = CiMap<BTreeMap<String, V>>;

/// Class, function, and constant discovery and lookup indexes.
#[derive(Clone)]
pub(crate) struct SymbolIndex {
    /// Maps a file URI to a list of `ClassInfo` extracted from that file.
    pub(crate) uri_classes_index: Arc<RwLock<HashMap<String, Vec<Arc<ClassInfo>>>>>,
    /// Global function definitions indexed by (case-insensitive) name.
    pub(crate) global_functions: Arc<RwLock<CiMap<(String, FunctionInfo)>>>,
    /// Every declaration of a function name that more than one file
    /// declares, keyed by name and then by declaring URI.
    ///
    /// A name only appears here once a second file declares it, so the
    /// overwhelming majority of functions cost nothing.  The lowest-sorting
    /// URI is the winner and is mirrored into `global_functions`; keeping
    /// the runners-up means that when the winning file stops declaring the
    /// name, the next declaration takes over instead of the name vanishing.
    pub(crate) duplicate_functions: Arc<RwLock<DuplicateDeclarations<FunctionInfo>>>,
    /// Global constants from `define()` / top-level `const` statements.
    pub(crate) global_defines: Arc<RwLock<HashMap<String, DefineInfo>>>,
    /// Per-URI record of the global function/constant symbols each file
    /// contributed at its last parse, for targeted eviction.
    pub(crate) uri_globals_index: Arc<RwLock<HashMap<String, UriGlobals>>>,
    /// Autoload function index: function FQN → file path on disk.
    pub(crate) autoload_function_index: Arc<RwLock<CiMap<PathBuf>>>,
    /// Completion provenance for autoloaded function symbols.  A name with
    /// no entry is project code.
    pub(crate) autoload_function_origin_index: Arc<RwLock<CiMap<ClassCompletionOrigin>>>,
    /// Autoload constant index: constant name → file path on disk.
    pub(crate) autoload_constant_index: Arc<RwLock<HashMap<String, PathBuf>>>,
    /// Completion provenance for autoloaded constant symbols.  A name with
    /// no entry is project code.
    pub(crate) autoload_constant_origin_index: Arc<RwLock<HashMap<String, ClassCompletionOrigin>>>,
    /// Paths of all files discovered through Composer's `autoload_files.php`.
    pub(crate) autoload_file_paths: Arc<RwLock<Vec<PathBuf>>>,
    /// Index of fully-qualified class names to file URIs.
    pub(crate) fqn_uri_index: Arc<RwLock<CiMap<String>>>,
    /// Completion provenance for fully-qualified class names.  A name with
    /// no entry is project code.
    pub(crate) fqn_origin_index: Arc<RwLock<CiMap<ClassCompletionOrigin>>>,
    /// Secondary index mapping FQNs directly to their parsed `ClassInfo`.
    pub(crate) fqn_class_index: Arc<RwLock<CiMap<Arc<ClassInfo>>>>,
    /// Every parsed declaration of a class name that more than one file
    /// declares, keyed by name and then by declaring URI.
    ///
    /// The class counterpart of [`duplicate_functions`](Self::duplicate_functions):
    /// a name only appears here once a second file declares it, the
    /// lowest-sorting URI is the winner mirrored into `fqn_uri_index` and
    /// `fqn_class_index`, and keeping the runners-up means the name
    /// survives the winning file dropping it.
    pub(crate) duplicate_classes: Arc<RwLock<DuplicateDeclarations<Arc<ClassInfo>>>>,
    /// The other files a class name was discovered in, when discovery
    /// found more than one: the locations a merge kept out of
    /// `fqn_uri_index` because the name was already indexed elsewhere,
    /// and the ones a later entry displaced from it.
    ///
    /// Like `duplicate_classes`, a name only appears here once a second
    /// file is found to declare it.  What this adds is the declarations no
    /// one has parsed yet: two Composer projects of a workspace that
    /// install the same package each have a copy, and a lookup from the
    /// project whose copy lost the index has to be able to find its own.
    pub(crate) class_alternates: Arc<RwLock<CiMap<Vec<String>>>>,
    /// Negative-result cache for `find_or_load_class`.
    pub(crate) class_not_found_cache: Arc<RwLock<CiSet>>,
    /// Negative-result cache for `find_or_load_function`.
    ///
    /// Without it, every lookup of a name no index carries re-runs the
    /// full fallback chain — including a walk over every known autoload
    /// file — and the forward walker asks about the same unresolved
    /// call over and over.  Entries are removed when a parse declares
    /// the name and cleared wholesale whenever the indexes grow, the
    /// same lifecycle `class_not_found_cache` follows.
    pub(crate) function_not_found_cache: Arc<RwLock<CiSet>>,
    /// Reverse inheritance index: parent FQN → list of child FQNs.
    pub(crate) gti_index: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// The same inheritance edges the other way round: child FQN → the
    /// parent FQNs (extends, implements, use) its declaration registered it
    /// under.
    ///
    /// Kept in step with [`gti_index`](Self::gti_index) so that dropping one
    /// file's classes touches only the child lists that file contributed to,
    /// instead of scanning every entry in a workspace-sized map on each
    /// edit, and so that re-registering a child tests a list the length of
    /// its own `extends`/`implements` clause rather than the list of every
    /// implementor of a popular interface.
    pub(crate) gti_parents_index: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// Identity of this set of indexes, unique for the life of the process
    /// and shared by every clone (a cloned `Backend` shares the indexes
    /// themselves).  Per-thread caches key on it so that a worker thread
    /// reused across `Backend`s — nextest runs many in one process — can
    /// never serve one project's classes to another.
    id: u64,
    /// Generation of the inputs to [`Backend::find_or_load_class`]:
    /// `fqn_class_index` and `class_not_found_cache`.  See
    /// [`note_class_lookup_change`](Self::note_class_lookup_change).
    class_lookup_generation: Arc<AtomicU64>,
    /// Bumped, under `function_not_found_cache`'s write lock, each time
    /// the cache is retired after `autoload_function_index` grew.  A
    /// lookup records a miss only if it is unchanged since the lookup
    /// began.
    function_index_generation: Arc<AtomicU64>,
}

impl SymbolIndex {
    pub(crate) fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            class_lookup_generation: Arc::new(AtomicU64::new(0)),
            function_index_generation: Arc::new(AtomicU64::new(0)),
            uri_classes_index: Arc::new(RwLock::new(HashMap::new())),
            global_functions: Arc::new(RwLock::new(CiMap::new())),
            duplicate_functions: Arc::new(RwLock::new(CiMap::new())),
            global_defines: Arc::new(RwLock::new(HashMap::new())),
            uri_globals_index: Arc::new(RwLock::new(HashMap::new())),
            autoload_function_index: Arc::new(RwLock::new(CiMap::new())),
            autoload_function_origin_index: Arc::new(RwLock::new(CiMap::new())),
            autoload_constant_index: Arc::new(RwLock::new(HashMap::new())),
            autoload_constant_origin_index: Arc::new(RwLock::new(HashMap::new())),
            autoload_file_paths: Arc::new(RwLock::new(Vec::new())),
            fqn_uri_index: Arc::new(RwLock::new(CiMap::new())),
            fqn_origin_index: Arc::new(RwLock::new(CiMap::new())),
            fqn_class_index: Arc::new(RwLock::new(CiMap::new())),
            duplicate_classes: Arc::new(RwLock::new(CiMap::new())),
            class_alternates: Arc::new(RwLock::new(CiMap::new())),
            class_not_found_cache: Arc::new(RwLock::new(CiSet::new())),
            function_not_found_cache: Arc::new(RwLock::new(CiSet::new())),
            gti_index: Arc::new(RwLock::new(HashMap::new())),
            gti_parents_index: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Drop the completion origin of every class, function and constant
    /// the discovery indexes no longer hold.  The origin maps are only
    /// ever written beside those indexes, so a vendor rescan or a deleted
    /// file that purges an entry would otherwise leave its origin behind.
    pub(crate) fn prune_orphaned_origins(&self) {
        {
            let idx = self.fqn_uri_index.read();
            self.fqn_origin_index
                .write()
                .retain(|fqn, _| idx.contains_key(fqn));
        }
        {
            let idx = self.autoload_function_index.read();
            self.autoload_function_origin_index
                .write()
                .retain(|fqn, _| idx.contains_key(fqn));
        }
        let idx = self.autoload_constant_index.read();
        self.autoload_constant_origin_index
            .write()
            .retain(|name, _| idx.contains_key(name));
    }

    pub(crate) fn function_index_generation(&self) -> u64 {
        self.function_index_generation.load(Ordering::Acquire)
    }

    /// Retire the function negative cache after the function index grew,
    /// invalidating misses recorded by lookups that began earlier.
    pub(crate) fn retire_function_misses(&self) {
        let mut nf_cache = self.function_not_found_cache.write();
        self.function_index_generation
            .fetch_add(1, Ordering::Release);
        nf_cache.clear();
    }

    /// Identity of this index, for keying per-thread caches.
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    /// The generation a class-lookup answer must be stamped with to still
    /// be valid.  Read it *before* performing the lookup: a bump that
    /// lands afterwards then invalidates the answer, rather than the
    /// answer being recorded as fresh when it already missed a change.
    pub(crate) fn class_lookup_generation(&self) -> u64 {
        self.class_lookup_generation.load(Ordering::Acquire)
    }

    /// Record that class lookups may now resolve differently, retiring
    /// every memoised answer in `class_loader_memo`.
    ///
    /// Call this *after* mutating `fqn_class_index` or clearing
    /// `class_not_found_cache` — the two structures every
    /// `find_or_load_class` answer is derived from.  Bumping afterwards is
    /// what makes the memo no staler than those caches themselves: a
    /// reader that already loaded the old generation stamps its answer
    /// with a value that no longer matches, so the answer is discarded
    /// rather than served.
    pub(crate) fn note_class_lookup_change(&self) {
        self.class_lookup_generation.fetch_add(1, Ordering::Release);
    }

    /// Every file known to declare the class `fqn` other than the one
    /// `fqn_uri_index` names, parsed or not.
    pub(crate) fn alternate_class_uris(&self, fqn: &str) -> Vec<String> {
        let mut uris: Vec<String> = self
            .duplicate_classes
            .read()
            .get(fqn)
            .map(|decls| decls.keys().cloned().collect())
            .unwrap_or_default();
        if let Some(alternates) = self.class_alternates.read().get(fqn) {
            for uri in alternates {
                if !uris.contains(uri) {
                    uris.push(uri.clone());
                }
            }
        }
        uris
    }

    /// Run `f` with the class indexes locked together.
    ///
    /// Every path that records or drops a class declaration goes through
    /// this, so `fqn_uri_index` and `fqn_class_index` cannot end up
    /// describing different files for the same name, and a runner-up
    /// declaration one path recorded is never left stale by another.  The
    /// locks are always taken in this order, so the paths cannot deadlock
    /// against each other.
    pub(crate) fn with_class_declarations<R>(
        &self,
        f: impl FnOnce(&mut ClassDeclarations<'_>) -> R,
    ) -> R {
        let mut uris = self.fqn_uri_index.write();
        let mut classes = self.fqn_class_index.write();
        let mut dupes = self.duplicate_classes.write();
        let mut alternates = self.class_alternates.write();
        f(&mut ClassDeclarations {
            uris: &mut uris,
            classes: &mut classes,
            dupes: &mut dupes,
            alternates: &mut alternates,
        })
    }
}

/// What a merge does with a name the index already holds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OnConflict {
    /// Keep the entry already there, so the scan that ran first wins.
    Keep,
    /// Take the scan's entry, for a rescan that supersedes what was indexed
    /// before.
    Replace,
}

impl SymbolIndex {
    /// Merge what a discovery scan found into the class, function and
    /// constant indexes, keeping every entry already there.
    ///
    /// Each scan only fills the gaps the ones before it left, so the order
    /// the discovery pipelines run in is their priority.  An origin is
    /// recorded only beside the location the index keeps, so it always
    /// describes the file the symbol resolves to: a scan that finds a name
    /// already indexed at another file leaves that entry's origin alone,
    /// and one that finds it at the same file refreshes it.  A name the
    /// scan has no origin for leaves the origin maps untouched.
    pub(crate) fn merge_scan(&self, scan: WorkspaceScanResult) {
        self.merge_keeping(scan, crate::util::path_to_uri);
    }

    /// [`merge_scan`](Self::merge_scan) for the classes of the PHAR
    /// archive at `phar_path`, whose classmap paths name entries inside
    /// the archive.
    pub(crate) fn merge_phar_scan(&self, scan: WorkspaceScanResult, phar_path: &Path) {
        self.merge_keeping(scan, |entry| crate::util::phar_entry_uri(phar_path, entry));
    }

    /// Replace what the indexes hold under the directory `tree` (given in
    /// every spelling it has; a file stands for itself) with what a rescan
    /// of it found.
    ///
    /// Each index is purged and refilled under one hold of its lock, so a
    /// lookup never sees the tree half gone.  The rescan's entries replace
    /// any already there, so a name the purge did not reach (one indexed
    /// under another spelling of the same file) still ends up where the
    /// rescan found it.
    pub(crate) fn replace_tree(&self, tree: &[PathBuf], scan: WorkspaceScanResult) {
        let WorkspaceScanResult {
            classmap,
            class_origins,
            function_index,
            function_origins,
            constant_index,
            constant_origins,
            package_roots: _,
        } = scan;
        // An entry of `tree` may also be a single file, which its own URI
        // matches exactly.
        let uri_prefixes: Vec<(String, String)> = tree
            .iter()
            .map(|dir| {
                (
                    crate::util::path_to_uri(dir),
                    crate::util::dir_uri_prefix(dir),
                )
            })
            .collect();
        let in_tree = |path: &PathBuf| tree.iter().any(|dir| path.starts_with(dir));
        let uri_in_tree = |uri: &str| {
            uri_prefixes
                .iter()
                .any(|(exact, prefix)| uri == exact || uri.starts_with(prefix.as_str()))
        };
        {
            let mut index = self.fqn_uri_index.write();
            let mut origins = self.fqn_origin_index.write();
            let mut alternates = self.class_alternates.write();
            let mut purged: Vec<String> = Vec::new();
            index.retain(|fqn, uri| {
                let keep = !uri_in_tree(uri);
                if !keep && !alternates.is_empty() {
                    purged.push(fqn.to_owned());
                }
                keep
            });
            if !alternates.is_empty() {
                alternates.retain(|_, uris| {
                    uris.retain(|uri| !uri_in_tree(uri));
                    !uris.is_empty()
                });
            }
            merge_into(
                &mut *index,
                &mut *origins,
                classmap,
                class_origins,
                crate::util::path_to_uri,
                OnConflict::Replace,
                Some(&mut |fqn: &str, uri: String| push_alternate(&mut alternates, fqn, uri)),
            );
            // A name the rescan no longer declares falls to another file
            // that does, as it would for a parsed duplicate.
            for fqn in purged {
                if !index.contains_key(&fqn)
                    && let Some(uri) = pop_alternate(&mut alternates, &fqn)
                {
                    index.insert(fqn, uri);
                }
            }
        }
        {
            let mut index = self.autoload_function_index.write();
            index.retain(|_, path| !in_tree(path));
            merge_into(
                &mut *index,
                &mut *self.autoload_function_origin_index.write(),
                function_index,
                function_origins,
                Path::to_path_buf,
                OnConflict::Replace,
                None,
            );
        }
        {
            let mut index = self.autoload_constant_index.write();
            index.retain(|_, path| !in_tree(path));
            merge_into(
                &mut *index,
                &mut *self.autoload_constant_origin_index.write(),
                constant_index,
                constant_origins,
                Path::to_path_buf,
                OnConflict::Replace,
                None,
            );
        }
        self.prune_orphaned_origins();
    }

    /// The merge behind [`merge_scan`](Self::merge_scan) and
    /// [`merge_phar_scan`](Self::merge_phar_scan).  `class_uri` names the
    /// URI a class's file is indexed under.
    fn merge_keeping(&self, scan: WorkspaceScanResult, class_uri: impl Fn(&Path) -> String) {
        let WorkspaceScanResult {
            classmap,
            class_origins,
            function_index,
            function_origins,
            constant_index,
            constant_origins,
            package_roots: _,
        } = scan;
        if !classmap.is_empty() {
            let mut index = self.fqn_uri_index.write();
            let mut origins = self.fqn_origin_index.write();
            let mut alternates = self.class_alternates.write();
            merge_into(
                &mut *index,
                &mut *origins,
                classmap,
                class_origins,
                class_uri,
                OnConflict::Keep,
                Some(&mut |fqn: &str, uri: String| push_alternate(&mut alternates, fqn, uri)),
            );
        }
        if !function_index.is_empty() {
            merge_into(
                &mut *self.autoload_function_index.write(),
                &mut *self.autoload_function_origin_index.write(),
                function_index,
                function_origins,
                Path::to_path_buf,
                OnConflict::Keep,
                None,
            );
        }
        if !constant_index.is_empty() {
            merge_into(
                &mut *self.autoload_constant_index.write(),
                &mut *self.autoload_constant_origin_index.write(),
                constant_index,
                constant_origins,
                Path::to_path_buf,
                OnConflict::Keep,
                None,
            );
        }
    }
}

/// Receives a location of a name that a merge did not index; see
/// [`merge_into`].
type Displaced<'a, V> = &'a mut dyn FnMut(&str, V);

/// Merge one kind of symbol into its index and origin map.  `location`
/// turns a scanned path into what the index stores, and is only called
/// when the index needs it.
///
/// `displaced` is handed every location of a name that the index ended up
/// not holding because another location of it won: the scan's own when it
/// is kept out, the one already indexed when the scan's replaces it.
/// Without it, a conflict only works out the scan's location when the
/// origin needs refreshing.
fn merge_into<V: PartialEq>(
    index: &mut impl NameMap<V>,
    origins: &mut impl NameMap<ClassCompletionOrigin>,
    entries: HashMap<String, PathBuf>,
    mut scan_origins: HashMap<String, ClassCompletionOrigin>,
    location: impl Fn(&Path) -> V,
    on_conflict: OnConflict,
    mut displaced: Option<Displaced<'_, V>>,
) {
    for (name, path) in entries {
        let origin = scan_origins.remove_entry(&name);
        match on_conflict {
            OnConflict::Replace => {
                match origin {
                    Some((key, origin)) => set_origin(origins, key, origin),
                    // Whatever origin the replaced entry had described its
                    // own file.
                    None => origins.remove(&name),
                }
                let here = location(&path);
                match displaced.as_mut() {
                    Some(displaced) => {
                        if let Some(previous) = index.insert(name.clone(), here)
                            && index.get(&name) != Some(&previous)
                        {
                            displaced(&name, previous);
                        }
                    }
                    None => {
                        index.insert(name, here);
                    }
                }
            }
            OnConflict::Keep if displaced.is_some() => {
                match index.try_insert_with(name, || location(&path)) {
                    Ok(()) => {
                        if let Some((key, origin)) = origin
                            && origin != ClassCompletionOrigin::Project
                        {
                            origins.insert(key, origin);
                        }
                    }
                    Err((name, existing)) => {
                        let here = location(&path);
                        if *existing != here {
                            if let Some(displaced) = displaced.as_mut() {
                                displaced(&name, here);
                            }
                        } else if let Some((key, origin)) = origin
                            && recorded_origin(origins, &name) != origin
                        {
                            set_origin(origins, key, origin);
                        }
                    }
                }
            }
            OnConflict::Keep => match index.try_insert_with(name, || location(&path)) {
                // A name new to the index has no origin of its own to clear.
                Ok(()) => {
                    if let Some((key, origin)) = origin
                        && origin != ClassCompletionOrigin::Project
                    {
                        origins.insert(key, origin);
                    }
                }
                // Already indexed.  Only a scan that found the same file
                // may refresh the origin, and its location is only worked
                // out when the refresh would change something.
                Err((name, existing)) => {
                    if let Some((key, origin)) = origin
                        && recorded_origin(origins, &name) != origin
                        && *existing == location(&path)
                    {
                        set_origin(origins, key, origin);
                    }
                }
            },
        }
    }
}

/// The origin recorded for `name`.
fn recorded_origin(
    origins: &impl NameMap<ClassCompletionOrigin>,
    name: &str,
) -> ClassCompletionOrigin {
    origins.get(name).copied().unwrap_or_default()
}

/// Record `origin` for `name`.  Project code is what every reader falls
/// back to, so it is recorded by having no entry, which keeps the origin
/// maps the size of the vendor tree rather than of the whole index.
fn set_origin(
    origins: &mut impl NameMap<ClassCompletionOrigin>,
    name: String,
    origin: ClassCompletionOrigin,
) {
    if origin == ClassCompletionOrigin::Project {
        origins.remove(&name);
    } else {
        origins.insert(name, origin);
    }
}

/// The two map shapes the discovery indexes come in: [`CiMap`] for the
/// class and function names PHP folds, a plain `HashMap` for constant
/// names, which it does not.
trait NameMap<V> {
    fn get(&self, name: &str) -> Option<&V>;
    /// Insert `make()` under `name` unless an entry already exists, in
    /// which case `name` is handed back with the value already there.
    fn try_insert_with(
        &mut self,
        name: String,
        make: impl FnOnce() -> V,
    ) -> Result<(), (String, &V)>;
    /// Insert `value` under `name`, returning what it replaced.
    fn insert(&mut self, name: String, value: V) -> Option<V>;
    fn remove(&mut self, name: &str);
}

impl<V> NameMap<V> for CiMap<V> {
    fn get(&self, name: &str) -> Option<&V> {
        CiMap::get(self, name)
    }

    fn try_insert_with(
        &mut self,
        name: String,
        make: impl FnOnce() -> V,
    ) -> Result<(), (String, &V)> {
        CiMap::try_insert_with(self, name, make)
    }

    fn insert(&mut self, name: String, value: V) -> Option<V> {
        CiMap::insert(self, name, value)
    }

    fn remove(&mut self, name: &str) {
        CiMap::remove(self, name);
    }
}

impl<V> NameMap<V> for HashMap<String, V> {
    fn get(&self, name: &str) -> Option<&V> {
        HashMap::get(self, name)
    }

    fn try_insert_with(
        &mut self,
        name: String,
        make: impl FnOnce() -> V,
    ) -> Result<(), (String, &V)> {
        // Probed before inserting, since the entry API would take the name
        // it has to hand back.
        if self.contains_key(&name) {
            let existing = &self[&name];
            return Err((name, existing));
        }
        self.insert(name, make());
        Ok(())
    }

    fn insert(&mut self, name: String, value: V) -> Option<V> {
        HashMap::insert(self, name, value)
    }

    fn remove(&mut self, name: &str) {
        HashMap::remove(self, name);
    }
}

impl SymbolIndex {
    /// Record the inheritance edges of `classes` in the GTI
    /// (go-to-implementation) reverse inheritance index.  For each class,
    /// inserts the class's FQN into the child list of every parent
    /// (parent_class, interfaces, used_traits), and records the same edges
    /// under the class itself in `gti_parents_index` so they can be
    /// withdrawn again without searching for them.
    pub(crate) fn index_inheritance(&self, classes: &[Arc<ClassInfo>]) {
        let mut gti = self.gti_index.write();
        let mut parents_index = self.gti_parents_index.write();
        for cls in classes {
            if cls.name.starts_with("__anonymous@") {
                continue;
            }
            if cls.parent_class.is_none() && cls.interfaces.is_empty() && cls.used_traits.is_empty()
            {
                continue;
            }

            let child_fqn = cls.fqn().to_string();
            let registered = parents_index.entry(child_fqn.clone()).or_default();

            for parent in cls
                .parent_class
                .iter()
                .chain(cls.interfaces.iter())
                .chain(cls.used_traits.iter())
            {
                let parent_fqn: &str = parent;
                // The edge is deduplicated against the child's own parents,
                // a list as long as its `extends`/`implements`/`use`
                // clauses, rather than against the parent's child list,
                // which grows with the number of implementors.
                if registered.iter().any(|p| p == parent_fqn) {
                    continue;
                }
                registered.push(parent_fqn.to_string());
                match gti.get_mut(parent_fqn) {
                    Some(children) => children.push(child_fqn.clone()),
                    None => {
                        gti.insert(parent_fqn.to_string(), vec![child_fqn.clone()]);
                    }
                }
            }
        }
    }

    /// Withdraw what [`index_inheritance`](Self::index_inheritance)
    /// recorded for the classes named `fqns`, before they are re-indexed or
    /// dropped: every GTI entry where one of them appears as a child.
    ///
    /// Only the parents each class was registered under are touched, so the
    /// cost is the size of the re-parsed file's inheritance clauses rather
    /// than the size of the workspace.
    pub(crate) fn withdraw_inheritance(&self, fqns: &[String]) {
        if fqns.is_empty() {
            return;
        }
        let mut gti = self.gti_index.write();
        let mut parents_index = self.gti_parents_index.write();
        for fqn in fqns {
            let Some(parents) = parents_index.remove(fqn.as_str()) else {
                continue;
            };
            for parent in parents {
                let Some(children) = gti.get_mut(&parent) else {
                    continue;
                };
                children.retain(|child| child != fqn);
                // Remove empty entries to avoid unbounded growth.
                if children.is_empty() {
                    gti.remove(&parent);
                }
            }
        }
    }
}

/// The class indexes, mutated together.
///
/// A class name can be declared by more than one file, which is how a
/// package ships a variant behind a `class_exists` guard.  The lowest
/// sorting URI wins, so the name resolves to the same declaration however
/// the parse workers were scheduled, and the declarations that lost are
/// kept so the name survives the winner withdrawing it.
pub(crate) struct ClassDeclarations<'a> {
    uris: &'a mut CiMap<String>,
    classes: &'a mut CiMap<Arc<ClassInfo>>,
    dupes: &'a mut DuplicateDeclarations<Arc<ClassInfo>>,
    alternates: &'a mut CiMap<Vec<String>>,
}

impl ClassDeclarations<'_> {
    /// The declaration lookups of `fqn` currently resolve to, if the file
    /// holding it has been parsed.
    pub(crate) fn winner(&self, fqn: &str) -> Option<&Arc<ClassInfo>> {
        self.classes.get(fqn)
    }

    /// Record that `uri` declares `fqn` with the given parsed class.
    pub(crate) fn declare(&mut self, fqn: &str, uri: &str, class: &Arc<ClassInfo>) -> Declared {
        // `get_mut` folds the name to lower case, allocating for the mixed
        // case names class FQNs invariably are.  Almost no project declares
        // a class twice, so skip the lookup outright while the record is
        // empty rather than paying it for every class of every parse.
        if !self.dupes.is_empty()
            && let Some(decls) = self.dupes.get_mut(fqn)
        {
            let previous = self.uris.get(fqn).cloned();
            decls.insert(uri.to_owned(), Arc::clone(class));
            self.publish_winner(fqn);
            let owner = self.uris.get(fqn);
            return Declared {
                won: owner.is_some_and(|owner| owner == uri),
                reowned: previous.is_some_and(|previous| Some(&previous) != owner),
            };
        }

        // A parse is better information than a classmap entry, which only
        // claims the name lives in a file: it names a file we have just
        // seen declare the name.  So only a second *parsed* declaration
        // starts a duplicate record.
        let reowned = self.uris.get(fqn).is_some_and(|owner| owner != uri);
        let displaced = match self.classes.get(fqn) {
            Some(class) if reowned => Some(Arc::clone(class)),
            _ => None,
        };
        match displaced {
            Some(displaced) => {
                let owner = self.uris.get(fqn).expect("reowned means an owner").clone();
                let mut decls = BTreeMap::new();
                decls.insert(owner, displaced);
                decls.insert(uri.to_owned(), Arc::clone(class));
                self.dupes.insert(fqn, decls);
                self.publish_winner(fqn);
                Declared {
                    won: self.uris.get(fqn).is_some_and(|owner| owner == uri),
                    reowned,
                }
            }
            None => {
                // An unparsed classmap entry for another file is only
                // displaced, not disproved.
                if let Some(previous) = self.uris.insert(fqn, uri.to_owned())
                    && previous != uri
                {
                    push_alternate(self.alternates, fqn, previous);
                }
                self.classes.insert(fqn, Arc::clone(class));
                Declared { won: true, reowned }
            }
        }
    }

    /// Drop `uri`'s declaration of `fqn`, handing the name to the
    /// next-lowest file that still declares it.
    pub(crate) fn withdraw(&mut self, fqn: &str, uri: &str) {
        if !self.dupes.is_empty()
            && let Some(decls) = self.dupes.get_mut(fqn)
        {
            decls.remove(uri);
            self.publish_winner(fqn);
            return;
        }
        if self.uris.get(fqn).is_some_and(|owner| owner == uri) {
            self.uris.remove(fqn);
            self.classes.remove(fqn);
        }
    }

    /// Record that `uri` is where `fqn` is expected to live, without
    /// having parsed it — what a classmap scan knows.
    ///
    /// A parsed declaration of the name keeps the entry: it describes a
    /// file we know declares the class, and `fqn_class_index` holds its
    /// members, so repointing the URI alone would make the two indexes
    /// disagree about which file the name refers to.
    pub(crate) fn note_discovered(&mut self, fqn: &str, uri: String) {
        if self.classes.contains_key(fqn) {
            if self.uris.get(fqn) != Some(&uri) {
                push_alternate(self.alternates, fqn, uri);
            }
            return;
        }
        if let Some(previous) = self.uris.insert(fqn, uri)
            && self.uris.get(fqn) != Some(&previous)
        {
            push_alternate(self.alternates, fqn, previous);
        }
    }

    /// Drop every declaration contributed by `uris`, promoting the
    /// next-lowest file that still declares each name.
    ///
    /// Returns the names that no file declares any more and the names that
    /// changed hands, so the caller can refresh the indexes derived from
    /// them (`gti_index`).
    pub(crate) fn withdraw_uris(&mut self, uris: &HashSet<String>) -> WithdrawnClasses {
        let affected: Vec<String> = self
            .dupes
            .iter()
            .filter(|(_, decls)| decls.keys().any(|u| uris.contains(u)))
            .map(|(fqn, _)| fqn.to_owned())
            .collect();

        let mut dropped: Vec<String> = Vec::new();
        let mut promoted: Vec<String> = Vec::new();
        for fqn in affected {
            let previous = self.uris.get(&fqn).cloned();
            if let Some(decls) = self.dupes.get_mut(&fqn) {
                decls.retain(|u, _| !uris.contains(u));
            }
            self.publish_winner(&fqn);
            match self.uris.get(&fqn) {
                Some(owner) if Some(owner) != previous.as_ref() => promoted.push(fqn),
                Some(_) => {}
                None => dropped.push(fqn),
            }
        }

        // Whatever the promotions above did not re-point still belongs to
        // a purged file.
        self.uris.retain(|fqn, owner| {
            if uris.contains(owner.as_str()) {
                dropped.push(fqn.to_owned());
                false
            } else {
                true
            }
        });
        for fqn in &dropped {
            self.classes.remove(fqn);
        }

        // A name whose last parsed declaration went falls to a file
        // discovery found declaring it too.
        if !self.alternates.is_empty() {
            self.alternates.retain(|_, alternates| {
                alternates.retain(|uri| !uris.contains(uri.as_str()));
                !alternates.is_empty()
            });
            dropped.retain(|fqn| match pop_alternate(self.alternates, fqn) {
                Some(uri) => {
                    self.uris.insert(fqn.as_str(), uri);
                    promoted.push(fqn.clone());
                    false
                }
                None => true,
            });
        }

        WithdrawnClasses { dropped, promoted }
    }

    /// Mirror the lowest-sorting declaration of `fqn` into the lookup
    /// indexes, dropping the name entirely once nothing declares it.
    fn publish_winner(&mut self, fqn: &str) {
        let Some(decls) = self.dupes.get(fqn) else {
            return;
        };
        match decls.iter().next() {
            Some((uri, class)) => {
                let (uri, class) = (uri.clone(), Arc::clone(class));
                self.uris.insert(fqn, uri);
                self.classes.insert(fqn, class);
            }
            None => {
                self.uris.remove(fqn);
                self.classes.remove(fqn);
            }
        }
        // One declarant left is not a duplicate; the lookup indexes alone
        // describe it, so stop paying for the second entry.
        if self.dupes.get(fqn).is_some_and(|d| d.len() <= 1) {
            self.dupes.remove(fqn);
        }
    }
}

/// Record `uri` as a file that also declares `fqn`.
fn push_alternate(alternates: &mut CiMap<Vec<String>>, fqn: &str, uri: String) {
    match alternates.get_mut(fqn) {
        Some(uris) => {
            if !uris.contains(&uri) {
                uris.push(uri);
            }
        }
        None => {
            alternates.insert(fqn, vec![uri]);
        }
    }
}

/// Take one of the files recorded as also declaring `fqn`.
fn pop_alternate(alternates: &mut CiMap<Vec<String>>, fqn: &str) -> Option<String> {
    let uris = alternates.get_mut(fqn)?;
    let uri = uris.pop();
    if uris.is_empty() {
        alternates.remove(fqn);
    }
    uri
}

/// What [`ClassDeclarations::declare`] did, for refreshing the indexes
/// derived from the class index (`gti_index`).
pub(crate) struct Declared {
    /// Whether the new declaration is the one now serving lookups, or a
    /// lower-sorting file's declaration of the same name still is.
    pub(crate) won: bool,
    /// Whether the name was taken from another file, whose entries in the
    /// derived indexes are now stale.
    pub(crate) reowned: bool,
}

/// What [`ClassDeclarations::withdraw_uris`] did, for refreshing the
/// indexes derived from the class index.
pub(crate) struct WithdrawnClasses {
    /// Names no file declares any more.
    pub(crate) dropped: Vec<String>,
    /// Names another file's declaration took over.
    pub(crate) promoted: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ClassCompletionOrigin::{Project, VendorExplicit, VendorTransitive};

    fn class_scan(classes: &[(&str, &str, ClassCompletionOrigin)]) -> WorkspaceScanResult {
        let mut scan = WorkspaceScanResult::default();
        for &(fqn, path, origin) in classes {
            scan.classmap.insert(fqn.to_string(), PathBuf::from(path));
            scan.class_origins.insert(fqn.to_string(), origin);
        }
        scan
    }

    /// The URI a class is indexed under and the origin recorded for it.
    fn class_entry(
        index: &SymbolIndex,
        fqn: &str,
    ) -> (Option<String>, Option<ClassCompletionOrigin>) {
        (
            index.fqn_uri_index.read().get(fqn).cloned(),
            index.fqn_origin_index.read().get(fqn).copied(),
        )
    }

    fn uri(path: &str) -> Option<String> {
        Some(crate::util::path_to_uri(Path::new(path)))
    }

    #[test]
    fn a_merge_keeps_an_indexed_class_and_the_origin_of_its_file() {
        let index = SymbolIndex::new();
        index.merge_scan(class_scan(&[("Foo", "/vendor/a/Foo.php", VendorExplicit)]));
        index.merge_scan(class_scan(&[(
            "Foo",
            "/vendor/b/Foo.php",
            VendorTransitive,
        )]));

        assert_eq!(
            class_entry(&index, "Foo"),
            (uri("/vendor/a/Foo.php"), Some(VendorExplicit))
        );
    }

    #[test]
    fn a_merge_that_finds_the_indexed_file_refreshes_its_origin() {
        let index = SymbolIndex::new();
        index.merge_scan(WorkspaceScanResult::from_classmap(HashMap::from([(
            "Foo".to_string(),
            PathBuf::from("/lib/Foo.php"),
        )])));
        index.merge_scan(class_scan(&[("Foo", "/lib/Foo.php", VendorExplicit)]));

        assert_eq!(
            class_entry(&index, "Foo"),
            (uri("/lib/Foo.php"), Some(VendorExplicit))
        );
    }

    #[test]
    fn project_code_is_recorded_by_having_no_origin() {
        let index = SymbolIndex::new();
        index.merge_scan(class_scan(&[("Foo", "/src/Foo.php", Project)]));

        assert_eq!(class_entry(&index, "Foo"), (uri("/src/Foo.php"), None));
    }

    #[test]
    fn replacing_a_tree_drops_what_the_rescan_no_longer_finds() {
        let index = SymbolIndex::new();
        index.merge_scan(class_scan(&[
            ("Moved", "/vendor/a/Moved.php", VendorExplicit),
            ("Removed", "/vendor/a/Removed.php", VendorExplicit),
            ("Local", "/src/Local.php", Project),
        ]));
        index.replace_tree(
            &[PathBuf::from("/vendor")],
            class_scan(&[("Moved", "/vendor/b/Moved.php", VendorTransitive)]),
        );

        assert_eq!(
            class_entry(&index, "Moved"),
            (uri("/vendor/b/Moved.php"), Some(VendorTransitive))
        );
        assert_eq!(class_entry(&index, "Removed"), (None, None));
        assert_eq!(class_entry(&index, "Local"), (uri("/src/Local.php"), None));
    }

    #[test]
    fn a_merge_remembers_the_declaration_it_kept_out() {
        let index = SymbolIndex::new();
        index.merge_scan(class_scan(&[("Foo", "/a/vendor/Foo.php", VendorExplicit)]));
        index.merge_scan(class_scan(&[("Foo", "/b/vendor/Foo.php", VendorExplicit)]));
        // The same file again is not a second declaration.
        index.merge_scan(class_scan(&[("Foo", "/a/vendor/Foo.php", VendorExplicit)]));

        assert_eq!(class_entry(&index, "Foo").0, uri("/a/vendor/Foo.php"));
        assert_eq!(
            index.alternate_class_uris("Foo"),
            vec![uri("/b/vendor/Foo.php").unwrap()]
        );
    }

    #[test]
    fn replacing_a_tree_hands_a_name_it_dropped_to_another_declaration() {
        let index = SymbolIndex::new();
        index.merge_scan(class_scan(&[("Foo", "/a/vendor/Foo.php", VendorExplicit)]));
        index.merge_scan(class_scan(&[("Foo", "/b/vendor/Foo.php", VendorExplicit)]));
        index.replace_tree(
            &[PathBuf::from("/a/vendor")],
            WorkspaceScanResult::default(),
        );

        assert_eq!(class_entry(&index, "Foo").0, uri("/b/vendor/Foo.php"));
        assert!(index.alternate_class_uris("Foo").is_empty());
    }

    #[test]
    fn a_rescan_that_takes_a_name_over_remembers_the_one_it_replaced() {
        let index = SymbolIndex::new();
        index.merge_scan(class_scan(&[("Foo", "/a/vendor/Foo.php", VendorExplicit)]));
        index.replace_tree(
            &[PathBuf::from("/b/vendor")],
            class_scan(&[("Foo", "/b/vendor/Foo.php", VendorExplicit)]),
        );

        assert_eq!(class_entry(&index, "Foo").0, uri("/b/vendor/Foo.php"));
        assert_eq!(
            index.alternate_class_uris("Foo"),
            vec![uri("/a/vendor/Foo.php").unwrap()]
        );
    }

    #[test]
    fn withdrawing_a_file_hands_its_names_to_another_declaration() {
        let index = SymbolIndex::new();
        index.merge_scan(class_scan(&[("Foo", "/a/vendor/Foo.php", VendorExplicit)]));
        index.merge_scan(class_scan(&[("Foo", "/b/vendor/Foo.php", VendorExplicit)]));
        let withdrawn = index.with_class_declarations(|decls| {
            decls.withdraw_uris(&HashSet::from([uri("/a/vendor/Foo.php").unwrap()]))
        });

        assert_eq!(withdrawn.promoted, vec!["Foo".to_string()]);
        assert!(withdrawn.dropped.is_empty());
        assert_eq!(class_entry(&index, "Foo").0, uri("/b/vendor/Foo.php"));
    }

    #[test]
    fn constants_merge_by_their_exact_name() {
        let constants = |entries: &[(&str, &str)]| WorkspaceScanResult {
            constant_index: entries
                .iter()
                .map(|&(name, path)| (name.to_string(), PathBuf::from(path)))
                .collect(),
            ..Default::default()
        };
        let index = SymbolIndex::new();
        index.merge_scan(constants(&[("FOO", "/a.php")]));
        index.merge_scan(constants(&[("FOO", "/b.php"), ("foo", "/b.php")]));

        let indexed = index.autoload_constant_index.read();
        assert_eq!(indexed.get("FOO"), Some(&PathBuf::from("/a.php")));
        assert_eq!(indexed.get("foo"), Some(&PathBuf::from("/b.php")));
    }
}
