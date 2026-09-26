use super::*;

use crate::php_type::ShapeEntry;

/// Extract the `(base subject key, key expression, negated)` of an
/// `array_key_exists($k, $arr)` check, unwrapping parentheses and a
/// leading `!`.
pub(super) fn array_key_exists_target<'b>(
    expr: &'b Expression<'b>,
) -> Option<(String, &'b Expression<'b>, bool)> {
    match expr {
        Expression::Parenthesized(inner) => array_key_exists_target(inner.expression),
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            array_key_exists_target(prefix.operand)
                .map(|(base, key, negated)| (base, key, !negated))
        }
        Expression::Call(Call::Function(call)) => {
            let Expression::Identifier(ident) = call.function else {
                return None;
            };
            if bytes_to_str(ident.value()).trim_start_matches('\\') != "array_key_exists" {
                return None;
            }
            let args: Vec<_> = call.argument_list.arguments.iter().collect();
            if args.len() < 2 {
                return None;
            }
            let base_key = narrowing::expr_to_subject_key(narrowing::argument_value(args[1]))?;
            Some((base_key, narrowing::argument_value(args[0]), false))
        }
        _ => None,
    }
}

/// The array key `expr` is known to name, normalised the way PHP stores
/// it: a written `'k'` or `3`, or a variable whose type is one literal.
///
/// A decimal-integer string is the integer key PHP turns it into, so
/// `'1'` and `1` both name the key shapes record as `1`.
pub(super) fn constant_array_key(expr: &Expression<'_>, scope: &ScopeState) -> Option<String> {
    use mago_syntax::cst::Literal;
    match unwrap_parens(expr) {
        Expression::Literal(Literal::Integer(int)) => int.value.map(|v| v.to_string()),
        Expression::Literal(Literal::String(_)) => {
            narrowing::string_literal_value(unwrap_parens(expr))
        }
        other => {
            let key = expr_to_subject(other)?;
            let types = scope.get(&key);
            if types.is_empty() {
                return None;
            }
            match ResolvedType::types_joined(types).as_literal()? {
                LiteralValue::Int(raw) => {
                    crate::php_type::is_decimal_int_array_key(raw).then(|| raw.to_string())
                }
                literal @ LiteralValue::String(_) => {
                    literal.string_content().map(|content| content.into_owned())
                }
                LiteralValue::Float(_) => None,
            }
        }
    }
}

/// Mark one key of an array shape as present, leaving its value type
/// alone.
///
/// The sibling [`strip_null_from_array_shape_key`] also drops `null` from
/// the value, which is what `isset()` proves.  `array_key_exists` proves
/// only presence, so a shape entry declared `?T` stays `?T`.
pub(super) fn mark_array_shape_key_present(base_var: &str, key_name: &str, scope: &mut ScopeState) {
    rewrite_shape_key(base_var, key_name, scope, |entry| {
        Some(ShapeEntry {
            optional: false,
            ..entry.clone()
        })
    });
}

/// Drop one optional key from an array shape, once the key is known to be
/// absent.
///
/// A key the shape requires is left alone: the check failing contradicts
/// the shape, which says nothing useful about which entry to keep.
pub(super) fn mark_array_shape_key_absent(base_var: &str, key_name: &str, scope: &mut ScopeState) {
    rewrite_shape_key(base_var, key_name, scope, |entry| {
        if entry.optional {
            None
        } else {
            Some(entry.clone())
        }
    });
}

/// Carry what a condition proved about an offset (`$x["k"]`) into the
/// entry the array's shape holds for it.
///
/// `is_int($x['k'])` narrows the offset's own key, but a read of `$x` still
/// showed the entry's old type: the offset and the shape it lives in were
/// two records of one value, and only one of them was told.  Only a proof
/// the entry's type admits is written, so a check the shape contradicts
/// leaves the shape to say so.
pub(super) fn write_offset_narrowing_into_shapes(
    condition: &Expression<'_>,
    scope: &mut ScopeState,
) {
    for key in collect_condition_property_keys(condition) {
        write_offset_key_into_shapes(&key, scope);
    }
}

/// [`write_offset_narrowing_into_shapes`] for one offset key
/// (`$x["k"]`), whatever narrowed it.
///
/// A union of shapes that the entry tells apart (the tagged-union idiom
/// `array{type: 'a', …}|array{type: 'b', …}`) also loses every member whose
/// entry cannot hold the narrowed value, so the branch sees only the
/// shapes the check left possible.
pub(crate) fn write_offset_key_into_shapes(key: &str, scope: &mut ScopeState) {
    let Some((base, segment)) = narrowing::split_trailing_bracket(key) else {
        return;
    };
    let Some(literal) = segment
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    else {
        return;
    };
    let types = scope.get(key);
    if types.is_empty() || !scope.contains(base) {
        return;
    }
    let narrowed = ResolvedType::types_joined(types);
    discard_shapes_excluding(base, literal, &narrowed, scope);

    let is_refinement = |entry: &ShapeEntry| {
        entry.value_type != narrowed && narrowed.is_subtype_of(&entry.value_type)
    };
    let refines_an_entry = scope.get(base).iter().any(|rt| {
        rt.type_string
            .union_members()
            .into_iter()
            .any(|shape| shape_entry_at(shape, literal).is_some_and(&is_refinement))
    });
    if !refines_an_entry {
        return;
    }
    rewrite_shape_key(base, literal, scope, |entry| {
        Some(if is_refinement(entry) {
            ShapeEntry {
                value_type: narrowed.clone(),
                ..entry.clone()
            }
        } else {
            entry.clone()
        })
    });
}

/// Drop the shapes in `base_var`'s union whose entry under `key` cannot
/// hold any value of `narrowed`.
///
/// Only a `narrowed` made of literals is decided, against an entry made of
/// scalars: whether a literal fits a scalar type is exact, while two class
/// or template types can share values neither names.  A proof every member
/// contradicts leaves the union alone, since the branch that sees it cannot
/// run.
fn discard_shapes_excluding(base_var: &str, key: &str, narrowed: &PhpType, scope: &mut ScopeState) {
    let literals: Vec<&PhpType> = narrowed.union_members();
    if !literals.iter().all(|m| m.as_literal().is_some()) {
        return;
    }
    let excludes = |shape: &PhpType| {
        let Some(entry) = shape_entry_at(shape, key) else {
            return false;
        };
        let decidable = entry.value_type.union_members().iter().all(|m| {
            matches!(m.kind(), TypeKind::Literal(_) | TypeKind::IntRange(..))
                || (matches!(m.kind(), TypeKind::Named(_)) && m.is_scalar())
        });
        decidable
            && !literals
                .iter()
                .any(|literal| literal.is_subtype_of(&entry.value_type))
    };

    let types = scope.get(base_var);
    let mut changed = false;
    let mut kept: Vec<ResolvedType> = Vec::with_capacity(types.len());
    for rt in types {
        let members = rt.type_string.union_members();
        let mut surviving: Vec<PhpType> = members
            .iter()
            .filter(|m| !excludes(m))
            .map(|m| (*m).clone())
            .collect();
        if surviving.len() == members.len() {
            kept.push(rt.clone());
            continue;
        }
        changed = true;
        if surviving.is_empty() {
            continue;
        }
        let type_string = if surviving.len() == 1 {
            surviving.swap_remove(0)
        } else {
            PhpType::union(surviving)
        };
        kept.push(ResolvedType {
            type_string,
            ..rt.clone()
        });
    }
    if changed && !kept.is_empty() {
        scope.set(base_var, kept);
    }
}

/// The entry the shape `ty` holds under the runtime key `key`.
fn shape_entry_at<'t>(ty: &'t PhpType, key: &str) -> Option<&'t ShapeEntry> {
    let TypeKind::ArrayShape(entries) = ty.kind() else {
        return None;
    };
    let runtime_keys = crate::php_type::runtime_shape_keys(entries)?;
    runtime_keys
        .iter()
        .position(|k| k == key)
        .map(|index| &entries[index])
}

/// Rewrite the entry an array shape holds under one runtime key, through
/// every nullable and union layer of the scope's type for `base_var`.
///
/// `rewrite` returning `None` drops the entry.
fn rewrite_shape_key(
    base_var: &str,
    key_name: &str,
    scope: &mut ScopeState,
    rewrite: impl Fn(&ShapeEntry) -> Option<ShapeEntry>,
) {
    let types = scope.get(base_var).to_vec();
    if types.is_empty() {
        return;
    }
    let narrowed: Vec<ResolvedType> = types
        .into_iter()
        .map(|mut rt| {
            rt.type_string = rewrite_shape_key_in(&rt.type_string, key_name, &rewrite);
            rt
        })
        .collect();
    scope.set(base_var, narrowed);
}

fn rewrite_shape_key_in(
    ty: &PhpType,
    key: &str,
    rewrite: &impl Fn(&ShapeEntry) -> Option<ShapeEntry>,
) -> PhpType {
    match ty.kind() {
        TypeKind::ArrayShape(entries) => {
            // A positional entry occupies the index it was appended at, so
            // `array{1, 2?}` holds its optional entry under key `1`.
            let runtime_keys = crate::php_type::runtime_shape_keys(entries);
            let mut new_entries: Vec<ShapeEntry> = Vec::with_capacity(entries.len());
            let mut dropped = false;
            for (i, e) in entries.iter().enumerate() {
                let entry_key = match &runtime_keys {
                    Some(keys) => Some(keys[i].as_str()),
                    None => e.key.as_deref(),
                };
                if entry_key != Some(key) {
                    new_entries.push(e.clone());
                } else if let Some(replaced) = rewrite(e) {
                    new_entries.push(replaced);
                } else {
                    dropped = true;
                }
            }
            // Dropping an entry must not move the positional entries after
            // it to a different index, so every survivor keeps its key.
            if dropped && let Some(keys) = &runtime_keys {
                let mut kept = keys.iter().filter(|k| k.as_str() != key);
                for entry in &mut new_entries {
                    if let Some(k) = kept.next() {
                        entry.key.get_or_insert_with(|| k.clone());
                    }
                }
            }
            PhpType::array_shape(new_entries)
        }
        TypeKind::Nullable(inner) => PhpType::nullable(rewrite_shape_key_in(inner, key, rewrite)),
        TypeKind::Union(members) => PhpType::union(
            members
                .iter()
                .map(|m| rewrite_shape_key_in(m, key, rewrite))
                .collect(),
        ),
        other => other.clone().into(),
    }
}

/// Qualify the unqualified class names in an assertion's type against
/// the namespace of the file that declared the tag.
///
/// `@phpstan-assert-if-true TestMethod $this` on an interface in
/// `PHPUnit\Event\Code` names `PHPUnit\Event\Code\TestMethod`, the way
/// PHP resolves every other unqualified name in that file.  The call
/// site's namespace has nothing to do with it, and the short-name index
/// that would otherwise find the class covers the project's own files
/// only — so a tag a vendor package declares on itself resolved to
/// nothing at all.
///
/// A name that does not resolve to a class under the declaring namespace
/// is left alone, so an already-qualified name and the short-name
/// fallback both keep working.
pub(super) fn qualify_assertion_type(
    asserted: &PhpType,
    declaring_namespace: Option<&str>,
    ctx: &ForwardWalkCtx<'_>,
) -> PhpType {
    let Some(namespace) = declaring_namespace.filter(|ns| !ns.is_empty()) else {
        return asserted.clone();
    };
    asserted.resolve_names(&|name| {
        if name.contains('\\') {
            return name.to_string();
        }
        let qualified = format!("{}\\{}", namespace, name);
        if (ctx.class_loader)(&qualified).is_some() {
            qualified
        } else {
            name.to_string()
        }
    })
}

/// The namespace part of a fully-qualified class name, or `None` for a
/// class in the global namespace.
pub(super) fn namespace_of_fqn(fqn: &str) -> Option<String> {
    let trimmed = fqn.trim_start_matches('\\');
    trimmed
        .rfind('\\')
        .map(|pos| trimmed[..pos].to_string())
        .filter(|ns| !ns.is_empty())
}

/// Apply one `@phpstan-assert-if-true` / `-if-false` conclusion to the
/// scope entry for `target`.
///
/// `target` is a scope key rather than a plain variable name: an
/// assertion whose subject is written `$this->getClassReflection()`
/// resolves to a member path off the receiver, which the scope tracks
/// under its own key once it has been seeded.
///
/// A class-named assertion narrows through the `instanceof` machinery. A
/// scalar or pseudo-type one (`!null`, `string`, `array` — PHPUnit's
/// `assertIsString` and every `!null` promise) names no class at all, so
/// that machinery would exclude nothing and include nothing; those are
/// routed through the same type guards the matching `is_*()` check uses.
pub(super) fn apply_assertion_to_key(
    target: &str,
    asserted_type: &PhpType,
    should_exclude: bool,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    scope_resolver: &dyn Fn(&str) -> Vec<ResolvedType>,
) {
    seed_synthetic_key_if_needed(target, scope, ctx);
    let mut results = scope.get(target).to_vec();
    if results.is_empty() {
        // A subject with no type yet takes the one the tag promises, the
        // way an `instanceof` gives an untyped variable its class.  An
        // exclusion has nothing to rule out.
        if should_exclude {
            return;
        }
        if narrowing::scalar_assert_guard_kind(asserted_type).is_some() {
            results.push(ResolvedType::from_type_string(asserted_type.clone()));
        } else {
            let var_ctx = build_var_ctx(target, ctx, scope_resolver);
            narrowing::include_instance_of(asserted_type, false, &var_ctx, &mut results);
        }
        scope.set(target, results);
        return;
    }

    // Only a check that names no class proves nothing survived it.  A
    // class exclusion ignores type arguments, so ruling out
    // `ReflectionClass<Picture>` empties a `ReflectionClass<object>` the
    // check never contradicted, and a class inclusion comes back empty
    // when the loader cannot find the class.
    let mut exhaustible = false;
    if let Some(kind) = narrowing::scalar_assert_guard_kind(asserted_type) {
        exhaustible = true;
        if should_exclude {
            narrowing::apply_type_guard_exclusion(kind, &mut results, Some(ctx.class_loader));
        } else {
            narrowing::apply_type_guard_inclusion(kind, &mut results, Some(ctx.class_loader));
        }
    } else if should_exclude && matches!(asserted_type.kind(), TypeKind::Union(_)) {
        // Ruling out a union rules out every member, so each one narrows on
        // its own.  That is what lets `!=null|''` (Laravel's `filled()`)
        // strip the null: the whole union names no class, so handing it to
        // the class machinery below resolved nothing and narrowed nothing.
        //
        // Only exclusion decomposes this way.  Narrowing *to* each member in
        // turn would leave the subject as the last member alone rather than
        // the union, so an included union stays whole.
        let var_ctx = build_var_ctx(target, ctx, scope_resolver);
        for member in asserted_type.union_members() {
            if let Some(kind) = narrowing::scalar_assert_guard_kind(member) {
                narrowing::apply_type_guard_exclusion(kind, &mut results, Some(ctx.class_loader));
            } else {
                ResolvedType::apply_narrowing(&mut results, |classes| {
                    narrowing::apply_instanceof_exclusion(member, &var_ctx, classes)
                });
            }
        }
    } else {
        let var_ctx = build_var_ctx(target, ctx, scope_resolver);
        if should_exclude {
            ResolvedType::apply_narrowing(&mut results, |classes| {
                narrowing::apply_instanceof_exclusion(asserted_type, &var_ctx, classes)
            });
        } else {
            ResolvedType::apply_narrowing(&mut results, |classes| {
                narrowing::apply_instanceof_inclusion(asserted_type, false, &var_ctx, classes)
            });
            narrow_type_arguments(asserted_type, &mut results, ctx);
        }
    }

    if !results.is_empty() {
        scope.set(target, results);
    } else if exhaustible {
        mark_exhausted(target, scope);
    }
}

/// Give entries of the asserted class the type arguments the assertion
/// names, where those are narrower than the entry's own.
///
/// A class-level check keeps an entry that is already the asserted class,
/// so `ReflectionClass<object>` stayed as it was under an assertion that
/// it is a `ReflectionClass<Picture>`.  The arguments are the part the
/// assertion adds, and an argument it names that does not fit inside the
/// entry's leaves the entry alone.
fn narrow_type_arguments(
    asserted_type: &PhpType,
    results: &mut [ResolvedType],
    ctx: &ForwardWalkCtx<'_>,
) {
    let TypeKind::Generic(asserted) = asserted_type.kind() else {
        return;
    };
    let mut replacement: Option<Vec<ResolvedType>> = None;
    for rt in results.iter_mut() {
        let Some(class) = rt.class_info.as_ref() else {
            continue;
        };
        if !class
            .fqn()
            .eq_ignore_ascii_case(asserted.name.trim_start_matches('\\'))
        {
            continue;
        }
        let TypeKind::Generic(current) = rt.type_string.kind() else {
            continue;
        };
        let narrower = current.args.len() == asserted.args.len()
            && asserted
                .args
                .iter()
                .zip(&current.args)
                .all(|(a, c)| c.is_mixed() || c.is_object() || a.is_subtype_of(c))
            && asserted.args != current.args;
        if !narrower {
            continue;
        }
        let replaced =
            replacement.get_or_insert_with(|| ctx.resolved_types_for(asserted_type.clone()));
        if let Some(first) = replaced.first() {
            *rt = first.clone();
        }
    }
}
