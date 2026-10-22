use super::*;

/// Apply `array_key_exists('k', $arr)` narrowing to an array shape's
/// optional key.
///
/// A shape's optional key reads as `T|null`, because the key may be
/// absent.  `array_key_exists` proves it is present, so the read is
/// plain `T` — the same refinement `isset()` gets, minus the extra proof
/// that the value itself is not null:
///
/// ```php
/// /** @param array{a?: array<int,string>} $shape */
/// if (array_key_exists('a', $shape)) {
///     return $shape['a']; // array<int,string>, not ?array<int,string>
/// }
/// ```
///
/// The key may be written as a literal or held in a variable whose type is
/// one.  `inverted` selects the polarity the caller establishes, so an
/// `if (!array_key_exists('a', $shape)) { return; }` guard refines its
/// fall-through.  In the direction that proves the key absent, an
/// optional entry is dropped from the shape.
pub(crate) fn apply_array_key_exists_narrowing<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    inverted: bool,
) {
    for operand in collect_and_chain_operands(condition) {
        let Some((array_expr, key_expr, negated)) = array_key_exists_target(operand) else {
            continue;
        };
        let Some(base_key) = condition_facts::expr_to_subject_key(array_expr) else {
            continue;
        };
        narrow_key_existence(&base_key, key_expr, negated == inverted, scope, ctx);
    }
}

/// Narrow the way PHPUnit's `assertArrayHasKey($key, $array)` (`present`)
/// or `assertArrayNotHasKey()` proves it, which is the
/// `array_key_exists($key, $array)` the assertion stands for.
pub(crate) fn apply_array_key_assertion(
    key_expr: &Expression<'_>,
    array_expr: &Expression<'_>,
    present: bool,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    if let Some(base_key) = condition_facts::expr_to_subject_key(array_expr) {
        narrow_key_existence(&base_key, key_expr, present, scope, ctx);
    }
    if present {
        narrow_key(array_expr, key_expr, false, scope, ctx);
    }
}

/// Refine `base_key`'s shape by whether it holds the key `key_expr` names.
fn narrow_key_existence(
    base_key: &str,
    key_expr: &Expression<'_>,
    present: bool,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // A property or static-property subject (`$this->excludePaths`)
    // is not a tracked local, so its type has to be brought into the
    // scope before it can be refined.
    seed_synthetic_key_if_needed(base_key, scope, ctx);
    let Some(key_name) = constant_array_key(key_expr, scope) else {
        // Whatever the key is, an array holding it is not empty: the
        // `[]` a loop's first pass carries cannot reach this branch,
        // so a read of the key does not see the `null` it would give.
        if present {
            refine_non_empty_in_scope(base_key, EmptyValue::Array, scope);
        }
        return;
    };
    if !present {
        mark_array_shape_key_absent(base_key, &key_name, scope);
        return;
    }
    mark_array_shape_key_present(base_key, &key_name, scope);
    // Seed the element key after the shape is refined, so an offset
    // read consulting it sees the present-key type rather than the
    // optional one it would have resolved a moment earlier.
    seed_synthetic_key_if_needed(&format!("{base_key}[\"{key_name}\"]"), scope, ctx);
}

/// Apply `in_array($var, $haystack, true)` narrowing.
///
/// When `inverted` is false (truthy branch / while body), the variable is
/// narrowed to the haystack's element type (inclusion).  When `inverted` is
/// true (else branch / guard clause inverse), the variable is narrowed by
/// excluding the values the haystack's type says it holds
/// ([`values_held_by`]), not the element type.
pub(crate) fn apply_in_array_narrowing<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    inverted: bool,
) {
    let scope_resolver = scope.snapshot_resolver();

    // Unwrap parentheses and detect negation.
    let (inner, negated) = condition_facts::unwrap_condition_negation(condition);

    // Check every variable the condition names as the potential needle.
    let var_names = scope_keys_named_by(condition, scope);
    for var_name in &var_names {
        if let Some(haystack_expr) = condition_facts::try_extract_in_array(inner, var_name) {
            let Some(haystack) = resolve_in_array_haystack_type_fw(haystack_expr, scope, ctx)
            else {
                continue;
            };

            // Determine whether to include or exclude:
            // - truthy + positive  → include (var IS in haystack)
            // - truthy + negated   → exclude (var is NOT in haystack)
            // - inverse + positive → exclude
            // - inverse + negated  → include
            let should_exclude = inverted ^ negated;

            let mut results = scope.get(var_name).to_vec();

            if should_exclude {
                let held = values_held_by(&haystack);
                if held.is_empty() {
                    continue;
                }
                // Strict equality is a proof about the value, so failing it
                // rules out every alternative a held value accounts for:
                // `!in_array($doc, [null, ''], true)` leaves a `?string`
                // needle holding `string`.  A needle that is nothing but
                // held values is in a branch that cannot run, which is the
                // reachability question rather than this one, so it keeps
                // the type it came in with.
                results = results
                    .into_iter()
                    .filter_map(|mut rt| {
                        for value in &held {
                            rt.type_string = strip_literal_from_type(&rt.type_string, value)?;
                        }
                        Some(rt)
                    })
                    .collect();
            } else {
                let Some(element_type) = haystack.iterable_element_type() else {
                    continue;
                };
                // An element that could be anything could be an object of
                // any class, so it proves nothing about the class layer
                // either: `narrow_value_by_element` draws the same line for
                // the declared type.
                if element_type.contains_mixed() || element_type.is_untyped() {
                    continue;
                }
                let var_ctx = build_var_ctx(var_name, ctx, &scope_resolver);
                // An element that can hold an object yet names no class
                // that loads (`object`, an unknown class) may be an
                // ancestor of the needle's classes, so they stay.
                let every_object_element_loads =
                    element_type.union_members().into_iter().all(|member| {
                        !member.is_object_like()
                            || !condition_facts::resolve_narrowing_target(member, &var_ctx)
                                .is_empty()
                    });
                if every_object_element_loads {
                    // The class layer concludes the needle is one of the
                    // element's classes, which would rule out a `string`
                    // alternative an element type of `Foo|string` admits.
                    // The entries with no class are narrowed by value
                    // below instead.
                    let (mut classless, mut classed): (Vec<_>, Vec<_>) =
                        results.into_iter().partition(|rt| rt.class_info.is_none());
                    // A `mixed` needle could have been any of the elements:
                    // the class layer brings in the ones that name a class,
                    // and the rest are added back below.
                    let had_mixed = classless.iter().any(|rt| rt.type_string.is_mixed());
                    classless.retain(|rt| !rt.type_string.is_mixed());
                    if !classed.is_empty() || had_mixed {
                        ResolvedType::apply_narrowing(&mut classed, |classes| {
                            condition_facts::apply_instanceof_inclusion(
                                &element_type,
                                false,
                                &var_ctx,
                                classes,
                            )
                        });
                    }
                    results = classed;
                    results.extend(classless);
                    if had_mixed {
                        let values: Vec<PhpType> = element_type
                            .union_members()
                            .into_iter()
                            .filter(|member| !member.is_object_like())
                            .cloned()
                            .collect();
                        if !values.is_empty() {
                            ResolvedType::extend_unique(
                                &mut results,
                                vec![ResolvedType::from_type_string(PhpType::union(values))],
                            );
                        }
                    }
                }
                // `apply_narrowing` works on the class layer, so a needle
                // with no class behind it (`?string`, `int|string`) comes
                // back untouched.  Strict equality against the haystack's
                // elements is a proof about the value, so it narrows the
                // declared type too.
                for rt in results.iter_mut() {
                    if rt.class_info.is_none()
                        && let Some(narrowed) =
                            narrow_value_by_element(&rt.type_string, &element_type)
                    {
                        rt.type_string = narrowed;
                    }
                }
                // A `null` the needle carries as an entry of its own is
                // ruled out by an element type that holds no `null`, the
                // same as one inside a `?string` entry is.
                if !element_type.accepts_null()
                    && results.iter().any(|rt| !rt.type_string.is_null())
                {
                    results.retain(|rt| !rt.type_string.is_null());
                }
            }

            if !results.is_empty() {
                scope.set(var_name, results);
            }
        }
    }
}

/// The needle's type once a strict `in_array` has proved it equals one of
/// the haystack's elements, or `None` when that proves nothing new.
///
/// Every alternative the needle could hold that no element could equal is
/// gone: `?string` against a `list<string>` haystack keeps only `string`,
/// which is what makes the `if (!in_array(…)) { abort(); }` gate leave a
/// definite value behind it.  Where the elements are *narrower* than the
/// alternative they match, the alternative is replaced by them, so a
/// constant list of literals narrows a `string` needle to exactly the
/// values the list names.
///
/// A needle typed `mixed`, or a haystack whose element type is unknown,
/// proves nothing worth recording — narrowing `mixed` to the element type
/// would claim the haystack is exhaustive over a type nothing constrains.
fn narrow_value_by_element(needle: &PhpType, element: &PhpType) -> Option<PhpType> {
    if needle.is_mixed() || element.is_mixed() || needle.is_untyped() || element.is_untyped() {
        return None;
    }
    let element_members = element.union_members();
    let mut kept: Vec<PhpType> = Vec::new();
    for member in needle.union_members() {
        let matching: Vec<PhpType> = element_members
            .iter()
            .filter(|e| e.is_subtype_of(member))
            .map(|e| (*e).clone())
            .collect();
        if !matching.is_empty() {
            for m in matching {
                if !kept.contains(&m) {
                    kept.push(m);
                }
            }
        } else if member.is_subtype_of(element) && !kept.contains(member) {
            kept.push(member.clone());
        }
    }
    if kept.is_empty() {
        return None;
    }
    let narrowed = PhpType::union(kept);
    (narrowed != *needle).then_some(narrowed)
}

/// The values a haystack of type `haystack` is known to hold.
///
/// A failed `in_array()` proves the needle is none of the values the
/// haystack holds, and a type that only bounds its elements names none of
/// them: a `list<string>` may hold no string at all, and a
/// `list<AdminUser>` none of the admins the needle could be.  A value is
/// named by a shape entry that is not optional and holds exactly one value
/// (`'draft'`, `null`), which is what an array literal or a constant list
/// resolves to, or by a non-empty array whose every element is that one
/// value.  A union holds only what each of its alternatives holds.
pub(super) fn values_held_by(haystack: &PhpType) -> Vec<PhpType> {
    if let Some(unsealed) = haystack.as_unsealed_shape() {
        return values_held_by(&unsealed.shape);
    }
    match haystack.kind() {
        TypeKind::ArrayShape(entries) => {
            let mut held: Vec<PhpType> = Vec::new();
            for entry in entries.iter() {
                if !entry.optional
                    && is_single_value(&entry.value_type)
                    && !held.contains(&entry.value_type)
                {
                    held.push(entry.value_type.clone());
                }
            }
            held
        }
        TypeKind::Union(members) => {
            let Some((first, rest)) = members.split_first() else {
                return Vec::new();
            };
            let mut held = values_held_by(first);
            for member in rest {
                if held.is_empty() {
                    break;
                }
                let other = values_held_by(member);
                held.retain(|value| other.contains(value));
            }
            held
        }
        _ => match haystack.iterable_element_type() {
            Some(element) if haystack.is_provably_non_empty() && is_single_value(&element) => {
                vec![element]
            }
            _ => Vec::new(),
        },
    }
}

/// Resolve the type of a haystack expression for `in_array` narrowing,
/// using the forward walker's scope instead of the backward scanner.
///
/// A variable whose scope type has no elements to read (a bare `array`, the
/// `[]` a list starts out as) is read from its docblock annotation instead.
pub(crate) fn resolve_in_array_haystack_type_fw(
    haystack_expr: &Expression<'_>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<PhpType> {
    // If the haystack is a simple variable, look it up in the scope.
    if let Expression::Variable(Variable::Direct(dv)) = haystack_expr {
        let var_name = bytes_to_str(dv.name).to_string();
        let joined = ResolvedType::types_joined(scope.get(&var_name));
        return joined.iterable_element_type().is_some().then_some(joined);
    }

    // For non-variable expressions (method calls, property access, etc.),
    // try resolving via the expression resolution pipeline.
    let scope_resolver = scope.snapshot_resolver();
    let var_ctx = build_var_ctx("", ctx, &scope_resolver);
    crate::type_engine::variable::resolution::resolve_arg_raw_type(haystack_expr, &var_ctx)
}
