//! The shared type-resolution engine.
//!
//! This is the project's single type-resolution engine — the code that
//! answers "what is the type of this expression here?" It is consumed by
//! diagnostics, hover, go-to-definition, and signature help, not just
//! completion.
//!
//! ## Top-level modules
//!
//! - **resolver**: Resolving a subject expression to a concrete class type
//! - **call_resolution**: Call expression and callable target resolution (method
//!   calls, static calls, function calls, constructor calls, signature help,
//!   named-argument completion)
//! - **subject_expr / subject_extraction / subject_resolution**: Extracting and
//!   resolving the left-hand side of `->`, `?->`, and `::` operators
//!
//! ### `types/` — Type resolution
//!
//! - **resolution**: Type-hint string to `ClassInfo` mapping (unions,
//!   intersections, generics, type aliases, object shapes, property types)
//! - **narrowing**: instanceof / assert / custom type guard narrowing
//! - **conditional**: PHPStan conditional return type resolution at call sites
//!
//! ### `variable/` — Variable type resolution
//!
//! - **resolution**: Variable type resolution via assignment scanning
//! - **rhs_resolution**: Right-hand-side expression resolution for variable
//!   assignments (instantiation, array access, function/method/static calls,
//!   property access, match, ternary, clone)
//! - **forward_walk**: The forward walker shared by diagnostics, completion,
//!   hover, go-to-definition, and signature help
//! - **class_string_resolution**: Class-string variable resolution (`$cls = User::class`)
//! - **raw_type_inference**: Raw type inference for variable assignments (array shapes,
//!   array functions, generator yields)
//! - **foreach_resolution**: Foreach value/key and array destructuring type resolution
//! - **closure_resolution**: Closure and arrow-function parameter resolution

pub(crate) mod call_resolution;
pub(crate) mod regex_shape;
pub(crate) mod resolver;
pub mod subject_expr;
pub(crate) mod subject_extraction;
pub(crate) mod subject_resolution;
pub(crate) mod trait_context;
pub mod types;
pub(crate) mod variable;

// ─── Pass-scoped memos ──────────────────────────────────────────────────────

/// Clears a pass-scoped thread-local memo when the pass that installed
/// it ends.
///
/// A nested activation gets a guard that owns nothing and clears nothing,
/// so an inner pass cannot discard the entries an outer one is still
/// relying on.
pub(crate) struct MemoGuard<T: 'static> {
    /// The memo to clear on drop, or `None` for a nested activation.
    cell: Option<&'static std::thread::LocalKey<std::cell::RefCell<Option<T>>>>,
}

impl<T: 'static> Drop for MemoGuard<T> {
    fn drop(&mut self) {
        if let Some(cell) = self.cell {
            cell.with(|c| *c.borrow_mut() = None);
        }
    }
}

/// Activate the pass-scoped memo held in `cell` for the current thread.
///
/// The memo is filled only by the outermost activation, which is the one
/// whose [`MemoGuard`] clears it again; see there for why nesting is a
/// no-op.
pub(crate) fn activate_memo<T: Default + 'static>(
    cell: &'static std::thread::LocalKey<std::cell::RefCell<Option<T>>>,
) -> MemoGuard<T> {
    if cell.with(|c| c.borrow().is_some()) {
        return MemoGuard { cell: None };
    }
    cell.with(|c| *c.borrow_mut() = Some(T::default()));
    MemoGuard { cell: Some(cell) }
}

// ─── Bounded, re-entrant, memoized inference ────────────────────────────────

/// Run `compute` under the memo / depth-cap / cycle-guard protocol every
/// bounded body-walking inference in this engine follows: serve a
/// completed answer straight from the memo, refuse to go deeper than
/// `max_depth`, refuse to re-enter the same `visited_key`, then remember
/// what `compute` returns under `memo_key` for the next caller that asks
/// the same question.
///
/// The memo is checked before the depth cap so a deep call chain still
/// benefits from a result computed at a shallower depth. Only a
/// *completed* run is memoized — the cap and the cycle guard both return
/// early without storing, so a cut-off `V::default()` can never shadow a
/// later caller's own unbounded reading of the same key.
///
/// `visited_key` is typically a strict subset of `memo_key` (e.g. a
/// method identity without the call-site argument types that decide the
/// answer): re-entry has to be keyed narrower than the memo so that a
/// method calling itself with a different argument on every hop still
/// re-enters the same key and stops at the depth cap, rather than never
/// re-entering and recursing forever.
#[allow(clippy::too_many_arguments)]
pub(crate) fn memoized_bounded_inference<MemoKey, VisitedKey, V>(
    memo: &'static std::thread::LocalKey<
        std::cell::RefCell<Option<std::collections::HashMap<MemoKey, V>>>,
    >,
    visited: &'static std::thread::LocalKey<
        std::cell::RefCell<std::collections::HashSet<VisitedKey>>,
    >,
    depth: &'static std::thread::LocalKey<std::cell::Cell<u8>>,
    max_depth: u8,
    memo_key: MemoKey,
    visited_key: VisitedKey,
    compute: impl FnOnce() -> V,
) -> V
where
    MemoKey: Eq + std::hash::Hash,
    VisitedKey: Eq + std::hash::Hash + Clone,
    V: Clone + Default,
{
    let memoized = memo.with(|c| c.borrow().as_ref().and_then(|m| m.get(&memo_key).cloned()));
    if let Some(cached) = memoized {
        return cached;
    }

    let d = depth.with(std::cell::Cell::get);
    if d >= max_depth {
        return V::default();
    }

    let already_visiting = visited.with(|set| !set.borrow_mut().insert(visited_key.clone()));
    if already_visiting {
        return V::default();
    }
    depth.with(|c| c.set(d + 1));

    let result = compute();

    depth.with(|c| c.set(d));
    visited.with(|set| {
        set.borrow_mut().remove(&visited_key);
    });
    memo.with(|c| {
        if let Some(m) = c.borrow_mut().as_mut() {
            m.insert(memo_key, result.clone());
        }
    });
    result
}

// ─── Re-exports ─────────────────────────────────────────────────────────────
//
// These preserve the `conditional_resolution` / `type_resolution` aliases used
// throughout the codebase.

pub use types::conditional as conditional_resolution;
pub(crate) use types::resolution as type_resolution;
