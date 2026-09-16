//! The worker pool the CLI subcommands share.
//!
//! Every project-wide command (`analyze`, `fix`, `format`) walks the same
//! shape: a list of files, one parse-sized worker per core, and a shared
//! counter each worker pulls its next index from so a slow file does not
//! stall the rest.

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
/// that takes the count from its own options (`analyze --threads`). It
/// is still capped at `count`, for the reason [`map_indexed`] gives.
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
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
        })
        .min(count);
    let next_idx = AtomicUsize::new(0);
    let work = &work;

    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..n_threads)
            .map(|worker| {
                let next_idx = &next_idx;
                std::thread::Builder::new()
                    .name(thread_name.into())
                    .stack_size(crate::PARSE_WORKER_STACK_SIZE)
                    .spawn_scoped(scope, move || {
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
                    })
                    .unwrap_or_else(|_| panic!("failed to spawn {thread_name} thread"))
            })
            .collect();

        let mut merged: Vec<(usize, R)> = Vec::new();
        for handle in handles {
            merged.extend(handle.join().unwrap_or_default());
        }
        merged
    })
}
