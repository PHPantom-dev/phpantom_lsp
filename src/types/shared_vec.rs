//! `SharedVec`: a cheap-to-clone, copy-on-write shared vector.

use super::*;

// ─── SharedVec ──────────────────────────────────────────────────────────────

/// A cheap-to-clone vector backed by `Arc<Vec<T>>`.
///
/// Cloning a `SharedVec` bumps a reference count (O(1)) instead of
/// deep-copying every element.  This is critical for [`ClassInfo`] which
/// contains hundreds of methods/properties/constants on Eloquent models —
/// a full `Vec::clone` allocated dozens of heap objects and dominated CPU
/// time in `perf` profiles.
///
/// Read access is transparent: `SharedVec<T>` derefs to `[T]`, so
/// `.iter()`, `.len()`, `.is_empty()`, indexing, and `for x in &sv` all
/// work unchanged.
///
/// Mutation uses copy-on-write via [`Arc::make_mut`].  Call
/// [`push`](SharedVec::push) for single insertions or
/// [`make_mut`](SharedVec::make_mut) for bulk operations.  When the
/// `Arc` has a refcount of 1 (the common case inside
/// `resolve_class_with_inheritance`), `make_mut` is a no-op.
#[derive(Debug)]
pub struct SharedVec<T>(Arc<Vec<T>>);

// ── Clone: O(1) Arc bump ────────────────────────────────────────────────────

impl<T> Clone for SharedVec<T> {
    #[inline]
    fn clone(&self) -> Self {
        SharedVec(Arc::clone(&self.0))
    }
}

// ── Default: empty vec ──────────────────────────────────────────────────────

impl<T> Default for SharedVec<T> {
    #[inline]
    fn default() -> Self {
        SharedVec(Arc::new(Vec::new()))
    }
}

// ── Deref to [T] ───────────────────────────────────────────────────────────

impl<T> std::ops::Deref for SharedVec<T> {
    type Target = [T];
    #[inline]
    fn deref(&self) -> &[T] {
        &self.0
    }
}

// ── IntoIterator for &SharedVec<T> ─────────────────────────────────────────
//
// This allows `for x in &class.methods` to keep working unchanged.

impl<'a, T> IntoIterator for &'a SharedVec<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

// ── PartialEq ──────────────────────────────────────────────────────────────

impl<T: PartialEq> PartialEq for SharedVec<T> {
    fn eq(&self, other: &Self) -> bool {
        *self.0 == *other.0
    }
}

// ── Convenience methods ────────────────────────────────────────────────────

impl<T: Clone> SharedVec<T> {
    /// Create an empty `SharedVec`.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Wrap an existing `Vec<T>`.
    #[inline]
    pub fn from_vec(v: Vec<T>) -> Self {
        SharedVec(Arc::new(v))
    }

    /// Append a single element (copy-on-write).
    #[inline]
    pub fn push(&mut self, val: T) {
        Arc::make_mut(&mut self.0).push(val);
    }

    /// Get a mutable reference to the inner `Vec` (copy-on-write).
    ///
    /// Use this for bulk operations (extend, sort, retain, …).
    #[inline]
    pub fn make_mut(&mut self) -> &mut Vec<T> {
        Arc::make_mut(&mut self.0)
    }

    /// Consume and return the inner `Vec`, cloning only if shared.
    #[inline]
    pub fn into_vec(self) -> Vec<T> {
        Arc::try_unwrap(self.0).unwrap_or_else(|arc| (*arc).clone())
    }
}

// Allow `SharedVec` to be used with serde if ever needed in the future,
// and support `From` conversions for ergonomic construction.

impl<T> From<Vec<T>> for SharedVec<T> {
    #[inline]
    fn from(v: Vec<T>) -> Self {
        SharedVec(Arc::new(v))
    }
}
