//! Owned name resolution data extracted from `mago_names::ResolvedNames`.
//!
//! [`OwnedResolvedNames`] mirrors the `ResolvedNames` API from `mago-names`
//! but owns its data, decoupling it from the arena lifetime.  Built once
//! per file in `update_ast_inner` for files open in the editor and stored
//! in `Backend::resolved_names` for the lifetime of the file.

use std::collections::HashMap;

use crate::atom::bytes_to_str;

/// Per-file name resolution data (owned, lifetime-free).
///
/// Each entry maps a byte offset in the source file to the fully-qualified
/// name that the identifier at that offset resolves to, together with a
/// flag indicating whether the resolution came from an explicit `use`
/// statement (as opposed to namespace-relative resolution or a definition).
///
/// Built from `mago_names::ResolvedNames` at the end of a parse pass.
#[derive(Debug, Clone, Default)]
pub struct OwnedResolvedNames {
    /// Byte offset → (fully-qualified name, was-imported flag).
    names: HashMap<u32, (String, bool)>,
}

impl OwnedResolvedNames {
    /// Build an `OwnedResolvedNames` by copying every entry out of the
    /// arena-backed `ResolvedNames`.
    pub fn from_resolved(resolved: &mago_names::ResolvedNames<'_>) -> Self {
        let mut names = HashMap::with_capacity(resolved.len());
        for (start, _end, fqn, imported) in resolved.iter() {
            names.insert(start, (bytes_to_str(fqn).to_owned(), imported));
        }
        Self { names }
    }

    /// Look up the fully-qualified name for the identifier at `offset`.
    ///
    /// Returns `None` when no resolved name exists at that position
    /// (e.g. keywords, literals, or identifiers that `mago-names` does
    /// not track).
    pub fn get(&self, offset: u32) -> Option<&str> {
        self.names.get(&offset).map(|(name, _)| name.as_str())
    }

    /// Whether the name at `offset` was introduced by an explicit `use`
    /// statement.
    ///
    /// Returns `false` when the offset is not tracked or when the name
    /// was resolved via the current namespace / is a definition.
    pub fn is_imported(&self, offset: u32) -> bool {
        self.names
            .get(&offset)
            .is_some_and(|(_, imported)| *imported)
    }

    /// Iterate all resolved name entries as `(offset, fqn, imported)` triples.
    ///
    /// Useful for cross-referencing resolved names with other data structures
    /// (e.g. checking which declared imports are actually used).
    #[allow(dead_code)]
    pub fn iter(&self) -> impl Iterator<Item = (u32, &str, bool)> {
        self.names
            .iter()
            .map(|(&offset, (fqn, imported))| (offset, fqn.as_str(), *imported))
    }
}
