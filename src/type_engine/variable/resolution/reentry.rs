//! Re-entry guards for variable resolution, and the key that identifies
//! one variable query.

use std::cell::RefCell;
use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::atom::Atom;

// Variable resolution can re-enter itself while building the top-level
// scope for `global` keyword resolution.  The cycle is:
//
//   resolve_variable_types → resolve_variable_in_statements
//   → top_level_scope_for_globals → RHS resolution → resolve_variable_types
//
// Two guards break this cycle at different levels:
//
// Guard 1 (`BUILDING_TOP_LEVEL_SCOPE`): prevents re-entrant top-level
//   scope construction for the same file.
// Guard 2 (`RESOLVING_VARS`): prevents re-entrant resolution of the
//   exact same variable query.

thread_local! {
    /// Source content addresses currently building a top-level scope
    /// via [`top_level_scope_for_globals`](super::super::forward_walk::top_level_scope_for_globals).
    /// Keyed by `content.as_ptr() as usize`: re-entry within one call
    /// tree always borrows the same slice, so pointer identity marks
    /// exactly the cycle we want to break.  Two independent copies of
    /// the same text are separate queries and must not block one
    /// another, which pointer identity also gets right.
    static BUILDING_TOP_LEVEL_SCOPE: RefCell<HashSet<usize>> =
        RefCell::new(HashSet::new());

    /// Variable resolution queries currently in progress on this
    /// thread.  Keyed by [`VarQueryKey`] so that only the exact same
    /// query is suppressed.
    static RESOLVING_VARS: RefCell<HashSet<VarQueryKey>> =
        RefCell::new(HashSet::new());

    #[cfg(test)]
    pub(super) static TEST_SCOPE_CACHE_HITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_test_scope_cache_hits() {
    TEST_SCOPE_CACHE_HITS.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn test_scope_cache_hits() -> usize {
    TEST_SCOPE_CACHE_HITS.with(std::cell::Cell::get)
}

/// What identifies one "what is the type of `$var` here?" question: the
/// source it is asked of, the hash of the un-prefixed variable name, the
/// offset, the class the question is asked in, and the
/// [body-inference context](crate::type_engine::call_resolution::body_inference_context)
/// it is asked under.
///
/// The context is there because a body walked for its return type seeds
/// its parameters with what the call site passed, so the same variable at
/// the same offset answers differently for each caller.
type VarQueryKey = ((usize, usize), u64, u32, Atom, u64);

/// Build the key identifying one variable query.
///
/// Callers spell the same variable both with and without the `$`
/// prefix, so the name is normalised before hashing.  It is hashed
/// rather than interned: this runs on every variable resolution, and
/// `atom` would take the global interner lock and allocate for the
/// unprefixed spelling.
pub(super) fn var_query_key(
    content: &str,
    var_name: &str,
    cursor_offset: u32,
    class_name: Atom,
) -> VarQueryKey {
    let mut hasher = DefaultHasher::new();
    var_name
        .strip_prefix('$')
        .unwrap_or(var_name)
        .hash(&mut hasher);
    (
        (content.as_ptr() as usize, content.len()),
        hasher.finish(),
        cursor_offset,
        class_name,
        crate::type_engine::call_resolution::body_inference_context(),
    )
}

/// RAII guard for [`BUILDING_TOP_LEVEL_SCOPE`].
pub(super) struct TopLevelScopeGuard {
    key: usize,
}

impl Drop for TopLevelScopeGuard {
    fn drop(&mut self) {
        BUILDING_TOP_LEVEL_SCOPE.with(|set| {
            set.borrow_mut().remove(&self.key);
        });
    }
}

/// Try to acquire the top-level scope build guard for `content`.
/// Returns `Some(guard)` on success, `None` when the same file is
/// already mid-construction (re-entry detected).
pub(super) fn try_acquire_top_level_guard(content: &str) -> Option<TopLevelScopeGuard> {
    let key = content.as_ptr() as usize;
    let inserted = BUILDING_TOP_LEVEL_SCOPE.with(|set| set.borrow_mut().insert(key));
    if inserted {
        Some(TopLevelScopeGuard { key })
    } else {
        None
    }
}

/// RAII guard for [`RESOLVING_VARS`].
pub(super) struct ResolvingVarGuard {
    key: VarQueryKey,
}

impl Drop for ResolvingVarGuard {
    fn drop(&mut self) {
        RESOLVING_VARS.with(|set| {
            set.borrow_mut().remove(&self.key);
        });
    }
}

/// Try to acquire the variable resolution guard for `key`.
/// Returns `Some(guard)` on success, `None` on re-entry.
///
/// Only the queries on the current stack (a handful) share the set, so
/// the hashed name in the key cannot realistically collide.
pub(super) fn try_acquire_var_guard(key: VarQueryKey) -> Option<ResolvingVarGuard> {
    let inserted = RESOLVING_VARS.with(|set| set.borrow_mut().insert(key));
    if inserted {
        Some(ResolvingVarGuard { key })
    } else {
        None
    }
}
