use super::*;

use crate::atom::atom;

fn named(name: &str) -> PhpType {
    PhpType::named(atom(name))
}

fn class(name: &str) -> Arc<ClassInfo> {
    Arc::new(ClassInfo {
        name: atom(name),
        ..Default::default()
    })
}

/// Each test gets its own owner id so they cannot see each other's slots
/// when nextest reuses a thread.
fn owner() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1_000);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[test]
fn probe_misses_before_anything_is_stored() {
    assert!(probe(owner(), 0, &named("App\\User")).is_none());
}

#[test]
fn stored_answer_is_returned() {
    let (owner, ty, cls) = (owner(), named("App\\User"), class("User"));
    store(owner, 7, &ty, &Some(Arc::clone(&cls)));

    let hit = probe(owner, 7, &ty).expect("memoised");
    assert!(Arc::ptr_eq(&hit.expect("class"), &cls));
}

#[test]
fn stored_negative_answer_is_returned() {
    let (owner, ty) = (owner(), named("App\\Missing"));
    store(owner, 1, &ty, &None);

    // `Some(None)` — memoised, and the memoised answer is "no such class".
    assert!(matches!(probe(owner, 1, &ty), Some(None)));
}

#[test]
fn a_later_generation_retires_the_answer() {
    let (owner, ty) = (owner(), named("App\\User"));
    store(owner, 1, &ty, &Some(class("User")));

    assert!(probe(owner, 2, &ty).is_none());
}

#[test]
fn another_index_does_not_see_the_answer() {
    let ty = named("App\\User");
    store(owner(), 1, &ty, &Some(class("User")));

    assert!(probe(owner(), 1, &ty).is_none());
}

#[test]
fn a_different_type_in_the_same_slot_misses() {
    let (owner, stored, other) = (owner(), named("App\\User"), named("App\\Order"));
    store(owner, 1, &stored, &Some(class("User")));

    // Not a collision in general, but must be a miss either way.
    assert!(probe(owner, 1, &other).is_none());
}

#[test]
fn storing_twice_keeps_the_newer_answer() {
    let (owner, ty, second) = (owner(), named("App\\User"), class("Replaced"));
    store(owner, 1, &ty, &Some(class("User")));
    store(owner, 2, &ty, &Some(Arc::clone(&second)));

    let hit = probe(owner, 2, &ty).expect("memoised");
    assert!(Arc::ptr_eq(&hit.expect("class"), &second));
}

#[test]
fn every_slot_is_reachable() {
    // A type whose handle lands in each slot at least once, proving the
    // index is masked into range rather than clamped.
    let owner = owner();
    let types: Vec<PhpType> = (0..SLOTS * 4)
        .map(|i| named(&format!("App\\C{i}")))
        .collect();
    let mut seen = vec![false; SLOTS];
    for ty in &types {
        seen[slot_of(ty)] = true;
    }
    assert!(seen.iter().filter(|s| **s).count() > SLOTS / 2);

    // And every one of them round-trips while it owns its slot.
    for ty in &types {
        store(owner, 1, ty, &Some(class("C")));
        assert!(probe(owner, 1, ty).is_some());
    }
}

#[test]
fn stored_refinement_is_returned() {
    let (owner, linked, refined) = (owner(), class("Guard"), class("Guard"));
    store_refined(owner, 3, &linked, &refined);

    let hit = probe_refined(owner, 3, &linked).expect("memoised");
    assert!(Arc::ptr_eq(&hit, &refined));
}

#[test]
fn a_later_refinement_generation_retires_the_refinement() {
    let (owner, linked) = (owner(), class("Guard"));
    store_refined(owner, 3, &linked, &class("Guard"));

    assert!(probe_refined(owner, 4, &linked).is_none());
}

#[test]
fn an_equal_but_distinct_class_misses_the_refinement() {
    let (owner, linked) = (owner(), class("Guard"));
    store_refined(owner, 1, &linked, &class("Guard"));

    // A re-indexed class is a new `Arc`, whatever it holds.
    assert!(probe_refined(owner, 1, &class("Guard")).is_none());
}
