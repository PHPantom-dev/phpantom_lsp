use super::*;

use crate::php_type::ShapeEntry;

/// The largest size a `count()` check spells out as a shape.  Beyond it
/// the list keeps its generic form, the same cut-off PHPStan makes.
const SHAPE_SIZE_LIMIT: i64 = 256;

/// Narrow a list or shape whose `count()` a condition compares with one
/// size.
///
/// ```php
/// /** @param list<int> $xs */
/// if (count($xs) === 3) { $xs; } // array{int, int, int}
/// ```
///
/// The size is a written integer, or the `count()` of a shape whose length
/// is fixed: `count($a) == count($b)` gives `$b` the length of an
/// `array{int, int, int}` `$a`.  A shape with optional entries keeps as
/// many of them as the size needs, when that says which ones.  The branch
/// where the sizes differ rules that one size out, which settles a shape
/// that had only one other size it could be.  A size the subject cannot
/// have (a negative one, or one its shape has no room for) exhausts it.
pub(super) fn apply_count_size_narrowing(
    condition: &Expression<'_>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    truthy: bool,
) {
    let (inner, negated) = narrowing::unwrap_condition_negation(condition);
    let Expression::Binary(bin) = inner else {
        return;
    };
    let equal = match bin.operator {
        BinaryOperator::Identical(_) | BinaryOperator::Equal(_) => !negated,
        BinaryOperator::NotIdentical(_) | BinaryOperator::NotEqual(_) => negated,
        _ => return,
    };
    let equal = equal == truthy;
    for (counted, other) in [(bin.lhs, bin.rhs), (bin.rhs, bin.lhs)] {
        let Some((subject, recursive)) = count_call(counted) else {
            continue;
        };
        let Some(size) = known_size(other, scope) else {
            continue;
        };
        // `count()` never returns a negative number.
        if size < 0 {
            if equal {
                seed_synthetic_key_if_needed(&subject, scope, ctx);
                if !scope.get(&subject).is_empty() {
                    mark_exhausted(&subject, scope);
                }
            }
            continue;
        }
        if !(0..SHAPE_SIZE_LIMIT).contains(&size) {
            continue;
        }
        seed_synthetic_key_if_needed(&subject, scope, ctx);
        let types = scope.get(&subject);
        if types.is_empty() {
            continue;
        }
        let mut changed = false;
        let mut narrowed: Vec<ResolvedType> = Vec::with_capacity(types.len());
        for rt in types {
            // A class-backed entry is no array to size, and a recursive
            // count adds the entries of nested arrays, so it is the list's
            // length only when no element is one.
            if rt.class_info.is_some()
                || recursive
                    && !rt
                        .type_string
                        .iterable_element_type()
                        .is_some_and(|element| is_never_an_array(&element))
            {
                narrowed.push(rt.clone());
                continue;
            }
            let sized = if equal {
                of_size(&rt.type_string, size as usize)
            } else {
                not_of_size(&rt.type_string, size as usize)
            };
            match sized {
                // Each alternative stays its own entry, so the join after
                // the branch can fold a sized shape back together with
                // the one the other branch sized.
                Some(members) => {
                    changed = true;
                    narrowed.extend(members.into_iter().map(ResolvedType::from_type_string));
                }
                None => narrowed.push(rt.clone()),
            }
        }
        if !changed {
            continue;
        }
        // Sizing two alternatives can leave the same shape twice.
        ResolvedType::drop_subsumed_entries(&mut narrowed, false);
        if narrowed.is_empty() {
            mark_exhausted(&subject, scope);
        } else {
            scope.set(&subject, narrowed);
        }
    }
}

/// The subject of a `count()` / `sizeof()` call, and whether it passed a
/// mode argument that may make it count recursively: anything but a
/// written `COUNT_NORMAL` or `0`.
fn count_call(expr: &Expression<'_>) -> Option<(String, bool)> {
    let Expression::Call(Call::Function(call)) = unwrap_parens(expr) else {
        return None;
    };
    let Expression::Identifier(ident) = call.function else {
        return None;
    };
    let name = crate::util::strip_fqn_prefix(bytes_to_str(ident.value()));
    if !name.eq_ignore_ascii_case("count") && !name.eq_ignore_ascii_case("sizeof") {
        return None;
    }
    let mut args = call.argument_list.arguments.iter();
    let first = args.next()?;
    let recursive = args
        .next()
        .is_some_and(|mode| !is_count_normal(narrowing::argument_value(mode)));
    Some((
        expr_to_subject(narrowing::argument_value(first))?,
        recursive,
    ))
}

/// Whether a `count()` mode argument is spelled as the non-recursive mode.
fn is_count_normal(mode: &Expression<'_>) -> bool {
    match unwrap_parens(mode) {
        Expression::ConstantAccess(access) => {
            crate::util::strip_fqn_prefix(bytes_to_str(access.name.value())) == "COUNT_NORMAL"
        }
        Expression::Literal(Literal::Integer(integer)) => integer.value == Some(0),
        _ => false,
    }
}

/// The size `expr` is known to be: an integer literal, or the `count()`
/// of a subject whose type is a shape with no optional entries.
///
/// A recursive count also counts the entries of nested arrays, so it gives
/// the shape's length only when no entry can hold one.
fn known_size(expr: &Expression<'_>, scope: &ScopeState) -> Option<i64> {
    if let Some(literal) = literal_comparand_type(expr) {
        return match literal.as_literal()? {
            LiteralValue::Int(_) => literal.as_literal()?.parse_i64(),
            _ => None,
        };
    }
    let (subject, recursive) = count_call(expr)?;
    let types = scope.get(&subject);
    let mut size = None;
    for rt in types {
        let TypeKind::ArrayShape(entries) = rt.type_string.kind() else {
            return None;
        };
        if entries.iter().any(|entry| entry.optional) {
            return None;
        }
        if recursive
            && entries
                .iter()
                .any(|entry| !is_never_an_array(&entry.value_type))
        {
            return None;
        }
        let len = entries.len() as i64;
        if size.is_some_and(|known| known != len) {
            return None;
        }
        size = Some(len);
    }
    size
}

/// Whether no value of `ty` is an array, so a recursive count skips it.
fn is_never_an_array(ty: &PhpType) -> bool {
    ty.union_members().iter().all(|member| {
        member.is_int_subtype()
            || member.is_string_subtype()
            || member.is_float()
            || matches!(member.as_literal(), Some(LiteralValue::Float(_)))
            || member.is_bool()
            || member.is_true()
            || member.is_false()
            || member.is_null()
            // `count()` does not look inside an object, `Countable` or not.
            || (member.is_object_like() && !member.is_array_like() && !member.is_iterable())
    })
}

/// The alternatives of `ty` that have exactly `size` entries, or `None`
/// when it is not a list or shape this knows how to size.  A union is
/// sized member by member.
fn of_size(ty: &PhpType, size: usize) -> Option<Vec<PhpType>> {
    refine_members(ty, &|member| match member.kind() {
        TypeKind::ArrayShape(entries) => {
            let is_list = is_list_ordered(member, entries);
            let (required, total) = shape_size_bounds(entries);
            if size < required || size > total {
                return Some(None);
            }
            // Which optional entries a size in between leaves is known
            // only for a list, whose entries are present from the front.
            (size == required || size == total || is_list)
                .then(|| Some(shape_prefix(member, entries, size, is_list)))
        }
        // A size of zero is the empty array, which the emptiness checks
        // already narrow to.
        _ if size == 0 => None,
        TypeKind::Generic(g)
            if g.args.len() == 1
                && matches!(
                    g.name.to_ascii_lowercase().as_str(),
                    "list" | "non-empty-list"
                ) =>
        {
            Some(Some(repeated_shape(&g.args[0], size)))
        }
        TypeKind::Named(name)
            if matches!(
                name.to_ascii_lowercase().as_str(),
                "list" | "non-empty-list"
            ) =>
        {
            Some(Some(repeated_shape(&PhpType::mixed(), size)))
        }
        _ => None,
    })
}

/// The alternatives of `ty` that do not have exactly `size` entries, with
/// the same answers as [`of_size`].
///
/// Only a shape learns anything: ruling one size out of a list leaves every
/// other size it could have.  A shape whose sizes run from its required
/// entries to all of them keeps the ones either side of `size`, which it
/// can spell when they are one size, or when it is a list and `size` is at
/// one end of the run.
fn not_of_size(ty: &PhpType, size: usize) -> Option<Vec<PhpType>> {
    refine_members(ty, &|member| {
        let TypeKind::ArrayShape(entries) = member.kind() else {
            return None;
        };
        let is_list = is_list_ordered(member, entries);
        let (required, total) = shape_size_bounds(entries);
        if size < required || size > total {
            return None;
        }
        if required == total {
            return Some(None);
        }
        if required + 1 == total {
            let other = if size == required { total } else { required };
            return Some(Some(shape_prefix(member, entries, other, is_list)));
        }
        if !is_list {
            return None;
        }
        if size == required {
            // The entry after the required ones is present too.
            let entries: Vec<ShapeEntry> = entries
                .iter()
                .enumerate()
                .map(|(i, entry)| ShapeEntry {
                    optional: entry.optional && i != required,
                    ..entry.clone()
                })
                .collect();
            return Some(Some(rebuild_shape(member, entries)));
        }
        if size == total {
            let entries = entries[..total - 1].to_vec();
            return Some(Some(rebuild_shape(member, entries)));
        }
        None
    })
}

/// Distribute a per-member sizing over a union: each member is refined by
/// `refine`, kept when it answers `None`, and dropped when it answers
/// `Some(None)`.  `None` when no member changed.
fn refine_members(
    ty: &PhpType,
    refine: &dyn Fn(&PhpType) -> Option<Option<PhpType>>,
) -> Option<Vec<PhpType>> {
    let mut changed = false;
    let mut kept = Vec::new();
    for member in ty.union_members() {
        match refine(member) {
            Some(sized) => {
                changed = true;
                kept.extend(sized);
            }
            None => kept.push(member.clone()),
        }
    }
    changed.then_some(kept)
}

/// How many entries a shape holds at the least and at the most.
fn shape_size_bounds(entries: &[ShapeEntry]) -> (usize, usize) {
    let required = entries.iter().filter(|entry| !entry.optional).count();
    (required, entries.len())
}

/// Whether a shape's entries are present from the front, the way a list's
/// are: it is a `list{…}`, or its keys are positional, and no required
/// entry follows an optional one.
fn is_list_ordered(shape: &PhpType, entries: &[ShapeEntry]) -> bool {
    let positional = matches!(shape.raw_kind(), TypeKind::ListShape(_))
        || entries.iter().all(|entry| entry.key.is_none());
    positional
        && entries
            .iter()
            .skip_while(|entry| !entry.optional)
            .all(|entry| entry.optional)
}

/// The shape with exactly `size` of its entries: its required ones and,
/// when `size` is more than those, every optional one (`size` is its
/// full length) or the ones at the front (a list).
fn shape_prefix(shape: &PhpType, entries: &[ShapeEntry], size: usize, is_list: bool) -> PhpType {
    let (required, total) = shape_size_bounds(entries);
    let sized: Vec<ShapeEntry> = if is_list {
        entries[..size].to_vec()
    } else if size == total {
        entries.to_vec()
    } else {
        debug_assert_eq!(size, required);
        entries
            .iter()
            .filter(|entry| !entry.optional)
            .cloned()
            .collect()
    };
    let sized = sized
        .into_iter()
        .map(|entry| ShapeEntry {
            optional: false,
            ..entry
        })
        .collect();
    rebuild_shape(shape, sized)
}

/// A shape with `entries`, tagged as a list when `like` was one.
fn rebuild_shape(like: &PhpType, entries: Vec<ShapeEntry>) -> PhpType {
    let shape = PhpType::array_shape(entries);
    if matches!(like.raw_kind(), TypeKind::ListShape(_)) {
        PhpType::as_list_shape(shape)
    } else {
        shape
    }
}

/// `array{T, T, …}` with `size` entries.
fn repeated_shape(value: &PhpType, size: usize) -> PhpType {
    PhpType::array_shape(
        (0..size)
            .map(|_| ShapeEntry {
                key: None,
                value_type: value.clone(),
                optional: false,
            })
            .collect(),
    )
}
