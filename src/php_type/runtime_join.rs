//! Joining the types a runtime expression's branches produce.
//!
//! A join keeps every alternative some branch can really produce and drops
//! only the ones another branch's type already takes in as a value set:
//! `1|int` joins to `int`, but `int|float` stays as it is, because the
//! subtype edge between those two is a parameter coercion rather than
//! value containment, and that is the difference from
//! [`PhpType::simplified`].

use super::normalize::{
    ShapeKeyIndex, equivalent_for_dedup, generalize_oversized_shapes, simplify_empty_string_union,
};
use super::*;

impl PhpType {
    /// Join alternatives that can be produced at runtime.
    ///
    /// This is intentionally separate from [`PhpType::simplified`], whose
    /// subtype lattice also models parameter coercions such as `int <: float`.
    /// A runtime expression may still produce either an int or a float, so
    /// those domains must remain distinct while true value-set subsets such
    /// as `1 <: int`, `1 <: numeric`, and `'x' <: scalar` are absorbed.
    pub(crate) fn join_runtime_value_types(members: Vec<PhpType>) -> PhpType {
        fn flatten(member: PhpType, flattened: &mut Vec<PhpType>) {
            match member.kind() {
                TypeKind::Union(members) => {
                    for member in members.iter().cloned() {
                        flatten(member, flattened);
                    }
                }
                TypeKind::Nullable(inner) => {
                    flatten(inner.clone(), flattened);
                    flattened.push(PhpType::null());
                }
                _ => flattened.push(member),
            }
        }

        let mut flattened = Vec::with_capacity(members.len());
        for member in members {
            flatten(member, &mut flattened);
        }
        if flattened.iter().any(PhpType::is_mixed) {
            return PhpType::mixed();
        }

        flattened.retain(|member| !member.is_never());
        dedup_types(&mut flattened);
        simplify_bool_union(&mut flattened);
        simplify_empty_string_union(&mut flattened);
        generalize_oversized_shapes(&mut flattened);

        // Branch joins are normally tiny. Pairwise containment keeps the
        // semantics explicit and handles equivalent aliases deterministically
        // without putting subtype policy into the global dedup key.
        //
        // A literal is only contained in an equal literal or in a wider
        // non-literal type, and `dedup_types` has already merged equal
        // literals, so two literals never need comparing. A `switch` that
        // assigns a different literal in every case joins hundreds of them,
        // and comparing every pair made each of those joins quadratic.
        let non_literals: Vec<usize> = (0..flattened.len())
            .filter(|&index| flattened[index].as_literal().is_none())
            .collect();
        let shapes = ShapeKeyIndex::new(&flattened);
        let keep: Vec<bool> = (0..flattened.len())
            .map(|index| {
                let subsumed_by = |candidate: usize| {
                    index != candidate
                        && is_runtime_value_subtype(&flattened[index], &flattened[candidate])
                        && (candidate < index
                            || !is_runtime_value_subtype(&flattened[candidate], &flattened[index]))
                };
                if flattened[index].as_literal().is_some() {
                    !non_literals.iter().any(|&candidate| subsumed_by(candidate))
                } else if let Some(candidates) = shapes.candidates(&flattened[index]) {
                    !candidates.into_iter().any(subsumed_by)
                } else {
                    !(0..flattened.len()).any(subsumed_by)
                }
            })
            .collect();

        crate::util::retain_by_mask(&mut flattened, &keep);

        if flattened.is_empty() {
            PhpType::never()
        } else {
            PhpType::union(flattened)
        }
    }
}

/// Whether `subtype`'s produced values are a subset of `supertype`'s, so a
/// join of the two can drop `subtype` without losing an alternative.
///
/// Only scalar value domains take part; see [`is_runtime_scalar_value_domain`].
pub(crate) fn is_runtime_value_subtype(subtype: &PhpType, supertype: &PhpType) -> bool {
    // `kind()` reads an unsealed shape as the array it widens to, an exact
    // class name as the `class-string<T>` it is one of, and an enum case as
    // its enum, so any of them would take in the wider type it is read as
    // and drop it from a join.
    if supertype.as_unsealed_shape().is_some() || supertype.is_exact_value_marker() {
        return subtype == supertype;
    }
    // `array{}` is the empty array, and every array type that does not
    // demand an entry has it as a member value.  That is real value
    // containment rather than the variance/coercion kind
    // [`is_runtime_scalar_value_domain`] rules out, so a branch that only
    // produced `[]` adds nothing beside a branch that produced an array of
    // the same family — which is what keeps the `[]` a loop or a by-ref
    // closure capture starts from out of the joined result.
    if subtype.is_empty_array_shape() && supertype.accepts_empty_array() {
        return true;
    }

    // `flatten` keeps a union supertype atomic when it is wrapped in a
    // marker (`Benevolent`'s failure branch has to survive on whatever
    // entry represents it — see `ResolvedType::types_joined`), so a
    // `Benevolent(int|float)` reaches here as one member rather than
    // decomposed into `int` and `float`.  A sibling that only duplicates
    // one of its members is still redundant, so check containment against
    // each member without decomposing (and so without disturbing) the
    // union itself.
    if let TypeKind::Union(members) = supertype.kind() {
        return members.iter().any(|m| is_runtime_value_subtype(subtype, m));
    }

    if let TypeKind::ArrayShape(entries) = subtype.kind() {
        return shape_values_contained_in(entries, subtype.is_list_shape(), supertype);
    }
    if let (Some(sub), Some(sup)) = (generic_array_parts(subtype), generic_array_parts(supertype)) {
        return generic_array_contained_in(&sub, &sup);
    }

    if !is_runtime_scalar_value_domain(subtype) || !is_runtime_scalar_value_domain(supertype) {
        return false;
    }

    if let (TypeKind::Named(sub), TypeKind::Named(sup)) = (subtype.kind(), supertype.kind())
        && sub.eq_ignore_ascii_case("number")
        && sup.eq_ignore_ascii_case("number")
        && (sub == "number") != (sup == "number")
    {
        // Lowercase `number` is PHPantom's numeric pseudo-type, while another
        // casing may be a real class (for example `BcMath\Number`).
        return false;
    }

    // PHP accepts an int for a float parameter, but the runtime value remains
    // an int. This is the coercive edge that must not participate in a
    // produced-value join.
    if subtype.is_int_subtype() && supertype.is_float_subtype() {
        return false;
    }
    subtype.is_subtype_of(supertype)
}

/// An array type spelled by its key and value types rather than entry by
/// entry: `array<K, V>`, `list<V>`, `non-empty-array<K, V>`, `V[]`.
struct GenericArrayParts {
    list: bool,
    non_empty: bool,
    /// `None` when the spelling names no key type (`array<V>`, `V[]`).
    key: Option<PhpType>,
    value: PhpType,
}

fn generic_array_parts(ty: &PhpType) -> Option<GenericArrayParts> {
    let (name, key, value) = match ty.kind() {
        TypeKind::Array(value) => ("array", None, value.clone()),
        TypeKind::Generic(generic) => match generic.args.as_slice() {
            [value] => (generic.name.as_str(), None, value.clone()),
            [key, value] => (generic.name.as_str(), Some(key.clone()), value.clone()),
            _ => return None,
        },
        _ => return None,
    };
    let (list, non_empty) = with_keyword_lowercase(name, |lower| match lower {
        "array" => Some((false, false)),
        "non-empty-array" => Some((false, true)),
        "list" => Some((true, false)),
        "non-empty-list" => Some((true, true)),
        _ => None,
    })?;
    Some(GenericArrayParts {
        list,
        non_empty,
        key,
        value,
    })
}

/// Whether every array `sub` describes is also one `sup` describes: its
/// keys and values fit, and it promises at least what `sup` promises about
/// being a list or non-empty.
fn generic_array_contained_in(sub: &GenericArrayParts, sup: &GenericArrayParts) -> bool {
    if (sup.list && !sub.list) || (sup.non_empty && !sub.non_empty) {
        return false;
    }
    let keys_fit = match (&sub.key, &sup.key) {
        (_, None) => true,
        (_, Some(sup_key)) if sup_key.is_mixed() || sup_key.is_array_key() => true,
        (None, Some(_)) => false,
        (Some(sub_key), Some(sup_key)) => {
            equivalent_for_dedup(sub_key, sup_key) || is_runtime_value_subtype(sub_key, sup_key)
        }
    };
    keys_fit
        && (sup.value.is_mixed()
            || equivalent_for_dedup(&sub.value, &sup.value)
            || is_runtime_value_subtype(&sub.value, &sup.value))
}

/// Whether every array an `array{…}` shape with `entries` describes is also
/// one `supertype` describes: a wider shape holding the same keys, or an
/// array whose key and value types take in every entry.
///
/// Entry values are compared with [`is_runtime_value_subtype`] itself, so
/// the same produced-value rules apply one level down: a shape holding an
/// `int` is covered by one holding `int|float`, but not by one holding only
/// `float`.
pub(super) fn shape_values_contained_in(
    entries: &[ShapeEntry],
    is_list: bool,
    supertype: &PhpType,
) -> bool {
    if let TypeKind::ArrayShape(wider) = supertype.kind()
        && keys_rule_out_nesting(entries, wider)
    {
        return false;
    }
    let value_fits = |value: &PhpType, wider: &PhpType| {
        wider.is_mixed()
            || equivalent_for_dedup(value, wider)
            || is_runtime_value_subtype(value, wider)
    };
    let Some(keys) = runtime_shape_keys(entries) else {
        return false;
    };
    if keys.iter().any(|key| key.contains("::")) {
        return false;
    }

    if let TypeKind::ArrayShape(wider) = supertype.kind() {
        if supertype.is_list_shape() && !is_list {
            return false;
        }
        let Some(wider_keys) = runtime_shape_keys(wider) else {
            return false;
        };
        // Every key the shape may hold is one the wider shape allows, with a
        // value it allows there, and the wider shape demands nothing the
        // shape may lack.
        let covered = entries.iter().zip(&keys).all(|(entry, key)| {
            wider_keys
                .iter()
                .position(|wider_key| wider_key == key)
                .is_some_and(|index| {
                    (!entry.optional || wider[index].optional)
                        && value_fits(&entry.value_type, &wider[index].value_type)
                })
        });
        return covered
            && wider
                .iter()
                .zip(&wider_keys)
                .all(|(entry, key)| entry.optional || keys.contains(key));
    }

    let parts = match supertype.kind() {
        TypeKind::Named(name)
            if with_keyword_lowercase(name, |lower| matches!(lower, "array" | "list")) =>
        {
            GenericArrayParts {
                list: with_keyword_lowercase(name, |lower| lower == "list"),
                non_empty: false,
                key: None,
                value: PhpType::mixed(),
            }
        }
        _ => match generic_array_parts(supertype) {
            Some(parts) => parts,
            None => return false,
        },
    };
    let GenericArrayParts {
        list,
        non_empty,
        key: key_type,
        value: value_type,
    } = parts;
    if list && !is_list {
        return false;
    }
    if non_empty && !entries.iter().any(|entry| !entry.optional) {
        return false;
    }
    let key_fits = |key: &str| {
        match &key_type {
        None => true,
        Some(key_type) if key_type.is_mixed() || key_type.is_array_key() => true,
        Some(key_type) => key_type.union_members().iter().any(|member| {
            if is_decimal_int_array_key(key) {
                matches!(member.kind(), TypeKind::Named(n) if with_keyword_lowercase(n, |lower| matches!(lower, "int" | "integer")))
            } else {
                matches!(member.kind(), TypeKind::Named(n) if with_keyword_lowercase(n, |lower| lower == "string"))
            }
        }),
    }
    };
    entries
        .iter()
        .zip(&keys)
        .all(|(entry, key)| key_fits(key) && value_fits(&entry.value_type, &value_type))
}

/// Whether a type is a scalar runtime value domain whose subtype edges describe
/// value-set containment rather than structural or parameter coercion.
///
/// Runtime branch joining must not apply the full subtype lattice to generic
/// arrays, callables, shapes, or other structured alternatives: their subtype
/// relations can be variance/coercion rules rather than proof that one branch's
/// produced values are redundant.
fn is_runtime_scalar_value_domain(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Literal(_)
        | TypeKind::IntRange(_, _)
        | TypeKind::ClassString(_)
        | TypeKind::InterfaceString(_) => true,
        TypeKind::Named(name) => {
            if name == "number" {
                return true;
            }
            with_keyword_lowercase(name, |lower| {
                matches!(
                    lower,
                    "int"
                        | "integer"
                        | int_refinement!()
                        | "float"
                        | "double"
                        | "real"
                        | "string"
                        | string_refinement!()
                        | "bool"
                        | "boolean"
                        | "true"
                        | "false"
                        | "null"
                        | "numeric"
                        | "scalar"
                        | "array-key"
                )
            })
        }
        _ => false,
    }
}
