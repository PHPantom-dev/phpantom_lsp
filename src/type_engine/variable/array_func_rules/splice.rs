//! The array `array_splice()` leaves in the variable it takes by reference.
//!
//! A constant shape with a literal offset and length is rewritten entry by
//! entry, following PHPStan's `ConstantArrayType::spliceArray()`: the
//! replacement's values go in at the offset, the entries the call removes
//! (the same ones `array_slice()` would return) come out, and the int keys
//! are renumbered while string keys stay. An optional entry before the
//! offset may shift which entries are removed, so those near the edges
//! become optional. Anything else becomes the generic array of both arrays'
//! keys and values, as in `ArrayType::spliceArray()`.

use crate::php_type::{PhpType, ShapeEntry, TypeKind, canonical_int_key};

use super::{explicit_key_shape_entries, generic_array_parts, shape_with_implicit_keys};

/// The `$length` argument of an `array_splice()` call.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::type_engine) enum SpliceLength {
    /// Left out or `null`: everything from the offset to the end.
    ToEnd,
    /// A literal int.
    Literal(i64),
    /// Anything else.
    Unknown,
}

/// What `array_splice($array, $offset, $length, $replacement)` leaves in
/// `$array`, given the array's type before the call, the offset when it is a
/// literal int, and the replacement already cast to an array (`array{}` when
/// there is none).
///
/// `exact` says a shape may be rewritten entry by entry; a loop body, which
/// the walker re-walks until the types settle, passes `false` so a shape
/// that changes size on every pass does not keep changing.
///
/// `None` when some alternative of `base` is not an array.
pub(in crate::type_engine) fn array_splice_leaves(
    base: &PhpType,
    offset: Option<i64>,
    length: SpliceLength,
    replacement: &PhpType,
    exact: bool,
) -> Option<PhpType> {
    let mut results: Vec<PhpType> = Vec::new();
    for member in base.union_members() {
        let shape = match (exact, offset, length, member.kind()) {
            (true, Some(offset), SpliceLength::ToEnd, TypeKind::ArrayShape(_)) => {
                splice_shape(member, offset, None, replacement)
            }
            (true, Some(offset), SpliceLength::Literal(length), TypeKind::ArrayShape(_)) => {
                splice_shape(member, offset, Some(length), replacement)
            }
            _ => None,
        };
        let spliced = match shape {
            Some(shape) => shape,
            None => splice_generic(member, offset == Some(0), length, replacement)?,
        };
        if !results.contains(&spliced) {
            results.push(spliced);
        }
    }
    Some(PhpType::union(results))
}

/// An entry of a spliced shape: its string key, or `None` for an int key
/// PHP renumbers.
struct Spliced {
    key: Option<String>,
    value: PhpType,
    optional: bool,
}

/// [`array_splice_leaves`] over one shape with a literal offset and length.
/// `None` where an optional entry leaves a renumbered key uncertain.
fn splice_shape(
    shape: &PhpType,
    offset_arg: i64,
    length_arg: Option<i64>,
    replacement: &PhpType,
) -> Option<PhpType> {
    let entries = explicit_key_shape_entries(shape)?;
    let count = entries.len() as i64;
    let mut offset = offset_arg;
    let mut length = length_arg.unwrap_or(count);
    // A negative offset or length cannot reach past the array's start.
    if count + offset <= 0 {
        offset = 0;
    }
    if count + length <= 0 {
        length = 0;
    }
    let offset_was_negative = offset < 0;
    if offset_was_negative {
        offset += count;
    }
    // An offset past the end appends.
    offset = offset.min(count);
    if length < 0 {
        length = (count - offset + length).max(0);
    }

    // The entries the call removes, keys kept, each paired with whether it
    // may be missing from them.
    let removed: Vec<(String, bool)> =
        super::slice_entries(entries.clone(), offset_arg, length_arg, true)
            .and_then(|extract| explicit_key_shape_entries(&extract))?
            .into_iter()
            .filter_map(|entry| Some((entry.key?, entry.optional)))
            .collect();
    let removal = |key: &str| {
        removed
            .iter()
            .find(|(removed, _)| removed == key)
            .map(|&(_, optional)| optional)
    };

    let mut results: Vec<PhpType> = Vec::new();
    for replacement in replacement.union_members() {
        let inserted = inserted_entries(replacement)?;
        let mut spliced: Vec<Spliced> = Vec::new();
        let mut has_inserted = false;
        let insert = |spliced: &mut Vec<Spliced>| {
            if let Inserted::Entries(values) = &inserted {
                spliced.extend(values.iter().map(|(value, optional)| Spliced {
                    key: None,
                    value: value.clone(),
                    optional: *optional,
                }));
            }
        };
        let mut optional_before = 0;
        let mut remove = 0;
        for (i, entry) in entries.iter().enumerate() {
            let i = i as i64;
            let mut optional = entry.optional;
            if !offset_was_negative && i < offset && optional {
                optional_before += 1;
            }
            if !has_inserted && i == offset + optional_before {
                remove = length;
                has_inserted = true;
                insert(&mut spliced);
            }
            let key = entry.key.as_deref()?;
            let in_removed = removal(key);
            if remove > 0 && in_removed == Some(optional) {
                remove -= 1;
                continue;
            }
            if !optional && in_removed == Some(true) {
                optional = true;
            }
            spliced.push(Spliced {
                key: canonical_int_key(key).is_none().then(|| key.to_string()),
                value: entry.value_type.clone(),
                optional,
            });
        }
        if !has_inserted {
            insert(&mut spliced);
        }
        // Where an optional entry leaves the renumbering open, the keys
        // and values are still known.
        let result = match inserted {
            Inserted::Generic => None,
            Inserted::Entries(_) => renumbered_shape(&spliced),
        }
        .unwrap_or_else(|| generic_spliced_array(&spliced, replacement));
        if !results.contains(&result) {
            results.push(result);
        }
    }
    // Generic arrays from several replacements merge into one.
    if results
        .iter()
        .any(|result| !matches!(result.kind(), TypeKind::ArrayShape(_)))
    {
        let non_empty = results.iter().all(PhpType::is_provably_non_empty);
        let mut keys = Vec::with_capacity(results.len());
        let mut values = Vec::with_capacity(results.len());
        for result in &results {
            keys.push(result.iterable_key_type()?);
            values.push(result.iterable_element_type()?);
        }
        let array = PhpType::generic_array(
            PhpType::join_runtime_value_types(keys),
            PhpType::join_runtime_value_types(values),
        );
        return Some(if non_empty {
            array.non_empty_array_form()
        } else {
            array
        });
    }
    Some(PhpType::union(results))
}

/// What one alternative of a replacement puts into the array.
enum Inserted {
    /// The values of a shape, in order, each paired with whether it may be
    /// missing.
    Entries(Vec<(PhpType, bool)>),
    /// Some number of a generic array's values.
    Generic,
}

fn inserted_entries(replacement: &PhpType) -> Option<Inserted> {
    if let Some(entries) = explicit_key_shape_entries(replacement) {
        return Some(Inserted::Entries(
            entries
                .into_iter()
                .map(|entry| (entry.value_type, entry.optional))
                .collect(),
        ));
    }
    replacement.is_array_like().then_some(Inserted::Generic)
}

/// The spliced entries as a shape, int keys numbered from zero. `None` when
/// an optional int-keyed entry leaves the number of a later one open.
fn renumbered_shape(spliced: &[Spliced]) -> Option<PhpType> {
    let mut next = 0;
    let mut uncertain = false;
    let mut entries = Vec::with_capacity(spliced.len());
    for entry in spliced {
        let key = match &entry.key {
            Some(key) => key.clone(),
            None => {
                if uncertain {
                    return None;
                }
                uncertain = entry.optional;
                next += 1;
                (next - 1).to_string()
            }
        };
        entries.push(ShapeEntry {
            key: Some(key),
            value_type: entry.value.clone(),
            optional: entry.optional,
        });
    }
    Some(shape_with_implicit_keys(entries))
}

/// The spliced entries with `inserted`'s values among them:
/// an array of their string keys and renumbered int keys, never empty when
/// one of the kept entries is always there.
fn generic_spliced_array(spliced: &[Spliced], inserted: &PhpType) -> PhpType {
    let mut keys: Vec<PhpType> = vec![PhpType::int_range("0", "max")];
    let mut values: Vec<PhpType> = vec![
        inserted
            .iterable_element_type()
            .unwrap_or_else(PhpType::mixed),
    ];
    let mut non_empty = inserted.is_provably_non_empty();
    for entry in spliced {
        if let Some(key) = &entry.key {
            keys.push(PhpType::literal_string_value(key));
        }
        values.push(entry.value.clone());
        non_empty |= !entry.optional;
    }
    let array = PhpType::generic_array(
        PhpType::join_runtime_value_types(keys),
        PhpType::join_runtime_value_types(values),
    );
    if non_empty {
        array.non_empty_array_form()
    } else {
        array
    }
}

/// [`array_splice_leaves`] over an array whose entries cannot be told
/// apart: the int keys renumbered (so an int-keyed array becomes a list),
/// the replacement's keys and values added, and non-empty only when the
/// replacement is or nothing was removed from an array that was.
fn splice_generic(
    member: &PhpType,
    offset_is_zero: bool,
    length: SpliceLength,
    replacement: &PhpType,
) -> Option<PhpType> {
    if !member.is_array_like() {
        return None;
    }
    let replacement_empty = replacement
        .union_members()
        .iter()
        .all(|member| member.is_empty_array_shape());
    if offset_is_zero && length == SpliceLength::ToEnd && replacement_empty {
        return Some(PhpType::array_shape(Vec::new()));
    }
    let container = member.generalized_array();
    let (form, key, value) = generic_array_parts(&container).or_else(|| {
        let value = container.iterable_element_type()?;
        Some((
            super::ArrayForm::Array,
            super::array_key_domain(&container),
            value,
        ))
    })?;
    let int_keyed = form.is_list() || key.as_ref().is_some_and(|key| key.is_int_subtype());

    let mut values = vec![value];
    let mut keys: Vec<PhpType> = Vec::new();
    for member in replacement.union_members() {
        if let Some(entries) = explicit_key_shape_entries(member) {
            keys.extend((0..entries.len()).map(|i| PhpType::literal_int(i.to_string())));
            values.extend(entries.into_iter().map(|entry| entry.value_type));
        } else {
            keys.push(PhpType::int_range("0", "max"));
            values.push(
                member
                    .iterable_element_type()
                    .unwrap_or_else(PhpType::mixed),
            );
        }
    }
    let value = PhpType::join_runtime_value_types(values);
    let non_empty = replacement.is_provably_non_empty()
        || (member.is_provably_non_empty() && length == SpliceLength::Literal(0));
    let result = if int_keyed {
        PhpType::list(value)
    } else {
        // Every int key is renumbered from zero; string keys stay.
        match key {
            Some(key) => {
                for member in key.union_members() {
                    keys.push(if member.is_int_subtype() {
                        PhpType::int_range("0", "max")
                    } else {
                        member.clone()
                    });
                }
            }
            None => {
                keys.push(PhpType::int_range("0", "max"));
                keys.push(PhpType::string());
            }
        }
        PhpType::generic_array(PhpType::join_runtime_value_types(keys), value)
    };
    Some(if non_empty {
        result.non_empty_array_form()
    } else {
        result
    })
}
