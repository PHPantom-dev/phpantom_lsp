//! Variables bound to one value by `=&`.
//!
//! `$r = &$a` makes `$r` and `$a` two names for the same value, so a write
//! to either one is a write to both. `$e = &$b['y']` binds `$e` to an
//! element of `$b` instead, so writing `$e` writes that element and
//! writing `$b` changes what `$e` reads.

use crate::atom::{Atom, atom};
use crate::php_type::PhpType;
use crate::type_engine::variable::array_shape_writes::{ArrayWriteKey, merge_nested_array_write};
use crate::types::ResolvedType;

use super::ScopeState;

/// What a reference-bound variable shares its value with.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RefTarget {
    /// Another variable (`$r = &$a`).
    Var(Atom),
    /// An element of an array variable (`$e = &$b['y'][$k]`). The keys are
    /// the ones the element was bound through, evaluated where the binding
    /// was made, as PHP evaluates them.
    Element {
        base: Atom,
        keys: Vec<ArrayWriteKey>,
    },
}

/// One `=&` binding: `alias` shares its value with `target`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RefLink {
    /// The variable on the left of `=&`.
    pub alias: Atom,
    pub target: RefTarget,
}

/// The bindings one path made, boxed behind [`ScopeState::ref_links`]
/// because scopes are cloned far more often than references are taken.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RefLinks(Vec<RefLink>);

impl RefLink {
    fn mentions(&self, name: &Atom) -> bool {
        self.alias == *name
            || match &self.target {
                RefTarget::Var(target) => target == name,
                RefTarget::Element { base, .. } => base == name,
            }
    }
}

impl ScopeState {
    /// Bind `alias` to `target`, dropping whatever `alias` was bound to
    /// before. `alias` must already hold the value it now shares.
    pub fn bind_reference(&mut self, alias: &str, target: RefTarget) {
        let alias = atom(alias);
        if target == RefTarget::Var(alias) {
            return;
        }
        self.unbind_reference(&alias);
        self.ref_links
            .get_or_insert_with(Box::default)
            .0
            .push(RefLink { alias, target });
    }

    /// Stop `name` sharing its value with anything, for a write that
    /// rebinds it (`=&` again, `unset()`, a by-reference `foreach`).
    ///
    /// Variables bound to each other only through `name`
    /// (`$s = &$r; $r = &$a;` before `$r` is rebound) still share a value
    /// afterwards, so they are linked directly.
    pub fn unbind_reference(&mut self, name: &Atom) {
        let Some(links) = self.ref_links.as_mut() else {
            return;
        };
        let links = &mut links.0;
        if !links.iter().any(|link| link.mentions(name)) {
            return;
        }
        let mut neighbours: Vec<Atom> = Vec::new();
        for link in links.iter() {
            if let RefTarget::Var(target) = &link.target {
                if link.alias == *name {
                    neighbours.push(*target);
                } else if target == name {
                    neighbours.push(link.alias);
                }
            }
        }
        links.retain(|link| !link.mentions(name));
        if let Some((first, rest)) = neighbours.split_first() {
            for other in rest {
                links.push(RefLink {
                    alias: *other,
                    target: RefTarget::Var(*first),
                });
            }
        }
        if links.is_empty() {
            self.ref_links = None;
        }
    }

    /// Detach the elements bound out of the array `name` holds, for a
    /// write that replaces the whole array: the bound variables keep the
    /// elements of the array that was there before.
    pub fn detach_bound_elements(&mut self, name: &str) {
        let Some(links) = self.ref_links.as_mut() else {
            return;
        };
        let name = atom(name);
        links.0.retain(
            |link| !matches!(&link.target, RefTarget::Element { base, .. } if *base == name),
        );
        if links.0.is_empty() {
            self.ref_links = None;
        }
    }

    /// Carry the entry just written to `name` over to everything bound to
    /// it.
    pub(super) fn propagate_reference_write(&mut self, name: Atom) {
        let Some(links) = self.ref_links.as_deref() else {
            return;
        };
        if !links.0.iter().any(|link| link.mentions(&name)) {
            return;
        }
        let links = links.0.clone();
        let mut visited = vec![name];
        // Each name still to carry its write on, with the element of it
        // the write went to when it came in through a bound element.
        let mut pending: Vec<(Atom, Option<&[ArrayWriteKey]>)> = vec![(name, None)];
        while let Some((current, written_at)) = pending.pop() {
            let written = self.locals.get(&current).cloned().unwrap_or_default();
            let unresolved = self.unresolved.contains_key(&current);
            for link in &links {
                let (other, value, at) = match &link.target {
                    RefTarget::Var(target) if link.alias == current => {
                        (*target, written.clone(), written_at)
                    }
                    RefTarget::Var(target) if *target == current => {
                        (link.alias, written.clone(), written_at)
                    }
                    RefTarget::Element { base, keys } if link.alias == current => {
                        // A value that is not an array keeps its type: the
                        // write went through `offsetSet()` or failed.
                        let Some(array) = self.with_element_written(*base, keys, &written) else {
                            continue;
                        };
                        (*base, array, Some(keys.as_slice()))
                    }
                    RefTarget::Element { base, keys } if *base == current => {
                        // A write to one element leaves the others alone.
                        if written_at.is_some_and(|at| keys_disjoint(at, keys)) {
                            continue;
                        }
                        (link.alias, element_of(&written, keys), None)
                    }
                    _ => continue,
                };
                if visited.contains(&other) {
                    continue;
                }
                visited.push(other);
                pending.push((other, at));
                self.write_shared_value(other, value, unresolved);
            }
        }
    }

    /// The types `base` holds once `value` is written at `keys` in it, or
    /// `None` when `base` is not an array.
    fn with_element_written(
        &self,
        base: Atom,
        keys: &[ArrayWriteKey],
        value: &[ResolvedType],
    ) -> Option<Vec<ResolvedType>> {
        let base_type = match self.locals.get(&base).map(Vec::as_slice) {
            None => PhpType::array_shape(Vec::new()),
            Some([]) => PhpType::array(),
            Some(types) => ResolvedType::types_joined(types),
        };
        if !base_type.is_array_like() {
            return None;
        }
        let value = if value.is_empty() {
            PhpType::mixed()
        } else {
            ResolvedType::types_joined(value)
        };
        Some(vec![ResolvedType::from_type_string(
            merge_nested_array_write(&base_type, keys, &value, false),
        )])
    }

    /// Store `value` in `name` as a write through a reference. Whatever
    /// was recorded about the old value goes with it.
    fn write_shared_value(&mut self, name: Atom, value: Vec<ResolvedType>, unresolved: bool) {
        self.invalidate_dependent_keys(&name);
        self.invalidate_proofs(&name);
        if value.is_empty() && unresolved {
            self.unresolved.insert(name, ());
        } else {
            self.unresolved.remove(&name);
        }
        self.locals.insert(name, value);
    }

    /// Join the bindings two paths made, as one step of
    /// [`merge_branch`](Self::merge_branch).
    ///
    /// Only a binding every path made survives. One that only some paths
    /// made is dropped rather than kept as a may-alias: carrying writes
    /// across it would mix the values of names that are bound to each
    /// other on one path and not on the other (a variable walking down
    /// into an array, say) into one another.
    pub(super) fn join_reference_links(&mut self, other: &ScopeState) {
        let Some(mine) = self.ref_links.as_mut() else {
            return;
        };
        match other.ref_links.as_deref() {
            Some(theirs) => mine.0.retain(|link| theirs.0.contains(link)),
            None => mine.0.clear(),
        }
        if mine.0.is_empty() {
            self.ref_links = None;
        }
    }
}

/// Whether two element paths name different elements for certain: they
/// part at a pair of literal keys that differ.
fn keys_disjoint(a: &[ArrayWriteKey], b: &[ArrayWriteKey]) -> bool {
    for (a, b) in a.iter().zip(b) {
        match (a.literal(), b.literal()) {
            (Some(a), Some(b)) if a == b => {}
            (Some(_), Some(_)) => return true,
            _ => return false,
        }
    }
    false
}

/// The element of `array` at `keys`, or nothing when it cannot be read off
/// the type. Each alternative of a union is read on its own, so one
/// lacking the key does not pool the others' entries into the answer.
fn element_of(array: &[ResolvedType], keys: &[ArrayWriteKey]) -> Vec<ResolvedType> {
    fn read(ty: &PhpType, keys: &[ArrayWriteKey]) -> Option<PhpType> {
        let Some((key, rest)) = keys.split_first() else {
            return Some(ty.clone());
        };
        let members = ty.union_members();
        if members.len() > 1 {
            let read = members
                .into_iter()
                .map(|member| read(member, keys))
                .collect::<Option<Vec<_>>>()?;
            return Some(PhpType::union(read).simplified());
        }
        let next = match (key.literal(), ty.shape_entries()) {
            (_, _) if matches!(key, ArrayWriteKey::Append) => None,
            (Some(key), Some(_)) => ty.shape_value_type(&key).cloned(),
            _ => ty.iterable_element_type(),
        }?;
        read(&next, rest)
    }
    if array.is_empty() {
        return Vec::new();
    }
    read(&ResolvedType::types_joined(array), keys)
        .map(|ty| vec![ResolvedType::from_type_string(ty)])
        .unwrap_or_default()
}
