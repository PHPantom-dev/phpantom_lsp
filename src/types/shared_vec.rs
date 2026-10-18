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

    /// Borrow the contents as a slice.
    ///
    /// Equivalent to the `Deref` coercion, but named explicitly so call
    /// sites do not accidentally resolve to an inherent slice method on
    /// the deref target (e.g. the unstable `str::as_str`).
    #[inline]
    pub fn as_slice(&self) -> &[T] {
        &self.0
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

    /// The backing allocation, for pools that share equal vectors
    /// between owners.
    #[inline]
    pub(crate) fn as_arc(&self) -> &Arc<Vec<T>> {
        &self.0
    }

    /// Wrap a backing allocation obtained from [`as_arc`](Self::as_arc).
    #[inline]
    pub(crate) fn from_arc(arc: Arc<Vec<T>>) -> Self {
        SharedVec(arc)
    }

    /// Whether no other `SharedVec` holds this allocation, so
    /// [`make_mut`](Self::make_mut) will not copy it.
    #[inline]
    pub(crate) fn is_unique(&self) -> bool {
        Arc::strong_count(&self.0) == 1 && Arc::weak_count(&self.0) == 0
    }

    /// Consume and return the inner `Vec`, cloning only if shared.
    #[inline]
    pub fn into_vec(self) -> Vec<T> {
        Arc::try_unwrap(self.0).unwrap_or_else(|arc| (*arc).clone())
    }
}

impl<T> SharedVec<T> {
    /// Memory-audit tooling: identity of the shared allocation.
    #[cfg(feature = "mem-audit")]
    pub fn audit_ptr(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }

    /// Memory-audit tooling: capacity of the shared `Vec`.
    #[cfg(feature = "mem-audit")]
    pub fn audit_capacity(&self) -> usize {
        self.0.capacity()
    }
}

impl<T> From<Vec<T>> for SharedVec<T> {
    #[inline]
    fn from(v: Vec<T>) -> Self {
        SharedVec(Arc::new(v))
    }
}

impl<T> FromIterator<T> for SharedVec<T> {
    #[inline]
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        SharedVec(Arc::new(iter.into_iter().collect()))
    }
}
