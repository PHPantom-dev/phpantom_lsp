//! Cached exact member references for declaration annotations.
//!
//! A count shown next to a declaration has to mean "references to *this*
//! symbol", which is the search Find References runs: resolve the receiver
//! of every candidate access and keep the ones whose type is in the
//! declaring class' hierarchy.  That search is far too slow for the
//! inlay-hint and CodeLens request paths (hundreds of milliseconds for one
//! member on a large project), so exact locations are computed on a background
//! thread and served from this bounded cache.  Inlay hints read their count;
//! CodeLens reuses the locations when the user opens the reference list.
//!
//! A hint is emitted only for a member whose count is already cached, and
//! a cached value keeps being served once it goes stale so the annotation
//! does not blink out between edits.  Clickable lenses require fresh locations
//! and are omitted for refresh-capable clients until recomputation finishes.
//! The reference index marks entries stale rather than dropping them, and the
//! next annotation request queues them for recomputation.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
    /// Set after any source edit, since receiver type resolution can change
    /// without changing the indexed member name or candidate count.
    locations_stale: bool,
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
    /// Set while a background computation runs, so a burst of inlay-hint
    /// requests schedules one job rather than one each.
    computing: AtomicBool,
    /// Serialises exact searches started by background refreshes and lazy
    /// CodeLens resolves.  A resolve that races the worker reuses its result
    /// instead of launching the same expensive scan twice.
    compute_lock: Mutex<()>,
}

fn slot(is_static: bool) -> usize {
    usize::from(is_static)
}

impl MemberRefCounts {
    fn get(&self, class_fqn: Atom, member: Atom, is_static: bool) -> Option<CachedReferences> {
        self.counts.read().by_member.get(&member)?.get(&class_fqn)?[slot(is_static)].clone()
    }

    /// Store freshly computed references, returning whether they differ from
    /// the result the editor was last given.
    fn store(
        &self,
        class_fqn: Atom,
        member: Atom,
        is_static: bool,
        locations: Vec<Location>,
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
                || cached.locations_stale
                || cached.count != count
                || cached.locations.as_deref() != cached_locations.as_deref()
        });
        let entry = &mut cache
            .by_member
            .entry(member)
            .or_default()
            .entry(class_fqn)
            .or_default()[slot(is_static)];
        *entry = Some(CachedReferences {
            count,
            locations: cached_locations,
            count_stale: false,
            locations_stale: false,
        });
        cache.location_count += new_location_count;
        changed
    }

    /// Mark every count for members of this name as needing recomputation.
    pub(crate) fn invalidate_member(&self, member: Atom) {
        let mut cache = self.counts.write();
        let Some(entries) = cache.by_member.get_mut(&member) else {
            return;
        };
        for slots in entries.values_mut() {
            for cached in slots.iter_mut().flatten() {
                cached.count_stale = true;
                cached.locations_stale = true;
            }
        }
    }

    /// Mark exact locations stale while preserving cached counts.
    ///
    /// A source edit can change the resolved receiver class without changing
    /// the member name or number of indexed candidates. Counts are invalidated
    /// more selectively, but clickable locations must never survive that edit.
    pub(crate) fn invalidate_locations_all(&self) {
        let mut cache = self.counts.write();
        for entries in cache.by_member.values_mut() {
            for slots in entries.values_mut() {
                for cached in slots.iter_mut().flatten() {
                    cached.locations_stale = true;
                }
            }
        }
    }

    /// Mark every cached count as needing recomputation.
    ///
    /// Used when a class' place in the inheritance graph changes, since
    /// that moves which accesses belong to which declaration.
    pub(crate) fn invalidate_all(&self) {
        let mut cache = self.counts.write();
        for entries in cache.by_member.values_mut() {
            for slots in entries.values_mut() {
                for cached in slots.iter_mut().flatten() {
                    cached.count_stale = true;
                    cached.locations_stale = true;
                }
            }
        }
    }

    pub(crate) fn has_pending(&self) -> bool {
        !self.pending.lock().is_empty()
    }

    /// Whether anything is cached at all.  Nothing is, until a declaration
    /// hint has been asked for, and the reference index skips its
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

    /// The cached reference count for a member declaration, queuing a
    /// background computation when there is none or the cached one is
    /// stale.
    pub(crate) fn member_ref_count_cached(
        &self,
        uri: &str,
        offset: u32,
        class_fqn: Atom,
        member: Atom,
        is_static: bool,
    ) -> Option<u32> {
        let cached = self.member_ref_counts.get(class_fqn, member, is_static);
        if cached.as_ref().is_none_or(|cached| cached.count_stale) {
            self.queue_member_references(uri, offset, class_fqn, member, is_static);
        }
        cached.map(|cached| cached.count)
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
                .is_none_or(|cached| cached.count_stale || cached.locations_stale)
        {
            self.queue_member_references(uri, offset, class_fqn, member, is_static);
        }
        cached.and_then(|cached| {
            if cached.count_stale || cached.locations_stale {
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
            && !cached.locations_stale
            && let Some(locations) = cached.locations
        {
            return locations
                .iter()
                .filter_map(CompactLocation::to_lsp)
                .collect();
        }

        let locations = self.member_declaration_references(uri, offset, &member, is_static);
        self.member_ref_counts
            .store(class_fqn, member, is_static, locations.clone());
        self.member_ref_counts.pending.lock().remove(&PendingCount {
            uri: Arc::from(uri),
            offset,
            class_fqn,
            member,
            is_static,
        });
        locations
    }

    /// Compute every queued member reference count.
    ///
    /// Returns `true` when at least one count changed, which is the signal
    /// to ask the editor to re-pull inlay hints.  Runs the search Find
    /// References runs, so the number matches what the user gets when they
    /// follow it.
    pub(crate) fn compute_pending_member_ref_counts(&self) -> bool {
        let _compute_guard = self.member_ref_counts.compute_lock.lock();
        // Taken rather than drained: a request that arrives while this
        // runs sees the counts it wants still stale and queues them
        // again, and clearing them at the end keeps that from buying a
        // second pass over work this one has already done.
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

        let _chain_guard = crate::type_engine::resolver::with_chain_resolution_cache();
        let _resolver_guard = crate::type_engine::call_resolution::activate_type_engine_caches();

        // A declaration may have moved or gone since the hint was requested.
        // Exclude stale offsets before preparing the shared semantic scan so
        // they cannot fall back to counting every member of that name.
        let valid_pending: Vec<_> = pending
            .iter()
            .filter(|item| self.declaration_still_at(item))
            .collect();
        let queries: Vec<_> = valid_pending
            .iter()
            .map(|item| crate::references::MemberDeclarationReferenceQuery {
                uri: Arc::clone(&item.uri),
                offset: item.offset,
                member: item.member,
                is_static: item.is_static,
            })
            .collect();
        let results = self.member_declaration_references_batch(&queries);

        let mut changed = false;
        for (item, locations) in valid_pending.into_iter().zip(results) {
            changed |= self.member_ref_counts.store(
                item.class_fqn,
                item.member,
                item.is_static,
                locations,
            );
        }

        let mut queue = self.member_ref_counts.pending.lock();
        for item in &pending {
            queue.remove(item);
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
    /// ask the editor to re-pull inlay hints once they land.
    ///
    /// At most one computation runs at a time. Requests that arrive while it
    /// runs join the same burst, which is drained before one editor refresh.
    /// Refreshing after every partial batch creates a feedback loop in clients
    /// that immediately re-request lenses for all open buffers.
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
                    if let Some(ref client) = backend.client {
                        if backend.supports_inlay_hint_refresh.load(Ordering::Acquire) {
                            let _ = client.inlay_hint_refresh().await;
                        }
                        if backend.supports_code_lens_refresh.load(Ordering::Acquire) {
                            let _ = client.code_lens_refresh().await;
                        }
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
}

pub(crate) fn new_member_ref_counts() -> Arc<MemberRefCounts> {
    Arc::new(MemberRefCounts::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::{
        InlayHint, InlayHintLabel, InlayHintParams, Position, Range, TextDocumentIdentifier, Url,
    };

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
    }

    fn parse(backend: &Backend, content: &str) {
        parse_extra(backend, URI, content);
    }

    fn hints_for(backend: &Backend, uri: &str, content: &str) -> Vec<InlayHint> {
        let range = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: content.lines().count() as u32,
                character: 0,
            },
        };
        backend
            .handle_inlay_hints(uri, content, range)
            .unwrap_or_default()
    }

    fn hints(backend: &Backend, content: &str) -> Vec<InlayHint> {
        hints_for(backend, URI, content)
    }

    fn count_on_line(hints: &[InlayHint], line: u32) -> Option<String> {
        hints
            .iter()
            .find(|hint| hint.position.line == line)
            .map(|hint| match &hint.label {
                InlayHintLabel::String(label) => label.clone(),
                InlayHintLabel::LabelParts(parts) => {
                    parts.iter().map(|part| part.value.as_str()).collect()
                }
            })
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
        hints_for(&backend, ORDER_URI, ORDER);

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
                .member_ref_count_cached(
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
                .member_ref_count_cached(
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
        hints(&backend, ONE_CALL);
        backend.compute_pending_member_ref_counts();
        assert_eq!(
            count_on_line(&hints(&backend, ONE_CALL), 2).as_deref(),
            Some(" 1 reference")
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

        // The count the editor already has keeps being served until the
        // new one is ready, so the annotation does not blink out.
        assert_eq!(
            count_on_line(&hints(&backend, &edited), 2).as_deref(),
            Some(" 1 reference")
        );
        assert!(backend.compute_pending_member_ref_counts());
        assert_eq!(
            count_on_line(&hints(&backend, &edited), 2).as_deref(),
            Some(" 2 references")
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
    fn an_edit_that_leaves_the_accesses_alone_recomputes_nothing() {
        let backend = Backend::new_test();
        parse(&backend, ONE_CALL);
        hints(&backend, ONE_CALL);
        backend.compute_pending_member_ref_counts();

        // The first edit is the one that records what the file's classes
        // inherit, so start measuring from the second.
        let edited = format!("{ONE_CALL}// a trailing comment\n");
        parse(&backend, &edited);
        hints(&backend, &edited);
        backend.compute_pending_member_ref_counts();

        let edited_again = format!("{edited}// another trailing comment\n");
        parse(&backend, &edited_again);
        assert_eq!(
            count_on_line(&hints(&backend, &edited_again), 2).as_deref(),
            Some(" 1 reference")
        );
        assert!(
            !backend.member_ref_counts.has_pending(),
            "an edit that touches no access should not queue a recomputation"
        );
    }

    #[tokio::test]
    async fn the_request_path_counts_off_the_request() {
        let backend = Backend::new_test();
        parse(&backend, ONE_CALL);

        let params = InlayHintParams {
            text_document: TextDocumentIdentifier {
                uri: Url::parse(URI).unwrap(),
            },
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: ONE_CALL.lines().count() as u32,
                    character: 0,
                },
            },
            work_done_progress_params: Default::default(),
        };

        let first = backend
            .inlay_hint_request(params.clone())
            .await
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            count_on_line(&first, 2),
            None,
            "the first request answers before the count is known"
        );

        for _ in 0..200 {
            if !backend.member_ref_counts.is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let second = backend
            .inlay_hint_request(params)
            .await
            .unwrap()
            .unwrap_or_default();
        assert_eq!(count_on_line(&second, 2).as_deref(), Some(" 1 reference"));
    }

    #[test]
    fn a_new_parent_class_recomputes_the_count() {
        let backend = Backend::new_test();
        let unrelated = r#"<?php
class Model {
    public function save(): void {}
}
class Order {
    public function save(): void {}
}
function persist(Order $order): void {
    $order->save();
}
"#;
        parse(&backend, unrelated);
        hints(&backend, unrelated);
        backend.compute_pending_member_ref_counts();
        assert_eq!(
            count_on_line(&hints(&backend, unrelated), 2).as_deref(),
            Some(" 0 references")
        );

        let inherited = unrelated.replace(
            "class Order {\n    public function save(): void {}",
            "class Order extends Model {",
        );
        parse(&backend, &inherited);
        hints(&backend, &inherited);
        assert!(backend.compute_pending_member_ref_counts());
        assert_eq!(
            count_on_line(&hints(&backend, &inherited), 2).as_deref(),
            Some(" 1 reference")
        );
    }

    #[test]
    fn chain_cache_does_not_leak_a_resolution_across_files() {
        let backend = Backend::new_test();

        const URI_A: &str = "file:///PenA.php";
        const URI_B: &str = "file:///PenB.php";
        const URI_CONSUMER_A: &str = "file:///ConsumerA.php";
        const URI_CONSUMER_B: &str = "file:///ConsumerB.php";

        // Two unrelated classes that happen to share a bare name and an
        // identically-shaped `make()->write()` chain, each imported under
        // that bare name by its own consumer file.
        let pen_a = r#"<?php
namespace App\A;

class Pen {
    public static function make(): self {
        return new self();
    }

    public function write(): void {}
}
"#;
        let pen_b = pen_a.replace("App\\A", "App\\B");

        let consumer_a = r#"<?php
namespace App;

use App\A\Pen;

function useA(): void {
    Pen::make()->write();
}
"#;
        let consumer_b = consumer_a
            .replace("App\\A", "App\\B")
            .replace("useA", "useB");

        parse_extra(&backend, URI_A, pen_a);
        parse_extra(&backend, URI_B, &pen_b);
        parse_extra(&backend, URI_CONSUMER_A, consumer_a);
        parse_extra(&backend, URI_CONSUMER_B, &consumer_b);

        // Queue both `write()` declarations for a count, then resolve them
        // in the same `compute_pending_member_ref_counts` pass so both
        // consumer files are scanned under one chain-cache activation —
        // the scenario where a text-only cache key leaks a resolution from
        // one file's `use` scope into the other's.
        hints_for(&backend, URI_A, pen_a);
        hints_for(&backend, URI_B, &pen_b);
        backend.compute_pending_member_ref_counts();

        assert_eq!(
            count_on_line(&hints_for(&backend, URI_A, pen_a), 8).as_deref(),
            Some(" 1 reference"),
            "App\\A\\Pen::write must only count ConsumerA's call, not ConsumerB's \
             identically-spelled `Pen::make()->write()` against `App\\B\\Pen`"
        );
        assert_eq!(
            count_on_line(&hints_for(&backend, URI_B, &pen_b), 8).as_deref(),
            Some(" 1 reference"),
            "App\\B\\Pen::write must only count ConsumerB's call"
        );
    }
}
