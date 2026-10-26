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

    /// Wrap an existing `Vec<T>`, releasing its spare capacity.
    #[inline]
    pub fn from_vec(v: Vec<T>) -> Self {
        v.into()
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
    fn from(mut v: Vec<T>) -> Self {
        // Shared lists are almost always read-only once built, and the
        // parsed ones live in the index for the whole session, so spare
        // growth capacity would stay allocated with nothing to fill it.
        v.shrink_to_fit();
        SharedVec(Arc::new(v))
    }
}

/// Wrap each record in an `Arc`, in a list allocated at its final size.
///
/// `values.into_iter().map(Arc::new).collect()` reuses the allocation of
/// `values` for the pointer list. A record is many times wider than a
/// pointer, so nearly all of that allocation would stay unused, and it
/// lives as long as the parsed class.
pub(crate) fn into_arcs<T>(values: Vec<T>) -> Vec<Arc<T>> {
    let mut shared = Vec::with_capacity(values.len());
    shared.extend(values.into_iter().map(Arc::new));
    shared
}

impl<T> FromIterator<T> for SharedVec<T> {
    #[inline]
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        SharedVec(Arc::new(iter.into_iter().collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_vec_releases_build_capacity_and_preserves_copy_on_write() {
        let mut values = Vec::with_capacity(4096);
        values.extend([1, 2, 3]);
        let shared: SharedVec<_> = values.into();
        assert_eq!(shared.0.capacity(), shared.len());
        let mut changed = shared.clone();
        assert!(Arc::ptr_eq(&shared.0, &changed.0));
        changed.push(4);
        assert_eq!(shared.as_slice(), [1, 2, 3]);
        assert_eq!(changed.into_vec(), [1, 2, 3, 4]);

        let empty = SharedVec::<u8>::from_vec(Vec::with_capacity(4096));
        assert_eq!(empty.0.capacity(), 0);
    }

    #[test]
    fn into_arcs_retains_only_pointer_storage_for_wide_records() {
        let records = vec![[7u64; 64]; 128];
        let shared = into_arcs(records);
        assert_eq!(shared.capacity(), shared.len());
        assert_eq!(shared.len(), 128);
        assert!(shared.iter().all(|record| **record == [7; 64]));
        let empty = into_arcs(Vec::<[u64; 64]>::with_capacity(128));
        assert_eq!(empty.capacity(), 0);
    }

    #[test]
    fn parsed_member_lists_do_not_retain_record_sized_buffers() {
        let classes = crate::Backend::parse_php_versioned(
            "<?php
            /** @method string virtualName() */
            class Example {
                public string $name;
                public const LABEL = 'example';
                public function name(string $fallback): string { return $fallback; }
                public function make() {
                    return new class { public string $name; public function name() {} };
                }
            }
            interface Named { public function name(string $fallback): string; }
            trait HasName { public string $name; public function name() {} }
            enum Label: string { case Example = 'example'; public function name() {} }
            ",
            None,
        );
        assert_eq!(classes.len(), 5);
        for class in classes {
            assert_eq!(class.methods.0.capacity(), class.methods.len());
            assert_eq!(class.properties.0.capacity(), class.properties.len());
            assert_eq!(class.constants.0.capacity(), class.constants.len());
            for method in &class.methods {
                assert_eq!(method.parameters.0.capacity(), method.parameters.len());
            }
            if let Some(doc) = &class.doc_members {
                assert_eq!(doc.methods.capacity(), doc.methods.len());
            }
        }
    }
}
