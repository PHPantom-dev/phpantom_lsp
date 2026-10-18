//! Resolved-class cache.
//!
//! The data structure behind [`ResolvedClassCache`]: the
//! `(FQN, generic args)`-keyed map of fully-resolved classes, the
//! auxiliary indices that keep eviction proportional to the dependent
//! set, the thread-local active-cache guard, and the transitive
//! eviction walk (`evict_fqn`).  The resolution logic that populates
//! the cache lives in [`super::resolve`].

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Weak};

use parking_lot::RwLock;

use crate::atom::Atom;
use crate::types::{ClassInfo, MethodInfo, PropertyInfo};
use crate::virtual_members::laravel::database_schema::SchemaIndex;
use crate::virtual_members::member_pool::MemberPool;

/// Cache key for [`ResolvedClassCache`]: fully-qualified class name
/// paired with the concrete generic type arguments used at this
/// instantiation site.
///
/// For non-generic classes the argument list is empty, keeping the
/// common case cheap.  For generic classes like
/// `Illuminate\Database\Eloquent\Builder<App\Models\User>`, the key
/// would be `("Illuminate\\Database\\Eloquent\\Builder", vec!["App\\Models\\User"])`.
///
/// Generic args are the display strings of the concrete type arguments
/// in positional order; two lookups share an entry only when they spell
/// the arguments identically.
pub type ResolvedClassCacheKey = (Atom, Vec<String>);

/// The inner store behind a [`ResolvedClassCache`].
///
/// Holds the resolved-class map plus two auxiliary indices that make
/// eviction proportional to the number of dependent classes rather than
/// the size of the whole cache:
///
/// * `fqn_keys` maps a class FQN to every generic-arg variant stored
///   under it, so all variants of a class can be removed without scanning.
/// * `reverse_deps` maps a dependency string (a parent / trait / interface
///   / mixin / cast value, exactly as stored on the dependent class) to the
///   set of cache keys that reference it, so the transitive eviction
///   cascade is a graph walk rather than repeated full-cache scans.
///
/// All three structures are kept consistent by going through [`Self::insert`],
/// [`Self::clear`], and [`evict_fqn`]; callers never mutate the map directly.
pub struct ResolvedCacheInner {
    /// Resolved classes keyed by FQN + concrete generic args.
    map: HashMap<ResolvedClassCacheKey, Arc<ClassInfo>>,
    /// FQN → all cache keys (generic-arg variants) stored under it.
    fqn_keys: HashMap<String, HashSet<ResolvedClassCacheKey>>,
    /// Dependency string → cache keys whose class directly depends on it.
    ///
    /// The string is stored verbatim from the dependent class (it may be a
    /// fully-qualified name or a short name), mirroring the dual FQN /
    /// short-name matching that eviction performs.
    reverse_deps: HashMap<String, HashSet<ResolvedClassCacheKey>>,
    /// Whether the current project uses Laravel (or a standalone Illuminate
    /// component).  When `false`, `resolve_class_fully_inner` skips the
    /// Laravel virtual member providers, the contract to concrete-class
    /// mixin bindings, and the post-resolution Laravel patches, so a
    /// non-Laravel project pays none of that scanning cost.
    ///
    /// Defaults to `true`: Laravel analysis stays enabled unless the server
    /// has parsed `composer.json` and confirmed no Laravel/Illuminate
    /// dependency.  This keeps behaviour unchanged for the CLI test
    /// backends and any code path that resolves without a project context.
    /// Not touched by [`Self::clear`] — it reflects the project, not the
    /// per-edit cache contents.
    is_laravel: bool,
    schema_index: SchemaIndex,
    /// Laravel's alias tables, shared by handle with the [`Backend`] that
    /// builds them (see
    /// [`LaravelAliasSlot`](crate::virtual_members::laravel::LaravelAliasSlot)).
    ///
    /// A virtual member provider receives this cache but never the
    /// `Backend`, and a facade whose `getFacadeAccessor()` names a container
    /// binding (`return 'sentry';`) needs the container table to reach the
    /// class it forwards to.  Sharing the slot rather than a snapshot means
    /// the provider always reads the table the `Backend` most recently
    /// built, including after a rebuild.  A cache created without a
    /// `Backend` (tests, the throwaway cache a cache-less resolution falls
    /// back to) holds an empty slot and simply forwards nothing.
    ///
    /// [`Backend`]: crate::Backend
    laravel_aliases: crate::virtual_members::laravel::LaravelAliasSlot,
    /// Classes currently being resolved by `resolve_class_fully_inner`,
    /// keyed per thread so one thread's in-progress resolution never
    /// degrades another thread's independent resolution of the same
    /// class.  When resolution of a class re-enters itself on the same
    /// thread (a genuine dependency cycle, e.g. two Eloquent models
    /// whose relationship providers resolve each other), the re-entrant
    /// call detects its own marker here and returns a base-inheritance
    /// partial result instead of recursing forever.  Entries live only
    /// for the duration of one synchronous resolution call tree; they
    /// are removed by an RAII guard, so a panic cannot leak them.
    in_flight: HashSet<(std::thread::ThreadId, Atom)>,
    /// Cross-thread claim on the first resolution of a not-yet-cached
    /// `(FQN, generic args)`, keyed the same as [`map`](Self::map).
    ///
    /// Unlike `in_flight` (which is deliberately keyed per thread so a
    /// genuine same-thread dependency cycle can break out with a
    /// partial result), this tracks which single thread is allowed to
    /// perform the *first* full resolution of a given key. Two threads
    /// that both reach a never-before-cached class at the same time
    /// (common for a class reached only via a parameter type hint,
    /// never eagerly toposorted) would otherwise both run the full
    /// merge independently and race to insert. Racing is not just
    /// wasted work: each thread's `in_flight` set reflects only *its
    /// own* concurrent call stack, so a thread that happens to already
    /// have an unrelated class marked in-flight from other nested work
    /// can take a different cycle-break path than a thread that
    /// doesn't, silently dropping virtual/interface members the other
    /// thread's result would have had. Making the first resolution
    /// single-flighted removes that divergence: every other consumer
    /// of the same not-yet-cached key waits for the one true
    /// computation instead of racing it. See
    /// [`claim_pending`](Self::claim_pending).
    pending: HashMap<ResolvedClassCacheKey, std::thread::ThreadId>,
    /// Interned transformed methods, keyed by the identity of the
    /// original `Arc<MethodInfo>` plus a fingerprint of the transform
    /// (template substitution map, bare-`self` replacement target,
    /// flag rewrites — see [`TransformFingerprint`]).
    ///
    /// The inheritance and virtual-member merges produce large numbers
    /// of *value-identical* transformed copies: every relation variant
    /// of the same model substitutes the same Builder methods with the
    /// same concrete type, every subclass of a generic parent without
    /// explicit `@extends` generics substitutes the same template
    /// bounds, and so on.  Interning collapses each distinct
    /// `(origin, transform)` pair to a single allocation shared by all
    /// classes that embed it.
    ///
    /// Entries carry a [`Weak`] witness of the origin.  When the origin
    /// dies (its file was re-parsed), a later lookup whose pointer
    /// happens to collide with the freed address fails the
    /// `Weak::upgrade` + `ptr_eq` validation and the entry is simply
    /// replaced, so stale entries can never leak a wrong method.
    substituted_methods: SubstitutedMembers<MethodInfo>,
    /// Interned transformed properties, keyed the same way as
    /// [`substituted_methods`](Self::substituted_methods) (origin
    /// `Arc<PropertyInfo>` identity plus a [`TransformFingerprint`]).
    ///
    /// Properties are transformed only by template substitution during
    /// the inheritance merge, so the fingerprint carries just the
    /// substitution map.  As with methods, every subclass of the same
    /// generic parent substitutes the same template bounds and so
    /// produces value-identical transformed properties, which interning
    /// collapses to one allocation shared across all of them.
    substituted_properties: SubstitutedMembers<PropertyInfo>,
    /// Loaded classes as linked by `Backend::link_loaded_class` (their
    /// `@phpstan-import-type` aliases linked into their members, and their
    /// docblock types checked against the native hints), keyed by the
    /// identity of the class as loaded.
    ///
    /// The [`Weak`] witness retires an entry once its class is re-parsed.
    /// An edit to a class the linking read does not re-parse the linked one,
    /// so each entry also lists every class the linking loaded, and
    /// [`evict_fqn`] drops the entries that list the evicted class.
    linked_classes: HashMap<usize, LinkedClassEntry>,
    /// Length of [`linked_classes`](Self::linked_classes) after its last
    /// dead-witness sweep.  See [`sweep_dead_witnesses`].
    linked_classes_swept_len: usize,
    /// Members of the cached classes, pooled by content so classes that
    /// inherit an identical member share one allocation of it.  See
    /// [`MemberPool`].
    ///
    /// Behind its own lock so classes can be pooled under a read guard
    /// of the cache, without blocking lookups.
    member_pool: parking_lot::Mutex<MemberPool>,
}

/// One class as linked.  See [`ResolvedCacheInner::linked_classes`].
struct LinkedClassEntry {
    witness: Weak<ClassInfo>,
    /// The FQN of the class.
    fqn: Atom,
    /// The classes the linking read, as it asked for them.
    sources: Vec<String>,
    /// The class lookup generation the linking ran at, when one of
    /// `sources` could not be loaded: indexing it later changes the
    /// generation, so the entry is only good while that stays the same.
    /// `None` when every one loaded, which only an edit to one of them can
    /// change.
    generation: Option<u64>,
    /// The linked class, or `None` when linking left the class as loaded.
    value: Option<Arc<ClassInfo>>,
}

/// Outcome of [`ResolvedCacheInner::claim_pending`].
pub(crate) enum PendingClaim {
    /// No other thread was resolving this key; the caller must now
    /// perform the full resolution and release the claim via
    /// [`ResolvedCacheInner::release_pending`] when done.
    Fresh,
    /// The calling thread already holds the claim (a same-thread
    /// re-entrant call). Proceed as if uncontended; do not release
    /// this claim independently of the outer call that took it.
    AlreadyOwnedByThisThread,
    /// A different thread holds the claim. The caller should wait for
    /// it to finish and read the cache instead of resolving again.
    OwnedByOtherThread,
}

/// Interned transformed copies of one kind of member, keyed by the
/// identity of the original `Arc` plus a [`TransformFingerprint`].  See
/// [`ResolvedCacheInner::substituted_methods`].
struct SubstitutedMembers<T> {
    entries: HashMap<(usize, u128), SubstitutedEntry<T>>,
    /// Length of `entries` after its last dead-witness sweep.  See
    /// [`sweep_dead_witnesses`].
    swept_len: usize,
}

/// One interned transformed member: the origin witness plus the shared
/// transformed value.
struct SubstitutedEntry<T> {
    witness: Weak<T>,
    value: Arc<T>,
}

impl<T> Default for SubstitutedMembers<T> {
    fn default() -> Self {
        SubstitutedMembers {
            entries: HashMap::new(),
            swept_len: 0,
        }
    }
}

impl<T> SubstitutedMembers<T> {
    /// The member interned for `origin` under the transform identified by
    /// `fp`.
    ///
    /// `None` when no entry exists or when the stored entry's witness no
    /// longer matches `origin` (the origin died and its address was
    /// reused): the caller then builds and [inserts](Self::insert) a fresh
    /// value.
    fn get(&self, origin: &Arc<T>, fp: u128) -> Option<Arc<T>> {
        let entry = self.entries.get(&(Arc::as_ptr(origin) as usize, fp))?;
        let alive = entry.witness.upgrade()?;
        Arc::ptr_eq(&alive, origin).then(|| Arc::clone(&entry.value))
    }

    /// Intern a transformed member built from `origin` under `fp`.
    fn insert(&mut self, origin: &Arc<T>, fp: u128, value: Arc<T>) {
        sweep_dead_witnesses(&mut self.entries, &mut self.swept_len, |e| {
            e.witness.strong_count() > 0
        });
        self.entries.insert(
            (Arc::as_ptr(origin) as usize, fp),
            SubstitutedEntry {
                witness: Arc::downgrade(origin),
                value,
            },
        );
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.swept_len = 0;
    }

    /// The approximate bytes of the table's buckets.
    #[cfg(feature = "mem-audit")]
    fn bucket_bytes(&self) -> usize {
        let cap = self.entries.capacity();
        if cap == 0 {
            return 0;
        }
        let slot = std::mem::size_of::<((usize, u128), SubstitutedEntry<T>)>();
        ((cap * 8).div_ceil(7).max(4)).next_power_of_two() * (slot + 1)
    }
}

/// Ceiling on the live entries of each map [`sweep_dead_witnesses`] keeps
/// (interned methods and properties, linked classes).  A sweep that leaves
/// this many clears the map (the merge simply stops sharing until it
/// refills), so a map never grows past twice the cap.  It exists only as a
/// backstop, far above what a full workspace resolution produces.
const SUBSTITUTED_METHODS_CAP: usize = 1 << 20;

/// Smallest interned-member map that is swept for dead witnesses, so a
/// small map is not rescanned on every few inserts.
const SUBSTITUTED_SWEEP_FLOOR: usize = 1 << 12;

/// Drop the entries of a map keyed by the identity of an origin (a member,
/// or a class as loaded) whose origin has died, once the map has doubled
/// since its last sweep.
///
/// An edit that re-parses a file kills the origins of its members and
/// classes, but their entries stay in the map: the transformed value is
/// held strongly, and the `Weak` witness keeps the dead origin's
/// allocation.  Sweeping at each doubling keeps the cost amortised O(1)
/// per insert.
fn sweep_dead_witnesses<K, V>(
    map: &mut HashMap<K, V>,
    swept_len: &mut usize,
    alive: impl Fn(&V) -> bool,
) {
    if map.len() < (*swept_len * 2).max(SUBSTITUTED_SWEEP_FLOOR) {
        return;
    }
    map.retain(|_, entry| alive(entry));
    if map.len() >= SUBSTITUTED_METHODS_CAP {
        map.clear();
    }
    *swept_len = map.len();
}

impl Default for ResolvedCacheInner {
    fn default() -> Self {
        ResolvedCacheInner {
            map: HashMap::new(),
            fqn_keys: HashMap::new(),
            reverse_deps: HashMap::new(),
            is_laravel: true,
            schema_index: SchemaIndex::default(),
            laravel_aliases: crate::virtual_members::laravel::new_alias_slot(),
            in_flight: HashSet::new(),
            pending: HashMap::new(),
            substituted_methods: SubstitutedMembers::default(),
            substituted_properties: SubstitutedMembers::default(),
            linked_classes: HashMap::new(),
            linked_classes_swept_len: 0,
            member_pool: parking_lot::Mutex::default(),
        }
    }
}

impl ResolvedCacheInner {
    /// Look up a resolved class by its `(FQN, generic args)` key.
    pub fn get(&self, key: &ResolvedClassCacheKey) -> Option<&Arc<ClassInfo>> {
        self.map.get(key)
    }

    /// Memory-audit tooling: expose the internal maps for deep-size
    /// accounting, plus the approximate bucket bytes of the two
    /// substituted-member intern tables.
    #[allow(clippy::type_complexity)]
    #[cfg(feature = "mem-audit")]
    pub(crate) fn audit_maps(
        &self,
    ) -> (
        &HashMap<ResolvedClassCacheKey, Arc<ClassInfo>>,
        &HashMap<String, HashSet<ResolvedClassCacheKey>>,
        &HashMap<String, HashSet<ResolvedClassCacheKey>>,
        usize,
    ) {
        let substituted =
            self.substituted_methods.bucket_bytes() + self.substituted_properties.bucket_bytes();
        (&self.map, &self.fqn_keys, &self.reverse_deps, substituted)
    }

    /// Whether a key is present in the cache.
    pub fn contains_key(&self, key: &ResolvedClassCacheKey) -> bool {
        self.map.contains_key(key)
    }

    /// Whether the project uses Laravel / Illuminate.  See the
    /// [`is_laravel`](ResolvedCacheInner::is_laravel) field.
    pub fn is_laravel(&self) -> bool {
        self.is_laravel
    }

    /// Record whether the project uses Laravel / Illuminate.
    ///
    /// Set once by the server after parsing `composer.json`.  Left
    /// untouched by [`Self::clear`] so cache invalidation on file edits
    /// does not lose the project classification.
    pub fn set_laravel(&mut self, is_laravel: bool) {
        self.is_laravel = is_laravel;
    }

    /// Mark `fqn` as being resolved on the current thread.  Returns
    /// `true` when freshly marked (caller should proceed with full
    /// resolution) and `false` on re-entry (caller should break the
    /// cycle with a partial result).
    pub fn mark_in_flight(&mut self, fqn: Atom) -> bool {
        self.in_flight.insert((std::thread::current().id(), fqn))
    }

    /// Remove the current thread's in-flight marker for `fqn`.
    pub fn unmark_in_flight(&mut self, fqn: Atom) {
        self.in_flight.remove(&(std::thread::current().id(), fqn));
    }

    /// Attempt to become the sole resolver of `key`'s first, not-yet-cached
    /// resolution. See [`pending`](Self::pending) for why this exists
    /// alongside `in_flight`.
    pub(crate) fn claim_pending(&mut self, key: &ResolvedClassCacheKey) -> PendingClaim {
        let current = std::thread::current().id();
        match self.pending.get(key) {
            Some(&owner) if owner == current => PendingClaim::AlreadyOwnedByThisThread,
            Some(_) => PendingClaim::OwnedByOtherThread,
            None => {
                self.pending.insert(key.clone(), current);
                PendingClaim::Fresh
            }
        }
    }

    /// Release a [`PendingClaim::Fresh`] claim on `key`, letting any
    /// thread waiting in `claim_pending` proceed.
    pub(crate) fn release_pending(&mut self, key: &ResolvedClassCacheKey) {
        self.pending.remove(key);
    }

    /// Whether another thread currently holds a fresh claim on `key`.
    pub(crate) fn is_pending(&self, key: &ResolvedClassCacheKey) -> bool {
        self.pending.contains_key(key)
    }

    pub fn schema_index(&self) -> &SchemaIndex {
        &self.schema_index
    }

    /// The concrete class FQN a Laravel container binding key is bound to,
    /// or `None` when the alias tables are not built or the key is bound
    /// only at runtime.
    ///
    /// The slot lock is only ever held for the length of a read or a
    /// single assignment (never across the build itself), so taking it
    /// under the cache lock cannot deadlock.
    pub(crate) fn container_alias_concrete_fqn(&self, key: &str) -> Option<String> {
        self.laravel_aliases.peek()?.container.get(key).cloned()
    }

    /// Share the `Backend`'s alias slot into this cache.  Called once at
    /// construction; the slot's contents are then always the tables the
    /// `Backend` most recently built.
    pub(crate) fn set_laravel_aliases(
        &mut self,
        slot: crate::virtual_members::laravel::LaravelAliasSlot,
    ) {
        self.laravel_aliases = slot;
    }

    pub fn set_schema_index(&mut self, schema_index: SchemaIndex) {
        self.schema_index = schema_index;
    }

    /// Number of cached entries (used by eviction tests).
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether the cache is empty (used by eviction tests).
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Insert a resolved class, updating the FQN and reverse-dependency
    /// indices so eviction stays cheap.
    pub fn insert(&mut self, key: ResolvedClassCacheKey, value: Arc<ClassInfo>) {
        // When replacing an existing entry, drop its old reverse-dep edges
        // first so a changed dependency set does not leave stale edges.
        if let Some(old_deps) = self.map.get(&key).map(|c| dependency_keys(&key, c)) {
            for dep in old_deps {
                self.unlink_reverse_edge(&dep, &key);
            }
        }

        let fqn = key.0.to_string();
        for dep in dependency_keys(&key, &value) {
            self.reverse_deps
                .entry(dep)
                .or_default()
                .insert(key.clone());
        }
        self.fqn_keys.entry(fqn).or_default().insert(key.clone());
        self.map.insert(key, value);
    }

    /// Remove all entries and indices.
    ///
    /// `in_flight` and `pending` are left untouched: their entries
    /// belong to resolution call trees currently running on other
    /// threads, not to the cached contents being invalidated.
    pub fn clear(&mut self) {
        self.map.clear();
        self.fqn_keys.clear();
        self.reverse_deps.clear();
        self.substituted_methods.clear();
        self.substituted_properties.clear();
        self.linked_classes.clear();
        self.linked_classes_swept_len = 0;
        self.member_pool.get_mut().clear();
    }

    /// Share the members of a resolved class about to be cached with the
    /// identical members of classes cached before it.  See
    /// [`member_pool`](Self::member_pool).
    pub(crate) fn share_members(&self, class: &mut ClassInfo) {
        self.member_pool.lock().share_class(class);
    }

    /// `class` as linked, when that was stored for this very class and is
    /// still good at the class lookup `generation`, with the classes the
    /// linking read.
    pub(crate) fn get_linked_class(
        &self,
        class: &Arc<ClassInfo>,
        generation: u64,
    ) -> Option<(Arc<ClassInfo>, &[String])> {
        let entry = self.linked_classes.get(&(Arc::as_ptr(class) as usize))?;
        let alive = entry.witness.upgrade()?;
        if !Arc::ptr_eq(&alive, class) || entry.generation.is_some_and(|g| g != generation) {
            return None;
        }
        let linked = entry.value.as_ref().unwrap_or(class);
        Some((Arc::clone(linked), entry.sources.as_slice()))
    }

    /// Store `class` as linked (`None` when linking left it as loaded),
    /// read from `sources`.  `generation` is the class lookup generation
    /// the linking ran at when it could not load one of them.
    pub(crate) fn insert_linked_class(
        &mut self,
        class: &Arc<ClassInfo>,
        value: Option<Arc<ClassInfo>>,
        sources: Vec<String>,
        generation: Option<u64>,
    ) {
        sweep_dead_witnesses(
            &mut self.linked_classes,
            &mut self.linked_classes_swept_len,
            |e| e.witness.strong_count() > 0,
        );
        self.linked_classes.insert(
            Arc::as_ptr(class) as usize,
            LinkedClassEntry {
                witness: Arc::downgrade(class),
                fqn: class.fqn(),
                sources,
                generation,
                value,
            },
        );
    }

    /// Drop every linked-class entry that read `fqn`, returning the FQNs
    /// of the classes they belonged to.
    fn evict_linked_classes_reading(&mut self, fqn: &str) -> Vec<String> {
        if self.linked_classes.is_empty() {
            return Vec::new();
        }
        let short = crate::util::short_name(fqn);
        let mut owners = Vec::new();
        self.linked_classes.retain(|_, entry| {
            let reads = entry
                .sources
                .iter()
                .any(|s| s.eq_ignore_ascii_case(fqn) || s.eq_ignore_ascii_case(short));
            if reads {
                owners.push(entry.fqn.to_string());
            }
            !reads
        });
        owners
    }

    /// Remove `key` from the reverse-dependency edge under `dep`, pruning
    /// the entry entirely when it becomes empty.
    fn unlink_reverse_edge(&mut self, dep: &str, key: &ResolvedClassCacheKey) {
        if let Some(set) = self.reverse_deps.get_mut(dep) {
            set.remove(key);
            if set.is_empty() {
                self.reverse_deps.remove(dep);
            }
        }
    }

    /// Remove every generic-arg variant of `fqn`, cleaning up both indices.
    ///
    /// Returns `true` when at least one entry was present and removed.
    fn remove_all_variants(&mut self, fqn: &str) -> bool {
        let keys = match self.fqn_keys.remove(fqn) {
            Some(set) => set,
            None => return false,
        };
        for key in keys {
            if let Some(value) = self.map.remove(&key) {
                for dep in dependency_keys(&key, &value) {
                    self.unlink_reverse_edge(&dep, &key);
                }
            }
        }
        true
    }
}

/// Thread-safe cache of fully-resolved classes, keyed by FQN + generic args.
///
/// Stored on [`Backend`](crate::Backend) and selectively invalidated
/// when a file is re-parsed (`update_ast` / `parse_and_cache_content`).
/// Within a single request cycle (completion, hover, etc.) the cache
/// eliminates redundant calls to
/// [`resolve_class_fully`](super::resolve_class_fully) for the same
/// class at the same generic instantiation.
///
/// Uses an [`RwLock`] rather than a mutex because the cache is read-mostly
/// during resolution (lookups vastly outnumber inserts), so concurrent
/// requests under a sustained keystroke barrage can read in parallel
/// instead of serializing on a single lock.
pub type ResolvedClassCache = Arc<RwLock<ResolvedCacheInner>>;

/// Create a new, empty [`ResolvedClassCache`].
pub fn new_resolved_class_cache() -> ResolvedClassCache {
    Arc::new(RwLock::new(ResolvedCacheInner::default()))
}

// ─── Thread-local resolved-class cache access ───────────────────────────────
//
// Many code paths (e.g. `type_hint_to_classes_typed`) call `resolve_class_fully`
// without access to the `Backend`'s `resolved_class_cache`.  Rather than
// threading the cache through dozens of function signatures, we use the
// same thread-local guard pattern as the parse cache: the caller activates
// the cache at a high level (e.g. the diagnostic loop), and inner functions
// pick it up via `active_resolved_class_cache()`.

thread_local! {
    /// Raw pointer to the currently-active [`ResolvedClassCache`], or null.
    ///
    /// Set by [`with_active_resolved_class_cache`], read by
    /// [`active_resolved_class_cache`].  The pointer is valid for the
    /// lifetime of the [`ResolvedCacheGuard`] that set it.
    static ACTIVE_RESOLVED_CACHE: Cell<*const ResolvedClassCache> = const { Cell::new(std::ptr::null()) };
}

/// RAII guard that clears the thread-local cache pointer on drop.
pub struct ResolvedCacheGuard {
    prev: *const ResolvedClassCache,
}

impl Drop for ResolvedCacheGuard {
    fn drop(&mut self) {
        ACTIVE_RESOLVED_CACHE.with(|c| c.set(self.prev));
    }
}

/// Activate a [`ResolvedClassCache`] for the current thread.
///
/// While the returned guard is alive, [`active_resolved_class_cache`]
/// returns `Some(&cache)`.  Supports nesting (restores the previous
/// cache on drop).
///
/// # Example
///
/// ```ignore
/// let _guard = with_active_resolved_class_cache(&backend.resolved_class_cache);
/// // … deep call stacks can now use active_resolved_class_cache()
/// ```
pub fn with_active_resolved_class_cache(cache: &ResolvedClassCache) -> ResolvedCacheGuard {
    let prev = ACTIVE_RESOLVED_CACHE.with(|c| c.get());
    ACTIVE_RESOLVED_CACHE.with(|c| c.set(cache as *const _));
    ResolvedCacheGuard { prev }
}

/// Return the currently-active [`ResolvedClassCache`], if any.
///
/// Returns `None` when no [`with_active_resolved_class_cache`] guard
/// is alive on this thread.
///
/// # Safety
///
/// The returned reference borrows the cache through a raw pointer set
/// by [`with_active_resolved_class_cache`].  This is safe because the
/// pointer is only non-null while the [`ResolvedCacheGuard`] (which
/// holds a borrow of the original `&ResolvedClassCache`) is alive.
pub fn active_resolved_class_cache() -> Option<&'static ResolvedClassCache> {
    let ptr = ACTIVE_RESOLVED_CACHE.with(|c| c.get());
    if ptr.is_null() {
        None
    } else {
        // SAFETY: ptr was set from a valid &ResolvedClassCache reference
        // and the ResolvedCacheGuard that owns the borrow is still alive
        // (it clears the pointer on drop).
        Some(unsafe { &*ptr })
    }
}

// ─── Transformed-method interning ───────────────────────────────────────────

/// Identity of a method transform, used as the interning key alongside
/// the origin `Arc<MethodInfo>`'s address.
///
/// Two merge sites that build a transformed copy of the same origin end
/// up with the same fingerprint exactly when the transform they apply is
/// identical: the same template substitution map, the same bare-`self`
/// replacement target (or none), and the same flag rewrites.  The
/// fingerprint is a 128-bit hash, making accidental collisions (which
/// would silently share a wrong method) astronomically unlikely.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct TransformFingerprint(pub(crate) u128);

impl TransformFingerprint {
    /// Fingerprint a template substitution map plus transform modifiers.
    ///
    /// * `subs` — the substitution map applied via
    ///   `apply_substitution_to_method`, or `None` when the transform
    ///   does not substitute.
    /// * `bare_self_target` — the class name substituted for a bare
    ///   `self` return type, when that rewrite applies.
    /// * `flags` — call-site-specific flag rewrites (e.g. "mark
    ///   virtual", "drop static"), so different transforms of the same
    ///   origin under the same substitution never collide.
    pub(crate) fn new(
        subs: Option<&HashMap<String, crate::php_type::PhpType>>,
        bare_self_target: Option<&str>,
        flags: u8,
    ) -> Self {
        use std::hash::{Hash, Hasher};

        // Hash the sorted substitution entries so map iteration order
        // cannot affect the fingerprint.  Two independent unseeded
        // hashers (distinguished by an initial marker byte) yield a
        // 128-bit result.
        let mut entries: Vec<(&str, String)> = subs
            .map(|s| s.iter().map(|(k, v)| (k.as_str(), v.to_string())).collect())
            .unwrap_or_default();
        entries.sort_unstable_by(|a, b| a.0.cmp(b.0));

        let mut halves = [0u64; 2];
        for (seed, half) in halves.iter_mut().enumerate() {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (seed as u8).hash(&mut h);
            for (k, v) in &entries {
                k.hash(&mut h);
                v.hash(&mut h);
            }
            bare_self_target.hash(&mut h);
            flags.hash(&mut h);
            *half = h.finish();
        }
        TransformFingerprint(((halves[0] as u128) << 64) | halves[1] as u128)
    }
}

/// Flag bytes distinguishing the transform shapes used with
/// [`TransformFingerprint`], so different transforms of the same origin
/// under the same substitution never collide.  Values must stay
/// globally unique across all interning call sites.
///
/// The plain-substitution transforms (inheritance / trait / interface /
/// generic-argument merges) use `0` — they are distinguished from each
/// other purely by their substitution map, and from the flagged
/// transforms below by these non-zero markers.
pub(crate) mod transform_flags {
    /// Builder instance method forwarded onto a model as `static`.
    pub const FORWARD_AS_STATIC: u8 = 1;
    /// Model `@method` virtual forwarded onto a Builder as an instance
    /// method.
    pub const FORWARD_AS_INSTANCE: u8 = 2;
    /// Mixin member marked virtual on the consuming class.
    pub const MIXIN_VIRTUAL: u8 = 4;
    /// Mixin member marked virtual with the decorated-forward return
    /// rewrite applied.
    pub const MIXIN_VIRTUAL_DECORATED: u8 = 4 | 8;
    /// Model scope method rewritten to its public Builder-instance form.
    pub const SCOPE_INSTANCE: u8 = 16;
    /// Builder-typed return rewritten to the `ForwardsCalls` decorator
    /// that the method was injected onto (an Eloquent relation).
    pub const FORWARDS_CALLS_SELF: u8 = 32;
    /// Model query factory (`newQuery()` and friends) whose return type
    /// is replaced with the model's own builder.
    pub const QUERY_FACTORY: u8 = 64;
    /// [`FORWARDS_CALLS_SELF`] for a method declared on the builder, whose
    /// self-referencing returns are rewritten to the decorator as well.
    pub const FORWARDS_CALLS_BUILDER_SELF: u8 = 128;
}

/// Return a transformed copy of `origin`, shared through the active
/// resolved-class cache's interning store when possible.
///
/// When a cache is active on this thread and already holds a value for
/// `(origin, fp)`, that shared `Arc` is returned and `build` is never
/// called.  Otherwise `build` produces the transformed `MethodInfo`,
/// which is interned (when a cache is active) so every later merge that
/// applies the same transform to the same origin shares the allocation.
///
/// Callers must ensure `fp` fully identifies the transform `build`
/// applies (see [`TransformFingerprint::new`]).
pub(crate) fn intern_transformed_method(
    origin: &Arc<MethodInfo>,
    fp: TransformFingerprint,
    build: impl FnOnce() -> MethodInfo,
) -> Arc<MethodInfo> {
    let Some(cache) = active_resolved_class_cache() else {
        return Arc::new(build());
    };
    if let Some(hit) = cache.read().substituted_methods.get(origin, fp.0) {
        return hit;
    }
    let built = build();
    let value = cache.read().member_pool.lock().share_method(built);
    cache
        .write()
        .substituted_methods
        .insert(origin, fp.0, Arc::clone(&value));
    value
}

/// Return a transformed copy of `origin`, shared through the active
/// resolved-class cache's property interning store when possible.
///
/// The property analogue of [`intern_transformed_method`]: when a cache
/// is active on this thread and already holds a value for `(origin, fp)`,
/// that shared `Arc` is returned and `build` is never called.  Otherwise
/// `build` produces the transformed `PropertyInfo`, which is interned
/// (when a cache is active) so every later merge that applies the same
/// transform to the same origin shares the allocation.
pub(crate) fn intern_transformed_property(
    origin: &Arc<PropertyInfo>,
    fp: TransformFingerprint,
    build: impl FnOnce() -> PropertyInfo,
) -> Arc<PropertyInfo> {
    let Some(cache) = active_resolved_class_cache() else {
        return Arc::new(build());
    };
    if let Some(hit) = cache.read().substituted_properties.get(origin, fp.0) {
        return hit;
    }
    let built = build();
    let value = cache.read().member_pool.lock().share_property(built);
    cache
        .write()
        .substituted_properties
        .insert(origin, fp.0, Arc::clone(&value));
    value
}

/// Evict all cache entries whose FQN matches the given name, then
/// transitively evict any cached class that depends on the evicted
/// FQN through `parent_class`, `used_traits`, `interfaces`, `mixins`,
/// or Eloquent cast classes.
///
/// Because the cache is keyed by `(FQN, generic_args)`, a single FQN
/// may have multiple entries (one per distinct generic instantiation).
/// This helper removes all of them, which is used during targeted
/// cache invalidation when a class definition changes.
///
/// Transitive eviction is necessary because a cached child class
/// (e.g. `ChildJob extends ScheduledJob`) embeds fully-merged members
/// from its parent.  When the parent's `@property` docblock changes,
/// the child's cache entry still holds the old inherited property and
/// must be discarded.
///
/// The dependent set is found by walking the reverse-dependency index
/// maintained by [`ResolvedCacheInner`], so the cost is proportional to
/// the number of dependents rather than the size of the whole cache.
/// The returned vector lists the seed FQN followed by every transitively
/// evicted dependent, or is empty when nothing matched.
pub fn evict_fqn(cache: &mut ResolvedCacheInner, fqn: &str) -> Vec<String> {
    // A class whose linking read `fqn` (an imported type alias, or the
    // ancestors of a docblock class) changed along with `fqn`, and its
    // dependents go too.
    let importers = cache.evict_linked_classes_reading(fqn);
    if cache.map.is_empty() {
        return vec![];
    }

    // Walk the reverse-dependency graph starting from `fqn`, collecting the
    // seed plus every transitively dependent FQN.  Each step is an O(1) index
    // lookup, so the cost is proportional to the dependent set rather than the
    // whole cache.  `evicted` preserves [seed, ...dependents] discovery order.
    let mut evicted: Vec<String> = vec![fqn.to_string()];
    let mut seen: HashSet<String> = HashSet::from([fqn.to_string()]);
    let mut frontier: Vec<String> = vec![fqn.to_string()];
    for importer in importers {
        if seen.insert(importer.clone()) {
            evicted.push(importer.clone());
            frontier.push(importer);
        }
    }

    while let Some(current) = frontier.pop() {
        // A dependent class stores the dependency either as the FQN or as the
        // short name, so look under both.
        let short = crate::util::short_name(&current);
        let mut dependents: Vec<String> = Vec::new();
        if let Some(set) = cache.reverse_deps.get(current.as_str()) {
            dependents.extend(set.iter().map(|k| k.0.to_string()));
        }
        if short != current.as_str()
            && let Some(set) = cache.reverse_deps.get(short)
        {
            dependents.extend(set.iter().map(|k| k.0.to_string()));
        }

        for dep in dependents {
            if seen.insert(dep.clone()) {
                evicted.push(dep.clone());
                frontier.push(dep);
            }
        }
    }

    // Remove every generic-arg variant of each collected FQN.  All dependents
    // come from the reverse index (so they are present), but the seed may not
    // have been cached — when nothing was actually removed, report no eviction.
    let mut any_removed = false;
    for f in &evicted {
        if cache.remove_all_variants(f) {
            any_removed = true;
        }
    }

    if any_removed { evicted } else { vec![] }
}

/// Collect the dependency strings a class references through its
/// `parent_class`, `used_traits`, `interfaces`, `mixins`, and
/// `casts_definitions`, plus the classes its generic arguments name: a
/// specialisation such as `Builder<User>` carries members read from
/// `User` (its scopes), which go stale when `User` changes.
///
/// Each value is stored verbatim as it appears on the class: it may be a
/// fully-qualified name (cross-file, post-resolution) or a short name
/// (same-file reference).  Eviction looks up the reverse index under both
/// the evicted FQN and its short name, so storing the raw value here
/// reproduces the dual FQN / short-name matching.
///
/// Cast values may carry a `:argument` suffix (e.g. `"DecimalCast:8:2"`);
/// only the class portion is recorded.
fn dependency_keys(key: &ResolvedClassCacheKey, cls: &ClassInfo) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();

    for arg in &key.1 {
        if arg.starts_with('\0') {
            continue;
        }
        if let Some(name) = crate::php_type::PhpType::parse(arg).base_name() {
            keys.push(name.to_string());
        }
    }

    if let Some(ref parent) = cls.parent_class {
        keys.push(parent.to_string());
    }
    keys.extend(cls.used_traits.iter().map(|t| t.to_string()));
    keys.extend(cls.interfaces.iter().map(|i| i.to_string()));
    keys.extend(cls.mixins.iter().map(|m| m.to_string()));

    if let Some(laravel) = cls.laravel() {
        for (_, cast_type) in &laravel.casts_definitions {
            let class_part = cast_type.split(':').next().unwrap_or(cast_type);
            keys.push(class_part.to_string());
        }
    }

    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweep_drops_dead_origins_once_the_map_doubles() {
        let mut map: HashMap<(usize, u128), Weak<u32>> = HashMap::new();
        let mut swept_len = 0;
        let live: Vec<Arc<u32>> = (0..SUBSTITUTED_SWEEP_FLOOR as u32).map(Arc::new).collect();
        for (i, origin) in live.iter().enumerate() {
            map.insert((i, 0), Arc::downgrade(origin));
        }
        for i in 0..SUBSTITUTED_SWEEP_FLOOR {
            let dead = Arc::new(0u32);
            map.insert((SUBSTITUTED_SWEEP_FLOOR + i, 0), Arc::downgrade(&dead));
        }

        sweep_dead_witnesses(&mut map, &mut swept_len, |w| w.strong_count() > 0);
        assert_eq!(map.len(), SUBSTITUTED_SWEEP_FLOOR);
        assert_eq!(swept_len, SUBSTITUTED_SWEEP_FLOOR);

        // Below twice the swept length nothing is rescanned.
        let dead = Arc::new(0u32);
        map.insert((usize::MAX, 0), Arc::downgrade(&dead));
        drop(dead);
        sweep_dead_witnesses(&mut map, &mut swept_len, |w| w.strong_count() > 0);
        assert_eq!(map.len(), SUBSTITUTED_SWEEP_FLOOR + 1);
    }

    fn class_named(name: &str) -> Arc<ClassInfo> {
        Arc::new(ClassInfo {
            name: crate::atom::atom(name),
            ..ClassInfo::default()
        })
    }

    #[test]
    fn a_linked_class_that_missed_a_class_lasts_one_lookup_generation() {
        let mut cache = ResolvedCacheInner::default();
        let complete = class_named("Complete");
        let missed = class_named("Missed");
        let linked = class_named("Linked");
        cache.insert_linked_class(&complete, None, vec!["Read".into()], None);
        cache.insert_linked_class(&missed, Some(Arc::clone(&linked)), vec![], Some(3));

        let (served, _) = cache.get_linked_class(&complete, 7).unwrap();
        assert!(Arc::ptr_eq(&served, &complete), "linking left it as loaded");
        let (served, _) = cache.get_linked_class(&missed, 3).unwrap();
        assert!(Arc::ptr_eq(&served, &linked));
        assert!(
            cache.get_linked_class(&missed, 4).is_none(),
            "the class index changed since"
        );
    }

    #[test]
    fn a_linked_class_does_not_keep_the_class_it_was_linked_from_alive() {
        let mut cache = ResolvedCacheInner::default();
        let class = class_named("Unchanged");
        let witness = Arc::downgrade(&class);
        cache.insert_linked_class(&class, None, vec![], None);
        drop(class);
        assert_eq!(witness.strong_count(), 0);
    }

    #[test]
    fn editing_a_class_the_linking_read_drops_the_linked_class() {
        let mut cache = ResolvedCacheInner::default();
        let class = Arc::new(ClassInfo {
            file_namespace: Some(crate::atom::atom("App")),
            ..(*class_named("Clock")).clone()
        });
        cache.insert_linked_class(&class, None, vec!["App\\Instant".into()], None);
        assert_eq!(
            cache.evict_linked_classes_reading("App\\Instant"),
            vec!["App\\Clock".to_string()]
        );
        assert!(cache.get_linked_class(&class, 0).is_none());
    }
}
