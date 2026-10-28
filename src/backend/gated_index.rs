//! An index with flags that let hot paths skip its lock.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use parking_lot::{ReentrantMutex, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// An index together with whether it holds anything and whether it is out
/// of date.
///
/// The hot paths that consult such an index (every class load asks the
/// macro and pivot indexes) read the flag first, so the common case where
/// the project has none takes no lock at all.  Whoever writes the index
/// keeps the flag current with [`Self::set_has_any`] or [`Self::replace`],
/// which also move [`Self::generation`].
#[derive(Default)]
pub(crate) struct GatedIndex<T> {
    index: RwLock<T>,
    has_any: AtomicBool,
    dirty: AtomicBool,
    generation: AtomicU64,
    /// Held for the length of a rebuild; see [`Self::rebuild_if_dirty`].
    rebuild_lock: ReentrantMutex<()>,
    rebuilding: AtomicBool,
}

impl<T> GatedIndex<T> {
    /// An index that has to be built before it is first read.
    pub(crate) fn stale(index: T) -> Self {
        Self {
            index: RwLock::new(index),
            has_any: AtomicBool::new(false),
            dirty: AtomicBool::new(true),
            generation: AtomicU64::new(0),
            rebuild_lock: ReentrantMutex::new(()),
            rebuilding: AtomicBool::new(false),
        }
    }

    pub(crate) fn read(&self) -> RwLockReadGuard<'_, T> {
        self.index.read()
    }

    pub(crate) fn write(&self) -> RwLockWriteGuard<'_, T> {
        self.index.write()
    }

    /// The lock around the index, for a helper that takes one.
    pub(crate) fn index(&self) -> &RwLock<T> {
        &self.index
    }

    /// Whether the index holds anything.
    pub(crate) fn has_any(&self) -> bool {
        self.has_any.load(Ordering::Relaxed)
    }

    /// Record whether the index holds anything, once a write to it is
    /// done.
    pub(crate) fn set_has_any(&self, has_any: bool) {
        self.has_any.store(has_any, Ordering::Relaxed);
        self.generation.fetch_add(1, Ordering::Release);
    }

    /// A stamp that moves with every finished write, for a memo of what was
    /// derived from the index.  Read it before reading the index.
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Swap in a rebuilt index.
    pub(crate) fn replace(&self, index: T, has_any: bool) {
        *self.index.write() = index;
        self.set_has_any(has_any);
    }

    /// Whether the index has to be rebuilt before it is read.
    pub(crate) fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }

    pub(crate) fn mark_dirty(&self) {
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// Run `rebuild` if the index is out of date, or wait for the rebuild
    /// another thread is already running.
    ///
    /// The mark is cleared when a rebuild starts, so without the wait a
    /// thread arriving mid-rebuild would read the old index as current.
    /// The lock is reentrant because a rebuild loads classes, and every
    /// class load comes back through here on the same thread.
    pub(crate) fn rebuild_if_dirty(&self, rebuild: impl FnOnce()) {
        if !self.dirty.load(Ordering::SeqCst) && !self.rebuilding.load(Ordering::SeqCst) {
            return;
        }
        let _lock = self.rebuild_lock.lock();
        // Raised before the mark is taken, so that a thread which finds the
        // mark gone also finds this set until the rebuild is over.  Only
        // the outermost call on the thread lowers it again.
        let _rebuilding =
            (!self.rebuilding.swap(true, Ordering::SeqCst)).then(|| Lower(&self.rebuilding));
        if self.dirty.swap(false, Ordering::SeqCst) {
            rebuild();
        }
    }
}

/// Lowers a flag when dropped, panic or not.
struct Lower<'a>(&'a AtomicBool);

impl Drop for Lower<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
#[path = "gated_index_tests.rs"]
mod tests;
