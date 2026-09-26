use super::*;

/// Narrow the key of a successful `isset($arr[$k])` or
/// `array_key_exists($k, $arr)` to the keys the array can hold.
///
/// Either check proves `$k` names an entry of `$arr`, so `$k` is one of
/// the array's keys:
///
/// ```php
/// $arr = ['1' => 1, '2' => 2, 3 => 3];
/// if (isset($arr[$s])) {
///     $s; // '1'|'2'|'3' for a string $s
/// }
/// ```
///
/// When every key is known, that is one of them, in whatever form PHP
/// accepts for it: `isset()` reads through PHP's key cast, so `true` finds
/// key `1`, `null` finds `''` and a float finds its integer part.
/// `array_key_exists()` takes only an `int` or `string` key.  When only the
/// key type is declared, `array_key_exists()` narrows the key to it; an
/// `isset()` on such an array proves nothing about the key, since PHP's
/// cast lets many values reach the same entry.
///
/// A key that is already one literal is the array's business, not the
/// key's: the shape-refining passes use it to mark the entry present.
pub(super) fn apply_key_domain_narrowing<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    for operand in collect_and_chain_operands(condition) {
        if let Some((_, key_expr, negated)) = array_key_exists_target(operand) {
            if !negated
                && let Expression::Call(Call::Function(call)) = unwrap_parens(operand)
                && let Some(array) = call.argument_list.arguments.iter().nth(1)
            {
                narrow_key(
                    narrowing::argument_value(array),
                    key_expr,
                    false,
                    scope,
                    ctx,
                );
            }
            continue;
        }
        let Expression::Construct(Construct::Isset(isset)) = unwrap_parens(operand) else {
            continue;
        };
        for value in isset.values.iter() {
            // `isset($a[$i][$j])` proves every offset on the way down.
            let mut current: &Expression<'_> = value;
            while let Expression::ArrayAccess(access) = current {
                narrow_key(access.array, access.index, true, scope, ctx);
                current = access.array;
            }
        }
    }
}

/// Narrow `key_expr` to the keys of the array `array_expr` evaluates to.
fn narrow_key(
    array_expr: &Expression<'_>,
    key_expr: &Expression<'_>,
    through_cast: bool,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    if constant_array_key(key_expr, scope).is_some()
        || matches!(unwrap_parens(key_expr), Expression::Literal(_))
    {
        return;
    }
    let Some(key) = expr_to_subject(key_expr) else {
        return;
    };
    let scope_resolver = scope.snapshot_resolver();
    let var_ctx = build_var_ctx("", ctx, &scope_resolver);
    let Some(array_type) =
        crate::type_engine::variable::resolution::resolve_arg_raw_type(array_expr, &var_ctx)
    else {
        return;
    };
    let Some(domain) = key_domain(&array_type, through_cast) else {
        return;
    };

    seed_synthetic_key_if_needed(&key, scope, ctx);
    let current = scope.get(&key).to_vec();
    let current_type = if current.is_empty() {
        // A call is keyed under its written form but only enters the scope
        // once something narrows it, so its own return type is the start.
        (!matches!(unwrap_parens(key_expr), Expression::Variable(_)))
            .then(|| {
                crate::type_engine::variable::resolution::resolve_arg_raw_type(key_expr, &var_ctx)
            })
            .flatten()
    } else {
        Some(ResolvedType::types_joined(&current))
    };
    let narrowed = match current_type {
        None => domain,
        Some(ty) if ty.is_mixed() || ty.is_untyped() => domain,
        Some(ty) => match narrow_to_key_domain(&ty, &domain) {
            Some(narrowed) => narrowed,
            None => return,
        },
    };
    scope.set(&key, vec![ResolvedType::from_type_string(narrowed)]);
}

/// Every value that finds an entry of an array of type `array_type`, or
/// `None` when that is not narrower than any key at all.
fn key_domain(array_type: &PhpType, through_cast: bool) -> Option<PhpType> {
    if let TypeKind::ArrayShape(entries) = array_type.kind() {
        if entries.is_empty() {
            return None;
        }
        let keys = crate::php_type::runtime_shape_keys(entries)?;
        if keys.iter().any(|key| key.contains("::")) {
            return None;
        }
        let (int_keys, string_keys): (Vec<&String>, Vec<&String>) = keys
            .iter()
            .partition(|key| crate::php_type::is_decimal_int_array_key(key));
        let mut domain: Vec<PhpType> = int_keys
            .iter()
            .map(|key| PhpType::literal_int(key.as_str()))
            .collect();
        // A decimal string reaches the same entry as the integer.
        domain.extend(
            int_keys
                .iter()
                .chain(string_keys.iter())
                .map(PhpType::literal_string_value),
        );
        if through_cast {
            let has_key = |k: &str| int_keys.iter().any(|key| key.as_str() == k);
            match (has_key("0"), has_key("1")) {
                (true, true) => domain.push(PhpType::bool()),
                (true, false) => domain.push(PhpType::false_()),
                (false, true) => domain.push(PhpType::true_()),
                (false, false) => {}
            }
            if string_keys.iter().any(|key| key.is_empty()) {
                domain.push(PhpType::null());
            }
            if !int_keys.is_empty() {
                domain.push(PhpType::float());
            }
        }
        return Some(PhpType::union(domain));
    }
    if through_cast || array_type.has_open_key_domain() {
        return None;
    }
    let key_type = array_type.iterable_key_type()?;
    (!key_type.is_mixed() && !key_type.is_array_key()).then_some(key_type)
}

/// The part of `key` that lies in `domain`, or `None` when that is all of
/// `key` or nothing of it.
///
/// Each alternative of `key` keeps the domain members it covers, so a
/// `string` key against the domain `1|'1'` becomes `'1'`; an alternative
/// that covers none of them but fits the domain whole stays.
fn narrow_to_key_domain(key: &PhpType, domain: &PhpType) -> Option<PhpType> {
    let domain_members = domain.union_members();
    let mut kept: Vec<PhpType> = Vec::new();
    for member in key.union_members() {
        let covered: Vec<&PhpType> = domain_members
            .iter()
            .copied()
            .filter(|d| d.is_subtype_of(member))
            .collect();
        if covered.is_empty() {
            if member.is_subtype_of(domain) && !kept.contains(member) {
                kept.push(member.clone());
            }
            continue;
        }
        for d in covered {
            if !kept.contains(d) {
                kept.push(d.clone());
            }
        }
    }
    if kept.is_empty() {
        return None;
    }
    let narrowed = PhpType::union(kept);
    (narrowed != *key).then_some(narrowed)
}
