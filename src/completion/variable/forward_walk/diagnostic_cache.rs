use super::*;
use std::collections::{BTreeMap, HashMap};

use crate::atom::{AtomMap, atom};
use crate::types::ResolvedType;

// ─── Hover scope cache (Phase 3) ────────────────────────────────────────────
//
// When multiple hover requests arrive for the same file content (e.g. a
// test file with 80+ `assertType()` calls), each request would otherwise
// trigger a full forward walk of the method body from statement 1 to the
// cursor position.  That produces O(n²) total work.
//
// The hover scope cache amortises this to O(n) per method body: the first
// hover that hits a given method walks the **full** body once (cursor at
// u32::MAX) and stores the resulting `ScopeSnapshotMap`.  Subsequent
// hovers on the same file content look up the pre-computed snapshots in
// O(log N) time via a `BTreeMap::range` search — no re-walk at all.
//
// Cache invalidation: the key is a 64-bit FNV-1a hash of the full content
// string.  This is robust against memory reuse (two different test contents
// that happen to land at the same address would produce different hashes),
// while remaining cheap to compute (single pass, no allocation).
//
// The hover cache must not interfere with the diagnostic scope cache:
// - It is only consulted / populated when `is_diagnostic_scope_active()`
//   returns `false`.
// - `build_diagnostic_scopes` never touches `HOVER_SCOPE_CACHE`.

pub(crate) struct HoverScopeCache {
    /// FNV-1a hash of the content string used to build this cache.
    /// When the content changes (different hash), the cache is reset.
    content_hash: u64,
    /// method_span_start → full-body scope snapshot map.
    methods: HashMap<u32, ScopeSnapshotMap>,
}

/// Compute a fast 64-bit FNV-1a hash of a byte slice.
pub(crate) fn fnv1a_hash(data: &[u8]) -> u64 {
    const OFFSET: u64 = 14695981039346656037;
    const PRIME: u64 = 1099511628211;
    let mut hash = OFFSET;
    for &b in data {
        hash ^= b as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

thread_local! {
    static HOVER_SCOPE_CACHE: RefCell<Option<HoverScopeCache>> =
        const { RefCell::new(None) };
}

/// Ensure the hover scope cache is active and valid for `content`.
///
/// If the cache already exists for the same content hash, this is a no-op
/// (the existing snapshots remain valid).  If the hash changed (content was
/// replaced), the cache is reset to empty so stale snapshots from the
/// previous content are discarded.
///
/// This must not be called while a diagnostic scope pass is in progress.
pub(crate) fn activate_hover_scope_cache(content: &str) {
    let content_hash = fnv1a_hash(content.as_bytes());
    HOVER_SCOPE_CACHE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        match borrow.as_ref() {
            Some(cache) if cache.content_hash == content_hash => {
                // Cache is already valid for this content — nothing to do.
            }
            _ => {
                *borrow = Some(HoverScopeCache {
                    content_hash,
                    methods: HashMap::new(),
                });
            }
        }
    });
}

/// Returns `true` when the hover scope cache is active.
pub(crate) fn is_hover_scope_cache_active() -> bool {
    HOVER_SCOPE_CACHE.with(|cell| cell.borrow().is_some())
}

/// Returns `true` when the hover scope cache already has a snapshot map
/// for the given method body.
pub(crate) fn hover_scope_has_method(method_span_start: u32) -> bool {
    HOVER_SCOPE_CACHE.with(|cell| {
        let borrow = cell.borrow();
        borrow
            .as_ref()
            .is_some_and(|c| c.methods.contains_key(&method_span_start))
    })
}

/// Store a complete scope snapshot map for a method body in the hover
/// scope cache.
pub(crate) fn populate_hover_scope_cache_for_method(
    method_span_start: u32,
    snapshots: ScopeSnapshotMap,
) {
    HOVER_SCOPE_CACHE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        if let Some(ref mut cache) = *borrow {
            cache.methods.insert(method_span_start, snapshots);
        }
    });
}

/// Extract and return the current contents of the diagnostic scope cache,
/// replacing it with an empty map.
///
/// Used by `build_method_snapshots_via_diag_cache` to harvest the
/// snapshots that were recorded by a temporary diagnostic-scope walk.
pub(crate) fn take_diagnostic_scope_map() -> ScopeSnapshotMap {
    DIAGNOSTIC_SCOPE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        match borrow.as_mut() {
            Some(map) => std::mem::take(map),
            None => BTreeMap::new(),
        }
    })
}

// ─── Diagnostic scope cache (Phase 2) ───────────────────────────────────────
//
// During a diagnostic pass, `build_diagnostic_scopes` walks every
// function/method body in the file once and records a scope snapshot at
// each statement boundary.  The snapshots are stored in a thread-local
// `BTreeMap<u32, HashMap<String, Vec<ResolvedType>>>` keyed by byte
// offset.  When `resolve_variable_types` is called for a diagnostic
// member-access span, `lookup_diagnostic_scope` finds the nearest
// snapshot at-or-before the requested offset and returns the variable's
// types in O(log N) time — no backward scanning, no recursion.

/// Scope snapshot map: byte offset → variable name → resolved types.
pub(crate) type ScopeSnapshotMap = BTreeMap<u32, AtomMap<Vec<ResolvedType>>>;

thread_local! {
    /// When `Some`, `lookup_diagnostic_scope` will consult this map.
    /// Activated by [`with_diagnostic_scope_cache`], cleared on guard
    /// drop.
    pub(crate) static DIAGNOSTIC_SCOPE: RefCell<Option<ScopeSnapshotMap>> =
        const { RefCell::new(None) };

    /// Set to `true` while `build_diagnostic_scopes` is populating the
    /// scope cache.  Code that would normally read from the cache should
    /// skip the lookup when this flag is set, because the cache is
    /// incomplete and may contain stale data from earlier offsets.
    pub(crate) static BUILDING_SCOPES: Cell<bool> = const { Cell::new(false) };
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
pub(crate) fn with_diagnostic_scope_cache() -> DiagnosticScopeGuard {
    let already_active = DIAGNOSTIC_SCOPE.with(|cell| cell.borrow().is_some());
    if already_active {
        return DiagnosticScopeGuard { owns: false };
    }
    DIAGNOSTIC_SCOPE.with(|cell| {
        *cell.borrow_mut() = Some(BTreeMap::new());
    });
    DiagnosticScopeGuard { owns: true }
}

/// Look up a variable's types from the diagnostic scope cache.
///
/// Finds the scope snapshot at the largest offset that is ≤ `offset`,
/// then returns the variable's types from that snapshot.  Returns
/// `None` when the cache is not active or no snapshot covers the
/// requested offset.
pub(crate) fn lookup_diagnostic_scope(var_name: &str, offset: u32) -> Option<Vec<ResolvedType>> {
    DIAGNOSTIC_SCOPE.with(|cell| {
        let borrow = cell.borrow();
        let map = borrow.as_ref()?;
        // Find the snapshot at-or-before `offset`.
        let (_snap_offset, snap) = map.range(..=offset).next_back()?;
        // If the variable is in the snapshot, return its types.
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

/// RAII guard that restores the saved [`SUSPEND_SNAPSHOT`] value on drop.
pub(crate) struct SnapshotResumeGuard(u32);

impl Drop for SnapshotResumeGuard {
    fn drop(&mut self) {
        SUSPEND_SNAPSHOT.with(|c| c.set(self.0));
    }
}

/// Temporarily clear any active snapshot suspension for the lifetime of
/// the returned guard, restoring it on drop.
///
/// Use this around a *dedicated* scope-building walk (e.g. the hover
/// scope cache population in [`resolve_in_method_body`]) that must record
/// snapshots even when it happens to run inside a nested variable
/// resolution that suspended recording via [`suspend_snapshot_recording`].
pub(crate) fn resume_snapshot_recording() -> SnapshotResumeGuard {
    let prev = SUSPEND_SNAPSHOT.with(|c| {
        let v = c.get();
        c.set(0);
        v
    });
    SnapshotResumeGuard(prev)
}
