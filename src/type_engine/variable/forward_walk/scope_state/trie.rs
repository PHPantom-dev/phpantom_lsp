//! A hash trie keyed by [`Atom`] whose clones share structure.
//!
//! The forward walker copies its scope at every branch, and the
//! diagnostic pass keeps a copy at every statement boundary. With an
//! ordinary hash map each copy costs as much as the scope is large, which
//! is fine inside a method and quadratic at the top level of a long
//! procedural file, where every statement so far has left its variable
//! behind. Here a clone is a reference count bump, a write copies only
//! the path down to the entry it changes, and [`AtomTrie::diff`] skips
//! every subtree two maps still share, so comparing a branch against the
//! scope it was forked from costs what the branch wrote rather than what
//! the scope holds.
//!
//! The layout is a hash array mapped trie over the hash `Atom` already
//! carries: each level consumes five bits of it, and the few keys whose
//! whole hash collides share a list at the bottom.

use std::ops::ControlFlow;
use std::sync::Arc;

use crate::atom::Atom;

const BITS: u32 = 5;
const MASK: u64 = (1 << BITS) - 1;
const HASH_BITS: u32 = u64::BITS;

pub(crate) struct AtomTrie<V> {
    root: Option<Arc<Node<V>>>,
    len: usize,
}

enum Slot<V> {
    Leaf(Arc<(Atom, V)>),
    Node(Arc<Node<V>>),
}

/// One level of the trie. Below the last level that still has hash bits
/// to consume, `bitmap` is unused and `slots` is a plain list of leaves
/// whose hashes are identical.
struct Node<V> {
    bitmap: u32,
    slots: Vec<Slot<V>>,
}

impl<V> Clone for Slot<V> {
    fn clone(&self) -> Self {
        match self {
            Slot::Leaf(leaf) => Slot::Leaf(Arc::clone(leaf)),
            Slot::Node(node) => Slot::Node(Arc::clone(node)),
        }
    }
}

impl<V> Clone for Node<V> {
    fn clone(&self) -> Self {
        Node {
            bitmap: self.bitmap,
            slots: self.slots.clone(),
        }
    }
}

impl<V> Clone for AtomTrie<V> {
    fn clone(&self) -> Self {
        AtomTrie {
            root: self.root.clone(),
            len: self.len,
        }
    }
}

impl<V> Default for AtomTrie<V> {
    fn default() -> Self {
        AtomTrie { root: None, len: 0 }
    }
}

impl<V: std::fmt::Debug> std::fmt::Debug for AtomTrie<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

fn hash_of(key: &Atom) -> u64 {
    key.precomputed_hash()
}

/// The bit `hash` selects at the level that starts at `shift`.
fn bit_at(hash: u64, shift: u32) -> u32 {
    1 << ((hash >> shift) & MASK)
}

/// Where the slot for `bit` sits in a node's packed slot list.
fn index_of(bitmap: u32, bit: u32) -> usize {
    (bitmap & (bit - 1)).count_ones() as usize
}

impl<V> Node<V> {
    /// A node holding two leaves whose keys differ, at the level that
    /// starts at `shift`.
    fn pair(a: Arc<(Atom, V)>, a_hash: u64, b: Arc<(Atom, V)>, b_hash: u64, shift: u32) -> Self {
        if shift >= HASH_BITS {
            return Node {
                bitmap: 0,
                slots: vec![Slot::Leaf(a), Slot::Leaf(b)],
            };
        }
        let (a_bit, b_bit) = (bit_at(a_hash, shift), bit_at(b_hash, shift));
        if a_bit == b_bit {
            let child = Node::pair(a, a_hash, b, b_hash, shift + BITS);
            return Node {
                bitmap: a_bit,
                slots: vec![Slot::Node(Arc::new(child))],
            };
        }
        let slots = if a_bit < b_bit {
            vec![Slot::Leaf(a), Slot::Leaf(b)]
        } else {
            vec![Slot::Leaf(b), Slot::Leaf(a)]
        };
        Node {
            bitmap: a_bit | b_bit,
            slots,
        }
    }

    fn get(&self, key: &Atom, hash: u64, shift: u32) -> Option<&Arc<(Atom, V)>> {
        let mut node = self;
        let mut shift = shift;
        loop {
            if shift >= HASH_BITS {
                return node.slots.iter().find_map(|slot| match slot {
                    Slot::Leaf(leaf) if leaf.0 == *key => Some(leaf),
                    _ => None,
                });
            }
            let bit = bit_at(hash, shift);
            if node.bitmap & bit == 0 {
                return None;
            }
            match &node.slots[index_of(node.bitmap, bit)] {
                Slot::Leaf(leaf) => return (leaf.0 == *key).then_some(leaf),
                Slot::Node(child) => {
                    node = child;
                    shift += BITS;
                }
            }
        }
    }

    /// Insert or replace, returning whether the key is new.
    fn insert(node: &mut Arc<Self>, key: Atom, hash: u64, shift: u32, value: V) -> bool {
        let node = Arc::make_mut(node);
        if shift >= HASH_BITS {
            for slot in &mut node.slots {
                if let Slot::Leaf(leaf) = slot
                    && leaf.0 == key
                {
                    replace_leaf(leaf, key, value);
                    return false;
                }
            }
            node.slots.push(Slot::Leaf(Arc::new((key, value))));
            return true;
        }
        let bit = bit_at(hash, shift);
        let idx = index_of(node.bitmap, bit);
        if node.bitmap & bit == 0 {
            node.bitmap |= bit;
            node.slots.insert(idx, Slot::Leaf(Arc::new((key, value))));
            return true;
        }
        match &mut node.slots[idx] {
            Slot::Node(child) => Node::insert(child, key, hash, shift + BITS, value),
            Slot::Leaf(leaf) if leaf.0 == key => {
                replace_leaf(leaf, key, value);
                false
            }
            Slot::Leaf(leaf) => {
                let existing = Arc::clone(leaf);
                let existing_hash = hash_of(&existing.0);
                let child = Node::pair(
                    existing,
                    existing_hash,
                    Arc::new((key, value)),
                    hash,
                    shift + BITS,
                );
                node.slots[idx] = Slot::Node(Arc::new(child));
                true
            }
        }
    }

    /// Remove a key the caller has checked is present. A node left with
    /// a single leaf is folded into its parent, so a subtree is only ever
    /// as deep as its keys need it to be.
    fn remove(node: &mut Arc<Self>, key: &Atom, hash: u64, shift: u32) {
        let node = Arc::make_mut(node);
        if shift >= HASH_BITS {
            node.slots
                .retain(|slot| !matches!(slot, Slot::Leaf(leaf) if leaf.0 == *key));
            return;
        }
        let bit = bit_at(hash, shift);
        if node.bitmap & bit == 0 {
            return;
        }
        let idx = index_of(node.bitmap, bit);
        match &mut node.slots[idx] {
            Slot::Leaf(leaf) => {
                if leaf.0 == *key {
                    node.slots.remove(idx);
                    node.bitmap &= !bit;
                }
            }
            Slot::Node(child) => {
                Node::remove(child, key, hash, shift + BITS);
                match child.slots.as_slice() {
                    [] => {
                        node.slots.remove(idx);
                        node.bitmap &= !bit;
                    }
                    [Slot::Leaf(only)] => {
                        let only = Arc::clone(only);
                        node.slots[idx] = Slot::Leaf(only);
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Overwrite a leaf's value, in place when no other map shares it.
fn replace_leaf<V>(leaf: &mut Arc<(Atom, V)>, key: Atom, value: V) {
    match Arc::get_mut(leaf) {
        Some(owned) => owned.1 = value,
        None => *leaf = Arc::new((key, value)),
    }
}

impl<V> AtomTrie<V> {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, key: &Atom) -> Option<&V> {
        self.root
            .as_deref()?
            .get(key, hash_of(key), 0)
            .map(|leaf| &leaf.1)
    }

    pub fn contains_key(&self, key: &Atom) -> bool {
        self.get(key).is_some()
    }

    pub fn insert(&mut self, key: Atom, value: V) {
        let hash = hash_of(&key);
        let added = match self.root.as_mut() {
            Some(root) => Node::insert(root, key, hash, 0, value),
            None => {
                self.root = Some(Arc::new(Node {
                    bitmap: bit_at(hash, 0),
                    slots: vec![Slot::Leaf(Arc::new((key, value)))],
                }));
                true
            }
        };
        if added {
            self.len += 1;
        }
    }

    /// Remove `key`, returning whether it was there.
    pub fn remove(&mut self, key: &Atom) -> bool {
        // Looking first keeps a miss from copying the path down to where
        // the key would have been.
        if !self.contains_key(key) {
            return false;
        }
        if let Some(root) = self.root.as_mut() {
            Node::remove(root, key, hash_of(key), 0);
        }
        self.len -= 1;
        if self.len == 0 {
            self.root = None;
        }
        true
    }

    /// Keep only the entries `keep` accepts.
    pub fn retain(&mut self, mut keep: impl FnMut(&Atom, &V) -> bool) {
        let doomed: Vec<Atom> = self
            .iter()
            .filter(|(key, value)| !keep(key, value))
            .map(|(key, _)| *key)
            .collect();
        for key in &doomed {
            self.remove(key);
        }
    }

    pub fn iter(&self) -> Iter<'_, V> {
        let mut stack = Vec::new();
        if let Some(root) = self.root.as_deref() {
            stack.push(root.slots.iter());
        }
        Iter { stack }
    }

    pub fn keys(&self) -> impl Iterator<Item = &Atom> {
        self.iter().map(|(key, _)| key)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.iter().map(|(_, value)| value)
    }

    /// Visit every key the two maps do not share an entry for: one only
    /// one side holds, or one both hold through separate writes. Entries
    /// reached through a subtree both maps still share are skipped
    /// without being looked at, so the cost follows how far the two have
    /// drifted apart rather than how large they are.
    ///
    /// A key both sides wrote the same value to is still visited; telling
    /// whether two separately written values agree is the caller's call.
    pub fn diff<'a, B>(
        &'a self,
        other: &'a Self,
        mut visit: impl FnMut(&'a Atom, Option<&'a V>, Option<&'a V>) -> ControlFlow<B>,
    ) -> ControlFlow<B> {
        match (self.root.as_ref(), other.root.as_ref()) {
            (None, None) => ControlFlow::Continue(()),
            (Some(a), None) => each_in(a, &mut |key, value| visit(key, Some(value), None)),
            (None, Some(b)) => each_in(b, &mut |key, value| visit(key, None, Some(value))),
            (Some(a), Some(b)) => {
                if Arc::ptr_eq(a, b) {
                    ControlFlow::Continue(())
                } else {
                    diff_nodes(a, b, 0, &mut visit)
                }
            }
        }
    }
}

impl<V: Clone> AtomTrie<V> {
    pub fn get_mut(&mut self, key: &Atom) -> Option<&mut V> {
        if !self.contains_key(key) {
            return None;
        }
        let hash = hash_of(key);
        let mut node = Arc::make_mut(self.root.as_mut()?);
        let mut shift = 0;
        loop {
            let idx = if shift >= HASH_BITS {
                node.slots
                    .iter()
                    .position(|slot| matches!(slot, Slot::Leaf(leaf) if leaf.0 == *key))?
            } else {
                index_of(node.bitmap, bit_at(hash, shift))
            };
            match &mut node.slots[idx] {
                Slot::Leaf(leaf) => return Some(&mut Arc::make_mut(leaf).1),
                Slot::Node(child) => {
                    node = Arc::make_mut(child);
                    shift += BITS;
                }
            }
        }
    }

    /// The entry for `key`, inserting `V::default()` first when there is
    /// none.
    pub fn get_or_insert_default(&mut self, key: Atom) -> &mut V
    where
        V: Default,
    {
        if !self.contains_key(&key) {
            self.insert(key, V::default());
        }
        self.get_mut(&key).expect("entry was just inserted")
    }
}

impl<T: Clone> AtomTrie<Vec<T>> {
    /// Drop the items `keep` rejects from every list, and every list left
    /// empty. Only the lists that lose something are copied.
    pub fn retain_items(&mut self, mut keep: impl FnMut(&T) -> bool) {
        let touched: Vec<Atom> = self
            .iter()
            .filter(|(_, items)| !items.iter().all(&mut keep))
            .map(|(key, _)| *key)
            .collect();
        for key in &touched {
            let Some(items) = self.get_mut(key) else {
                continue;
            };
            items.retain(&mut keep);
            if items.is_empty() {
                self.remove(key);
            }
        }
    }
}

impl<V: PartialEq> PartialEq for AtomTrie<V> {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len
            && self
                .diff(other, |_, mine, theirs| match (mine, theirs) {
                    (Some(mine), Some(theirs)) if mine == theirs => ControlFlow::Continue(()),
                    _ => ControlFlow::Break(()),
                })
                .is_continue()
    }
}

impl<V> FromIterator<(Atom, V)> for AtomTrie<V> {
    fn from_iter<I: IntoIterator<Item = (Atom, V)>>(iter: I) -> Self {
        let mut trie = AtomTrie::default();
        for (key, value) in iter {
            trie.insert(key, value);
        }
        trie
    }
}

impl<'a, V> IntoIterator for &'a AtomTrie<V> {
    type Item = (&'a Atom, &'a V);
    type IntoIter = Iter<'a, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<V: Clone> IntoIterator for AtomTrie<V> {
    type Item = (Atom, V);
    type IntoIter = std::vec::IntoIter<(Atom, V)>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
            .map(|(key, value)| (*key, value.clone()))
            .collect::<Vec<_>>()
            .into_iter()
    }
}

pub(crate) struct Iter<'a, V> {
    stack: Vec<std::slice::Iter<'a, Slot<V>>>,
}

impl<'a, V> Iterator for Iter<'a, V> {
    type Item = (&'a Atom, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.stack.last_mut()?.next() {
                None => {
                    self.stack.pop();
                }
                Some(Slot::Leaf(leaf)) => return Some((&leaf.0, &leaf.1)),
                Some(Slot::Node(node)) => self.stack.push(node.slots.iter()),
            }
        }
    }
}

fn each_in<'a, V, B>(
    node: &'a Node<V>,
    visit: &mut impl FnMut(&'a Atom, &'a V) -> ControlFlow<B>,
) -> ControlFlow<B> {
    for slot in &node.slots {
        each_in_slot(slot, visit)?;
    }
    ControlFlow::Continue(())
}

fn each_in_slot<'a, V, B>(
    slot: &'a Slot<V>,
    visit: &mut impl FnMut(&'a Atom, &'a V) -> ControlFlow<B>,
) -> ControlFlow<B> {
    match slot {
        Slot::Leaf(leaf) => visit(&leaf.0, &leaf.1),
        Slot::Node(node) => each_in(node, visit),
    }
}

type Visit<'v, 'a, V, B> = dyn FnMut(&'a Atom, Option<&'a V>, Option<&'a V>) -> ControlFlow<B> + 'v;

fn diff_nodes<'a, V, B>(
    a: &'a Node<V>,
    b: &'a Node<V>,
    shift: u32,
    visit: &mut Visit<'_, 'a, V, B>,
) -> ControlFlow<B> {
    if shift >= HASH_BITS {
        return diff_leaf_lists(&a.slots, &b.slots, visit);
    }
    let mut remaining = a.bitmap | b.bitmap;
    while remaining != 0 {
        let bit = remaining.isolate_lowest_one();
        remaining &= !bit;
        let mine = (a.bitmap & bit != 0).then(|| &a.slots[index_of(a.bitmap, bit)]);
        let theirs = (b.bitmap & bit != 0).then(|| &b.slots[index_of(b.bitmap, bit)]);
        match (mine, theirs) {
            (Some(x), None) => each_in_slot(x, &mut |key, value| visit(key, Some(value), None))?,
            (None, Some(y)) => each_in_slot(y, &mut |key, value| visit(key, None, Some(value)))?,
            (Some(Slot::Node(x)), Some(Slot::Node(y))) => {
                if !Arc::ptr_eq(x, y) {
                    diff_nodes(x, y, shift + BITS, visit)?;
                }
            }
            (Some(Slot::Leaf(x)), Some(Slot::Leaf(y))) => {
                if Arc::ptr_eq(x, y) {
                    continue;
                }
                if x.0 == y.0 {
                    visit(&x.0, Some(&x.1), Some(&y.1))?;
                } else {
                    visit(&x.0, Some(&x.1), None)?;
                    visit(&y.0, None, Some(&y.1))?;
                }
            }
            (Some(Slot::Leaf(x)), Some(Slot::Node(y))) => {
                diff_leaf_against(x, y, shift + BITS, true, visit)?;
            }
            (Some(Slot::Node(x)), Some(Slot::Leaf(y))) => {
                diff_leaf_against(y, x, shift + BITS, false, visit)?;
            }
            (None, None) => {}
        }
    }
    ControlFlow::Continue(())
}

/// Diff a lone leaf against the subtree on the other side of the same
/// slot. `leaf_is_mine` says which side of the comparison the leaf is on.
fn diff_leaf_against<'a, V, B>(
    leaf: &'a Arc<(Atom, V)>,
    subtree: &'a Node<V>,
    shift: u32,
    leaf_is_mine: bool,
    visit: &mut Visit<'_, 'a, V, B>,
) -> ControlFlow<B> {
    let found = subtree.get(&leaf.0, hash_of(&leaf.0), shift);
    let mut pair = |key: &'a Atom, lone: Option<&'a V>, other: Option<&'a V>| {
        if leaf_is_mine {
            visit(key, lone, other)
        } else {
            visit(key, other, lone)
        }
    };
    match found {
        Some(same) if Arc::ptr_eq(same, leaf) => {}
        Some(same) => pair(&leaf.0, Some(&leaf.1), Some(&same.1))?,
        None => pair(&leaf.0, Some(&leaf.1), None)?,
    }
    each_in(subtree, &mut |key, value| {
        if *key == leaf.0 {
            ControlFlow::Continue(())
        } else {
            pair(key, None, Some(value))
        }
    })
}

fn diff_leaf_lists<'a, V, B>(
    a: &'a [Slot<V>],
    b: &'a [Slot<V>],
    visit: &mut Visit<'_, 'a, V, B>,
) -> ControlFlow<B> {
    let leaves = |slots: &'a [Slot<V>]| {
        slots.iter().filter_map(|slot| match slot {
            Slot::Leaf(leaf) => Some(leaf),
            Slot::Node(_) => None,
        })
    };
    for x in leaves(a) {
        match leaves(b).find(|y| y.0 == x.0) {
            Some(y) if Arc::ptr_eq(x, y) => {}
            Some(y) => visit(&x.0, Some(&x.1), Some(&y.1))?,
            None => visit(&x.0, Some(&x.1), None)?,
        }
    }
    for y in leaves(b) {
        if !leaves(a).any(|x| x.0 == y.0) {
            visit(&y.0, None, Some(&y.1))?;
        }
    }
    ControlFlow::Continue(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atom::atom;
    use std::collections::BTreeMap;

    fn diff_of(
        a: &AtomTrie<u32>,
        b: &AtomTrie<u32>,
    ) -> BTreeMap<String, (Option<u32>, Option<u32>)> {
        let mut out = BTreeMap::new();
        let _ = a.diff::<()>(b, |key, x, y| {
            out.insert(key.to_string(), (x.copied(), y.copied()));
            ControlFlow::Continue(())
        });
        out
    }

    #[test]
    fn insert_get_remove_round_trip() {
        let mut trie = AtomTrie::default();
        for i in 0..2000u32 {
            trie.insert(atom(&format!("$v{i}")), i);
        }
        assert_eq!(trie.len(), 2000);
        for i in 0..2000u32 {
            assert_eq!(trie.get(&atom(&format!("$v{i}"))), Some(&i));
        }
        trie.insert(atom("$v7"), 70);
        assert_eq!(trie.get(&atom("$v7")), Some(&70));
        assert_eq!(trie.len(), 2000);
        for i in (0..2000u32).step_by(2) {
            assert!(trie.remove(&atom(&format!("$v{i}"))));
        }
        assert!(!trie.remove(&atom("$v0")));
        assert_eq!(trie.len(), 1000);
        assert_eq!(trie.iter().count(), 1000);
        assert_eq!(trie.get(&atom("$v2")), None);
        assert_eq!(trie.get(&atom("$v3")), Some(&3));
        for i in (1..2000u32).step_by(2) {
            trie.remove(&atom(&format!("$v{i}")));
        }
        assert!(trie.is_empty());
        assert!(trie.root.is_none());
    }

    #[test]
    fn writes_leave_clones_untouched() {
        let mut base = AtomTrie::default();
        for i in 0..500u32 {
            base.insert(atom(&format!("$v{i}")), i);
        }
        let snapshot = base.clone();
        base.insert(atom("$v1"), 100);
        *base.get_mut(&atom("$v2")).unwrap() = 200;
        base.remove(&atom("$v3"));
        assert_eq!(snapshot.get(&atom("$v1")), Some(&1));
        assert_eq!(snapshot.get(&atom("$v2")), Some(&2));
        assert_eq!(snapshot.get(&atom("$v3")), Some(&3));
        assert_eq!(snapshot.len(), 500);
    }

    #[test]
    fn diff_reports_only_what_changed() {
        let mut base = AtomTrie::default();
        for i in 0..500u32 {
            base.insert(atom(&format!("$v{i}")), i);
        }
        let mut branch = base.clone();
        assert!(diff_of(&base, &branch).is_empty());
        branch.insert(atom("$v1"), 100);
        branch.remove(&atom("$v2"));
        branch.insert(atom("$new"), 7);
        let diff = diff_of(&base, &branch);
        assert_eq!(diff.len(), 3);
        assert_eq!(diff["$v1"], (Some(1), Some(100)));
        assert_eq!(diff["$v2"], (Some(2), None));
        assert_eq!(diff["$new"], (None, Some(7)));
    }

    #[test]
    fn diff_between_unrelated_maps_matches_keys() {
        let a: AtomTrie<u32> = (0..300u32).map(|i| (atom(&format!("$a{i}")), i)).collect();
        let b: AtomTrie<u32> = (150..450u32)
            .map(|i| (atom(&format!("$a{i}")), i))
            .collect();
        let diff = diff_of(&a, &b);
        assert_eq!(diff.len(), 450);
        assert_eq!(diff["$a0"], (Some(0), None));
        assert_eq!(diff["$a200"], (Some(200), Some(200)));
        assert_eq!(diff["$a400"], (None, Some(400)));
    }
}
