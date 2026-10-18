//! Owned name resolution data extracted from `mago_names::ResolvedNames`.
//!
//! [`OwnedResolvedNames`] mirrors the `ResolvedNames` API from `mago-names`
//! but owns its data, decoupling it from the arena lifetime.  Built once
//! per file for every file the indexer parses, not only the ones open in
//! the editor, and stored in `Backend::resolved_names` for the lifetime
//! of the file.

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
    /// Memory-audit tooling: `(entry count, approximate heap bytes)`.
    #[cfg(feature = "mem-audit")]
    pub fn audit_heap(&self) -> (usize, usize) {
        let cap = self.names.capacity();
        let buckets = if cap == 0 {
            0
        } else {
            ((cap * 8).div_ceil(7).max(4)).next_power_of_two()
        };
        let mut bytes = buckets * (std::mem::size_of::<(u32, (String, bool))>() + 1);
        for (fqn, _) in self.names.values() {
            bytes += fqn.capacity();
        }
        (self.names.len(), bytes)
    }

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

    /// The FQN, without a leading `\`, that the class name `ident` stands
    /// for: the name recorded at its offset, else the name as written.
    /// `None` for an empty name.
    pub fn class_fqn(&self, ident: &mago_syntax::cst::Identifier<'_>) -> Option<String> {
        use mago_span::HasSpan;
        let fqn = self
            .get(ident.span().start.offset)
            .unwrap_or_else(|| bytes_to_str(ident.value()))
            .trim_start_matches('\\');
        (!fqn.is_empty()).then(|| fqn.to_string())
    }

    /// The FQN a `Foo::class` fetch names, as [`Self::class_fqn`] reads it.
    /// `None` for `self::class`, `static::class`, `parent::class`, and any
    /// other expression.
    pub fn class_fetch_fqn(&self, expr: &mago_syntax::cst::Expression<'_>) -> Option<String> {
        self.class_fqn(crate::parser::class_fetch_identifier(expr)?)
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
