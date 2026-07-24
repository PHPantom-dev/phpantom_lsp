/// Array access (`$arr[0]`, `$arr['key']`) resolution: extracts the
/// generic element type or array-shape value type from the base array's
/// annotation or assignment, including chained bracket access.
use std::collections::HashMap;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::docblock;
use crate::php_type::PhpType;
use crate::types::ResolvedType;

use crate::completion::resolver::VarResolutionCtx;

use super::{resolve_rhs_expression, resolve_var_types};

/// Resolve `$arr[0]` / `$arr[$key]` by extracting the generic element
/// type from the base array's annotation or assignment.
pub(super) fn resolve_rhs_array_access<'b>(
    array_access: &ArrayAccess<'b>,
    expr: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    // Collect bracket segments and find the innermost base variable by
    // walking through nested ArrayAccess nodes.  This handles both
    // single access (`$result['data']`) and chained access
    // (`$result['items'][0]`).
    let mut segments: Vec<ArrayBracketSegment> = Vec::new();
    let mut current_expr: &Expression<'_> = array_access.array;

    // Classify the outermost (current) index first.
    segments.push(classify_array_index(array_access.index));

    // Walk inward through nested ArrayAccess nodes.
    while let Expression::ArrayAccess(inner) = current_expr {
        segments.push(classify_array_index(inner.index));
        current_expr = inner.array;
    }

    // Segments were collected innermost-last; reverse to left-to-right order.
    segments.reverse();

    let access_offset = expr.span().start.offset as usize;

    // Resolve the base expression's raw type string.
    // For bare variables (`$var['key']`), use docblock or assignment scanning.
    // For property chains (`$obj->prop['key']`), resolve the property type.
    let raw_type: Option<PhpType> = if let Expression::Variable(Variable::Direct(base_dv)) =
        current_expr
    {
        let base_var = bytes_to_str(base_dv.name).to_string();
        // When a scope_var_resolver is available (forward walk),
        // prefer it over the docblock scan.  The forward walk
        // already incorporates @var annotations AND applies
        // condition-based narrowing (e.g. null stripping on array
        // shape keys through guard clauses).  Falling back to the
        // raw docblock would discard that narrowing.
        let scope_result = if ctx.scope_var_resolver.is_some() {
            let resolved = resolve_var_types(&base_var, ctx, access_offset as u32);
            if resolved.is_empty() {
                None
            } else {
                Some(ResolvedType::types_joined(&resolved))
            }
        } else {
            None
        };
        scope_result
            .or_else(|| {
                docblock::find_iterable_raw_type_in_source(ctx.content, access_offset, &base_var)
                    .map(|t| crate::util::resolve_php_type_names(&t, ctx.class_loader))
            })
            .or_else(|| {
                let resolved = resolve_var_types(&base_var, ctx, access_offset as u32);
                if resolved.is_empty() {
                    None
                } else {
                    Some(ResolvedType::types_joined(&resolved))
                }
            })
    } else {
        // Non-variable base (e.g. property access `$obj->prop['key']`,
        // method call `$obj->getItems()['key']`, etc.).
        // Resolve the base expression to get its type.
        let base_resolved = resolve_rhs_expression(current_expr, ctx);
        if base_resolved.is_empty() {
            None
        } else {
            Some(ResolvedType::types_joined(&base_resolved))
        }
    };

    let Some(mut current) = raw_type else {
        // The base expression's type is unknown (e.g. an untyped parameter
        // or an unresolvable call). Accessing an offset on an unknown value
        // yields `mixed`, matching PHPStan's treatment of `mixed[$k]`. This
        // is the honest answer rather than an empty (untyped) result, and it
        // lets the `??` handler union it without a special case.
        return vec![ResolvedType::from_type_string(PhpType::mixed())];
    };

    // Expand type aliases so that shape/generic extraction can see the
    // underlying type (e.g. a `@phpstan-type` alias).
    if let Some(expanded) = crate::completion::type_resolution::resolve_type_alias_typed(
        &current,
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    ) {
        current = expanded;
    }

    // Walk each bracket segment, narrowing the type at each step.
    for seg in &segments {
        // Try pure-type extraction first (array shapes, generics).
        let extracted = match seg {
            ArrayBracketSegment::StringKey(key) | ArrayBracketSegment::IntKey(key) => current
                .shape_value_type(key)
                .cloned()
                .or_else(|| current.extract_element_type().cloned()),
            // A dynamic (non-literal) key can address any entry, so a
            // shape yields the union of its value types (via
            // `iterable_element_type`); generic arrays yield their
            // value type as before.
            ArrayBracketSegment::ElementAccess => current.iterable_element_type(),
        };

        if let Some(element) = extracted {
            current = element;
        } else {
            // Fallback: when the current type is a plain class name (e.g.
            // `OpeningHours`), resolve the class and check its iterable
            // generics (`@extends`, `@implements`) for the element type.
            // This handles `$obj->prop['key']` where `prop` is a collection
            // class like `OpeningHours extends DataCollection<string, Day>`.
            let class_element = crate::completion::type_resolution::type_hint_to_classes_typed(
                &current,
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            )
            .into_iter()
            .find_map(|cls| {
                let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                    &cls,
                    ctx.class_loader,
                    ctx.resolved_class_cache,
                );
                crate::completion::variable::foreach_resolution::extract_iterable_element_type_from_class(
                    &merged,
                    ctx.class_loader,
                )
            });

            if let Some(element) = class_element {
                current = element;
            } else if current.is_bare_array() || current.is_mixed() {
                // Bare `array` and `mixed` have unknown element types;
                // accessing any key yields `mixed`.
                current = PhpType::mixed();
            } else {
                return vec![];
            }
        }

        // After each segment, the resulting type might itself be an
        // alias (e.g. a shape value defined as another alias).
        if let Some(expanded) = crate::completion::type_resolution::resolve_type_alias_typed(
            &current,
            &ctx.current_class.name,
            ctx.all_classes,
            ctx.class_loader,
        ) {
            current = expanded;
        }
    }

    let classes = crate::completion::type_resolution::type_hint_to_classes_typed(
        &current,
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    );
    if classes.is_empty() {
        // No class matched (e.g. `list<Rule>`, `int`, `string`).
        // Return a type-string-only entry so the type information
        // is preserved for downstream consumers like foreach
        // element extraction.
        vec![ResolvedType::from_type_string(current)]
    } else {
        ResolvedType::from_classes_with_hint(classes, current)
    }
}

/// Classification of an array access index expression.
pub(super) enum ArrayBracketSegment {
    /// A string-key access, e.g. `['items']`.
    StringKey(String),
    /// An integer-literal index access, e.g. `[0]` or `[2]`. Carries the
    /// decimal string form so it can address positional shape entries
    /// (`array{Foo, Bar}`) as well as explicit numeric keys.
    IntKey(String),
    /// A variable or otherwise non-literal index access, e.g. `[$i]`.
    ElementAccess,
}

/// Classify an array index expression as a string key, integer-literal
/// index, or generic element access.
pub(super) fn classify_array_index(index: &Expression<'_>) -> ArrayBracketSegment {
    match index {
        Expression::Literal(Literal::String(s)) => {
            let key = s
                .value
                .map(|v| bytes_to_str(v).to_string())
                .unwrap_or_else(|| {
                    let raw_str = bytes_to_str(s.raw);
                    crate::text_scan::unquote_php_string(raw_str)
                        .unwrap_or(raw_str)
                        .to_string()
                });
            ArrayBracketSegment::StringKey(key)
        }
        // An integer literal index (`$pair[0]`) addresses either an explicit
        // numeric shape key or a positional tuple entry. Use the parsed value
        // so hex/octal/binary literals map to their decimal index form.
        Expression::Literal(Literal::Integer(i)) => match i.value {
            Some(value) => ArrayBracketSegment::IntKey(value.to_string()),
            None => ArrayBracketSegment::ElementAccess,
        },
        _ => ArrayBracketSegment::ElementAccess,
    }
}

/// Insert a template substitution, unioning with any existing entry.
/// When two arguments bind to the same `@template T`, the resolved type
/// is the union of all inferred argument types (e.g. `T` from `$a: int`
/// and `$b: float` becomes `int|float`).
pub(crate) fn insert_or_union(subs: &mut HashMap<String, PhpType>, key: String, value: PhpType) {
    use std::collections::hash_map::Entry;
    match subs.entry(key) {
        Entry::Vacant(e) => {
            e.insert(value);
        }
        Entry::Occupied(mut e) => {
            let existing = e.get().clone();
            if existing == value {
                return;
            }
            let mut parts = match existing {
                PhpType::Union(parts) => parts,
                other => vec![other],
            };
            match value {
                PhpType::Union(new_parts) => {
                    for p in new_parts {
                        if !parts.contains(&p) {
                            parts.push(p);
                        }
                    }
                }
                other => {
                    if !parts.contains(&other) {
                        parts.push(other);
                    }
                }
            }
            e.insert(if parts.len() == 1 {
                parts.into_iter().next().unwrap()
            } else {
                PhpType::Union(parts)
            });
        }
    }
}

/// Compute the type to bind a template parameter `T` to when it appears
/// inside a `class-string<T>` parameter hint, given the resolved type of
/// the call-site argument.  Returns `None` when the argument yields no
/// usable class, so the caller lets `T` fall back to its declared bound.
///
/// This mirrors PHPStan's `GenericClassStringType::inferTemplateTypes`:
///
/// - `X::class` resolves to `PhpType::Named("X")` — bound directly to the
///   class.
/// - A string literal naming a class (e.g. `'Iterator'`) binds to the
///   class it names, never to the literal's own `string` type — otherwise
///   `T` would become `string`, producing the absurd `class-string<string>`.
/// - `class-string<X>` unwraps to `X` so the substitution does not
///   double-wrap into `class-string<class-string<X>>`.
/// - A bare `class-string` (unknown inner class) binds to `object`, the
///   universal upper bound, so any class-string satisfies the parameter.
/// - Any other type (e.g. plain `string`) yields `None`; `T` then resolves
///   to its declared bound rather than the nonsensical `class-string<T>`.
pub(crate) fn class_string_inner_binding(
    arg_text: &str,
    ctx: &crate::completion::resolver::ResolutionCtx<'_>,
) -> Option<PhpType> {
    // A quoted string literal naming a class binds to that class.  This is
    // checked against the raw argument text because `resolve_arg_text_to_type`
    // collapses every string literal to the bare `string` type, discarding
    // the content that names the class.  The literal is unescaped first so a
    // source-level `'Foo\\Bar'` binds to the runtime class `Foo\Bar` rather
    // than the doubled-backslash spelling, which no class lookup would match.
    let trimmed = arg_text.trim();
    if let Some(unescaped) = crate::util::unescape_php_string_literal(trimmed) {
        let content = unescaped.trim();
        // Only treat the literal as a class name when its content is a
        // valid class identifier; otherwise it doesn't name a class and
        // must not bind `T`.
        if content.is_empty()
            || !content
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '\\')
        {
            return None;
        }
        let fqn = match (ctx.class_loader)(content) {
            Some(cls) => cls.fqn().to_string(),
            None => content.to_string(),
        };
        return Some(PhpType::Named(fqn));
    }

    class_string_inner_from_type(&Backend::resolve_arg_text_to_type(arg_text, ctx)?)
}

/// Unwrap the class layer bound to `T` in `class-string<T>` from an
/// already-resolved argument [`PhpType`].
///
/// A union of class-strings (e.g. from a `foreach` over a class-constant
/// array) binds `T` to the union of the inner classes, so each member is
/// checked against `T`'s bound individually and the concrete union is kept
/// for the return type rather than collapsing to the declared bound.
pub(super) fn class_string_inner_from_type(ty: &PhpType) -> Option<PhpType> {
    match ty {
        PhpType::ClassString(Some(inner)) => Some(inner.as_ref().clone()),
        PhpType::ClassString(None) => Some(PhpType::Named("object".to_string())),
        // A class name binds directly; a scalar keyword (`string`, `int`,
        // …) is not a class, so it must not bind `T` — otherwise a plain
        // `string` argument would produce `class-string<string>`.
        PhpType::Named(name) => {
            if crate::php_type::is_builtin_non_class_type(name) {
                None
            } else {
                Some(PhpType::Named(name.clone()))
            }
        }
        // A union of class-strings binds `T` to the union of the inner
        // classes.  Every member must yield a class; if any member is not
        // a class-string the whole binding is abandoned so `T` falls back
        // to its declared bound.
        PhpType::Union(members) => {
            let mut parts: Vec<PhpType> = Vec::with_capacity(members.len());
            for member in members {
                let inner = class_string_inner_from_type(member)?;
                if !parts.contains(&inner) {
                    parts.push(inner);
                }
            }
            match parts.len() {
                0 => None,
                1 => Some(parts.into_iter().next().unwrap()),
                _ => Some(PhpType::Union(parts)),
            }
        }
        _ => None,
    }
}
