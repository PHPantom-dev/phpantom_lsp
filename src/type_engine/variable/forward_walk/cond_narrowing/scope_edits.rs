use super::*;

/// Rewrite every type a variable could hold with `refine`, dropping the
/// members it rules out, and write the result back.
///
/// Returns whether anything survived. A variable with no recorded type is
/// left alone and reports `true`: nothing was ruled out, there was just
/// nothing to rule out. A variable `refine` emptied keeps what it had,
/// because the branch that asked is dead and saying so is the
/// reachability question rather than this one; a caller that wants to
/// answer it reads the return value.
fn refine_in_scope(
    var_name: &str,
    scope: &mut ScopeState,
    refine: impl FnMut(ResolvedType) -> Option<ResolvedType>,
) -> bool {
    let types = scope.get(var_name).to_vec();
    if types.is_empty() {
        return true;
    }

    let refined: Vec<ResolvedType> = types.into_iter().filter_map(refine).collect();
    if refined.is_empty() {
        return false;
    }
    scope.set(var_name, refined);
    true
}

/// Replace everything a variable could hold with `value`, provided one of
/// the types it holds could have been that value.
///
/// A union with nothing `admits` matches says the opposite of what the
/// condition proved, so it is left as it was.
fn narrow_to_in_scope(
    var_name: &str,
    scope: &mut ScopeState,
    value: PhpType,
    admits: impl Fn(&PhpType) -> bool,
) {
    let types = scope.get(var_name).to_vec();
    if types.iter().any(|rt| admits(&rt.type_string)) {
        scope.set(var_name, vec![ResolvedType::from_type_string(value)]);
    }
}

/// Whether a type has `false` among the values it could hold.
fn holds_false(ty: &PhpType) -> bool {
    let is_false = |t: &PhpType| matches!(t.kind(), TypeKind::Named(n) if n == "false");
    match ty.kind() {
        TypeKind::Union(members) => members.iter().any(is_false),
        _ => is_false(ty),
    }
}

/// Refine a variable's type to its non-empty counterpart.
///
/// `string` becomes `non-empty-string`, `array<K, V>` becomes
/// `non-empty-array<K, V>`, `list<T>` becomes `non-empty-list<T>`, and the
/// empty literal itself (`''`, `array{}`) drops out of a union. Members
/// outside the compared domain are left alone: `$x !== ''` on a
/// `string|array` says nothing about the array half.
pub(crate) fn refine_non_empty_in_scope(var_name: &str, empty: EmptyValue, scope: &mut ScopeState) {
    refine_in_scope(var_name, scope, |mut rt| {
        rt.type_string = refine_non_empty_type(&rt.type_string, empty)?;
        Some(rt)
    });
}

/// Refine a variable's type to its empty counterpart.
///
/// The mirror of [`refine_non_empty_in_scope`]: `array<K, V>` becomes
/// `array{}` and `string` becomes `''`, while a member that promises at
/// least one entry (`non-empty-array`, a shape with a required key) drops
/// out of a union because no empty value could have been it. Members
/// outside the compared domain are left alone, exactly as the non-empty
/// side leaves them: `count($x) === 0` on a `Countable|array` says nothing
/// about the object half.
pub(crate) fn refine_empty_in_scope(var_name: &str, empty: EmptyValue, scope: &mut ScopeState) {
    refine_in_scope(var_name, scope, |mut rt| {
        rt.type_string = refine_empty_type(&rt.type_string, empty)?;
        Some(rt)
    });
}

/// Narrow a variable in scope to `null` only.
///
/// Used when a condition like `$x === null` is true: the variable must
/// be null.  Replaces the variable's type with `null` if it currently
/// contains a nullable type, or sets it to `null` if the variable has
/// any type at all.
pub(crate) fn narrow_to_null_in_scope(var_name: &str, scope: &mut ScopeState) {
    // A `null` among the values the variable could hold is what makes
    // replacing them all with `null` sound.  A union that has none says
    // the opposite — `bool|string` came out of a falsy branch as `null`
    // while `non_null_type()` stood in for this check, because a union
    // with nothing to strip still has a non-null part.
    narrow_to_in_scope(var_name, scope, PhpType::null(), PhpType::accepts_null);
}

/// Narrow a variable in scope to `false` only.
///
/// Mirrors [`narrow_to_null_in_scope`] but for `false`: used when a
/// condition like `$x !== false` is known to be false, so the variable
/// must be `false`.
pub(crate) fn narrow_to_false_in_scope(var_name: &str, scope: &mut ScopeState) {
    narrow_to_in_scope(var_name, scope, PhpType::false_(), holds_false);
}

/// Keep only what a variable could hold and still be falsy.
///
/// The mirror of [`strip_falsy_from_scope`], which is what the branch a
/// truthy test *enters* applies. Both go through the same rule
/// ([`PhpType::falsy_type`] / [`PhpType::truthy_type`]), so the two halves
/// of one `if` agree about what the condition split.
///
/// A `bool` is the member that matters: keeping it whole on the path that
/// skipped the branch makes the two paths look like they could hold the
/// same value, so a join has nothing to key what the branch filled
/// against and re-testing the flag below recovers nothing:
///
/// ```php
/// $a = null;
/// if ($isI) { $a = makeA(); }
/// if ($isI) { $a->go(); }   // needs the skipped path to say `false`
/// ```
///
/// A variable nothing falsy could have been is left as it was rather than
/// emptied: the branch is dead, and saying so is the reachability
/// question rather than this one.
pub(crate) fn narrow_to_falsy_in_scope(var_name: &str, scope: &mut ScopeState) {
    narrow_to_falsy_part_in_scope(var_name, scope, PhpType::falsy_type);
}

/// Keep only what a variable could hold and still compare `== null`.
///
/// PHP compares `null` against a string as `''`, so `'0'` is the one falsy
/// value that is not loosely equal to `null`; everything else falsy is.
pub(crate) fn narrow_to_loosely_null_in_scope(var_name: &str, scope: &mut ScopeState) {
    narrow_to_falsy_part_in_scope(var_name, scope, |ty| {
        let falsy = ty.falsy_type()?;
        let is_zero_string = |m: &PhpType| {
            m.as_literal()
                .and_then(LiteralValue::string_content)
                .is_some_and(|text| text == "0")
        };
        if !falsy.union_members().into_iter().any(is_zero_string) {
            return Some(falsy);
        }
        let mut kept: Vec<PhpType> = falsy
            .union_members()
            .into_iter()
            .filter(|m| !is_zero_string(m))
            .cloned()
            .collect();
        match kept.len() {
            0 => None,
            1 => kept.pop(),
            _ => Some(PhpType::union(kept)),
        }
    });
}

fn narrow_to_falsy_part_in_scope(
    var_name: &str,
    scope: &mut ScopeState,
    falsy_part: impl Fn(&PhpType) -> Option<PhpType>,
) {
    refine_in_scope(var_name, scope, |mut rt| {
        let falsy = falsy_part(&rt.type_string)?;
        // An object is truthy, so a falsy `?Customer` is a `null` that
        // no longer names a class.  Leaving the resolved class beside
        // it makes the entry read as a `Customer` to everything that
        // consults the class rather than the type, and a join with the
        // branch's own `Customer` then collapses the pair to whichever
        // type string came last.
        let dropped_class =
            falsy.unwrap_nullable().class_name() != rt.type_string.unwrap_nullable().class_name();
        if dropped_class {
            rt.class_info = None;
        }
        rt.type_string = falsy;
        Some(rt)
    });
}

pub(crate) fn strip_null_from_scope(var_name: &str, scope: &mut ScopeState) {
    let survived = refine_in_scope(var_name, scope, |mut rt| {
        match rt.type_string.non_null_type() {
            Some(non_null) => {
                rt.type_string = non_null;
                Some(rt)
            }
            None if rt.type_string == PhpType::null() => None,
            None => Some(rt),
        }
    });
    if !survived {
        // Everything the variable could hold was `null`, so a path that
        // proves it is not null cannot run.  Saying so keeps the dead
        // path's end state out of the join instead of letting the
        // impossible `null` receiver there erase what the live paths
        // knew — which is what a first-iteration `if ($acc === null)`
        // seed/merge accumulator depends on.
        //
        // Inside that path the variable holds nothing at all, which is
        // `never`: left on `null`, every use of it there was checked
        // against the value the condition just ruled out.
        scope.set(
            var_name,
            vec![ResolvedType::from_type_string(PhpType::never())],
        );
        scope.unreachable = true;
    }
}

/// Strip both `null` and `false` from a variable's type in the scope.
///
/// Used after falsy guard clauses (`if (!$var) { throw; }`) where the
/// variable is known to be truthy (non-null and non-false) after the guard.
pub(crate) fn strip_falsy_from_scope(var_name: &str, scope: &mut ScopeState) {
    refine_in_scope(var_name, scope, |mut rt| {
        rt.type_string = rt.type_string.truthy_type()?;
        Some(rt)
    });
}

/// Strip `false` (but not `null`) from a variable's type in the scope.
///
/// Used after a strict-equality guard clause (`if ($var === false) {
/// throw; }`) where only `false` was ruled out — unlike
/// [`strip_falsy_from_scope`], which also strips `null` for the broader
/// `!$var`/`empty($var)` idiom that guards against both.
pub(crate) fn strip_false_from_scope(var_name: &str, scope: &mut ScopeState) {
    let is_false = |t: &PhpType| matches!(t.kind(), TypeKind::Named(n) if n == "false");
    refine_in_scope(var_name, scope, |mut rt| {
        let ty = &rt.type_string;
        if is_false(ty) {
            return None;
        }
        if let TypeKind::Union(members) = ty.kind() {
            let non_false: Vec<PhpType> =
                members.iter().filter(|m| !is_false(m)).cloned().collect();
            if non_false.is_empty() {
                return None;
            }
            rt.type_string = PhpType::union(non_false);
        }
        Some(rt)
    });
}

/// Split a single-level array access key like `$a["test"]` into base
/// variable and key name.  Returns `None` for non-array-access keys and
/// for multi-level access (`$a["x"]["y"]`), which this single-key
/// narrowing cannot represent and would otherwise mis-split.
pub(crate) fn split_array_access_key(key: &str) -> Option<(&str, &str)> {
    let bracket_pos = key.find("[\"")?;
    let base = &key[..bracket_pos];
    // The base must be a plain expression with no earlier array access.
    if base.contains('[') {
        return None;
    }
    let key_name = key[bracket_pos + 2..].strip_suffix("\"]")?;
    // A nested access leaves bracket characters inside the extracted key
    // (e.g. `x"]["y`); reject it rather than narrowing a bogus key.
    if key_name.contains('[') || key_name.contains(']') {
        return None;
    }
    Some((base, key_name))
}

/// Strip `null` from a specific array shape key on a variable.
///
/// Given variable `$a` typed as `array{test: ?int}` and key `"test"`,
/// rewrites the variable's type to `array{test: int}`.  This modifies
/// the base variable's type directly so the narrowed shape survives
/// scope merges (unlike synthetic scope entries which are stripped).
/// Remove `null` from an array element a check proved non-null.
///
/// A constant shape records each element's type inline, so the refinement
/// belongs on the base variable, where it survives scope merges.  A generic
/// `array<K, V|null>` has no per-key slot to refine — narrowing its value
/// type would wrongly claim every other key is non-null too — so the proof
/// is recorded on the synthetic `$a["k"]` scope key that offset reads
/// consult.
pub(super) fn strip_null_from_array_element(
    access_key: &str,
    base_var: &str,
    key_name: &str,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    strip_null_from_array_shape_key(base_var, key_name, scope);
    seed_synthetic_key_if_needed(access_key, scope, ctx);
    strip_null_from_scope(access_key, scope);
}

/// Strip `null` from whatever a condition proved non-null, whether that
/// is a plain subject or one element of an array.
pub(super) fn strip_null_from_subject(
    var_name: &str,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    match split_array_access_key(var_name) {
        Some((base, key)) => strip_null_from_array_element(var_name, base, key, scope, ctx),
        None => {
            seed_synthetic_key_if_needed(var_name, scope, ctx);
            strip_null_from_scope(var_name, scope);
        }
    }
}

/// Like [`strip_null_from_subject`], but an array element is narrowed on
/// the base variable's shape only.
///
/// This is what a guard clause leaves behind: the synthetic `$a["k"]`
/// entry belongs to the branch that was taken, while the shape refinement
/// is what the code past the guard reads.
pub(super) fn strip_null_from_subject_shape(
    var_name: &str,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    match split_array_access_key(var_name) {
        Some((base, key)) => strip_null_from_array_shape_key(base, key, scope),
        None => {
            seed_synthetic_key_if_needed(var_name, scope, ctx);
            strip_null_from_scope(var_name, scope);
        }
    }
}

pub(crate) fn strip_null_from_array_shape_key(
    base_var: &str,
    key_name: &str,
    scope: &mut ScopeState,
) {
    let types = scope.get(base_var).to_vec();
    if types.is_empty() {
        return;
    }
    let narrowed: Vec<ResolvedType> = types
        .into_iter()
        .map(|mut rt| {
            rt.type_string = strip_null_from_shape_key(&rt.type_string, key_name);
            rt
        })
        .collect();
    scope.set(base_var, narrowed);
}

/// Recursively strip `null` from a specific key in an array shape type.
pub(crate) fn strip_null_from_shape_key(
    ty: &crate::php_type::PhpType,
    key: &str,
) -> crate::php_type::PhpType {
    use crate::php_type::{PhpType, ShapeEntry, TypeKind};
    match ty.kind() {
        TypeKind::ArrayShape(entries) => {
            let new_entries: Vec<ShapeEntry> = entries
                .iter()
                .map(|e| {
                    if e.key.as_deref() == Some(key) {
                        let non_null = e
                            .value_type
                            .non_null_type()
                            .unwrap_or_else(|| e.value_type.clone());
                        ShapeEntry {
                            key: e.key.clone(),
                            value_type: non_null,
                            optional: false, // known to be present (was checked)
                        }
                    } else {
                        e.clone()
                    }
                })
                .collect();
            PhpType::array_shape(new_entries)
        }
        TypeKind::Nullable(inner) => {
            // `?array{test: ?int}` → `?array{test: int}`
            PhpType::nullable(strip_null_from_shape_key(inner, key))
        }
        TypeKind::Union(members) => {
            let new_members: Vec<PhpType> = members
                .iter()
                .map(|m| strip_null_from_shape_key(m, key))
                .collect();
            PhpType::union(new_members)
        }
        other => other.clone().into(),
    }
}

#[cfg(test)]
mod tests {
    use super::split_array_access_key;

    #[test]
    fn splits_single_level_string_key() {
        assert_eq!(split_array_access_key("$a[\"test\"]"), Some(("$a", "test")));
    }

    #[test]
    fn rejects_non_array_access() {
        assert_eq!(split_array_access_key("$a"), None);
    }

    #[test]
    fn rejects_nested_array_access() {
        // `$a["x"]["y"]` must not be mis-split into base `$a` and key
        // `x"]["y`; single-key narrowing cannot represent it.
        assert_eq!(split_array_access_key("$a[\"x\"][\"y\"]"), None);
    }

    #[test]
    fn rejects_base_with_earlier_access() {
        assert_eq!(split_array_access_key("$a[0][\"y\"]"), None);
    }
}
