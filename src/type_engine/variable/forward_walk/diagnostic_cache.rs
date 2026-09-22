use super::*;
use std::collections::BTreeMap;

use crate::atom::{AtomMap, atom};
use crate::types::ResolvedType;

// ─── Diagnostic scope cache ─────────────────────────────────────────────────
//
// During a diagnostic pass, `build_diagnostic_scopes` walks every
// function/method body in the file once and records a scope snapshot at
// each statement boundary.  The snapshots are stored in a thread-local
// `BTreeMap<u32, HashMap<String, Vec<ResolvedType>>>` keyed by byte
// offset.  When `resolve_variable_types` is called for a diagnostic
// member-access span, `lookup_diagnostic_scope` finds the nearest
// snapshot at-or-before the requested offset and returns the variable's
// types in O(log N) time — no backward scanning, no recursion.
//
// A member-reference search shares the cache but not the whole-file
// walk: it fills the same map through
// `build_diagnostic_scopes_for_offsets` for the bodies holding the
// accesses it asked about, and `ScopeCoverage` below records which those
// were, so a lookup elsewhere in the file is not answered from the
// nearest body the walk happened to reach.

/// Scope snapshot map: byte offset → variable name → resolved types.
pub(crate) type ScopeSnapshotMap = BTreeMap<u32, AtomMap<Vec<ResolvedType>>>;

/// Which part of the file the snapshots in [`DIAGNOSTIC_SCOPE`] describe.
///
/// A lookup answers from the nearest snapshot at-or-before the requested
/// offset, which is only meaningful if the walk that recorded it also
/// walked the offset being asked about.  After a whole-file walk that is
/// every offset.  A targeted walk
/// (`build_diagnostic_scopes_for_offsets`) walks only the bodies that
/// enclose the offsets it was given, so for an offset it skipped the
/// nearest preceding snapshot belongs to an unrelated function and must
/// not answer for it.
pub(crate) enum ScopeCoverage {
    /// Every offset in the file was walked.
    Whole,
    /// Only these byte ranges were walked.
    Regions(Vec<(u32, u32)>),
}

thread_local! {
    /// When `Some`, `lookup_diagnostic_scope` will consult this map.
    /// Activated by [`with_diagnostic_scope_cache`], cleared on guard
    /// drop.
    pub(crate) static DIAGNOSTIC_SCOPE: RefCell<Option<ScopeSnapshotMap>> =
        const { RefCell::new(None) };

    /// What [`DIAGNOSTIC_SCOPE`] is authoritative for.  Reset alongside
    /// it by [`with_diagnostic_scope_cache`] and its guard.
    static SCOPE_COVERAGE: RefCell<ScopeCoverage> =
        const { RefCell::new(ScopeCoverage::Whole) };

    /// Set to `true` while `build_diagnostic_scopes` is populating the
    /// scope cache.  Code that would normally read from the cache should
    /// skip the lookup when this flag is set, because the cache is
    /// incomplete and may contain stale data from earlier offsets.
    pub(crate) static BUILDING_SCOPES: Cell<bool> = const { Cell::new(false) };
}

/// Whether the active snapshots were built by a walk that covered
/// `offset`.  Always true after a whole-file walk.
pub(crate) fn scope_snapshots_cover(offset: u32) -> bool {
    SCOPE_COVERAGE.with(|cell| match &*cell.borrow() {
        ScopeCoverage::Whole => true,
        ScopeCoverage::Regions(regions) => regions
            .iter()
            .any(|&(start, end)| (start..=end).contains(&offset)),
    })
}

/// Whether the active snapshots came from a whole-file walk.
pub(crate) fn scope_coverage_is_whole() -> bool {
    SCOPE_COVERAGE.with(|cell| matches!(&*cell.borrow(), ScopeCoverage::Whole))
}

/// Declare that the snapshots about to be recorded describe only the
/// regions passed to [`record_covered_region`].  A no-op when a targeted
/// walk already narrowed the coverage, so a second targeted walk adds to
/// what the first one covered instead of discarding it.
pub(crate) fn begin_targeted_coverage() {
    SCOPE_COVERAGE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        if matches!(&*borrow, ScopeCoverage::Whole) {
            *borrow = ScopeCoverage::Regions(Vec::new());
        }
    });
}

/// Declare that the snapshots describe every offset in the file.
pub(crate) fn set_whole_scope_coverage() {
    SCOPE_COVERAGE.with(|cell| {
        *cell.borrow_mut() = ScopeCoverage::Whole;
    });
}

/// Record that a targeted walk walked `start..=end`.
pub(crate) fn record_covered_region(start: u32, end: u32) {
    SCOPE_COVERAGE.with(|cell| {
        if let ScopeCoverage::Regions(ref mut regions) = *cell.borrow_mut() {
            regions.push((start, end));
        }
    });
}

/// RAII guard that clears the diagnostic scope cache on drop.
pub(crate) struct DiagnosticScopeGuard {
    owns: bool,
}

impl Drop for DiagnosticScopeGuard {
    fn drop(&mut self) {
        if self.owns {
            DIAGNOSTIC_SCOPE.with(|cell| {
                *cell.borrow_mut() = None;
            });
            set_whole_scope_coverage();
            end_unreachable_collection();
        }
    }
}

/// RAII guard that resets [`BUILDING_SCOPES`] to `false` on drop.
pub(crate) struct BuildingScopesGuard;

impl Drop for BuildingScopesGuard {
    fn drop(&mut self) {
        BUILDING_SCOPES.with(|cell: &Cell<bool>| cell.set(false));
    }
}

/// Returns `true` while `build_diagnostic_scopes` is populating the
/// scope cache.
pub(crate) fn is_building_scopes() -> bool {
    BUILDING_SCOPES.with(|cell: &Cell<bool>| cell.get())
}

/// Activate the thread-local diagnostic scope cache.
///
/// Returns a guard that clears the cache on drop.  If the cache is
/// already active (nested call), the guard is a no-op.
///
/// The walk that populates the cache is also the walk that discovers
/// which branches cannot run, so the collection of unreachable ranges
/// shares this guard's lifetime: the ranges are still there when the
/// collectors that ran against the cache hand back their diagnostics.
pub(crate) fn with_diagnostic_scope_cache() -> DiagnosticScopeGuard {
    let already_active = DIAGNOSTIC_SCOPE.with(|cell| cell.borrow().is_some());
    if already_active {
        return DiagnosticScopeGuard { owns: false };
    }
    DIAGNOSTIC_SCOPE.with(|cell| {
        *cell.borrow_mut() = Some(BTreeMap::new());
    });
    set_whole_scope_coverage();
    begin_unreachable_collection();
    DiagnosticScopeGuard { owns: true }
}

/// Look up a variable's types from the diagnostic scope cache.
///
/// Finds the scope snapshot at the largest offset that is ≤ `offset`,
/// then returns the variable's types from that snapshot.  Returns
/// `None` when the cache is not active or no snapshot covers the
/// requested offset.
pub(crate) fn lookup_diagnostic_scope(var_name: &str, offset: u32) -> Option<Vec<ResolvedType>> {
    if !scope_snapshots_cover(offset) {
        return None;
    }
    DIAGNOSTIC_SCOPE.with(|cell| {
        let borrow = cell.borrow();
        let map = borrow.as_ref()?;
        let (_snap_offset, snap) = map.range(..=offset).next_back()?;
        // If the snapshot exists but the variable is absent, the
        // forward walker has already walked this scope region and
        // determined the variable has no known type here.  Return
        // empty rather than `None` so the caller treats the variable
        // as unresolved at this position.
        let result = snap.get(&atom(var_name)).cloned().unwrap_or_default();
        Some(result)
    })
}

/// Check whether the diagnostic scope cache is currently active.
pub(crate) fn is_diagnostic_scope_active() -> bool {
    DIAGNOSTIC_SCOPE.with(|cell| cell.borrow().is_some())
}

/// Puts back the snapshots [`suspend_diagnostic_scope`] set aside.
pub(crate) struct DiagnosticScopeSuspendGuard(Option<ScopeSnapshotMap>);

impl Drop for DiagnosticScopeSuspendGuard {
    fn drop(&mut self) {
        DIAGNOSTIC_SCOPE.with(|cell| {
            *cell.borrow_mut() = self.0.take();
        });
    }
}

/// Deactivate the diagnostic scope cache until the returned guard drops.
///
/// The snapshots describe each body as its own declarations define it.  A
/// body being read for its return type with its parameters seeded from a
/// call site is a different scope at the very same offsets, so serving it
/// from the snapshots would answer the question that was not asked.
pub(crate) fn suspend_diagnostic_scope() -> DiagnosticScopeSuspendGuard {
    DiagnosticScopeSuspendGuard(DIAGNOSTIC_SCOPE.with(|cell| cell.borrow_mut().take()))
}

/// Insert a scope snapshot into the diagnostic scope cache at the given
/// byte offset.
pub(crate) fn record_scope_snapshot(offset: u32, scope: &ScopeState) {
    // Skip recording while a nested variable-resolution walk is in
    // progress.  Those walks (see [`suspend_snapshot_recording`]) spin up
    // their own fresh scope to answer a single "what is this variable's
    // type?" query and must not overwrite the authoritative snapshots
    // built by the dedicated diagnostic-scope walk.  Their statement
    // offsets can even come from a different file (e.g. a return-type
    // inference walking the callee's body) and would otherwise collide
    // with the outer file's offsets in the shared map.
    if SUSPEND_SNAPSHOT.with(|c| c.get()) > 0 {
        return;
    }
    DIAGNOSTIC_SCOPE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        if let Some(ref mut map) = *borrow {
            map.insert(offset, scope.locals.clone());
        }
    });
}

thread_local! {
    /// Non-zero while a nested variable-resolution walk is running.
    /// Consulted by [`record_scope_snapshot`] to suppress snapshot
    /// writes that would pollute the authoritative diagnostic scope
    /// cache.  A counter (rather than a bool) so nested resolution
    /// walks compose correctly.
    static SUSPEND_SNAPSHOT: Cell<u32> = const { Cell::new(0) };
}

/// RAII guard that decrements the [`SUSPEND_SNAPSHOT`] counter on drop.
pub(crate) struct SnapshotSuspendGuard;

impl Drop for SnapshotSuspendGuard {
    fn drop(&mut self) {
        SUSPEND_SNAPSHOT.with(|c| c.set(c.get().saturating_sub(1)));
    }
}

/// Suspend diagnostic scope snapshot recording for the lifetime of the
/// returned guard.
///
/// The dedicated diagnostic-scope walk ([`build_diagnostic_scopes`])
/// resolves assignment right-hand sides and method return types as it
/// goes.  That resolution can re-enter the forward walker
/// ([`resolve_in_method_body`], [`resolve_in_top_level`], etc.) to look
/// up a variable's type, which walks a body with a fresh scope.  Without
/// this guard those nested walks would record their own snapshots into
/// the active cache, clobbering the outer scope (dropping variables like
/// a query builder assigned earlier in the method) and producing
/// false-positive "type could not be resolved" diagnostics on some
/// call-chain branches but not others.
pub(crate) fn suspend_snapshot_recording() -> SnapshotSuspendGuard {
    SUSPEND_SNAPSHOT.with(|c| c.set(c.get() + 1));
    SnapshotSuspendGuard
}
