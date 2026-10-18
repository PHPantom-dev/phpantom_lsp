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

pub(crate) mod body_reads;
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

/// The kinds of bounded body-walking inference, each with a depth budget
/// of its own.
#[derive(Clone, Copy)]
pub(crate) enum BoundedInference {
    /// Reading a method body for the type it returns.
    BodyReturn = 0,
    /// Reading a callee body for what it leaves in a by-reference
    /// parameter.
    OutParam = 1,
}

/// How deep each [`BoundedInference`] kind is nested on this thread,
/// indexed by kind.
type InferenceDepths = [u8; 2];

thread_local! {
    /// The nesting depth of every [`BoundedInference`] kind on this thread.
    static INFERENCE_DEPTHS: std::cell::Cell<InferenceDepths> =
        const { std::cell::Cell::new([0; 2]) };

    /// Bumped whenever an inference is cut short (by a depth cap or the
    /// cycle guard) or is answered from a memo entry that was. A frame
    /// that sees it move while its own `compute` runs knows its result
    /// was shaped by a cut somewhere below it.
    static INFERENCE_CUTS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// How many inferences of `kind` are walking a body on this thread.
pub(crate) fn inference_depth(kind: BoundedInference) -> u8 {
    INFERENCE_DEPTHS.with(|d| d.get()[kind as usize])
}

fn note_inference_cut() {
    INFERENCE_CUTS.with(|c| c.set(c.get().wrapping_add(1)));
}

/// One answer in a bounded-inference memo.
#[derive(Clone)]
pub(crate) struct Memoized<V> {
    value: V,
    /// `None` when nothing below the computation was cut short, so the
    /// answer holds for every caller. Otherwise the nesting depths it was
    /// computed at: a cut-off reading only holds for a caller with no more
    /// budget than it had, i.e. one nested at least as deep in every kind.
    cut_at: Option<InferenceDepths>,
}

/// A bounded-inference memo: its answers by key.
pub(crate) type BoundedMemo<K, V> = std::collections::HashMap<K, Memoized<V>>;

impl<V> Memoized<V> {
    fn serves(&self, depths: InferenceDepths) -> bool {
        self.cut_at
            .is_none_or(|at| at.iter().zip(depths).all(|(&at, now)| now >= at))
    }
}

/// Run `compute` under the memo / depth-cap / cycle-guard protocol every
/// bounded body-walking inference in this engine follows: serve an answer
/// straight from the memo when it holds for this caller, refuse to go
/// deeper than `max_depth`, refuse to re-enter the same `visited_key`,
/// then remember what `compute` returns under `memo_key` for the next
/// caller that asks the same question.
///
/// The memo is checked before the depth cap so a deep call chain still
/// benefits from a result computed at a shallower depth. A result that a
/// cut anywhere below it shaped is remembered together with the depths it
/// was computed at, and only served to callers nested at least as deep: a
/// caller with more budget left would have read further, so it computes
/// its own answer instead of inheriting the truncated one. Each key is
/// therefore computed at most once per depth it is reached at, which keeps
/// pathological files (where the cap is hit constantly) from re-walking
/// the same bodies.
///
/// `visited_key` is typically a strict subset of `memo_key` (e.g. a
/// method identity without the call-site argument types that decide the
/// answer): re-entry has to be keyed narrower than the memo so that a
/// method calling itself with a different argument on every hop still
/// re-enters the same key and stops at the depth cap, rather than never
/// re-entering and recursing forever.
pub(crate) fn memoized_bounded_inference<MemoKey, VisitedKey, V>(
    memo: &'static std::thread::LocalKey<std::cell::RefCell<Option<BoundedMemo<MemoKey, V>>>>,
    visited: &'static std::thread::LocalKey<
        std::cell::RefCell<std::collections::HashSet<VisitedKey>>,
    >,
    kind: BoundedInference,
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
    let depths = INFERENCE_DEPTHS.with(std::cell::Cell::get);
    let memoized = memo.with(|c| {
        c.borrow()
            .as_ref()
            .and_then(|m| m.get(&memo_key))
            .filter(|entry| entry.serves(depths))
            .map(|entry| (entry.value.clone(), entry.cut_at.is_some()))
    });
    if let Some((cached, was_cut)) = memoized {
        if was_cut {
            note_inference_cut();
        }
        return cached;
    }

    let d = depths[kind as usize];
    if d >= max_depth {
        note_inference_cut();
        return V::default();
    }

    let already_visiting = visited.with(|set| !set.borrow_mut().insert(visited_key.clone()));
    if already_visiting {
        note_inference_cut();
        return V::default();
    }
    let mut nested = depths;
    nested[kind as usize] = d + 1;
    INFERENCE_DEPTHS.with(|c| c.set(nested));

    // Restores the depths and releases the key on drop, so a panic inside
    // `compute` (caught by the request handler, which keeps the thread)
    // does not leave this key answering nothing for the thread's life.
    struct Restore<VisitedKey: Eq + std::hash::Hash + 'static> {
        visited: &'static std::thread::LocalKey<
            std::cell::RefCell<std::collections::HashSet<VisitedKey>>,
        >,
        previous_depths: InferenceDepths,
        key: Option<VisitedKey>,
    }
    impl<VisitedKey: Eq + std::hash::Hash + 'static> Drop for Restore<VisitedKey> {
        fn drop(&mut self) {
            let previous = self.previous_depths;
            INFERENCE_DEPTHS.with(|c| c.set(previous));
            if let Some(key) = self.key.take() {
                self.visited.with(|set| {
                    set.borrow_mut().remove(&key);
                });
            }
        }
    }
    let restore = Restore {
        visited,
        previous_depths: depths,
        key: Some(visited_key),
    };

    let cuts_before = INFERENCE_CUTS.with(std::cell::Cell::get);
    let result = compute();
    let was_cut = INFERENCE_CUTS.with(std::cell::Cell::get) != cuts_before;

    drop(restore);
    memo.with(|c| {
        if let Some(m) = c.borrow_mut().as_mut() {
            m.insert(
                memo_key,
                Memoized {
                    value: result.clone(),
                    cut_at: was_cut.then_some(depths),
                },
            );
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
