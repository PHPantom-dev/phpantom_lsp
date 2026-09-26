//! Array-shape writes: how a variable's tracked array type changes when
//! an element is assigned, pushed, unset, or `+`-merged into it.

use mago_syntax::cst::*;

use crate::atom::{bytes_to_str, literal_bytes_to_str};
use crate::php_type::{
    LiteralValue, PhpType, ShapeEntry, TypeKind, is_decimal_int_array_key, runtime_shape_keys,
};
use crate::types::ResolvedType;

/// Walk a (possibly nested) `ArrayAccess` chain and return the base
/// subject's scope key and the ordered list of index expressions from
/// outermost to innermost.
///
/// For `$var['a']['b']['c']` returns `Some(("$var", [expr_a, expr_b, expr_c]))`.
/// A property path is as much a base as a variable is: `$this->arr['a']`
/// returns `Some(("$this->arr", [expr_a]))`.  Returns `None` for any other
/// base.
pub(super) fn extract_nested_array_access_chain<'a, 'b>(
    outermost: &'a ArrayAccess<'b>,
) -> Option<(String, Vec<&'a Expression<'b>>)> {
    let mut keys: Vec<&'a Expression<'b>> = Vec::new();
    keys.push(outermost.index);

    let mut current: &'a Expression<'b> = outermost.array;
    loop {
        match current {
            Expression::ArrayAccess(inner) => {
                keys.push(inner.index);
                current = inner.array;
            }
            Expression::Variable(Variable::Direct(dv)) => {
                // We collected keys innermost-first; reverse so the
                // outermost key (closest to the variable) comes first.
                keys.reverse();
                return Some((bytes_to_str(dv.name).to_string(), keys));
            }
            Expression::Access(Access::Property(_) | Access::StaticProperty(_)) => {
                let base = crate::type_engine::types::narrowing::expr_to_subject_key(current)?;
                keys.reverse();
                return Some((base, keys));
            }
            _ => return None,
        }
    }
}

/// A single key segment in a (possibly nested) array write like
/// `$var['a'][$i]['b'] = …`.
pub(super) enum ArrayWriteKey {
    /// A string-literal key tracked as a shape entry, e.g. `['name']`.
    Shape(String),
    /// A dynamic (variable / expression / numeric) key tracked as a
    /// generic `array<K, V>` level. Carries the inferred key type.
    ///
    /// `slot` holds the index a written-out integer named, which lets a
    /// write update that positional slot of a tuple-style shape instead
    /// of collapsing the shape into the generic pair.
    Keyed {
        key_type: PhpType,
        slot: Option<usize>,
    },
    /// A trailing `[]` append, as in `$var['a'][] = …`. Only ever the
    /// last segment of a chain.
    Append,
}

/// Merge a nested array write with a mix of literal-string and dynamic
/// key segments into the base type.
///
/// Literal segments build/extend array shapes (like
/// [`merge_nested_shape_keys`]); dynamic segments build/extend generic
/// `array<K, V>` levels (like [`merge_keyed_type`]). For example,
/// merging `['data', $count, 'earnings']` with value `Decimal` into a
/// bare `array` produces:
///   `array{data: array<int, array{earnings: Decimal}>}`
///
/// A dynamic write onto an existing shape may land on any of its keys, so
/// the shape widens to the `array<K, V>` its entries and the written pair
/// describe together.
///
/// A trailing [`ArrayWriteKey::Append`] appends to the innermost level,
/// so `$rows[$id][] = $name` starting from `array{}` produces
/// `array<int, list<string>>`. Appending to a shape that tracks literal
/// keys adds the entry PHP's next free integer key would take.
///
/// `in_loop` says whether this write sits lexically inside a loop body.
/// The forward walker re-walks a loop body to a fixed point rather than
/// simulating its actual iteration count, so an append inside one runs an
/// unknowable number of times; `in_loop` keeps such an append widening
/// straight to the `list<T>`/`array<K, V>` it is building instead of
/// growing a tracked shape (or a pile of different-length shape variants)
/// by one entry per re-walk. Outside a loop the write runs exactly once,
/// so it keeps the shape's arity — see [`append_to_shape`].
pub(super) fn merge_nested_array_write(
    base: &PhpType,
    keys: &[ArrayWriteKey],
    value_type: &PhpType,
    in_loop: bool,
) -> PhpType {
    // Every level a write descends through holds at least the entry the
    // write put there, so the result is non-empty even when the tracked
    // key/value pair says nothing about which keys those are. That is what
    // lets a later `foreach` over the array know its body runs, and so
    // keeps a `null` sentinel assigned ahead of that loop from surviving
    // it. A write on only some paths gives the promise back at the branch
    // join, where `array{} | non-empty-array<K, V>` widens to
    // `array<K, V>`.
    merge_nested_array_write_inner(base, keys, value_type, in_loop).non_empty_array_form()
}

fn merge_nested_array_write_inner(
    base: &PhpType,
    keys: &[ArrayWriteKey],
    value_type: &PhpType,
    in_loop: bool,
) -> PhpType {
    debug_assert!(!keys.is_empty());
    // A union of shapes is one of them at runtime, and the write lands in
    // whichever it is. Merging it into the union as a whole found no shape
    // to update and fell back to the generic pair, which pooled every
    // slot's type into one: `$pair[3] = $x` on `list{A, B}|array{C, D}` read
    // back as `array<int, A|B|C|D|X>`.
    if let TypeKind::Union(members) = base.kind()
        && members
            .iter()
            .all(|member| member.shape_entries().is_some())
    {
        return PhpType::union(
            members
                .iter()
                .map(|member| merge_nested_array_write_inner(member, keys, value_type, in_loop))
                .collect(),
        );
    }
    match &keys[0] {
        ArrayWriteKey::Shape(key) => {
            if keys.len() == 1 {
                merge_shape_key(base, key, value_type)
            } else {
                let inner_base = shape_slot_base(base, key);
                let inner_merged =
                    merge_nested_array_write(&inner_base, &keys[1..], value_type, in_loop);
                merge_shape_key(base, key, &inner_merged)
            }
        }
        ArrayWriteKey::Keyed { key_type, slot } => {
            // A written-out index into a shape that already has that slot,
            // positional or spelled out as an integer key, updates it in
            // place, keeping the shape's arity and the slots the write did
            // not name. Folding it into the generic pair instead would
            // union every slot's type together, so reading any one of them
            // back gives the union.
            if let Some(slot) = slot
                && let Some(entries) = base.shape_entries()
                && let Some(index) = integer_keyed_entry_index(entries, *slot)
                    .or_else(|| positional_entry_index(entries, *slot))
            {
                let inner_merged = if keys.len() == 1 {
                    value_type.clone()
                } else {
                    merge_nested_array_write(
                        &entries[index].value_type,
                        &keys[1..],
                        value_type,
                        in_loop,
                    )
                };
                let mut updated: Vec<ShapeEntry> = entries.to_vec();
                updated[index].value_type = inner_merged.widen_scalar_literals();
                updated[index].optional = false;
                return PhpType::array_shape(updated);
            }
            // A written-out index the shape does not have yet adds that
            // slot, the same way a string key adds its entry. An empty
            // shape is left to the generic pair: it has no arity to keep,
            // and a run of `$data[0] = …; $data[1] = …;` onto `[]` is
            // rarely a promise about how many entries there are.
            if let Some(slot) = slot
                && let TypeKind::ArrayShape(entries) = base.kind()
                && !entries.is_empty()
                && let Some(runtime_keys) = runtime_shape_keys(entries)
            {
                let slot_key = slot.to_string();
                if !runtime_keys.contains(&slot_key) {
                    let inner_merged = if keys.len() == 1 {
                        value_type.clone()
                    } else {
                        merge_nested_array_write(
                            &PhpType::array_shape(Vec::new()),
                            &keys[1..],
                            value_type,
                            in_loop,
                        )
                    }
                    .widen_scalar_literals();
                    if i64::try_from(*slot).ok() == next_append_index(entries) {
                        return append_to_shape(base, entries, &inner_merged);
                    }
                    let mut updated: Vec<ShapeEntry> = entries.to_vec();
                    updated.push(ShapeEntry {
                        key: Some(slot_key),
                        value_type: inner_merged,
                        optional: false,
                    });
                    return PhpType::array_shape(updated);
                }
            }
            if keys.len() == 1
                && let Some(entries) = base.shape_entries()
                && let Some(updated) = write_literal_keys_into_shape(entries, key_type, value_type)
            {
                return updated;
            }
            if keys.len() == 1 {
                return merge_keyed_type(base, key_type, value_type);
            }
            // The inner write started from the element type the array
            // already holds, so what it produced is that element after the
            // write rather than a new value beside it, and it replaces the
            // element type instead of joining it. Joining would keep the
            // element as it was before the write, so a key added below a
            // dynamic key could never be read back as present.
            let inner_base = keyed_slot_base(base);
            let inner_merged =
                merge_nested_array_write(&inner_base, &keys[1..], value_type, in_loop);
            merge_keyed_type_inner(base, key_type, &inner_merged, false)
        }
        ArrayWriteKey::Append => {
            debug_assert_eq!(keys.len(), 1, "`[]` is only valid as the last segment");
            // A shape that tracks literal keys keeps them, and the append
            // lands on the next free integer key beside them. Outside a
            // loop a purely positional shape keeps its arity the same way;
            // inside one it takes the general mutation treatment instead,
            // because the number of appends the walker sees is not the
            // number of times the statement actually runs.
            if let TypeKind::ArrayShape(entries) = base.kind()
                && (!in_loop || entries.iter().any(|entry| entry.key.is_some()))
            {
                return append_to_shape(base, entries, value_type);
            }
            merge_push_type(base, value_type)
        }
    }
}

/// Apply an `unset($var[key1][key2]…)` element removal to a (possibly
/// nested) array type.
///
/// `keys` holds the array-access keys from outermost to innermost, each
/// `Some(literal)` for a string-literal key or `None` for a dynamic one —
/// mirroring [`ArrayWriteKey::Shape`]/[`ArrayWriteKey::Keyed`] but without
/// carrying a value type, since removal needs no new one. The innermost
/// level applies [`PhpType::after_element_unset`]; outer levels reuse the
/// same auto-vivifying descent as [`merge_nested_array_write`] to find the
/// slot the removal lands in, then write the updated slot back.
pub(super) fn apply_nested_array_unset(base: &PhpType, keys: &[Option<String>]) -> PhpType {
    debug_assert!(!keys.is_empty());
    let key = keys[0].as_deref();
    if keys.len() == 1 {
        return base.after_element_unset(key);
    }
    let inner_base = match key {
        Some(k) => shape_slot_base(base, k),
        None => keyed_slot_base(base),
    };
    let inner_updated = apply_nested_array_unset(&inner_base, &keys[1..]);
    match key {
        Some(k) => merge_shape_key(base, k, &inner_updated),
        None => {
            let key_type = base
                .iterable_key_type()
                .unwrap_or_else(|| PhpType::union(vec![PhpType::int(), PhpType::string()]));
            merge_keyed_type(base, &key_type, &inner_updated)
        }
    }
}

/// Extend a tracked shape with the entry a `[]` append writes.
///
/// PHP hands an append the next free integer key, so the shape keeps every
/// key it already tracks and gains one more, holding on to the exact value
/// written the same way an array literal's own entries do — a shape is
/// only worth tracking at all because it is exact, and unlike a mutation
/// onto a generic `array<K, V>` pair this keeps the shape's arity rather
/// than folding into a wider domain. When that index is not knowable — an
/// optional integer-keyed entry may or may not be there, and shifts every
/// index after it — the shape widens to `array<K, V>` instead.
fn append_to_shape(base: &PhpType, entries: &[ShapeEntry], value_type: &PhpType) -> PhpType {
    let Some(index) = next_append_index(entries) else {
        return merge_keyed_type(base, &PhpType::int(), value_type);
    };
    // A positional entry is read back by counting the positional entries
    // before it, so it only spells the same key the append writes while no
    // explicit integer key has moved the index along.
    let positional_count = entries.iter().filter(|entry| entry.key.is_none()).count() as i64;
    let mut merged = entries.to_vec();
    merged.push(ShapeEntry {
        key: (index != positional_count).then(|| index.to_string()),
        value_type: value_type.clone(),
        optional: false,
    });
    let shape = PhpType::array_shape(merged);
    if base.is_list_shape() && index == positional_count {
        PhpType::as_list_shape(shape)
    } else {
        shape
    }
}

/// The integer key a `[]` append writes to a shape holding `entries`.
///
/// Positional entries take the next free index in order, an explicit
/// integer key raises the cursor past itself, and string keys leave it
/// alone. Returns `None` when an optional integer-keyed entry leaves the
/// next index unknowable.
fn next_append_index(entries: &[ShapeEntry]) -> Option<i64> {
    let mut next: i64 = 0;
    for entry in entries {
        let index = match entry.key.as_deref() {
            None => Some(next),
            Some(key) => key.parse::<i64>().ok(),
        };
        let Some(index) = index else { continue };
        if entry.optional {
            return None;
        }
        next = next.max(index.checked_add(1)?);
    }
    Some(next)
}

/// The type an inner write should build on for the shape entry `key`.
///
/// A missing entry auto-vivifies: PHP creates an empty array there, so an
/// empty shape (rather than an unconstrained `array`) is the honest
/// starting point — it lets the nested merge below build a precise type
/// instead of unioning against `mixed`.
fn shape_slot_base(base: &PhpType, key: &str) -> PhpType {
    if let Some(value) = base.shape_value_type(key) {
        return value.clone();
    }
    // A base that tracks one value type for every key already describes
    // what sits under this one, tracked or not.
    if matches!(base.kind(), TypeKind::ArrayShape(_)) {
        return PhpType::array_shape(Vec::new());
    }
    keyed_slot_base(base)
}

/// The type an inner write should build on below a dynamic key segment.
///
/// Like [`shape_slot_base`], an unknown element type auto-vivifies to an
/// empty shape rather than an unconstrained `array`.
fn keyed_slot_base(base: &PhpType) -> PhpType {
    match base.iterable_element_type() {
        Some(elem) if !elem.is_empty() && !elem.is_mixed() => elem,
        _ => PhpType::array_shape(Vec::new()),
    }
}

/// Extract a string key from an array access index expression.
///
/// Returns `Some(key)` for string-literal keys like `'name'` or `"age"`.
/// Returns `None` for numeric keys, variable indices, and other
/// non-string-literal expressions — these are not tracked as shape
/// entries.
pub(super) fn extract_array_key_for_shape(index: &Expression<'_>) -> Option<String> {
    if let Expression::Literal(Literal::String(s)) = index {
        let key = match s.value {
            Some(bytes) => literal_bytes_to_str(bytes)?.to_string(),
            None => crate::text_scan::unquote_php_string(bytes_to_str(s.raw))
                .unwrap_or(bytes_to_str(s.raw))
                .to_string(),
        };
        // PHP casts canonical decimal-integer strings (including negatives)
        // to int keys. Keep non-canonical numeric-looking strings such as
        // `"08"`, `"+8"`, and `"1.5"` as exact shape keys.
        if is_decimal_int_array_key(&key) {
            return None;
        }
        Some(key)
    } else {
        None
    }
}

/// The literal integer an index expression spells out, if it is one.
///
/// A write through such an index can update the matching slot of a
/// tuple-style shape it already has (`$tuple[1] = …`). It deliberately
/// does not *create* a numeric-keyed shape: `$data[0] = 'x'` on a plain
/// array leaves the tracked `array<int, string>` pair alone, because a
/// written-out index is usually one of many an unrolled or generated
/// write sequence touches, not a promise about the array's arity.
pub(super) fn extract_array_write_index(index: &Expression<'_>) -> Option<usize> {
    if let Expression::Literal(Literal::Integer(int_lit)) = index {
        return int_lit.value.and_then(|v| usize::try_from(v).ok());
    }
    None
}

/// Merge a `(key, value_type)` pair into an existing `PhpType` to
/// produce an `ArrayShape`.
///
/// If `base` is already an `ArrayShape`, the key is added or updated.
/// Otherwise a new shape is created with just the given key.
///
/// Returns `PhpType::array_shape(entries)` with the merged entries.
fn merge_shape_key(base: &PhpType, key: &str, value_type: &PhpType) -> PhpType {
    // A base that tracks key and value types instead of individual keys
    // (`array<string, int>`, `list<User>`, `User[]`) still holds whatever
    // it held before the write. Rebuilding it as a one-entry shape would
    // claim the written key is the only one there, so the write folds into
    // the tracked pair instead.
    if base.is_array_like()
        && !matches!(base.kind(), TypeKind::ArrayShape(_))
        && base.iterable_key_type().is_some()
    {
        let key_type = if is_decimal_int_array_key(key) {
            PhpType::int()
        } else {
            PhpType::string()
        };
        return merge_keyed_type(base, &key_type, value_type);
    }

    let mut entries: Vec<ShapeEntry> = base
        .shape_entries()
        .map(<[ShapeEntry]>::to_vec)
        .unwrap_or_default();
    let written = ShapeEntry {
        key: Some(key.to_string()),
        value_type: value_type.widen_scalar_literals(),
        optional: false,
    };
    // PHP keeps an overwritten key where it was; only a new key goes on
    // the end.
    match entries
        .iter_mut()
        .find(|entry| entry.key.as_deref() == Some(key))
    {
        Some(existing) => *existing = written,
        None => entries.push(written),
    }

    PhpType::array_shape(entries)
}

/// Write `value_type` through a key known to be one of a few literals into
/// a shape that already holds every one of them, or `None` when that is not
/// the case.
///
/// The write lands on exactly one of the keys without saying which, so each
/// entry it may have hit holds either its old value or the new one; a
/// single literal is a write to that one entry.  Widening to `array<K, V>`
/// instead would lose every key, which is what a `$seen[$k] = true` after
/// an `isset($seen[$k])` check did.
fn write_literal_keys_into_shape(
    entries: &[ShapeEntry],
    key_type: &PhpType,
    value_type: &PhpType,
) -> Option<PhpType> {
    let written: Vec<String> = key_type
        .union_members()
        .iter()
        .map(|member| match member.as_literal()? {
            LiteralValue::Int(raw) => is_decimal_int_array_key(raw).then(|| raw.to_string()),
            // A decimal-integer string names the same entry as the integer,
            // and shapes record both under the same spelling.
            literal @ LiteralValue::String(_) => {
                literal.string_content().map(std::borrow::Cow::into_owned)
            }
            LiteralValue::Float(_) => None,
        })
        .collect::<Option<_>>()?;
    let runtime_keys = runtime_shape_keys(entries)?;
    if !written.iter().all(|key| runtime_keys.contains(key)) {
        return None;
    }
    let value_type = value_type.widen_scalar_literals();
    let single = written.len() == 1;

    let updated: Vec<ShapeEntry> = entries
        .iter()
        .zip(&runtime_keys)
        .map(|(entry, key)| {
            let mut entry = entry.clone();
            if written.contains(key) {
                if single {
                    entry.value_type = value_type.clone();
                    entry.optional = false;
                } else {
                    entry.value_type = PhpType::join_runtime_value_types(vec![
                        entry.value_type.clone(),
                        value_type.clone(),
                    ]);
                }
            }
            entry
        })
        .collect();
    Some(PhpType::array_shape(updated))
}

/// The position in `entries` of the entry whose explicit key is the
/// integer `index`.
fn integer_keyed_entry_index(entries: &[ShapeEntry], index: usize) -> Option<usize> {
    entries.iter().position(|entry| {
        entry
            .key
            .as_deref()
            .filter(|key| is_decimal_int_array_key(key))
            .and_then(|key| key.parse::<usize>().ok())
            == Some(index)
    })
}

/// The position in `entries` of the `index`th unkeyed entry.
fn positional_entry_index(entries: &[ShapeEntry], index: usize) -> Option<usize> {
    let mut positional = 0usize;
    for (slot, entry) in entries.iter().enumerate() {
        if entry.key.is_none() {
            if positional == index {
                return Some(slot);
            }
            positional += 1;
        }
    }
    None
}

/// Merge a push element type into an existing `PhpType` to produce
/// a `Generic("list", …)` type.
///
/// If `base` already has a generic value type (e.g. `list<User>`),
/// the new type is unioned with it (e.g. `list<User|Admin>`).
/// Otherwise, produces `list<value_type>`.
///
/// Returns `PhpType::list(elem_type)` or
/// `PhpType::named("array")` when no element types are available.
pub(super) fn merge_push_type(base: &PhpType, value_type: &PhpType) -> PhpType {
    // A base that already holds string keys stays a keyed array: an append
    // adds an integer key beside them, it does not make the value a list.
    if base.is_array_like()
        && base
            .iterable_key_type()
            .is_some_and(|key| !key.is_subtype_of(&PhpType::int()))
    {
        return merge_keyed_type(base, &PhpType::int(), value_type);
    }

    let mut elem_types: Vec<PhpType> = Vec::new();
    let value_type = value_type.widen_scalar_literals();

    // Extract existing element types from the base.
    let existing_elem = base.iterable_element_type();
    if let Some(existing_elem) = &existing_elem {
        for member in existing_elem.union_members() {
            if !member.is_empty() {
                elem_types.push(member.clone());
            }
        }
    }

    // Add new value type members (union-aware).
    for member in value_type.union_members() {
        if !member.is_empty() && !elem_types.iter().any(|e| e.equivalent(member)) {
            elem_types.push(member.clone());
        }
    }

    if elem_types.is_empty() {
        return PhpType::array();
    }

    let elem_type = join_element_types(elem_types, &value_type, existing_elem.as_ref());

    PhpType::list(elem_type)
}

/// Join the member types collected for one of a container's positions
/// (element or key), keeping the benevolence marker the collection dropped.
///
/// Splitting a union into its members loses the marker sitting above them,
/// and a type that was lenient on its own has to stay lenient once it is
/// inside `list<…>` / `array<…, …>`: the position's comparison is the same
/// comparison a direct return makes, so a union nobody wrote down would
/// otherwise be enforced against every declared type the moment it is
/// collected into a container. That applies just as much to the key an
/// `Arg[]` hands out as to the value beside it. The marker only survives
/// while every contributing source carried it — a member the code did
/// spell out makes the whole position worth enforcing again.
fn join_element_types(
    members: Vec<PhpType>,
    incoming: &PhpType,
    existing: Option<&PhpType>,
) -> PhpType {
    let joined = PhpType::join_runtime_value_types(members);
    let existing_is_lenient = existing.is_none_or(|ty| ty.is_empty() || ty.is_benevolent());
    if incoming.is_benevolent() && existing_is_lenient {
        return PhpType::benevolent(joined);
    }
    joined
}

/// Merge a keyed element type into an existing `PhpType` to produce
/// a `Generic("array", …)` type.
///
/// Similar to [`merge_push_type`] but preserves the key type from the
/// index expression instead of assuming sequential integer keys.
///
/// When the base already has a generic value type (e.g.
/// `array<string, User>`), the new value type is unioned with it and
/// key types are unioned as well.
///
/// Returns `PhpType::generic_array(key, val)`,
/// `PhpType::generic_array_val(val)` when no key types are
/// available, or `PhpType::named("array")` when no element types
/// are available.
pub(super) fn merge_keyed_type(
    base: &PhpType,
    key_type: &PhpType,
    value_type: &PhpType,
) -> PhpType {
    merge_keyed_type_inner(base, key_type, value_type, true)
}

/// [`merge_keyed_type`], but with `union_values` false the written value
/// replaces the base's value type instead of joining it. The keys still
/// join either way.
fn merge_keyed_type_inner(
    base: &PhpType,
    key_type: &PhpType,
    value_type: &PhpType,
    union_values: bool,
) -> PhpType {
    // Normalizing rebuilds a key through `kind()`, which sees straight
    // through the benevolence marker, so the leniency decision below reads
    // the types as they arrived rather than as they normalize.
    let existing_key = base.iterable_key_type();
    let normalized_key = normalize_array_key_type(key_type)
        .unwrap_or_else(|| PhpType::union(vec![PhpType::int(), PhpType::string()]));
    let value_type = value_type.widen_scalar_literals();

    // Collect existing key types from the base.
    let mut key_types: Vec<PhpType> = Vec::new();
    if let Some(normalized_existing) = existing_key
        .as_ref()
        .and_then(normalize_array_key_type)
        .filter(|key| !key.is_empty())
    {
        for member in normalized_existing.union_members() {
            if !key_types.iter().any(|e| e.equivalent(member)) {
                key_types.push(member.clone());
            }
        }
    }
    // Add new key type members.
    for member in normalized_key.union_members() {
        if !member.is_empty() && !key_types.iter().any(|e| e.equivalent(member)) {
            key_types.push(member.clone());
        }
    }

    // Collect existing value types from the base.
    let mut elem_types: Vec<PhpType> = Vec::new();
    let existing_elem = base.iterable_element_type().filter(|_| union_values);
    if let Some(existing_elem) = &existing_elem {
        for member in existing_elem.union_members() {
            if !member.is_empty() {
                elem_types.push(member.clone());
            }
        }
    }
    // Add new value type members.
    for member in value_type.union_members() {
        if !member.is_empty() && !elem_types.iter().any(|e| e.equivalent(member)) {
            elem_types.push(member.clone());
        }
    }

    if elem_types.is_empty() {
        return PhpType::array();
    }

    let val_type = join_element_types(elem_types, &value_type, existing_elem.as_ref());

    if key_types.is_empty() {
        // No key type information — use a single-param generic.
        PhpType::generic_array_val(val_type)
    } else {
        let k_type = join_element_types(key_types, key_type, existing_key.as_ref());
        PhpType::generic_array(k_type, val_type)
    }
}

/// Merge the operands of an array `+` / `+=`.
///
/// PHP's array union keeps every key already present on the left and adds
/// only the keys the right side contributes. Two tracked shapes therefore
/// merge into a single shape; anything looser keeps whatever key/value
/// information both sides carry instead of collapsing to a bare `array`.
pub(super) fn merge_array_plus(lhs: &PhpType, rhs: &PhpType) -> PhpType {
    if let (TypeKind::ArrayShape(lhs_entries), TypeKind::ArrayShape(rhs_entries)) =
        (lhs.kind(), rhs.kind())
        && let Some(lhs_keys) = runtime_shape_keys(lhs_entries)
        && let Some(rhs_keys) = runtime_shape_keys(rhs_entries)
    {
        // Two positional shapes union index by index, so their entries stay
        // positional. Once either side spells a key out, the entries behind
        // it no longer sit at their own index.
        let all_positional = lhs_entries
            .iter()
            .chain(rhs_entries)
            .all(|entry| entry.key.is_none());
        let mut entries: Vec<ShapeEntry> = Vec::with_capacity(lhs_entries.len());
        for (entry, key) in lhs_entries.iter().zip(&lhs_keys) {
            let rhs_match = rhs_keys
                .iter()
                .position(|other| other == key)
                .map(|index| &rhs_entries[index])
                .filter(|_| entry.optional);
            match rhs_match {
                // An optional left key may be absent at runtime, in which
                // case the right side's value for it wins.
                Some(other) => entries.push(ShapeEntry {
                    key: entry.key.clone(),
                    value_type: PhpType::union(vec![
                        entry.value_type.clone(),
                        other.value_type.clone(),
                    ]),
                    optional: other.optional,
                }),
                None => entries.push(entry.clone()),
            }
        }
        for (entry, key) in rhs_entries.iter().zip(&rhs_keys) {
            if lhs_keys.contains(key) {
                continue;
            }
            entries.push(ShapeEntry {
                key: entry
                    .key
                    .clone()
                    .or_else(|| (!all_positional).then(|| key.clone())),
                ..entry.clone()
            });
        }
        let merged = PhpType::array_shape(entries);
        return if all_positional && lhs.is_list_shape() && rhs.is_list_shape() {
            PhpType::as_list_shape(merged)
        } else {
            merged
        };
    }

    let Some(rhs_value) = rhs.iterable_element_type().filter(|v| !v.is_empty()) else {
        return PhpType::array();
    };
    let rhs_key = rhs
        .iterable_key_type()
        .unwrap_or_else(|| PhpType::union(vec![PhpType::int(), PhpType::string()]));
    merge_keyed_type(lhs, &rhs_key, &rhs_value)
}

/// Infer the source type of an array-access index expression from what the
/// shared RHS resolver made of it.
///
/// [`merge_keyed_type`] performs collection-boundary normalization exactly
/// once. Returning the exact source type here preserves distinctions such as a
/// known non-numeric string versus a broad `string`.
///
/// The caller resolves the index rather than this function, so that an index
/// PHP only builds by computing it (`$m[$line + 1]`) goes through the same
/// path an assignment's RHS does. Falling back to `int|string` for anything
/// the narrower expression resolver cannot answer is what widened an
/// all-integer key domain to the full `array-key`.
pub(super) fn infer_array_key_type(index: &Expression<'_>, resolved: &[ResolvedType]) -> PhpType {
    // Fast path: literal values.
    if let Expression::Literal(Literal::Integer(_)) = index {
        return PhpType::int();
    }

    if !resolved.is_empty() {
        let joined = ResolvedType::types_joined(resolved);
        if !joined.is_mixed() {
            return joined;
        }
    }

    // Benevolent for the same reason an unknown foreach key is: this
    // `int|string` is PHP's whole key domain standing in for a type nobody
    // measured, so the array it keys must not be held to both branches.
    PhpType::benevolent(PhpType::union(vec![PhpType::int(), PhpType::string()]))
}

/// Normalize every possible runtime array-key branch to `int` or `string`.
///
/// PHP truncates float keys and coerces bool keys to int, while null becomes
/// the empty string key. Literal and refined scalar types must not escape
/// into `array<K, V>` payloads.
pub(super) fn normalize_array_key_type(ty: &PhpType) -> Option<PhpType> {
    fn is_non_numeric_string_domain(ty: &PhpType) -> bool {
        match ty.kind() {
            TypeKind::ClassString(_) | TypeKind::InterfaceString(_) => true,
            TypeKind::Named(name) => matches!(
                name.to_ascii_lowercase().as_str(),
                "class-string"
                    | "interface-string"
                    | "trait-string"
                    | "enum-string"
                    | "callable-string"
            ),
            TypeKind::Generic(generic) => matches!(
                generic.name.to_ascii_lowercase().as_str(),
                "class-string" | "interface-string"
            ),
            _ => false,
        }
    }

    fn collect(ty: &PhpType, normalized: &mut Vec<PhpType>) -> bool {
        match ty.kind() {
            TypeKind::Union(members) => members.iter().all(|member| collect(member, normalized)),
            TypeKind::Nullable(inner) => {
                if !collect(inner, normalized) {
                    return false;
                }
                // PHP converts the nullable branch to the empty string key.
                push_unique(normalized, PhpType::string());
                true
            }
            _ if ty.is_null() => {
                push_unique(normalized, PhpType::string());
                true
            }
            TypeKind::Literal(value) if matches!(&**value, LiteralValue::String(_)) => {
                let content = value.string_content().unwrap_or_default();
                push_unique(
                    normalized,
                    if is_decimal_int_array_key(&content) {
                        PhpType::int()
                    } else {
                        PhpType::string()
                    },
                );
                true
            }
            _ if ty.is_array_key() => {
                push_unique(normalized, PhpType::int());
                push_unique(normalized, PhpType::string());
                true
            }
            _ if ty.is_int_coercible_key() => {
                push_unique(normalized, PhpType::int());
                true
            }
            // A class, interface, or callable name can never be a decimal
            // integer, so PHP stores it as the string it is and the refined
            // type is already a valid key.
            _ if is_non_numeric_string_domain(ty) => {
                push_unique(normalized, ty.clone());
                true
            }
            _ if ty.is_string_subtype() => {
                // Only a *literal* decimal-integer string is known to become
                // an int key (handled above).  A broad string keeps `string`,
                // because widening it to `int|string` would mismatch every
                // `array<string, T>` the value is declared against.
                push_unique(normalized, PhpType::string());
                true
            }
            _ => false,
        }
    }

    fn push_unique(types: &mut Vec<PhpType>, member: PhpType) {
        if !types.iter().any(|existing| existing == &member) {
            types.push(member);
        }
    }

    let mut normalized = Vec::new();
    if !collect(ty, &mut normalized) || normalized.is_empty() {
        return None;
    }
    match normalized.len() {
        1 => normalized.into_iter().next(),
        _ => Some(PhpType::union(normalized)),
    }
}

#[cfg(test)]
#[path = "array_shape_writes_tests.rs"]
mod tests;
