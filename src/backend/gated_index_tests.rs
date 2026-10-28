use std::sync::mpsc;
use std::sync::{Arc, Barrier};

use super::GatedIndex;

#[test]
fn a_reader_arriving_mid_rebuild_waits_for_it() {
    let index = Arc::new(GatedIndex::stale(0u32));
    let started = Arc::new(Barrier::new(2));
    let (finish_tx, finish_rx) = mpsc::channel::<()>();

    let rebuilder = {
        let index = Arc::clone(&index);
        let started = Arc::clone(&started);
        std::thread::spawn(move || {
            index.rebuild_if_dirty(|| {
                started.wait();
                finish_rx.recv().unwrap();
                index.replace(1, true);
            });
        })
    };

    // The rebuild has taken the mark by now, so the index looks clean.
    started.wait();
    assert!(!index.is_dirty());
    let reader = {
        let index = Arc::clone(&index);
        std::thread::spawn(move || {
            index.rebuild_if_dirty(|| panic!("the rebuild in progress should be awaited"));
            *index.read()
        })
    };
    // Give the reader the chance to answer early if it is going to.
    std::thread::sleep(std::time::Duration::from_millis(50));
    finish_tx.send(()).unwrap();

    rebuilder.join().unwrap();
    assert_eq!(reader.join().unwrap(), 1);
}

#[test]
fn a_rebuild_may_read_through_its_own_index() {
    let index = GatedIndex::stale(0u32);
    let mut nested = false;
    index.rebuild_if_dirty(|| {
        // A rebuild loads classes, which come back here on the same thread.
        index.rebuild_if_dirty(|| unreachable!("the mark was already taken"));
        nested = true;
        index.replace(1, true);
    });
    assert!(nested);
    assert_eq!(*index.read(), 1);

    // Done, so nothing is left for the next reader to wait on or rebuild.
    index.rebuild_if_dirty(|| unreachable!("the index is current"));
    index.mark_dirty();
    let mut rebuilt = false;
    index.rebuild_if_dirty(|| rebuilt = true);
    assert!(rebuilt);
}
