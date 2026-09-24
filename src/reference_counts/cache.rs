use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use parking_lot::{Mutex, RwLock};
use tower_lsp::lsp_types::{Location, Range, Url};

use crate::atom::{Atom, AtomMap};

/// Upper bound on cached member names.  Reached only by browsing tens of
/// thousands of declarations in one session, and the cache is then dropped
/// whole: keeping it in LRU order would cost more than recomputing.
const MAX_CACHED_MEMBERS: usize = 20_000;

/// Keep exact locations only while their aggregate stays small enough for an
/// interactive cache.  Counts remain cacheable for unusually popular symbols.
pub(super) const MAX_CACHED_LOCATIONS: usize = 50_000;
pub(super) const MAX_LOCATIONS_PER_MEMBER: usize = 5_000;
const MAX_CACHED_URIS: usize = 50_000;

/// How long the workspace has to go unedited before queued declarations are
/// searched for.  Long enough that a burst of keystrokes costs one search
/// rather than one each, short enough that a pause fills the lenses in.
pub(super) const EDIT_PAUSE: std::time::Duration = std::time::Duration::from_millis(400);

/// A member declaration whose count still has to be computed.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct PendingCount {
    pub(super) uri: Arc<str>,
    /// Offset of the declaration's name, used to find the enclosing class.
    pub(super) offset: u32,
    pub(super) class_fqn: Atom,
    pub(super) member: Atom,
    pub(super) is_static: bool,
}

#[derive(Clone)]
pub(super) struct CachedReferences {
    pub(super) count: u32,
    pub(super) locations: Option<Arc<[CompactLocation]>>,
    /// Set when the reference index changed in a way that can affect this
    /// count.  The value is still served; it is only a recompute request.
    pub(super) count_stale: bool,
    /// What has to be rescanned before the cached locations can be served.
    pub(super) locations_stale: Staleness,
}

/// Which files' contributions to a cached result an edit can have changed.
///
/// Reparsing a file moves the offsets of the accesses *in that file* and can
/// change what their receivers resolve to, but it leaves every other file's
/// accesses exactly where they were.  Recording which files were touched is
/// what lets an edit rescan those files alone instead of searching the whole
/// workspace again for every declaration in the open file.
#[derive(Clone, Default, PartialEq, Eq)]
pub(super) enum Staleness {
    #[default]
    Fresh,
    /// Only these files were reparsed.
    Files(HashSet<Arc<str>>),
    /// A signature or inheritance change: any access anywhere can have
    /// changed which declaration it belongs to.
    Everything,
}

impl Staleness {
    pub(super) fn is_fresh(&self) -> bool {
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
    pub(super) fn query(&self) -> crate::references::MemberDeclarationReferenceQuery {
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
pub(super) struct RescanPlan {
    /// The files to search again.
    pub(super) rescan: HashSet<Arc<str>>,
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
    pub(super) fn merge(self, found: Vec<Location>) -> Vec<Location> {
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
pub(super) struct CompactLocation {
    uri: Arc<str>,
    range: Range,
}

impl CompactLocation {
    pub(super) fn to_lsp(&self) -> Option<Location> {
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
pub(super) struct ReferenceCache {
    pub(super) by_member: AtomMap<MemberCounts>,
    pub(super) location_count: usize,
    pub(super) uris: HashSet<Arc<str>>,
}

#[derive(Default)]
pub(crate) struct MemberRefCounts {
    pub(super) counts: RwLock<ReferenceCache>,
    pub(super) pending: Mutex<HashSet<PendingCount>>,
    /// Per-file digest of the inheritance each class declares, so a file
    /// that starts extending something can be told from one that only
    /// changed a method body.
    pub(super) class_shapes: RwLock<HashMap<String, u64>>,
    /// Set while a background computation runs, so a burst of CodeLens
    /// requests schedules one job rather than one each.
    pub(super) computing: AtomicBool,
    /// Serialises exact searches started by background refreshes and lazy
    /// CodeLens resolves.  A resolve that races the worker reuses its result
    /// instead of launching the same expensive scan twice.
    pub(super) compute_lock: Mutex<()>,
    /// Bumped by every invalidation, so a search that took seconds can tell
    /// whether the content it read is still what the editor holds.
    pub(super) epoch: AtomicU64,
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
    pub(super) fn since_last_invalidation(&self) -> std::time::Duration {
        self.last_invalidation
            .lock()
            .map_or(EDIT_PAUSE, |at| at.elapsed())
    }

    pub(super) fn get(
        &self,
        class_fqn: Atom,
        member: Atom,
        is_static: bool,
    ) -> Option<CachedReferences> {
        self.counts.read().by_member.get(&member)?.get(&class_fqn)?[slot(is_static)].clone()
    }

    /// How a queued declaration can be brought up to date without searching
    /// the workspace again, or `None` when nothing cached survives the edit.
    pub(super) fn rescan_plan(&self, item: &PendingCount) -> Option<RescanPlan> {
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
    pub(super) fn is_fresh(&self, item: &PendingCount) -> bool {
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
    pub(super) fn store(
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
    ///
    /// An Eloquent scope or accessor is counted under the name it is used
    /// by (`active` for `scopeActive`), so the methods that name can stand
    /// for are marked too.
    pub(crate) fn invalidate_member(&self, member: Atom, uris: &HashSet<Arc<str>>) {
        let declaring = crate::virtual_members::laravel::declaring_method_names(&member)
            .map(|name| crate::atom::atom(&name));
        let mut cache = self.counts.write();
        let mut marked = false;
        for name in std::iter::once(member).chain(declaring) {
            let Some(entries) = cache.by_member.get_mut(&name) else {
                continue;
            };
            for slots in entries.values_mut() {
                for cached in slots.iter_mut().flatten() {
                    cached.count_stale = true;
                    cached.locations_stale.merge(Staleness::Files(uris.clone()));
                    marked = true;
                }
            }
        }
        if marked {
            self.record_invalidation();
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
        let mut cache = self.counts.write();
        let mut marked = false;
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
                                marked = true;
                            }
                        }
                        None => {
                            cached.locations_stale = Staleness::Everything;
                            marked = true;
                        }
                    }
                }
            }
        }
        if marked {
            self.record_invalidation();
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

pub(crate) fn new_member_ref_counts() -> Arc<MemberRefCounts> {
    Arc::new(MemberRefCounts::default())
}
