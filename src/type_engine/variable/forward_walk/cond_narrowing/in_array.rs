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
        let Some((base_key, key_expr, negated)) = array_key_exists_target(operand) else {
            continue;
        };
        let Some(key_name) = constant_array_key(key_expr, scope) else {
            continue;
        };
        // A property or static-property subject (`$this->excludePaths`)
        // is not a tracked local, so its shape has to be brought into
        // the scope before it can be refined.
        seed_synthetic_key_if_needed(&base_key, scope, ctx);
        if negated != inverted {
            mark_array_shape_key_absent(&base_key, &key_name, scope);
            continue;
        }
        mark_array_shape_key_present(&base_key, &key_name, scope);
        // Seed the element key after the shape is refined, so an offset
        // read consulting it sees the present-key type rather than the
        // optional one it would have resolved a moment earlier.
        seed_synthetic_key_if_needed(&format!("{base_key}[\"{key_name}\"]"), scope, ctx);
    }
}

/// Apply `in_array($var, $haystack, true)` narrowing.
///
/// When `inverted` is false (truthy branch / while body), the variable is
/// narrowed to the haystack's element type (inclusion).  When `inverted` is
/// true (else branch / guard clause inverse), the variable is narrowed by
/// excluding the element type.
pub(crate) fn apply_in_array_narrowing<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    inverted: bool,
) {
    let scope_resolver = scope.snapshot_resolver();

    // Unwrap parentheses and detect negation.
    let (inner, negated) = narrowing::unwrap_condition_negation(condition);

    // Check every variable in scope as the potential needle.
    let var_names: Vec<Atom> = scope.locals.keys().copied().collect();
    for var_name in &var_names {
        if let Some(haystack_expr) = narrowing::try_extract_in_array(inner, var_name) {
            // Resolve the haystack's type from the scope to extract the
            // element type.  This replaces the backward scanner's
            // `resolve_arg_raw_type` with a scope-based lookup.
            let element_type = resolve_in_array_element_type_fw(haystack_expr, scope, ctx);
            let element_type = match element_type {
                Some(et) => et,
                None => continue,
            };

            // Determine whether to include or exclude:
            // - truthy + positive  → include (var IS in haystack)
            // - truthy + negated   → exclude (var is NOT in haystack)
            // - inverse + positive → exclude
            // - inverse + negated  → include
            let should_exclude = inverted ^ negated;

            let var_ctx = build_var_ctx(var_name, ctx, &scope_resolver);
            let mut results = scope.get(var_name).to_vec();

            if should_exclude {
                // Skip exclusion when it would remove ALL type information.
                let would_remove_all = {
                    let mut test = results.clone();
                    ResolvedType::apply_narrowing(&mut test, |classes| {
                        narrowing::apply_instanceof_exclusion(&element_type, &var_ctx, classes)
                    });
                    test.is_empty()
                };
                if !would_remove_all {
                    ResolvedType::apply_narrowing(&mut results, |classes| {
                        narrowing::apply_instanceof_exclusion(&element_type, &var_ctx, classes)
                    });
                }
                // `apply_narrowing` only reaches the class layer, so a
                // needle with no class behind it comes back untouched.
                // Strict equality is a proof about the value, so failing
                // it rules out every alternative the haystack's elements
                // account for: `!in_array($doc, [null, ''], true)` leaves
                // a `?string` needle holding `string`.
                for rt in results.iter_mut() {
                    if rt.class_info.is_some() {
                        continue;
                    }
                    let mut narrowed = rt.type_string.clone();
                    for member in element_type.union_members() {
                        match strip_literal_from_type(&narrowed, member) {
                            Some(kept) => narrowed = kept,
                            // Excluding everything would leave the needle
                            // with no type at all, which says less than
                            // the type it came in with.
                            None => continue,
                        }
                    }
                    rt.type_string = narrowed;
                }
            } else {
                ResolvedType::apply_narrowing(&mut results, |classes| {
                    narrowing::apply_instanceof_inclusion(&element_type, false, &var_ctx, classes)
                });
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

/// Resolve the element type of a haystack expression for `in_array`
/// narrowing, using the forward walker's scope instead of the backward
/// scanner.
pub(crate) fn resolve_in_array_element_type_fw(
    haystack_expr: &Expression<'_>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<PhpType> {
    // If the haystack is a simple variable, look it up in the scope.
    if let Expression::Variable(Variable::Direct(dv)) = haystack_expr {
        let var_name = bytes_to_str(dv.name).to_string();
        let types = scope.get(&var_name);
        if !types.is_empty() {
            let joined = ResolvedType::types_joined(types);
            if let Some(elem) = joined.extract_element_type() {
                return Some(elem.clone());
            }
            // Try extracting value type for generic collections.
            if let Some(val) = joined.extract_value_type(true) {
                return Some(val.clone());
            }
        }
        // Fall back to docblock annotation.
        let offset = haystack_expr.span().start.offset as usize;
        let from_docblock =
            crate::docblock::find_iterable_raw_type_in_source(ctx.content, offset, &var_name)
                .map(|t| crate::util::resolve_php_type_names(&t, ctx.class_loader));
        if let Some(raw) = from_docblock
            && let Some(elem) = raw.extract_element_type()
        {
            return Some(elem.clone());
        }
        return None;
    }

    // For non-variable expressions (method calls, property access, etc.),
    // try resolving via the expression resolution pipeline.
    let scope_resolver = scope.snapshot_resolver();
    let var_ctx = build_var_ctx("", ctx, &scope_resolver);
    let raw_type =
        crate::type_engine::variable::resolution::resolve_arg_raw_type(haystack_expr, &var_ctx);
    // A constant list (`self::APPROVED`, `[Status::ACTIVE, …]`) resolves to
    // a shape rather than a parameterised array, and its elements are the
    // shape's values.
    raw_type.and_then(|t| t.iterable_element_type())
}
