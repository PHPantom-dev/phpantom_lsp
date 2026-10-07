//! The pull-index worker pool every parallel scan shares.
//!
//! The CLI subcommands (`analyze`, `fix`, `format`), workspace indexing,
//! eager class resolution, and the find-references file scan all walk
//! the same shape: a list of items, one parse-sized worker per core, and
//! a shared counter each worker pulls its next index from so a slow item
//! does not stall the rest.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Run `work` over `0..count` on a pool of parse-sized workers, and
/// return what the workers produced paired with the index it came from.
///
/// Results arrive in worker completion order, so a caller that needs
/// input order scatters them by index. `work` is handed the worker's own
/// number as well, for tracing which thread picked a file up.
///
/// Workers are never more numerous than the items they have to do: each
/// one reserves [`crate::PARSE_WORKER_STACK_SIZE`], so a run over a
/// handful of files must not spawn one per core.
pub(crate) fn map_indexed<R, F>(thread_name: &'static str, count: usize, work: F) -> Vec<(usize, R)>
where
    R: Send,
    F: Fn(usize, usize) -> Option<R> + Sync,
{
    map_indexed_with_threads(thread_name, count, None, work)
}

/// [`map_indexed`], with the pool sized by the caller.
///
/// `threads` overrides the one-worker-per-core default, for a command
/// that takes the count from its own options (`analyze --threads`) or a
/// scan that sizes its pool by how much work each worker should get. It
/// is still capped at `count`, for the reason [`map_indexed`] gives.
///
/// A worker the OS refuses to spawn is logged, and whatever the workers
/// that did start leave unclaimed is finished on the calling thread, so
/// a request handler under thread pressure degrades to a serial scan
/// rather than a panic. A worker that panics loses the items it had
/// claimed; that is logged too.
pub(crate) fn map_indexed_with_threads<R, F>(
    thread_name: &'static str,
    count: usize,
    threads: Option<usize>,
    work: F,
) -> Vec<(usize, R)>
where
    R: Send,
    F: Fn(usize, usize) -> Option<R> + Sync,
{
    if count == 0 {
        return Vec::new();
    }
    let n_threads = threads
        .filter(|n| *n > 0)
        .unwrap_or_else(available_cores)
        .min(count);
    let next_idx = AtomicUsize::new(0);
    let drain = |worker: usize| {
        let mut produced: Vec<(usize, R)> = Vec::new();
        loop {
            let i = next_idx.fetch_add(1, Ordering::Relaxed);
            if i >= count {
                break;
            }
            if let Some(result) = work(worker, i) {
                produced.push((i, result));
            }
        }
        produced
    };

    let (produced, spawn_failed) = run_workers(thread_name, n_threads, &drain);
    let mut merged: Vec<(usize, R)> = produced.into_iter().flatten().collect();
    if spawn_failed {
        merged.extend(drain(n_threads));
    }
    merged
}

/// How many workers a pool runs when the caller does not say.
fn available_cores() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

/// Run `drain` on `n_threads` parse-sized workers named `thread_name`,
/// each handed its own worker number, and return what the ones that
/// finished produced, along with whether the OS refused to spawn any.
///
/// A refused spawn and a worker that panics are both logged. The caller
/// decides what becomes of the items such a worker would have claimed.
fn run_workers<T, F>(thread_name: &'static str, n_threads: usize, drain: &F) -> (Vec<T>, bool)
where
    T: Send,
    F: Fn(usize) -> T + Sync,
{
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(n_threads);
        let mut spawn_failed = false;
        for worker in 0..n_threads {
            match std::thread::Builder::new()
                .name(thread_name.into())
                .stack_size(crate::PARSE_WORKER_STACK_SIZE)
                .spawn_scoped(scope, move || drain(worker))
            {
                Ok(handle) => handles.push(handle),
                Err(error) => {
                    tracing::error!("failed to spawn {thread_name} thread: {error}");
                    spawn_failed = true;
                }
            }
        }

        let mut produced = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.join() {
                Ok(result) => produced.push(result),
                Err(_) => tracing::error!("{thread_name} thread panicked"),
            }
        }
        (produced, spawn_failed)
    })
}

/// Run `work` over `0..ready_after.len()` on a pool of parse-sized
/// workers, where item `i` does not start until `ready_after[i]` items
/// have finished.
///
/// `ready_after` must be non-decreasing, which splits the items into
/// layers: the items of one layer run concurrently, and a layer starts
/// once every layer before it is done. Items are claimed in index
/// order, so every item a waiting worker depends on has already been
/// claimed by a worker that will finish it, and the pool cannot
/// deadlock however few workers actually started.
///
/// `setup` runs once on each worker before its first item, and what it
/// returns lives until the worker runs out of items: a thread-local
/// memo the items should share is activated there.
///
/// The pool is never wider than the widest layer, so a run whose layers
/// all hold one item stays on the calling thread. A worker the OS
/// refuses to spawn is logged, and a worker that panics loses only the
/// item it was on; whatever the workers leave unclaimed is finished on
/// the calling thread.
pub(crate) fn for_each_layered<G, S, F>(
    thread_name: &'static str,
    ready_after: &[usize],
    setup: S,
    work: F,
) where
    S: Fn() -> G + Sync,
    F: Fn(usize) + Sync,
{
    let count = ready_after.len();
    if count == 0 {
        return;
    }
    debug_assert!(ready_after.windows(2).all(|pair| pair[0] <= pair[1]));
    let widest_layer = ready_after
        .chunk_by(|a, b| a == b)
        .map(<[usize]>::len)
        .max()
        .unwrap_or(1);
    let n_threads = available_cores().min(widest_layer);

    let next_idx = AtomicUsize::new(0);
    let finished = (parking_lot::Mutex::new(0usize), parking_lot::Condvar::new());

    /// Counts its item as finished when dropped, so an item that panics
    /// still releases the layer after it.
    struct Finish<'a>(&'a (parking_lot::Mutex<usize>, parking_lot::Condvar));
    impl Drop for Finish<'_> {
        fn drop(&mut self) {
            *self.0.0.lock() += 1;
            self.0.1.notify_all();
        }
    }

    let drain = || {
        let _setup = setup();
        loop {
            let i = next_idx.fetch_add(1, Ordering::Relaxed);
            if i >= count {
                break;
            }
            {
                let mut done = finished.0.lock();
                while *done < ready_after[i] {
                    finished.1.wait(&mut done);
                }
            }
            let _finish = Finish(&finished);
            work(i);
        }
    };

    if n_threads <= 1 {
        drain();
        return;
    }

    run_workers(thread_name, n_threads, &|_| drain());
    drain();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layered_items_start_only_after_earlier_layers_finish() {
        // Three layers: 0..4, 4..5, 5..9.
        let ready_after = [0, 0, 0, 0, 4, 5, 5, 5, 5];
        let finished = parking_lot::Mutex::new(Vec::new());
        let finished_before = parking_lot::Mutex::new(vec![0usize; ready_after.len()]);
        for_each_layered(
            "layered-test",
            &ready_after,
            || (),
            |index| {
                finished_before.lock()[index] = finished.lock().len();
                std::thread::sleep(std::time::Duration::from_millis(5));
                finished.lock().push(index);
            },
        );

        let mut ran = finished.into_inner();
        ran.sort_unstable();
        assert_eq!(ran, (0..ready_after.len()).collect::<Vec<_>>());
        for (index, (&seen, &required)) in finished_before
            .into_inner()
            .iter()
            .zip(&ready_after)
            .enumerate()
        {
            assert!(
                seen >= required,
                "item {index} started after {seen} items, needs {required}"
            );
        }
    }
}
