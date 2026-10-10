//! The variable map a [`ScopeState`](super::ScopeState) carries.

use std::ops::ControlFlow;

use super::trie::{AtomTrie, Iter};
use crate::atom::Atom;
use crate::types::ResolvedType;

/// Scope key → resolved types, shared structurally between copies.
///
/// Besides the map itself, this keeps an index of the keys that are not
/// a bare variable name (`$a->b`, `$a["k"]`, `f($a)`, `self::$b`). Every
/// key a reassignment or a call invalidates is one of those, so the
/// invalidation passes visit the index instead of every variable the
/// scope holds.
#[derive(Clone, Debug, Default)]
pub(crate) struct Locals {
    map: AtomTrie<Vec<ResolvedType>>,
    compound: AtomTrie<()>,
}

/// Whether `key` is anything other than a bare variable name.
fn is_compound(key: &str) -> bool {
    key.bytes()
        .any(|b| !(b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80))
}

impl Locals {
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn get(&self, key: &Atom) -> Option<&Vec<ResolvedType>> {
        self.map.get(key)
    }

    pub fn get_mut(&mut self, key: &Atom) -> Option<&mut Vec<ResolvedType>> {
        self.map.get_mut(key)
    }

    pub fn contains_key(&self, key: &Atom) -> bool {
        self.map.contains_key(key)
    }

    pub fn insert(&mut self, key: Atom, types: Vec<ResolvedType>) {
        if is_compound(&key) && !self.compound.contains_key(&key) {
            self.compound.insert(key, ());
        }
        self.map.insert(key, types);
    }

    /// The entry for `key`, created empty when there is none.
    pub fn get_or_insert_default(&mut self, key: Atom) -> &mut Vec<ResolvedType> {
        if !self.map.contains_key(&key) {
            self.insert(key, Vec::new());
        }
        self.map.get_mut(&key).expect("entry was just inserted")
    }

    pub fn remove(&mut self, key: &Atom) {
        if self.map.remove(key) {
            self.compound.remove(key);
        }
    }

    /// Keep only the compound keys `keep` accepts; bare variables are
    /// kept without being looked at.
    pub fn retain_compound(&mut self, mut keep: impl FnMut(&Atom) -> bool) {
        if self.compound.is_empty() {
            return;
        }
        let doomed: Vec<Atom> = self
            .compound
            .keys()
            .filter(|key| !keep(key))
            .copied()
            .collect();
        for key in &doomed {
            self.remove(key);
        }
    }

    /// The keys that are not a bare variable name.
    pub fn compound_keys(&self) -> impl Iterator<Item = &Atom> {
        self.compound.keys()
    }

    pub fn keys(&self) -> impl Iterator<Item = &Atom> {
        self.map.keys()
    }

    /// Visit the keys the two maps do not share an entry for; see
    /// [`AtomTrie::diff`].
    /// Whether any key holds a different entry here than in `other`.
    ///
    /// Entries are compared by identity, so this is cheap between a scope
    /// and a copy of it: only the paths a write has since replaced are
    /// visited.
    pub fn differs_from(&self, other: &Locals) -> bool {
        self.map
            .diff::<()>(&other.map, |_, _, _| ControlFlow::Break(()))
            .is_break()
    }

    pub fn diff<'a, B>(
        &'a self,
        other: &'a Self,
        visit: impl FnMut(
            &'a Atom,
            Option<&'a Vec<ResolvedType>>,
            Option<&'a Vec<ResolvedType>>,
        ) -> ControlFlow<B>,
    ) -> ControlFlow<B> {
        self.map.diff(&other.map, visit)
    }
}

impl FromIterator<(Atom, Vec<ResolvedType>)> for Locals {
    fn from_iter<I: IntoIterator<Item = (Atom, Vec<ResolvedType>)>>(iter: I) -> Self {
        let mut locals = Locals::default();
        for (key, types) in iter {
            locals.insert(key, types);
        }
        locals
    }
}

impl<'a> IntoIterator for &'a Locals {
    type Item = (&'a Atom, &'a Vec<ResolvedType>);
    type IntoIter = Iter<'a, Vec<ResolvedType>>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.iter()
    }
}

impl IntoIterator for Locals {
    type Item = (Atom, Vec<ResolvedType>);
    type IntoIter = std::vec::IntoIter<(Atom, Vec<ResolvedType>)>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.into_iter()
    }
}
