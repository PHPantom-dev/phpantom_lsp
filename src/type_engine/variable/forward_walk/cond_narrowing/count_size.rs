use super::*;

use crate::php_type::ShapeEntry;

/// The largest size a `count()` check spells out as a shape.  Beyond it
/// the list keeps its generic form, the same cut-off PHPStan makes.
const SHAPE_SIZE_LIMIT: i64 = 256;

/// Narrow a list whose `count()` a condition pins to one size into the
/// shape with that many entries.
///
/// ```php
/// /** @param list<int> $xs */
/// if (count($xs) === 3) { $xs; } // array{int, int, int}
/// ```
///
/// The size is a written integer, or the `count()` of a shape whose length
/// is fixed: `count($a) == count($b)` gives `$b` the length of an
/// `array{int, int, int}` `$a`.  A list-shaped shape with optional entries
/// keeps as many of them as the size needs.  Only the branch where the
/// sizes are equal learns anything; an inequality rules out one size of
/// many.
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
    if equal != truthy {
        return;
    }
    for (counted, other) in [(bin.lhs, bin.rhs), (bin.rhs, bin.lhs)] {
        let Some(subject) = exact_count_subject(counted) else {
            continue;
        };
        let Some(size) = known_size(other, scope) else {
            continue;
        };
        if !(1..SHAPE_SIZE_LIMIT).contains(&size) {
            continue;
        }
        seed_synthetic_key_if_needed(&subject, scope, ctx);
        let types = scope.get(&subject);
        if types.is_empty() {
            continue;
        }
        let mut changed = false;
        let narrowed: Vec<ResolvedType> = types
            .iter()
            .filter_map(|rt| {
                let sized = list_of_size(&rt.type_string, size as usize);
                match sized {
                    Some(Some(ty)) => {
                        changed = true;
                        Some(ResolvedType::from_type_string(ty))
                    }
                    // A shape that cannot have this many entries.
                    Some(None) => {
                        changed = true;
                        None
                    }
                    None => Some(rt.clone()),
                }
            })
            .collect();
        if changed && !narrowed.is_empty() {
            scope.set(&subject, narrowed);
        }
    }
}

/// The subject of a `count($x)` / `sizeof($x)` that counts only the top
/// level: a second argument (`COUNT_RECURSIVE`) counts nested entries too.
fn exact_count_subject(expr: &Expression<'_>) -> Option<String> {
    match count_call(expr)? {
        (subject, false) => Some(subject),
        (_, true) => None,
    }
}

/// The subject of a `count()` / `sizeof()` call, and whether it passed a
/// mode argument that may make it count recursively.
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
    let has_mode = args.next().is_some();
    Some((expr_to_subject(narrowing::argument_value(first))?, has_mode))
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
    })
}

/// `ty` as a list of exactly `size` entries: `Some(Some(shape))` when it
/// is a list that can have that many, `Some(None)` when it is a list that
/// cannot, and `None` when it is not a list this knows how to size.
fn list_of_size(ty: &PhpType, size: usize) -> Option<Option<PhpType>> {
    match ty.kind() {
        TypeKind::ArrayShape(entries) => {
            if entries.iter().any(|entry| entry.key.is_some()) {
                return None;
            }
            let required = entries.iter().filter(|entry| !entry.optional).count();
            if size < required || size > entries.len() {
                return Some(None);
            }
            let sized: Vec<ShapeEntry> = entries[..size]
                .iter()
                .map(|entry| ShapeEntry {
                    optional: false,
                    ..entry.clone()
                })
                .collect();
            Some(Some(PhpType::array_shape(sized)))
        }
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
