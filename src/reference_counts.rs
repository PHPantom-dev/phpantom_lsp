//! Cached exact member references for declaration annotations.
//!
//! A count shown next to a declaration has to mean "references to *this*
//! symbol", which is the search Find References runs: resolve the receiver
//! of every candidate access and keep the ones whose type is in the
//! declaring class' hierarchy.  That search is far too slow for the CodeLens
//! request path (hundreds of milliseconds for one member on a large
//! project), so exact locations are computed on a background thread and
//! served from this bounded cache; the lens titles its count from them and
//! hands them straight to the client when the user opens the reference list.
//!
//! Clickable lenses require fresh locations, so a lens whose result is
//! being recomputed carries a placeholder rather than a count it cannot
//! back up.  The reference index marks entries stale rather than dropping
//! them, and the next lens request queues them.
//!
//! Staleness records *which files* an edit reparsed, because that is what
//! makes recomputation affordable: the accesses in those files are searched
//! for again and every other file's cached locations are merged back in, so
//! typing in one file does not re-search the workspace once per declaration
//! in it.  Only a change that can move a receiver's type anywhere (a
//! signature, a docblock, an inheritance edit) falls back to searching
//! everything.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use parking_lot::{Mutex, RwLock};
use tower_lsp::lsp_types::{Location, Range, Url};

use crate::Backend;
use crate::atom::{Atom, AtomMap};
use crate::class_lookup::find_class_at_offset;

/// Upper bound on cached member names.  Reached only by browsing tens of
/// thousands of declarations in one session, and the cache is then dropped
/// whole: keeping it in LRU order would cost more than recomputing.
const MAX_CACHED_MEMBERS: usize = 20_000;

/// Keep exact locations only while their aggregate stays small enough for an
/// interactive cache.  Counts remain cacheable for unusually popular symbols.
const MAX_CACHED_LOCATIONS: usize = 50_000;
const MAX_LOCATIONS_PER_MEMBER: usize = 5_000;
const MAX_CACHED_URIS: usize = 50_000;

/// How long the workspace has to go unedited before queued declarations are
/// searched for.  Long enough that a burst of keystrokes costs one search
/// rather than one each, short enough that a pause fills the lenses in.
const EDIT_PAUSE: std::time::Duration = std::time::Duration::from_millis(400);

/// A member declaration whose count still has to be computed.
#[derive(Clone, PartialEq, Eq, Hash)]
struct PendingCount {
    uri: Arc<str>,
    /// Offset of the declaration's name, used to find the enclosing class.
    offset: u32,
    class_fqn: Atom,
    member: Atom,
    is_static: bool,
}

#[derive(Clone)]
struct CachedReferences {
    count: u32,
    locations: Option<Arc<[CompactLocation]>>,
    /// Set when the reference index changed in a way that can affect this
    /// count.  The value is still served; it is only a recompute request.
    count_stale: bool,
    /// What has to be rescanned before the cached locations can be served.
    locations_stale: Staleness,
}

/// Which files' contributions to a cached result an edit can have changed.
///
/// Reparsing a file moves the offsets of the accesses *in that file* and can
/// change what their receivers resolve to, but it leaves every other file's
/// accesses exactly where they were.  Recording which files were touched is
/// what lets an edit rescan those files alone instead of searching the whole
/// workspace again for every declaration in the open file.
#[derive(Clone, Default, PartialEq, Eq)]
enum Staleness {
    #[default]
    Fresh,
    /// Only these files were reparsed.
    Files(HashSet<Arc<str>>),
    /// A signature or inheritance change: any access anywhere can have
    /// changed which declaration it belongs to.
    Everything,
}

impl Staleness {
    fn is_fresh(&self) -> bool {
        *self == Staleness::Fresh
    }

    /// Widen to also cover `other`, keeping the more pessimistic of the two.
    fn merge(&mut self, other: Staleness) {
        *self = match (std::mem::take(self), other) {
            (Staleness::Everything, _) | (_, Staleness::Everything) => Staleness::Everything,
            (Staleness::Fresh, new) => new,
            (current, Staleness::Fresh) => current,
            (Staleness::Files(mut current), Staleness::Files(new)) => {
                current.extend(new);
                Staleness::Files(current)
            }
        };
    }
}

impl PendingCount {
    fn query(&self) -> crate::references::MemberDeclarationReferenceQuery {
        crate::references::MemberDeclarationReferenceQuery {
            uri: Arc::clone(&self.uri),
            offset: self.offset,
            member: self.member,
            is_static: self.is_static,
        }
    }
}

/// A declaration whose cached locations only have to be refreshed in the
/// files an edit reparsed.
struct RescanPlan {
    /// The files to search again.
    rescan: HashSet<Arc<str>>,
    /// The cached locations from every other file, which the edit cannot
    /// have moved.
    keep: Vec<Location>,
}

impl RescanPlan {
    /// Combine what was kept with what the restricted search found.
    ///
    /// The search runs over the union of every queued declaration's files,
    /// so results from a file this declaration did not have to rescan are
    /// dropped rather than added twice.
    fn merge(self, found: Vec<Location>) -> Vec<Location> {
        let mut locations = self.keep;
        locations.extend(
            found
                .into_iter()
                .filter(|location| self.rescan.contains(location.uri.as_str())),
        );
        crate::references::sort_locations_for_references(&mut locations);
        locations
    }
}

/// A cached LSP location without a separately allocated `Url` string for
/// every occurrence.  URI strings are interned per cache below.
#[derive(Clone, PartialEq, Eq)]
struct CompactLocation {
    uri: Arc<str>,
    range: Range,
}

impl CompactLocation {
    fn to_lsp(&self) -> Option<Location> {
        Some(Location {
            uri: Url::parse(&self.uri).ok()?,
            range: self.range,
        })
    }
}

/// Per-member-name counts, keyed by the class that declares the member.
///
/// The member name is the outer key because that is the granularity the
/// reference index can invalidate at: a file that gains or loses an access
/// to `save` can only change counts of members named `save`.  The two
/// slots are the instance and static member of that name.
type MemberCounts = AtomMap<[Option<CachedReferences>; 2]>;

#[derive(Default)]
struct ReferenceCache {
    by_member: AtomMap<MemberCounts>,
    location_count: usize,
    uris: HashSet<Arc<str>>,
}

#[derive(Default)]
pub(crate) struct MemberRefCounts {
    counts: RwLock<ReferenceCache>,
    pending: Mutex<HashSet<PendingCount>>,
    /// Per-file digest of the inheritance each class declares, so a file
    /// that starts extending something can be told from one that only
    /// changed a method body.
    class_shapes: RwLock<HashMap<String, u64>>,
    /// Set while a background computation runs, so a burst of CodeLens
    /// requests schedules one job rather than one each.
    computing: AtomicBool,
    /// Serialises exact searches started by background refreshes and lazy
    /// CodeLens resolves.  A resolve that races the worker reuses its result
    /// instead of launching the same expensive scan twice.
    compute_lock: Mutex<()>,
    /// Bumped by every invalidation, so a search that took seconds can tell
    /// whether the content it read is still what the editor holds.
    epoch: AtomicU64,
    /// When the last invalidation landed, so a search can wait for typing to
    /// stop instead of racing the next keystroke.
    last_invalidation: Mutex<Option<std::time::Instant>>,
}

fn slot(is_static: bool) -> usize {
    usize::from(is_static)
}

impl MemberRefCounts {
    /// Note that cached results just went out of date.
    ///
    /// The epoch tells a search that already ran that its answer describes
    /// replaced content; the timestamp lets the next one wait for typing to
    /// stop first.
    fn record_invalidation(&self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
        *self.last_invalidation.lock() = Some(std::time::Instant::now());
    }

    /// How long ago the last invalidation landed.  A cache nothing has
    /// invalidated yet reports a long time, so nothing waits on it.
    fn since_last_invalidation(&self) -> std::time::Duration {
        self.last_invalidation
            .lock()
            .map_or(EDIT_PAUSE, |at| at.elapsed())
    }

    fn get(&self, class_fqn: Atom, member: Atom, is_static: bool) -> Option<CachedReferences> {
        self.counts.read().by_member.get(&member)?.get(&class_fqn)?[slot(is_static)].clone()
    }

    /// How a queued declaration can be brought up to date without searching
    /// the workspace again, or `None` when nothing cached survives the edit.
    fn rescan_plan(&self, item: &PendingCount) -> Option<RescanPlan> {
        let cache = self.counts.read();
        let cached = cache.by_member.get(&item.member)?.get(&item.class_fqn)?[slot(item.is_static)]
            .as_ref()?;
        let Staleness::Files(files) = &cached.locations_stale else {
            return None;
        };
        let keep = cached
            .locations
            .as_ref()?
            .iter()
            .filter(|location| !files.contains(&location.uri))
            .filter_map(CompactLocation::to_lsp)
            .collect();
        Some(RescanPlan {
            rescan: files.clone(),
            keep,
        })
    }

    /// Whether the cached result for a declaration can be served as is.
    fn is_fresh(&self, item: &PendingCount) -> bool {
        self.get(item.class_fqn, item.member, item.is_static)
            .is_some_and(|cached| !cached.count_stale && cached.locations_stale.is_fresh())
    }

    /// Store freshly computed references, returning whether they differ from
    /// the result the editor was last given.
    ///
    /// `invalidated_since` says an edit landed while these were being
    /// computed, so they describe content the editor has already replaced.
    /// The value is still worth keeping (it is closer than nothing and stops
    /// the count from blinking), but the staleness the edit recorded is left
    /// in place so the entry is recomputed rather than trusted.
    fn store(
        &self,
        class_fqn: Atom,
        member: Atom,
        is_static: bool,
        locations: Vec<Location>,
        invalidated_since: bool,
    ) -> bool {
        let mut cache = self.counts.write();
        let previous = cache
            .by_member
            .get(&member)
            .and_then(|members| members.get(&class_fqn))
            .and_then(|slots| slots[slot(is_static)].clone());
        let previous_location_count = previous
            .as_ref()
            .and_then(|cached| cached.locations.as_ref())
            .map_or(0, |locations| locations.len());
        let count = locations.len() as u32;
        let cache_locations = locations.len() <= MAX_LOCATIONS_PER_MEMBER;
        let new_location_count = if cache_locations { locations.len() } else { 0 };

        if cache.by_member.len() >= MAX_CACHED_MEMBERS
            || cache.location_count - previous_location_count + new_location_count
                > MAX_CACHED_LOCATIONS
            || cache.uris.len() >= MAX_CACHED_URIS
        {
            cache.by_member.clear();
            cache.location_count = 0;
            cache.uris.clear();
        } else {
            cache.location_count -= previous_location_count;
        }

        let cached_locations = cache_locations.then(|| {
            let locations: Vec<CompactLocation> = locations
                .into_iter()
                .map(|location| {
                    let uri = match cache.uris.get(location.uri.as_str()) {
                        Some(uri) => Arc::clone(uri),
                        None => {
                            let uri: Arc<str> = Arc::from(location.uri.as_str());
                            cache.uris.insert(Arc::clone(&uri));
                            uri
                        }
                    };
                    CompactLocation {
                        uri,
                        range: location.range,
                    }
                })
                .collect();
            Arc::<[CompactLocation]>::from(locations)
        });

        let changed = previous.as_ref().is_none_or(|cached| {
            cached.count_stale
                || !cached.locations_stale.is_fresh()
                || cached.count != count
                || cached.locations.as_deref() != cached_locations.as_deref()
        });
        let (count_stale, locations_stale) = match (invalidated_since, &previous) {
            (true, Some(cached)) => (cached.count_stale, cached.locations_stale.clone()),
            (true, None) => (true, Staleness::Everything),
            (false, _) => (false, Staleness::Fresh),
        };
        let entry = &mut cache
            .by_member
            .entry(member)
            .or_default()
            .entry(class_fqn)
            .or_default()[slot(is_static)];
        *entry = Some(CachedReferences {
            count,
            locations: cached_locations,
            count_stale,
            locations_stale,
        });
        cache.location_count += new_location_count;
        changed
    }

    /// Mark every count for members of this name as needing recomputation,
    /// because `uris` changed what they contribute to it.
    pub(crate) fn invalidate_member(&self, member: Atom, uris: &HashSet<Arc<str>>) {
        self.record_invalidation();
        let mut cache = self.counts.write();
        let Some(entries) = cache.by_member.get_mut(&member) else {
            return;
        };
        for slots in entries.values_mut() {
            for cached in slots.iter_mut().flatten() {
                cached.count_stale = true;
                cached.locations_stale.merge(Staleness::Files(uris.clone()));
            }
        }
    }

    /// Mark the locations a reparse of `uris` can have moved.
    ///
    /// A cached result is only affected when one of its own locations sits in
    /// a reparsed file: offsets elsewhere did not move, and an access
    /// elsewhere still resolves to the same receiver.  An entry whose
    /// locations were too many to cache cannot be checked, so it is rescanned
    /// in full.
    pub(crate) fn invalidate_locations_in(&self, uris: &HashSet<Arc<str>>) {
        if uris.is_empty() {
            return;
        }
        self.record_invalidation();
        let mut cache = self.counts.write();
        for entries in cache.by_member.values_mut() {
            for slots in entries.values_mut() {
                for cached in slots.iter_mut().flatten() {
                    match &cached.locations {
                        Some(locations) => {
                            if locations
                                .iter()
                                .any(|location| uris.contains(&location.uri))
                            {
                                cached.locations_stale.merge(Staleness::Files(uris.clone()));
                            }
                        }
                        None => cached.locations_stale = Staleness::Everything,
                    }
                }
            }
        }
    }

    /// Mark every cached location stale.
    ///
    /// A signature or docblock change settles receiver types against the
    /// whole workspace, so an access in a file nothing touched can start
    /// belonging to a different declaration.
    pub(crate) fn invalidate_locations_all(&self) {
        self.record_invalidation();
        let mut cache = self.counts.write();
        for entries in cache.by_member.values_mut() {
            for slots in entries.values_mut() {
                for cached in slots.iter_mut().flatten() {
                    cached.locations_stale = Staleness::Everything;
                }
            }
        }
    }

    /// Mark every cached count as needing recomputation.
    ///
    /// Used when a class' place in the inheritance graph changes, since
    /// that moves which accesses belong to which declaration.
    pub(crate) fn invalidate_all(&self) {
        self.record_invalidation();
        let mut cache = self.counts.write();
        for entries in cache.by_member.values_mut() {
            for slots in entries.values_mut() {
                for cached in slots.iter_mut().flatten() {
                    cached.count_stale = true;
                    cached.locations_stale = Staleness::Everything;
                }
            }
        }
    }

    pub(crate) fn has_pending(&self) -> bool {
        !self.pending.lock().is_empty()
    }

    /// Whether anything is cached at all.  Nothing is, until a declaration
    /// lens has been asked for, and the reference index skips its
    /// invalidation bookkeeping until then.
    pub(crate) fn is_empty(&self) -> bool {
        self.counts.read().by_member.is_empty()
    }

    #[cfg(feature = "mem-audit")]
    pub(crate) fn audit_heap(&self) -> (usize, usize, usize, usize, usize) {
        use std::mem::size_of;

        let cache = self.counts.read();
        let mut bytes =
            cache.by_member.capacity() * (size_of::<Atom>() + size_of::<MemberCounts>() + 1);
        let mut allocations = usize::from(cache.by_member.capacity() > 0);
        let mut entries = 0usize;
        for members in cache.by_member.values() {
            bytes += members.capacity()
                * (size_of::<Atom>() + size_of::<[Option<CachedReferences>; 2]>() + 1);
            allocations += usize::from(members.capacity() > 0);
            for cached in members.values().flat_map(|slots| slots.iter().flatten()) {
                entries += 1;
                if let Some(locations) = &cached.locations {
                    bytes +=
                        size_of::<usize>() * 2 + locations.len() * size_of::<CompactLocation>();
                    allocations += 1;
                }
            }
        }
        bytes += cache.uris.capacity() * (size_of::<Arc<str>>() + 1);
        allocations += usize::from(cache.uris.capacity() > 0);
        for uri in &cache.uris {
            bytes += size_of::<usize>() * 2 + uri.len();
            allocations += 1;
        }
        (
            cache.by_member.len(),
            entries,
            cache.location_count,
            bytes,
            allocations,
        )
    }

    #[cfg(feature = "mem-audit")]
    pub(crate) fn clear_cached(&self) {
        let mut cache = self.counts.write();
        cache.by_member.clear();
        cache.location_count = 0;
        cache.uris.clear();
        drop(cache);
        self.pending.lock().clear();
        self.class_shapes.write().clear();
    }
}

impl Backend {
    /// Whether the inheritance the file's classes declare differs from the
    /// last time it was indexed, recording the new shape either way.
    ///
    /// A file whose shape was never recorded counts as changed: the
    /// recording starts when the first count is cached, so the first edit
    /// to a file after that has nothing to compare against.
    pub(crate) fn class_shape_changed(&self, uri: &str) -> bool {
        let shape = self.class_shape(uri);
        let mut shapes = self.member_ref_counts.class_shapes.write();
        match (shapes.get(uri).copied(), shape) {
            (previous, Some(shape)) if previous != Some(shape) => {
                shapes.insert(uri.to_string(), shape);
                true
            }
            (Some(_), None) => {
                shapes.remove(uri);
                true
            }
            _ => false,
        }
    }

    pub(crate) fn forget_class_shape(&self, uri: &str) {
        self.member_ref_counts.class_shapes.write().remove(uri);
    }

    /// A digest of every class the file declares and what it inherits
    /// from, or `None` when the file declares no class.
    fn class_shape(&self, uri: &str) -> Option<u64> {
        let classes = self.symbols.uri_classes_index.read().get(uri).cloned()?;
        if classes.is_empty() {
            return None;
        }
        let mut hasher = DefaultHasher::new();
        for class in classes.iter() {
            class.fqn().hash(&mut hasher);
            class.parent_class.hash(&mut hasher);
            class.interfaces.hash(&mut hasher);
            class.used_traits.hash(&mut hasher);
        }
        Some(hasher.finish())
    }

    /// Fresh exact locations for a member declaration, if already cached.
    /// Missing or stale entries are queued for the shared background worker.
    pub(crate) fn member_ref_locations_cached(
        &self,
        uri: &str,
        offset: u32,
        class_fqn: Atom,
        member: Atom,
        is_static: bool,
    ) -> Option<Vec<Location>> {
        self.member_ref_locations(uri, offset, class_fqn, member, is_static, true)
    }

    /// Fresh exact locations without queuing a background computation.
    ///
    /// Clients without CodeLens refresh receive a lazy lens and resolve only
    /// the entries they display. Avoiding a background queue here prevents
    /// that resolve from waiting behind every declaration in the file.
    pub(crate) fn member_ref_locations_ready(
        &self,
        uri: &str,
        offset: u32,
        class_fqn: Atom,
        member: Atom,
        is_static: bool,
    ) -> Option<Vec<Location>> {
        self.member_ref_locations(uri, offset, class_fqn, member, is_static, false)
    }

    fn member_ref_locations(
        &self,
        uri: &str,
        offset: u32,
        class_fqn: Atom,
        member: Atom,
        is_static: bool,
        queue_if_missing: bool,
    ) -> Option<Vec<Location>> {
        let cached = self.member_ref_counts.get(class_fqn, member, is_static);
        if queue_if_missing
            && cached
                .as_ref()
                .is_none_or(|cached| cached.count_stale || !cached.locations_stale.is_fresh())
        {
            self.queue_member_references(uri, offset, class_fqn, member, is_static);
        }
        cached.and_then(|cached| {
            if cached.count_stale || !cached.locations_stale.is_fresh() {
                return None;
            }
            cached.locations.map(|locations| {
                locations
                    .iter()
                    .filter_map(CompactLocation::to_lsp)
                    .collect()
            })
        })
    }

    fn queue_member_references(
        &self,
        uri: &str,
        offset: u32,
        class_fqn: Atom,
        member: Atom,
        is_static: bool,
    ) {
        self.member_ref_counts.pending.lock().insert(PendingCount {
            uri: Arc::from(uri),
            offset,
            class_fqn,
            member,
            is_static,
        });
    }

    /// Exact locations for a lazy CodeLens resolve, reusing a fresh cache hit
    /// or computing and storing the declaration once under the shared search
    /// lock.
    pub(crate) fn resolve_member_ref_locations(
        &self,
        uri: &str,
        offset: u32,
        class_fqn: Atom,
        member: Atom,
        is_static: bool,
    ) -> Vec<Location> {
        if let Some(locations) =
            self.member_ref_locations_cached(uri, offset, class_fqn, member, is_static)
        {
            return locations;
        }

        let _compute_guard = self.member_ref_counts.compute_lock.lock();
        if let Some(cached) = self.member_ref_counts.get(class_fqn, member, is_static)
            && !cached.count_stale
            && cached.locations_stale.is_fresh()
            && let Some(locations) = cached.locations
        {
            return locations
                .iter()
                .filter_map(CompactLocation::to_lsp)
                .collect();
        }

        let epoch = self.member_ref_counts.epoch.load(Ordering::Acquire);
        let locations = self.member_declaration_references(uri, offset, &member, is_static);
        let invalidated_since = self.member_ref_counts.epoch.load(Ordering::Acquire) != epoch;
        self.member_ref_counts.store(
            class_fqn,
            member,
            is_static,
            locations.clone(),
            invalidated_since,
        );
        if !invalidated_since {
            self.member_ref_counts.pending.lock().remove(&PendingCount {
                uri: Arc::from(uri),
                offset,
                class_fqn,
                member,
                is_static,
            });
        }
        locations
    }

    /// Compute every queued member reference count.
    ///
    /// Returns `true` when at least one count changed, which is the signal
    /// to ask the editor to re-pull lenses.  Runs the search Find References
    /// runs, so the number matches what the user gets when they follow it.
    pub fn compute_pending_member_ref_counts(&self) -> bool {
        let _compute_guard = self.member_ref_counts.compute_lock.lock();
        // Taken rather than drained: an edit that lands while this runs marks
        // the entries it affects stale again, and the epoch below is what
        // tells the results apart from the content the editor now holds.
        let pending: Vec<PendingCount> = self
            .member_ref_counts
            .pending
            .lock()
            .iter()
            .cloned()
            .collect();
        if pending.is_empty() {
            return false;
        }
        let epoch = self.member_ref_counts.epoch.load(Ordering::Acquire);

        let _chain_guard = crate::type_engine::resolver::with_chain_resolution_cache();
        let _resolver_guard = crate::type_engine::call_resolution::activate_type_engine_caches();

        // A declaration may have moved or gone since the lens was requested.
        // Exclude stale offsets before preparing the shared semantic scan so
        // they cannot fall back to counting every member of that name.
        let valid_pending: Vec<_> = pending
            .iter()
            .filter(|item| self.declaration_still_at(item))
            .collect();

        // An edit only moves the accesses in the files it reparsed.  Those
        // declarations are rescanned in those files alone and merged with
        // what is still cached for every other file, which is what keeps a
        // keystroke from re-searching the workspace once per declaration.
        let mut full = Vec::new();
        let mut partial = Vec::new();
        for item in valid_pending {
            match self.member_ref_counts.rescan_plan(item) {
                Some(plan) => partial.push((item, plan)),
                None => full.push(item),
            }
        }

        let mut results: Vec<(&PendingCount, Vec<Location>)> = Vec::new();
        if !full.is_empty() {
            let queries: Vec<_> = full.iter().map(|item| item.query()).collect();
            results.extend(
                full.into_iter()
                    .zip(self.member_declaration_references_batch(&queries)),
            );
        }
        if !partial.is_empty() {
            let mut scope: HashSet<Arc<str>> = HashSet::new();
            for (_, plan) in &partial {
                scope.extend(plan.rescan.iter().cloned());
            }
            let queries: Vec<_> = partial.iter().map(|(item, _)| item.query()).collect();
            let scanned = self.member_declaration_references_batch_in(&queries, Some(&scope));
            for ((item, plan), found) in partial.into_iter().zip(scanned) {
                results.push((item, plan.merge(found)));
            }
        }

        let mut changed = false;
        let invalidated_since = self.member_ref_counts.epoch.load(Ordering::Acquire) != epoch;
        for (item, locations) in results {
            changed |= self.member_ref_counts.store(
                item.class_fqn,
                item.member,
                item.is_static,
                locations,
                invalidated_since,
            );
        }

        // Only declarations whose result is now trustworthy leave the queue.
        // One the edit above invalidated stays on it, or the recomputation it
        // asked for would be dropped and its count frozen at what this pass
        // read from content the editor has already replaced.
        let mut queue = self.member_ref_counts.pending.lock();
        for item in &pending {
            if !invalidated_since || self.member_ref_counts.is_fresh(item) {
                queue.remove(item);
            }
        }
        changed
    }

    fn declaration_still_at(&self, item: &PendingCount) -> bool {
        let classes = {
            let index = self.symbols.uri_classes_index.read();
            match index.get(item.uri.as_ref()) {
                Some(classes) => classes.clone(),
                None => return false,
            }
        };
        find_class_at_offset(&classes, item.offset)
            .is_some_and(|class| class.fqn() == item.class_fqn)
    }

    /// Run the queued member reference counts on a background thread and
    /// ask the editor to re-pull lenses once they land.
    ///
    /// At most one computation runs at a time. Requests that arrive while it
    /// runs join the same burst, which is drained before one editor refresh.
    /// Refreshing after every partial batch creates a feedback loop in clients
    /// that immediately re-request lenses for all open buffers.
    ///
    /// The burst also waits for typing to pause.  A change to a signature
    /// settles receiver types across the whole workspace, so every keystroke
    /// in a method name would otherwise start a search that the next
    /// keystroke invalidates before it finishes.
    pub(crate) fn schedule_member_ref_counts(&self) {
        if !self.member_ref_counts.has_pending()
            || self
                .member_ref_counts
                .computing
                .swap(true, Ordering::AcqRel)
        {
            return;
        }

        let backend = self.clone_for_blocking();
        tokio::spawn(async move {
            backend.await_edit_pause().await;
            let worker = backend.clone_for_blocking();
            let changed = crate::server::run_blocking_cancel_safe("member ref counts", move || {
                let mut changed = false;
                loop {
                    changed |= worker.compute_pending_member_ref_counts();

                    // Pair the empty check with clearing `computing` under
                    // the queue lock. A request either lands before this and
                    // is drained by the loop, or lands afterwards, observes
                    // `computing == false`, and starts the next worker.
                    let pending = worker.member_ref_counts.pending.lock();
                    if pending.is_empty() {
                        worker
                            .member_ref_counts
                            .computing
                            .store(false, Ordering::Release);
                        return changed;
                    }
                }
            })
            .await;

            match changed {
                Some(true) => {
                    if let Some(ref client) = backend.client
                        && backend.supports_code_lens_refresh.load(Ordering::Acquire)
                    {
                        let _ = client.code_lens_refresh().await;
                    }
                }
                Some(false) => {}
                // A panicking task never cleared the flag; without this the
                // counts would never be computed again this session.
                None => backend
                    .member_ref_counts
                    .computing
                    .store(false, Ordering::Release),
            }
        });
    }

    /// Wait until no file has been reparsed for [`EDIT_PAUSE`].
    async fn await_edit_pause(&self) {
        loop {
            let since = self.member_ref_counts.since_last_invalidation();
            if since >= EDIT_PAUSE {
                return;
            }
            tokio::time::sleep(EDIT_PAUSE - since).await;
        }
    }
}

pub(crate) fn new_member_ref_counts() -> Arc<MemberRefCounts> {
    Arc::new(MemberRefCounts::default())
}

/// Unit tests for the count cache itself.
///
/// These stay in the crate because they assert on state the public API
/// deliberately does not expose: staleness epochs, the bounded exact
/// location store, and which scope snapshots a batch of counts reused.
/// The tests that only drive a `Backend` through the lens handlers live in
/// `tests/integration/code_lens.rs`.
#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::{CodeLens, Position, Range};

    const URI: &str = "file:///test.php";

    fn parse_extra(backend: &Backend, uri: &str, content: &str) {
        backend
            .open_files
            .write()
            .insert(uri.to_string(), Arc::new(content.to_string()));
        backend.update_ast(uri, content);
        backend
            .workspace_indexed
            .store(true, std::sync::atomic::Ordering::Release);
        // The cache exists for the warm lens a refresh-capable client is
        // shown once the background search lands, which is the path these
        // tests measure.
        backend
            .supports_code_lens_refresh
            .store(true, std::sync::atomic::Ordering::Release);
    }

    fn parse(backend: &Backend, content: &str) {
        parse_extra(backend, URI, content);
    }

    fn lenses_for(backend: &Backend, uri: &str, content: &str) -> Vec<CodeLens> {
        backend.handle_code_lens(uri, content).unwrap_or_default()
    }

    fn lenses(backend: &Backend, content: &str) -> Vec<CodeLens> {
        lenses_for(backend, URI, content)
    }

    /// The title of the lens on `line`, absent while the declaration's
    /// references are still being computed.
    fn count_on_line(lenses: &[CodeLens], line: u32) -> Option<String> {
        lenses
            .iter()
            .find(|lens| lens.range.start.line == line)
            .and_then(|lens| lens.command.as_ref())
            .map(|command| command.title.clone())
    }

    #[test]
    fn exact_location_cache_is_bounded_and_interns_uris() {
        let cache = MemberRefCounts::default();
        let location = Location {
            uri: Url::parse("file:///uses.php").unwrap(),
            range: Range::new(Position::new(1, 2), Position::new(1, 6)),
        };

        for index in 0..=MAX_CACHED_LOCATIONS / MAX_LOCATIONS_PER_MEMBER {
            cache.store(
                crate::atom::atom("Order"),
                crate::atom::atom(&format!("member{index}")),
                false,
                vec![location.clone(); MAX_LOCATIONS_PER_MEMBER],
                false,
            );
        }

        let state = cache.counts.read();
        assert!(state.location_count <= MAX_CACHED_LOCATIONS);
        assert_eq!(state.location_count, MAX_LOCATIONS_PER_MEMBER);
        assert_eq!(state.by_member.len(), 1);
        assert_eq!(state.uris.len(), 1);
    }

    const ONE_CALL: &str = r#"<?php
class Order {
    public function save(): void {}
}
function persist(Order $order): void {
    $order->save();
}
"#;

    #[test]
    fn batch_member_counts_reuse_forward_walked_scope_snapshots() {
        const ORDER_URI: &str = "file:///Order.php";
        const CONSUMER_URI: &str = "file:///Consumer.php";
        const ORDER: &str = "<?php\nclass Order {\n    public function save(): void {}\n}\n";
        const CONSUMER: &str = r#"<?php
function persist(Order $order): void {
    $order->save();
    $order->save();
    $order->save();
}
"#;

        let backend = Backend::new_test();
        parse_extra(&backend, ORDER_URI, ORDER);
        parse_extra(&backend, CONSUMER_URI, CONSUMER);
        lenses_for(&backend, ORDER_URI, ORDER);

        crate::type_engine::variable::resolution::reset_test_scope_cache_hits();
        backend.compute_pending_member_ref_counts();

        let declaration_offset = ORDER.find("save").unwrap() as u32;
        assert_eq!(
            backend
                .member_ref_locations_cached(
                    ORDER_URI,
                    declaration_offset,
                    crate::atom::atom("Order"),
                    crate::atom::atom("save"),
                    false,
                )
                .unwrap()
                .len(),
            3
        );
        assert!(
            crate::type_engine::variable::resolution::test_scope_cache_hits() >= 3,
            "each repeated receiver lookup should reuse the one forward-walked file scope"
        );
    }

    #[test]
    fn later_member_batches_reuse_the_semantic_file_index() {
        const SERVICE_URI: &str = "file:///Service.php";
        const CONSUMER_URI: &str = "file:///Consumer.php";
        const SERVICE: &str = r#"<?php
class Service {
    public function save(): void {}
    public function cancel(): void {}
}
"#;
        const CONSUMER: &str = r#"<?php
function run(Service $service): void {
    $service->save();
    $service->cancel();
}
"#;

        let backend = Backend::new_test();
        parse_extra(&backend, SERVICE_URI, SERVICE);
        parse_extra(&backend, CONSUMER_URI, CONSUMER);
        backend.workspace_indexed.store(true, Ordering::Release);

        let class_fqn = crate::atom::atom("Service");
        let save_offset = SERVICE.find("save").unwrap() as u32;
        assert!(
            backend
                .member_ref_locations_cached(
                    SERVICE_URI,
                    save_offset,
                    class_fqn,
                    crate::atom::atom("save"),
                    false,
                )
                .is_none()
        );
        crate::type_engine::variable::resolution::reset_test_scope_cache_hits();
        backend.compute_pending_member_ref_counts();
        assert!(crate::type_engine::variable::resolution::test_scope_cache_hits() > 0);

        let consumer_map = backend
            .symbol_maps
            .read()
            .get(CONSUMER_URI)
            .cloned()
            .unwrap();
        assert!(
            backend
                .resolved_member_file(CONSUMER_URI, &consumer_map)
                .is_some(),
            "the first member query should index every receiver in its candidate file"
        );

        let cancel_offset = SERVICE.find("cancel").unwrap() as u32;
        assert!(
            backend
                .member_ref_locations_cached(
                    SERVICE_URI,
                    cancel_offset,
                    class_fqn,
                    crate::atom::atom("cancel"),
                    false,
                )
                .is_none()
        );
        crate::type_engine::variable::resolution::reset_test_scope_cache_hits();
        backend.compute_pending_member_ref_counts();
        assert_eq!(
            crate::type_engine::variable::resolution::test_scope_cache_hits(),
            0,
            "a later member name must not rebuild or query the file's variable scopes"
        );
    }

    #[test]
    fn ready_only_location_lookup_does_not_queue_background_work() {
        let backend = Backend::new_test();
        parse(&backend, ONE_CALL);
        let declaration_offset = ONE_CALL.find("save").unwrap() as u32;

        assert!(
            backend
                .member_ref_locations_ready(
                    URI,
                    declaration_offset,
                    crate::atom::atom("Order"),
                    crate::atom::atom("save"),
                    false,
                )
                .is_none()
        );
        assert!(!backend.member_ref_counts.has_pending());
    }

    #[test]
    fn an_edit_that_adds_an_access_recomputes_the_count() {
        let backend = Backend::new_test();
        parse(&backend, ONE_CALL);
        lenses(&backend, ONE_CALL);
        backend.compute_pending_member_ref_counts();
        assert_eq!(
            count_on_line(&lenses(&backend, ONE_CALL), 2).as_deref(),
            Some("1 reference")
        );
        let declaration_offset = ONE_CALL.find("save").unwrap() as u32;
        assert_eq!(
            backend
                .member_ref_locations_cached(
                    URI,
                    declaration_offset,
                    crate::atom::atom("Order"),
                    crate::atom::atom("save"),
                    false,
                )
                .expect("exact reference locations should be cached")
                .len(),
            1
        );

        let edited = ONE_CALL.replace("$order->save();", "$order->save();\n    $order->save();");
        parse(&backend, &edited);

        assert!(
            backend
                .member_ref_locations_cached(
                    URI,
                    declaration_offset,
                    crate::atom::atom("Order"),
                    crate::atom::atom("save"),
                    false,
                )
                .is_none(),
            "stale locations must not be served to a clickable lens"
        );

        // A lens the user can click has to list what it counted, so the
        // pre-edit references are not shown again.  It keeps its line with a
        // placeholder rather than disappearing and shifting the file.
        assert_eq!(
            count_on_line(&lenses(&backend, &edited), 2).as_deref(),
            Some("- references")
        );
        assert!(backend.compute_pending_member_ref_counts());
        assert_eq!(
            count_on_line(&lenses(&backend, &edited), 2).as_deref(),
            Some("2 references")
        );
        assert_eq!(
            backend
                .member_ref_locations_cached(
                    URI,
                    declaration_offset,
                    crate::atom::atom("Order"),
                    crate::atom::atom("save"),
                    false,
                )
                .expect("edited exact locations should replace the stale cache")
                .len(),
            2
        );
    }

    #[test]
    fn changing_only_a_receiver_type_invalidates_cached_locations() {
        const ORDER_URI: &str = "file:///Order.php";
        const BUYER_URI: &str = "file:///Buyer.php";
        const CONSUMER_URI: &str = "file:///Consumer.php";
        let backend = Backend::new_test();
        let order = "<?php\nclass Order { public function save(): void {} }\n";
        let buyer = "<?php\nclass Buyer { public function save(): void {} }\n";
        let consumer = "<?php\nfunction persist(Order $value): void { $value->save(); }\n";
        parse_extra(&backend, ORDER_URI, order);
        parse_extra(&backend, BUYER_URI, buyer);
        parse_extra(&backend, CONSUMER_URI, consumer);

        let declaration_offset = order.find("save").unwrap() as u32;
        assert!(
            backend
                .member_ref_locations_cached(
                    ORDER_URI,
                    declaration_offset,
                    crate::atom::atom("Order"),
                    crate::atom::atom("save"),
                    false,
                )
                .is_none()
        );
        backend.compute_pending_member_ref_counts();
        assert_eq!(
            backend
                .member_ref_locations_cached(
                    ORDER_URI,
                    declaration_offset,
                    crate::atom::atom("Order"),
                    crate::atom::atom("save"),
                    false,
                )
                .unwrap()
                .len(),
            1
        );

        let edited = consumer.replace("Order $value", "Buyer $value");
        parse_extra(&backend, CONSUMER_URI, &edited);
        assert!(
            backend
                .member_ref_locations_cached(
                    ORDER_URI,
                    declaration_offset,
                    crate::atom::atom("Order"),
                    crate::atom::atom("save"),
                    false,
                )
                .is_none(),
            "a type-only edit must not leave a clickable lens pointing at stale locations"
        );
        backend.compute_pending_member_ref_counts();
        assert!(
            backend
                .member_ref_locations_cached(
                    ORDER_URI,
                    declaration_offset,
                    crate::atom::atom("Order"),
                    crate::atom::atom("save"),
                    false,
                )
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn an_edit_that_leaves_the_accesses_alone_keeps_the_count() {
        let backend = Backend::new_test();
        parse(&backend, ONE_CALL);
        lenses(&backend, ONE_CALL);
        backend.compute_pending_member_ref_counts();

        // The first edit is the one that records what the file's classes
        // inherit, so start measuring from the second.
        let edited = format!("{ONE_CALL}// a trailing comment\n");
        parse(&backend, &edited);
        lenses(&backend, &edited);
        backend.compute_pending_member_ref_counts();

        let edited_again = format!("{edited}// another trailing comment\n");
        parse(&backend, &edited_again);

        let cached = backend
            .member_ref_counts
            .get(crate::atom::atom("Order"), crate::atom::atom("save"), false)
            .expect("the computed count should survive the edit");
        assert_eq!(cached.count, 1);
        assert!(
            !cached.count_stale,
            "an edit that touches no access should not invalidate the count"
        );
        // Exact locations are another matter: any edit can move them, so
        // the clickable lens recomputes before it is shown again.
        assert!(!cached.locations_stale.is_fresh());
    }

    #[test]
    fn an_edit_in_an_unrelated_file_leaves_a_cached_declaration_alone() {
        const ORDER_URI: &str = "file:///Order.php";
        const CONSUMER_URI: &str = "file:///Consumer.php";
        const UNRELATED_URI: &str = "file:///helpers.php";
        const ORDER: &str = "<?php\nclass Order {\n    public function save(): void {}\n}\n";
        const CONSUMER: &str =
            "<?php\nfunction persist(Order $order): void {\n    $order->save();\n}\n";

        let backend = Backend::new_test();
        parse_extra(&backend, ORDER_URI, ORDER);
        parse_extra(&backend, CONSUMER_URI, CONSUMER);
        parse_extra(&backend, UNRELATED_URI, "<?php\nfunction noop(): void {}\n");
        lenses_for(&backend, ORDER_URI, ORDER);
        backend.compute_pending_member_ref_counts();
        assert_eq!(
            count_on_line(&lenses_for(&backend, ORDER_URI, ORDER), 2).as_deref(),
            Some("1 reference")
        );

        // A file that holds none of the cached locations was reparsed.
        parse_extra(
            &backend,
            UNRELATED_URI,
            "<?php\nfunction noop(): void {}\n// touched\n",
        );

        assert_eq!(
            count_on_line(&lenses_for(&backend, ORDER_URI, ORDER), 2).as_deref(),
            Some("1 reference"),
            "an edit that cannot have moved a cached location must not blank the lens"
        );
        assert!(
            !backend.member_ref_counts.has_pending(),
            "nor queue the declaration for another workspace search"
        );
    }

    #[test]
    fn an_edit_rescans_the_file_it_touched_and_keeps_the_rest() {
        const ORDER_URI: &str = "file:///Order.php";
        const FIRST_URI: &str = "file:///First.php";
        const SECOND_URI: &str = "file:///Second.php";
        const ORDER: &str = "<?php\nclass Order {\n    public function save(): void {}\n}\n";
        const FIRST: &str = "<?php\nfunction first(Order $order): void {\n    $order->save();\n    $order->save();\n}\n";
        let second = |calls: usize| {
            let body = "    $order->save();\n".repeat(calls);
            format!("<?php\nfunction second(Order $order): void {{\n{body}}}\n")
        };

        let backend = Backend::new_test();
        parse_extra(&backend, ORDER_URI, ORDER);
        parse_extra(&backend, FIRST_URI, FIRST);
        parse_extra(&backend, SECOND_URI, &second(1));
        lenses_for(&backend, ORDER_URI, ORDER);
        backend.compute_pending_member_ref_counts();
        assert_eq!(
            count_on_line(&lenses_for(&backend, ORDER_URI, ORDER), 2).as_deref(),
            Some("3 references")
        );

        // Only the second file changes.  Its accesses are counted again and
        // the first file's cached ones are carried over untouched.
        parse_extra(&backend, SECOND_URI, &second(3));
        assert_eq!(
            count_on_line(&lenses_for(&backend, ORDER_URI, ORDER), 2).as_deref(),
            Some("- references"),
            "the lens holds its line while the touched file is rescanned"
        );
        assert!(backend.compute_pending_member_ref_counts());

        let locations = backend
            .member_ref_locations_cached(
                ORDER_URI,
                ORDER.find("save").unwrap() as u32,
                crate::atom::atom("Order"),
                crate::atom::atom("save"),
                false,
            )
            .expect("the rescan should leave a complete result");
        assert_eq!(locations.len(), 5);
        assert_eq!(
            locations
                .iter()
                .filter(|location| location.uri.as_str() == FIRST_URI)
                .count(),
            2,
            "the untouched file's references must survive the rescan"
        );
    }

    #[test]
    fn a_result_computed_before_an_edit_is_not_marked_fresh() {
        let backend = Backend::new_test();
        parse(&backend, ONE_CALL);
        lenses(&backend, ONE_CALL);
        backend.compute_pending_member_ref_counts();

        let class_fqn = crate::atom::atom("Order");
        let member = crate::atom::atom("save");
        let declaration_offset = ONE_CALL.find("save").unwrap() as u32;

        // What a search finishing after an edit landed looks like: it carries
        // locations read from content the editor has already replaced.
        backend.queue_member_references(URI, declaration_offset, class_fqn, member, false);
        backend.member_ref_counts.invalidate_locations_all();
        backend
            .member_ref_counts
            .store(class_fqn, member, false, Vec::new(), true);

        assert!(
            !backend.member_ref_counts.is_fresh(&PendingCount {
                uri: Arc::from(URI),
                offset: declaration_offset,
                class_fqn,
                member,
                is_static: false,
            }),
            "a result read from replaced content must stay stale"
        );
        assert!(
            backend
                .member_ref_locations_cached(URI, declaration_offset, class_fqn, member, false)
                .is_none(),
            "and must not be served to a clickable lens"
        );
        assert!(
            backend.member_ref_counts.has_pending(),
            "the recomputation the edit asked for must survive"
        );
    }
}
