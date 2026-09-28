//! The workspace symbol indexes, grouped out of `Backend`.
//!
//! These are the class, function, and constant lookup tables that answer
//! "where is symbol X defined?" across the whole workspace. All fields are
//! `Arc`-wrapped, so `#[derive(Clone)]` shares them with a cloned `Backend`
//! (the same semantics the per-request clone had as individual fields).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;

use crate::ci_map::{CiMap, CiSet};
use crate::types::{ClassInfo, DefineInfo, FunctionInfo, MethodStore};
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
    /// Completion provenance for autoloaded function symbols.
    pub(crate) autoload_function_origin_index: Arc<RwLock<CiMap<ClassCompletionOrigin>>>,
    /// Autoload constant index: constant name → file path on disk.
    pub(crate) autoload_constant_index: Arc<RwLock<HashMap<String, PathBuf>>>,
    /// Completion provenance for autoloaded constant symbols.
    pub(crate) autoload_constant_origin_index: Arc<RwLock<HashMap<String, ClassCompletionOrigin>>>,
    /// Paths of all files discovered through Composer's `autoload_files.php`.
    pub(crate) autoload_file_paths: Arc<RwLock<Vec<PathBuf>>>,
    /// Index of fully-qualified class names to file URIs.
    pub(crate) fqn_uri_index: Arc<RwLock<CiMap<String>>>,
    /// Completion provenance for fully-qualified class names.
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
    /// Negative-result cache for `find_or_load_class`.
    pub(crate) class_not_found_cache: Arc<RwLock<CiSet>>,
    /// Global method store: `(class_fqn, method_name)` → `Arc<MethodInfo>`.
    pub(crate) method_store: MethodStore,
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
}

impl SymbolIndex {
    pub(crate) fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            class_lookup_generation: Arc::new(AtomicU64::new(0)),
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
            class_not_found_cache: Arc::new(RwLock::new(CiSet::new())),
            method_store: Arc::new(RwLock::new(HashMap::new())),
            gti_index: Arc::new(RwLock::new(HashMap::new())),
            gti_parents_index: Arc::new(RwLock::new(HashMap::new())),
        }
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

    /// Run `f` with the three class indexes locked together.
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
        f(&mut ClassDeclarations {
            uris: &mut uris,
            classes: &mut classes,
            dupes: &mut dupes,
        })
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
                self.uris.insert(fqn, uri.to_owned());
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
            return;
        }
        self.uris.insert(fqn, uri);
    }

    /// Drop every declaration contributed by `uris`, promoting the
    /// next-lowest file that still declares each name.
    ///
    /// Returns the names that no file declares any more and the names that
    /// changed hands, so the caller can refresh the indexes derived from
    /// them (`method_store`, `gti_index`).
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

/// What [`ClassDeclarations::declare`] did, for refreshing the indexes
/// derived from the class index (`method_store`, `gti_index`).
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
