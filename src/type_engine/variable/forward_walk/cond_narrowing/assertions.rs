use super::*;

/// Extract the `(base subject key, key name, negated)` of an
/// `array_key_exists('k', $arr)` check, unwrapping parentheses and a
/// leading `!`.
pub(super) fn array_key_exists_target(expr: &Expression<'_>) -> Option<(String, String, bool)> {
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
            let key_name = narrowing::string_literal_value(narrowing::argument_value(args[0]))?;
            let base_key = narrowing::expr_to_subject_key(narrowing::argument_value(args[1]))?;
            Some((base_key, key_name, false))
        }
        _ => None,
    }
}

/// Mark one key of an array shape as present, leaving its value type
/// alone.
///
/// The sibling [`strip_null_from_array_shape_key`] also drops `null` from
/// the value, which is what `isset()` proves.  `array_key_exists` proves
/// only presence, so a shape entry declared `?T` stays `?T`.
pub(super) fn mark_array_shape_key_present(base_var: &str, key_name: &str, scope: &mut ScopeState) {
    let types = scope.get(base_var).to_vec();
    if types.is_empty() {
        return;
    }
    let narrowed: Vec<ResolvedType> = types
        .into_iter()
        .map(|mut rt| {
            rt.type_string = mark_shape_key_present(&rt.type_string, key_name);
            rt
        })
        .collect();
    scope.set(base_var, narrowed);
}

/// Recursively clear the `optional` flag on one key of an array shape.
fn mark_shape_key_present(ty: &crate::php_type::PhpType, key: &str) -> crate::php_type::PhpType {
    use crate::php_type::{PhpType, ShapeEntry, TypeKind};
    match ty.kind() {
        TypeKind::ArrayShape(entries) => {
            let new_entries: Vec<ShapeEntry> = entries
                .iter()
                .map(|e| {
                    if e.key.as_deref() == Some(key) {
                        ShapeEntry {
                            key: e.key.clone(),
                            value_type: e.value_type.clone(),
                            optional: false,
                        }
                    } else {
                        e.clone()
                    }
                })
                .collect();
            PhpType::array_shape(new_entries)
        }
        TypeKind::Nullable(inner) => PhpType::nullable(mark_shape_key_present(inner, key)),
        TypeKind::Union(members) => PhpType::union(
            members
                .iter()
                .map(|m| mark_shape_key_present(m, key))
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
        return;
    }

    if let Some(kind) = narrowing::scalar_assert_guard_kind(asserted_type) {
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
        }
    }

    if !results.is_empty() {
        scope.set(target, results);
    }
}
