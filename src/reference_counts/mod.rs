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

use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use tower_lsp::lsp_types::Location;

use crate::Backend;
use crate::atom::Atom;
use crate::class_lookup::find_class_at_offset;

mod cache;

use cache::{CompactLocation, EDIT_PAUSE, PendingCount};
pub(crate) use cache::{MemberRefCounts, new_member_ref_counts};

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
        // This runs on its own blocking thread rather than under an LSP
        // request, so it has to hand the workspace's resolved classes to the
        // search itself; see `Backend::with_file_content`.
        let _resolved_classes_guard =
            crate::virtual_members::with_active_resolved_class_cache(&self.resolved_class_cache);

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
                Some(true) => backend.request_code_lens_refresh().await,
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

/// Unit tests for the count cache itself.
///
/// These stay in the crate because they assert on state the public API
/// deliberately does not expose: staleness epochs, the bounded exact
/// location store, and which scope snapshots a batch of counts reused.
/// The tests that only drive a `Backend` through the lens handlers live in
/// `tests/integration/code_lens.rs`.
#[cfg(test)]
mod tests;
