//! Per-thread memos for [`Backend::find_or_load_class_typed`]: one for the
//! name search, one for the framework refinements applied to what it found.
//!
//! [`Backend`]: crate::Backend
//!
//! The diagnostic pass asks for the same classes over and over: one
//! `analyze` run on a large Laravel project makes ~3.7 M name → class
//! lookups over a few thousand distinct types. Every one of them walked the
//! multi-phase loader in `resolution/`, hashing the name
//! case-insensitively for a read lock on `class_not_found_cache` and
//! another on `fqn_class_index`. With one worker per core those two lock
//! words were the pass's ceiling: the loader and the lock and hash code
//! under it were ~22% of its samples, ~7% of that spent purely acquiring
//! the read locks.
//!
//! Interned [`PhpType`] handles are a far cheaper key than the name: one
//! multiply to index a slot, then pointer equality. A repeat lookup
//! touches nothing but this thread's own table.
//!
//! # Freshness
//!
//! A slot is served only while its stamp matches
//! [`SymbolIndex::class_lookup_generation`], which
//! [`SymbolIndex::note_class_lookup_change`] bumps on every mutation of
//! `fqn_class_index` and every clear of `class_not_found_cache`. Those are
//! the two structures a loader answer is derived from, so a memoised
//! answer is never staler than the caches it came from. Retired slots are
//! overwritten in place rather than swept, so invalidation costs nothing.
//!
//! [`SymbolIndex::class_lookup_generation`]: crate::symbol_index::SymbolIndex::class_lookup_generation
//! [`SymbolIndex::note_class_lookup_change`]: crate::symbol_index::SymbolIndex::note_class_lookup_change
//!
//! # Why a fixed table
//!
//! Memory is bounded with no eviction policy to tune: `SLOTS` entries per
//! thread that ever looks a class up, holding one type handle and one
//! `Arc<ClassInfo>` each — both already kept alive by the interner and the
//! class index. A long-lived server cannot grow it, and a collision costs
//! one recomputation.
//!
//! # Why per-thread
//!
//! A shared table would reintroduce the lock this memo exists to avoid.
//! Threads are not the unit of ownership, though, so each slot records
//! which [`SymbolIndex`] produced it: a thread reused across `Backend`s
//! (nextest runs many in one process) cannot be served another project's
//! classes.
//!
//! [`SymbolIndex`]: crate::symbol_index::SymbolIndex

use std::cell::RefCell;
use std::sync::Arc;

use crate::php_type::PhpType;
use crate::types::ClassInfo;

/// Slots per thread, a power of two so the index is a mask.
///
/// 8 KB per thread at 32 bytes a slot. Measured against 1024 on two large
/// Laravel projects: the smaller table won slightly on both, so the working
/// set of one file's diagnostics fits and the locality of a table that
/// stays in L1 is worth more than the collisions avoided by a larger one.
const SLOTS: usize = 256;

/// One memoised answer.
struct Slot {
    /// The type this answer is for, or `None` in a slot never written.
    ///
    /// Held as a handle rather than as the address it hashes to: dropping
    /// the last handle to a type frees the interned node, and a later type
    /// could be allocated at the same address and match a stale key.
    key: Option<PhpType>,
    /// The loader's answer, itself `None` for "no such class".
    class: Option<Arc<ClassInfo>>,
    /// Identity of the index that produced `class`.
    owner: u64,
    /// Index generation `class` was looked up at.
    generation: u64,
}

impl Slot {
    const fn empty() -> Slot {
        Slot {
            key: None,
            class: None,
            owner: 0,
            generation: 0,
        }
    }
}

thread_local! {
    /// Allocated on the first lookup, so threads that never resolve a
    /// class pay nothing for it.
    static TABLE: RefCell<Option<Box<[Slot]>>> = const { RefCell::new(None) };
}

/// Slot for `ty`: Fibonacci hashing of the node address, whose low bits
/// are always zero from the allocator's alignment.
#[inline]
fn slot_of(ty: &PhpType) -> usize {
    const PHI: u64 = 0x9e37_79b9_7f4a_7c15;
    ((ty.identity() as u64).wrapping_mul(PHI) >> (64 - SLOTS.trailing_zeros())) as usize
}

/// The memoised answer for `ty`, or `None` when there is none and the
/// caller must run the loader and report back to [`store`].
///
/// The outer `Option` distinguishes "not memoised" from a memoised
/// "no such class"; both are worth caching, since a negative answer costs
/// the same multi-phase walk as a positive one.
pub(crate) fn probe(owner: u64, generation: u64, ty: &PhpType) -> Option<Option<Arc<ClassInfo>>> {
    TABLE.with(|table| {
        let table = table.borrow();
        let slot = &table.as_ref()?[slot_of(ty)];
        if slot.owner != owner || slot.generation != generation {
            return None;
        }
        // Pointer equality: a different type hashing to this slot is a
        // miss, not a wrong answer.
        (slot.key.as_ref()? == ty).then(|| slot.class.clone())
    })
}

/// Record `class` as the answer for `ty`, replacing whatever occupied the
/// slot.
///
/// Called only after [`probe`] has returned, never with the loader still
/// running: loading a class parses a file, which resolves further classes
/// and re-enters this table.
pub(crate) fn store(owner: u64, generation: u64, ty: &PhpType, class: &Option<Arc<ClassInfo>>) {
    TABLE.with(|table| {
        let mut table = table.borrow_mut();
        let table = table
            .get_or_insert_with(|| (0..SLOTS).map(|_| Slot::empty()).collect::<Vec<_>>().into());
        table[slot_of(ty)] = Slot {
            key: Some(ty.clone()),
            class: class.clone(),
            owner,
            generation,
        };
    });
}

/// One memoised refinement: the class as linked, and the class every
/// framework hook made of it.
struct RefinedSlot {
    /// `(linked, refined)`, or `None` in a slot never written.
    ///
    /// The linked class is held rather than its address for the same reason
    /// [`Slot::key`] is: a freed class's address can be reused.
    entry: Option<(Arc<ClassInfo>, Arc<ClassInfo>)>,
    owner: u64,
    /// Refinement generation `entry` was computed at.
    generation: u64,
}

thread_local! {
    /// The refinements the class loader runs after the name search, keyed
    /// on the class they refine.
    ///
    /// A refinement that changes a class copies it into a new `Arc`, and
    /// most lookups change nothing yet still pay for checking.  Handing back
    /// the same `Arc` on a repeat also keeps the pointer-identity shortcuts
    /// downstream (the scope merge) working for refined classes.  Fresh
    /// only while the hooks' combined generation is unchanged; the class
    /// being refined is the key, so a re-indexed class misses on its own.
    static REFINED: RefCell<Option<Box<[RefinedSlot]>>> = const { RefCell::new(None) };
}

#[inline]
fn refined_slot_of(class: &Arc<ClassInfo>) -> usize {
    const PHI: u64 = 0x9e37_79b9_7f4a_7c15;
    ((Arc::as_ptr(class) as usize as u64).wrapping_mul(PHI) >> (64 - SLOTS.trailing_zeros()))
        as usize
}

/// The memoised refinement of `class`, or `None` when the caller must run
/// the hooks and report back to [`store_refined`].
pub(crate) fn probe_refined(
    owner: u64,
    generation: u64,
    class: &Arc<ClassInfo>,
) -> Option<Arc<ClassInfo>> {
    REFINED.with(|table| {
        let table = table.borrow();
        let slot = &table.as_ref()?[refined_slot_of(class)];
        if slot.owner != owner || slot.generation != generation {
            return None;
        }
        let (linked, refined) = slot.entry.as_ref()?;
        Arc::ptr_eq(linked, class).then(|| Arc::clone(refined))
    })
}

/// Record `refined` as what the hooks made of `class`.
///
/// Like [`store`], called only once the hooks have returned: they load
/// classes themselves.
pub(crate) fn store_refined(
    owner: u64,
    generation: u64,
    class: &Arc<ClassInfo>,
    refined: &Arc<ClassInfo>,
) {
    REFINED.with(|table| {
        let mut table = table.borrow_mut();
        let table = table.get_or_insert_with(|| {
            (0..SLOTS)
                .map(|_| RefinedSlot {
                    entry: None,
                    owner: 0,
                    generation: 0,
                })
                .collect::<Vec<_>>()
                .into()
        });
        table[refined_slot_of(class)] = RefinedSlot {
            entry: Some((Arc::clone(class), Arc::clone(refined))),
            owner,
            generation,
        };
    });
}

#[cfg(test)]
#[path = "class_loader_memo_tests.rs"]
mod tests;
