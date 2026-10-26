//! Reading and joining array and object shapes.

use super::*;

impl PhpType {
    /// Look up the value type for a specific key in an array shape.
    ///
    /// Given a parsed `array{name: string, user: User}` and key `"user"`,
    /// returns `Some(&PhpType::named("User"))`.
    ///
    /// For positional (unkeyed) entries like `array{User, Address}`, a
    /// numeric string key (e.g. `"0"`, `"1"`) matches the entry at that
    /// index position. This mirrors PHPStan's behaviour where positional
    /// entries implicitly have numeric keys.
    ///
    /// Also handles nullable shapes (`?array{…}`) by delegating to the
    /// inner type.
    ///
    /// Returns `None` if this is not an array shape or the key is not found.
    pub fn shape_value_type(&self, key: &str) -> Option<&PhpType> {
        self.shape_entry(key).map(|entry| &entry.value_type)
    }

    /// The shape entry `key` addresses, which [`shape_value_type`] reads the
    /// value type off.
    ///
    /// A caller that has to know whether the entry may be absent (an offset
    /// read of a missing key is `null`, an argument for it is not required)
    /// needs the entry itself rather than just its type.
    ///
    /// [`shape_value_type`]: PhpType::shape_value_type
    pub fn shape_entry(&self, key: &str) -> Option<&ShapeEntry> {
        if let Some(unsealed) = self.as_unsealed_shape() {
            return unsealed.shape.shape_entry(key);
        }
        match self.kind() {
            TypeKind::ArrayShape(entries) => {
                // First try an exact key match (handles named and explicit
                // numeric keys like `array{0: User, 1: Address}`).
                if let Some(entry) = entries.iter().find(|e| e.key.as_deref() == Some(key)) {
                    return Some(entry);
                }
                // Fall back to positional index matching: if the key is a
                // valid numeric index, match the Nth positional (unkeyed)
                // entry. This handles `array{User, Address}` where the
                // entries have `key: None`.
                if let Ok(idx) = key.parse::<usize>() {
                    let mut positional_idx = 0usize;
                    for entry in entries {
                        if entry.key.is_none() {
                            if positional_idx == idx {
                                return Some(entry);
                            }
                            positional_idx += 1;
                        }
                    }
                }
                None
            }
            TypeKind::Nullable(inner) => inner.shape_entry(key),
            // `array<string, mixed>&array{at: Carbon}` holds the entry its
            // shape member lists; the other members only bound the rest.
            TypeKind::Union(members) | TypeKind::Intersection(members) => {
                members.iter().find_map(|m| m.shape_entry(key))
            }
            _ => None,
        }
    }

    /// Look up the value type for a specific key in an array shape,
    /// returning an owned `PhpType`.
    ///
    /// Unlike [`shape_value_type`](Self::shape_value_type), this method
    /// accounts for optional entries: when a key is marked optional
    /// (`key?: type`), the returned type is wrapped in `Nullable` so
    /// that downstream narrowing can strip `null` when the key is
    /// known to be present.
    ///
    /// Returns `None` if this is not an array shape or the key is not
    /// found.
    pub fn extract_shape_key_type(&self, key: &str) -> Option<PhpType> {
        if let Some(unsealed) = self.as_unsealed_shape() {
            return unsealed.shape.extract_shape_key_type(key);
        }
        match self.kind() {
            TypeKind::ArrayShape(_) => self.shape_entry(key).map(|entry| {
                if entry.optional {
                    PhpType::nullable(entry.value_type.clone())
                } else {
                    entry.value_type.clone()
                }
            }),
            TypeKind::Nullable(inner) => inner.extract_shape_key_type(key),
            TypeKind::Union(members) => {
                // Every alternative contributes its own entry: reading
                // offset 1 of `array{string, null}|array{string, Err}` is
                // `null|Err`, not whichever alternative comes first.
                let found: Vec<PhpType> = members
                    .iter()
                    .filter_map(|m| m.extract_shape_key_type(key))
                    .collect();
                (!found.is_empty()).then(|| PhpType::union(found))
            }
            _ => None,
        }
    }

    /// Return the shape entries if this is an `ArrayShape` or `ObjectShape`.
    ///
    /// Also handles nullable shapes by delegating to the inner type.
    /// Returns `None` for all other variants.
    pub fn shape_entries(&self) -> Option<&[ShapeEntry]> {
        match self.kind() {
            TypeKind::ArrayShape(entries) | TypeKind::ObjectShape(entries) => Some(entries),
            TypeKind::Nullable(inner) => inner.shape_entries(),
            TypeKind::Union(members) => {
                // Find the first array/object shape member in the union.
                members.iter().find_map(|m| m.shape_entries())
            }
            _ => None,
        }
    }

    /// The entries a shape is known to hold, whether it is sealed or not:
    /// what [`shape_entries`](Self::shape_entries) answers, and also the
    /// listed entries of an unsealed `array{…, ...<K, V>}`.
    ///
    /// The entries of an unsealed shape are not the whole array, so this
    /// is for a caller that only reads them (offering a key, say), never
    /// for one that builds a shape back out of them.
    pub fn known_shape_entries(&self) -> Option<&[ShapeEntry]> {
        if let Some(unsealed) = self.as_unsealed_shape() {
            return unsealed.shape.shape_entries();
        }
        match self.kind() {
            TypeKind::ArrayShape(entries) | TypeKind::ObjectShape(entries) => Some(entries),
            TypeKind::Nullable(inner) => inner.known_shape_entries(),
            TypeKind::Union(members) | TypeKind::Intersection(members) => {
                members.iter().find_map(|m| m.known_shape_entries())
            }
            _ => None,
        }
    }

    /// This type as an array shape, whether it is sealed or unsealed: the
    /// entries it lists, and what the entries beyond them are known to be.
    ///
    /// `None` for anything that is not itself a shape, nullable and union
    /// wrappers included.
    pub(crate) fn shape_parts(&self) -> Option<ShapeParts<'_>> {
        if let Some(unsealed) = self.as_unsealed_shape() {
            let TypeKind::ArrayShape(entries) = unsealed.shape.kind() else {
                return None;
            };
            return Some(ShapeParts {
                entries,
                tail: Some((&unsealed.key, &unsealed.value)),
                is_list: unsealed.shape.is_list_shape(),
            });
        }
        let TypeKind::ArrayShape(entries) = self.kind() else {
            return None;
        };
        Some(ShapeParts {
            entries,
            tail: None,
            is_list: self.is_list_shape(),
        })
    }

    /// Return `true` if this type is an array shape (`array{…}`).
    ///
    /// Also returns `true` for nullable shapes written as either
    /// `?array{…}` or `array{…}|null`.
    pub fn is_array_shape(&self) -> bool {
        self.nullable_array_shape_parts().is_some()
    }

    /// Return `true` if this type is a list shape (`list{…}`), which an
    /// `array{…}` written with the same entries is not: only the former
    /// requires the keys to run `0, 1, 2, …` in order.
    pub fn is_list_shape(&self) -> bool {
        matches!(self.raw_kind(), TypeKind::ListShape(_))
    }

    /// Return `true` if this type is an object shape (`object{…}`).
    ///
    /// Also returns `true` for `?object{…}`.
    pub fn is_object_shape(&self) -> bool {
        match self.kind() {
            TypeKind::ObjectShape(_) => true,
            TypeKind::Nullable(inner) => inner.is_object_shape(),
            _ => false,
        }
    }

    /// Join two array shapes into a single shape that covers both
    /// variants.
    ///
    /// This is the union of two shapes expressed as one shape:
    /// `array{a: int}` joined with `array{a: int, b: string}` is
    /// `array{a: int, b?: string}`.
    ///
    /// - A key present on both sides unions the two value types
    ///   (recursively joining nested shapes) and stays required unless
    ///   optional on either side.
    /// - A key present on only one side becomes optional — the other
    ///   variant does not guarantee it.
    ///
    /// Branch merging uses this to fold the shape a variable has after
    /// one branch with the shape it has after another.  Folding keeps
    /// the variable at a single tracked shape no matter how many
    /// branches write to it; accumulating one variant per branch
    /// instead makes every later merge compare all variants pairwise,
    /// which turns large procedural methods with hundreds of
    /// conditional writes into quadratic-and-worse walks.
    ///
    /// Handles nullable shapes written as `?array{…}` or
    /// `array{…}|null` on either side (the join is nullable when either
    /// side is). Returns `None` when either side is not an array shape or
    /// contains positional (unkeyed) entries — those are list-style shapes
    /// where a per-key join is not meaningful.
    pub fn join_shapes(&self, other: &PhpType) -> Option<PhpType> {
        let (left, left_nullable) = self.nullable_array_shape_parts()?;
        let (right, right_nullable) = other.nullable_array_shape_parts()?;
        let joined = PhpType::array_shape(Self::join_shape_entries(left, right)?);
        if left_nullable || right_nullable {
            Some(PhpType::nullable(joined))
        } else {
            Some(joined)
        }
    }

    /// Extract the one array shape this type describes, plus whether it
    /// also admits `null`.
    ///
    /// PHPDoc accepts both `?array{…}` and `array{…}|null`, and
    /// [`union`](Self::union) does not fold the second spelling into the
    /// first, so the two arrive here as different type kinds. Branch
    /// merging has to treat them as one type or every write to a nullable
    /// shape leaves a separate alternative behind.
    ///
    /// A union naming more than one shape, or anything besides a shape and
    /// `null`, has no single shape to answer with and returns `None`.
    fn nullable_array_shape_parts(&self) -> Option<(&[ShapeEntry], bool)> {
        match self.kind() {
            TypeKind::ArrayShape(entries) => Some((entries, false)),
            TypeKind::Nullable(inner) => {
                let (entries, _) = inner.nullable_array_shape_parts()?;
                Some((entries, true))
            }
            TypeKind::Union(members) => {
                let mut shape: Option<&[ShapeEntry]> = None;
                let mut nullable = false;
                for member in members {
                    if member.is_null() {
                        nullable = true;
                    } else if let TypeKind::ArrayShape(entries) = member.kind() {
                        if shape.replace(entries).is_some() {
                            return None;
                        }
                    } else {
                        return None;
                    }
                }
                shape.map(|entries| (entries, nullable))
            }
            _ => None,
        }
    }

    /// Join two shape entry lists (see [`join_shapes`]).
    ///
    /// Entries are paired by the key they occupy at runtime, so a
    /// positional entry an append added beside named keys lines up with
    /// the index it sits at. Keys from `a` keep their order; keys only in
    /// `b` follow in `b`'s order. Returns `None` when either side's keys
    /// are not decidable, and when both sides are a bare list of values:
    /// two literals written slot by slot describe unrelated arrays, and
    /// pairing their slots up would invent a row neither one holds.
    ///
    /// When only one side is a bare list, the other side's explicit key at
    /// each shared position is the anchor that makes pairing safe, *if* the
    /// two sides agree on the value that position holds. A position they
    /// disagree on (`array{string}` against `array{0: int}`) is not a
    /// narrower and a wider spelling of the same shape, it is two
    /// differently-tagged alternatives that merely happen to share a
    /// length, so the whole pairing is refused rather than inventing an
    /// `int|string` at a slot neither alternative holds both of.
    ///
    /// [`join_shapes`]: Self::join_shapes
    fn join_shape_entries(a: &[ShapeEntry], b: &[ShapeEntry]) -> Option<Vec<ShapeEntry>> {
        fn is_value_list(entries: &[ShapeEntry]) -> bool {
            entries.iter().any(|entry| entry.key.is_none())
                && !entries.iter().any(|entry| {
                    entry
                        .key
                        .as_deref()
                        .is_some_and(|k| !is_canonical_int_key(k))
                })
        }
        let a_is_list = is_value_list(a);
        let b_is_list = is_value_list(b);
        if a_is_list && b_is_list {
            return None;
        }
        if a_is_list || b_is_list {
            let (list_side, keyed_side) = if a_is_list { (a, b) } else { (b, a) };
            let list_keys = runtime_shape_keys(list_side)?;
            let keyed_keys = runtime_shape_keys(keyed_side)?;
            // At least one position has to actually land on an explicit key
            // in `keyed_side`, or there is no anchor at all: an empty shape
            // (`array{}`) shares no key with anything, so it must not be
            // free to absorb an unrelated positional shape just because
            // nothing contradicts it.
            let mut overlapped = false;
            let anchored = list_side.iter().zip(&list_keys).all(|(entry, key)| {
                match keyed_keys.iter().position(|other| other == key) {
                    Some(index) => {
                        overlapped = true;
                        let other_value = &keyed_side[index].value_type;
                        entry.value_type.is_mixed()
                            || other_value.is_mixed()
                            || entry.value_type.equivalent(other_value)
                    }
                    None => true,
                }
            });
            if !overlapped || !anchored {
                return None;
            }
        }
        let keys_a = runtime_shape_keys(a)?;
        let keys_b = runtime_shape_keys(b)?;
        // An entry that may be absent no longer sits at the index a
        // positional spelling counts it out at, so it takes its key along.
        let entry =
            |source: &ShapeEntry, key: &String, value_type: PhpType, optional: bool| ShapeEntry {
                key: match &source.key {
                    Some(key) => Some(key.clone()),
                    None if optional => Some(key.clone()),
                    None => None,
                },
                value_type,
                optional,
            };
        let mut joined: Vec<ShapeEntry> = Vec::with_capacity(a.len().max(b.len()));
        for (ea, key) in a.iter().zip(&keys_a) {
            match keys_b.iter().position(|other| other == key) {
                Some(index) => joined.push(entry(
                    ea,
                    key,
                    Self::join_values(&ea.value_type, &b[index].value_type),
                    ea.optional || b[index].optional,
                )),
                None => joined.push(entry(ea, key, ea.value_type.clone(), true)),
            }
        }
        for (eb, key) in b.iter().zip(&keys_b) {
            if !keys_a.contains(key) {
                joined.push(entry(eb, key, eb.value_type.clone(), true));
            }
        }
        Some(joined)
    }

    /// Union two value types for a joined shape key.
    ///
    /// Equivalent members are kept once and nested shape members are
    /// joined rather than accumulated, so repeated merges cannot grow
    /// the value type without bound. A member whose produced values are
    /// already covered by another (`123` beside `int`) is dropped for the
    /// same reason: one branch writing a literal and another writing the
    /// base type must not leave both spellings behind.
    fn join_values(a: &PhpType, b: &PhpType) -> PhpType {
        use crate::php_type::runtime_join::is_runtime_value_subtype;

        if a.equivalent(b) {
            return a.clone();
        }
        let mut members: Vec<PhpType> = a.union_members().into_iter().cloned().collect();
        'incoming: for m in b.union_members() {
            let mut replaced = None;
            for (index, existing) in members.iter_mut().enumerate() {
                if existing.equivalent(m) {
                    continue 'incoming;
                }
                if is_runtime_value_subtype(m, existing) {
                    continue 'incoming;
                }
                if is_runtime_value_subtype(existing, m) {
                    *existing = m.clone();
                    replaced = Some(index);
                    break;
                }
                if let Some(joined) = existing.join_shapes(m) {
                    *existing = joined;
                    continue 'incoming;
                }
            }
            match replaced {
                // The incoming member may cover more than the one it took
                // the place of: `'b'|'c'` joined with `string` is `string`.
                Some(index) => {
                    let mut position = 0;
                    members.retain(|existing| {
                        let keep = position <= index || !is_runtime_value_subtype(existing, m);
                        position += 1;
                        keep
                    });
                }
                None => members.push(m.clone()),
            }
        }
        if members.len() == 1 {
            members.into_iter().next().unwrap()
        } else {
            PhpType::union(members)
        }
    }

    /// Look up the value type for a specific property in an object shape.
    ///
    /// Given a parsed `object{name: string, user: User}` and key `"user"`,
    /// returns `Some(&PhpType::named("User"))`.
    ///
    /// Also handles nullable object shapes (`?object{…}`).
    ///
    /// Returns `None` if this is not an object shape or the property
    /// is not found.
    pub fn object_shape_property_type(&self, prop: &str) -> Option<&PhpType> {
        match self.kind() {
            TypeKind::ObjectShape(entries) => entries
                .iter()
                .find(|e| e.key.as_deref() == Some(prop))
                .map(|e| &e.value_type),
            TypeKind::Nullable(inner) => inner.object_shape_property_type(prop),
            _ => None,
        }
    }
}
