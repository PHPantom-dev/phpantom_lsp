//! A proof map that can find the proofs reading a variable without
//! visiting every other proof the scope holds.

use std::ops::Deref;

use super::trie::AtomTrie;
use super::{ImpliedNarrowing, PregOutcome, VarAssertion};
use crate::atom::{Atom, atom};
use crate::php_type::PhpType;

/// The scope keys a proof value is about, besides the holder it is
/// recorded under.
pub(crate) trait ProofSubjects {
    fn for_each_subject(&self, visit: &mut dyn FnMut(&Atom));
}

/// One item of a list-valued proof, and the scope key it is about.
pub(crate) trait ProofItem {
    fn subject(&self) -> Option<&Atom>;
}

impl ProofItem for VarAssertion {
    fn subject(&self) -> Option<&Atom> {
        Some(&self.subject)
    }
}

impl ProofItem for Atom {
    fn subject(&self) -> Option<&Atom> {
        Some(self)
    }
}

impl ProofItem for ImpliedNarrowing {
    fn subject(&self) -> Option<&Atom> {
        Some(&self.key)
    }
}

impl ProofItem for PhpType {
    fn subject(&self) -> Option<&Atom> {
        None
    }
}

impl<T: ProofItem> ProofSubjects for Vec<T> {
    fn for_each_subject(&self, visit: &mut dyn FnMut(&Atom)) {
        self.iter().filter_map(ProofItem::subject).for_each(visit);
    }
}

impl ProofSubjects for PregOutcome {
    fn for_each_subject(&self, visit: &mut dyn FnMut(&Atom)) {
        visit(&self.matches_var);
    }
}

/// Holder → proof, indexed by the variables each entry reads.
///
/// Reassigning a variable has to drop every proof that reads it, and the
/// proofs a scope holds grow with the code walked so far: at the top level
/// of a long script, with every statement before it. The index maps each
/// `$variable` named in a holder or one of its subjects to the holders
/// that name it, so the invalidation only visits those.
///
/// The index is allowed to over-approximate. A removed entry leaves its
/// holder listed, and the lookup re-checks every holder it finds, so the
/// only cost of a stale listing is one wasted lookup. What it must never
/// do is miss a holder, which is why every write goes through this type.
pub(crate) struct ProofMap<V> {
    map: AtomTrie<V>,
    mentions: AtomTrie<Vec<Atom>>,
}

impl<V> Clone for ProofMap<V> {
    fn clone(&self) -> Self {
        ProofMap {
            map: self.map.clone(),
            mentions: self.mentions.clone(),
        }
    }
}

impl<V> Default for ProofMap<V> {
    fn default() -> Self {
        ProofMap {
            map: AtomTrie::default(),
            mentions: AtomTrie::default(),
        }
    }
}

impl<V: std::fmt::Debug> std::fmt::Debug for ProofMap<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.map.fmt(f)
    }
}

impl<V: PartialEq> PartialEq for ProofMap<V> {
    fn eq(&self, other: &Self) -> bool {
        self.map == other.map
    }
}

impl<V> Deref for ProofMap<V> {
    type Target = AtomTrie<V>;

    fn deref(&self) -> &AtomTrie<V> {
        &self.map
    }
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || !c.is_ascii()
}

/// Visit every `$name` token in `key`. A key that is nothing but one
/// variable is its own token and is not interned again.
fn each_variable(key: &Atom, visit: &mut impl FnMut(Atom)) {
    let text = key.as_str();
    let mut from = 0;
    while let Some(found) = text[from..].find('$') {
        let start = from + found;
        let rest = &text[start + 1..];
        let len = rest.find(|c: char| !is_name_char(c)).unwrap_or(rest.len());
        if len > 0 {
            if start == 0 && len == rest.len() {
                visit(*key);
                return;
            }
            visit(atom(&text[start..start + 1 + len]));
        }
        from = start + 1;
    }
}

/// The first `$name` token in `key`, which every key that reads `key`
/// spells out as well.
fn first_variable(key: &str) -> Option<Atom> {
    let start = key.find('$')?;
    let rest = &key[start + 1..];
    let len = rest.find(|c: char| !is_name_char(c)).unwrap_or(rest.len());
    (len > 0).then(|| atom(&key[start..start + 1 + len]))
}

fn note_mention(mentions: &mut AtomTrie<Vec<Atom>>, variable: Atom, holder: Atom) {
    let listed = mentions
        .get(&variable)
        .is_some_and(|holders| holders.contains(&holder));
    if !listed {
        mentions.get_or_insert_default(variable).push(holder);
    }
}

impl<V: ProofSubjects> ProofMap<V> {
    fn index(mentions: &mut AtomTrie<Vec<Atom>>, holder: Atom, value: &V) {
        let mut note = |variable: Atom| note_mention(mentions, variable, holder);
        each_variable(&holder, &mut note);
        value.for_each_subject(&mut |subject| each_variable(subject, &mut note));
    }

    pub fn insert(&mut self, holder: Atom, value: V) {
        Self::index(&mut self.mentions, holder, &value);
        self.map.insert(holder, value);
    }

    pub fn remove(&mut self, holder: &Atom) -> bool {
        self.map.remove(holder)
    }

    /// Edit the entry for `holder`, creating it empty first when there is
    /// none.
    pub fn update(&mut self, holder: Atom, edit: impl FnOnce(&mut V))
    where
        V: Clone + Default,
    {
        edit(self.map.get_or_insert_default(holder));
        if let Some(value) = self.map.get(&holder) {
            Self::index(&mut self.mentions, holder, value);
        }
    }

    /// Keep only the entries `keep` accepts, for a `keep` that accepts
    /// every entry whose holder and subjects never spell out `variable`'s
    /// first `$name`. Only the entries that do are looked at.
    pub fn retain_mentioning(&mut self, variable: &str, mut keep: impl FnMut(&Atom, &V) -> bool) {
        if self.map.is_empty() {
            return;
        }
        let Some(token) = first_variable(variable) else {
            self.map.retain(keep);
            return;
        };
        let Some(holders) = self.mentions.get(&token) else {
            return;
        };
        let doomed: Vec<Atom> = holders
            .iter()
            .filter(|holder| self.map.get(holder).is_some_and(|v| !keep(holder, v)))
            .copied()
            .collect();
        for holder in &doomed {
            self.map.remove(holder);
        }
    }
}

impl<T: Clone + ProofItem> ProofMap<Vec<T>> {
    /// Append `item` to `holder`'s list unless an item `same` matches is
    /// already there.
    pub fn push_unique(&mut self, holder: Atom, item: T, same: impl Fn(&T, &T) -> bool) {
        if self
            .map
            .get(&holder)
            .is_some_and(|items| items.iter().any(|existing| same(existing, &item)))
        {
            return;
        }
        let mentions = &mut self.mentions;
        let mut note = |variable: Atom| note_mention(mentions, variable, holder);
        each_variable(&holder, &mut note);
        if let Some(subject) = item.subject() {
            each_variable(subject, &mut note);
        }
        self.map.get_or_insert_default(holder).push(item);
    }

    /// Drop the items `keep` rejects from every list, and every list left
    /// empty.
    pub fn retain_items(&mut self, keep: impl FnMut(&T) -> bool) {
        self.map.retain_items(keep);
    }

    /// Like [`Self::retain_items`], for a `keep` that accepts every item
    /// of an entry that never spells out `variable`'s first `$name`.
    pub fn retain_items_mentioning(&mut self, variable: &str, mut keep: impl FnMut(&T) -> bool) {
        if self.map.is_empty() {
            return;
        }
        let Some(token) = first_variable(variable) else {
            self.map.retain_items(keep);
            return;
        };
        let Some(holders) = self.mentions.get(&token) else {
            return;
        };
        let touched: Vec<Atom> = holders
            .iter()
            .filter(|holder| {
                self.map
                    .get(holder)
                    .is_some_and(|items| !items.iter().all(&mut keep))
            })
            .copied()
            .collect();
        for holder in &touched {
            let Some(items) = self.map.get_mut(holder) else {
                continue;
            };
            items.retain(&mut keep);
            if items.is_empty() {
                self.map.remove(holder);
            }
        }
    }
}
