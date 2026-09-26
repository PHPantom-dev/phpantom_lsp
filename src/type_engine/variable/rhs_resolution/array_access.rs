/// Array access (`$arr[0]`, `$arr['key']`) resolution: extracts the
/// generic element type or array-shape value type from the base array's
/// annotation or assignment, including chained bracket access.
use std::collections::HashMap;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::Backend;
use crate::atom::{atom, bytes_to_str, literal_bytes_to_str};
use crate::docblock;
use crate::php_type::{LiteralValue, PhpType, TypeKind};
use crate::types::ResolvedType;

use crate::type_engine::resolver::VarResolutionCtx;

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
    let mut segments: Vec<(ArrayBracketSegment, &Expression<'_>)> = Vec::new();
    let mut current_expr: &Expression<'_> = array_access.array;

    // Classify the outermost (current) index first.
    segments.push((classify_array_index(array_access.index), array_access.index));

    // Walk inward through nested ArrayAccess nodes.
    while let Expression::ArrayAccess(inner) = current_expr {
        segments.push((classify_array_index(inner.index), inner.index));
        current_expr = inner.array;
    }

    // Segments were collected innermost-last; reverse to left-to-right order.
    segments.reverse();

    let access_offset = expr.span().start.offset as usize;

    // Resolve the base expression's type.
    // For bare variables (`$var['key']`), use docblock or assignment scanning.
    // For property chains (`$obj->prop['key']`), resolve the property type.
    let raw_type: Option<PhpType> = if let Expression::Variable(Variable::Direct(base_dv)) =
        current_expr
    {
        let base_var = bytes_to_str(base_dv.name).to_string();
        // The forward walker's scope comes before the docblock scan.  It
        // already incorporates @var annotations AND applies
        // condition-based narrowing (e.g. null stripping on array
        // shape keys through guard clauses, or the `string` arm an
        // enclosing `is_array()` ruled out), which the raw docblock
        // discards.  The two channels the walker publishes it through are
        // consulted in turn: the resolver it threads down its own call
        // tree, then the snapshot cache it leaves for the diagnostic
        // consumers it does not drive.
        let scope_result = if ctx.scope_var_resolver.is_some() {
            let resolved = resolve_var_types(&base_var, ctx, access_offset as u32);
            if resolved.is_empty() {
                None
            } else {
                Some(ResolvedType::types_joined(&resolved))
            }
        } else {
            let resolved = crate::type_engine::variable::resolution::walker_scope_types(
                &base_var,
                access_offset as u32,
                None,
            );
            if resolved.is_empty() {
                None
            } else {
                Some(ResolvedType::types_joined(&resolved))
            }
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
    if let Some(expanded) = crate::type_engine::type_resolution::resolve_type_alias_typed(
        &current,
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    ) {
        current = expanded;
    }

    // Walk each bracket segment, narrowing the type at each step.
    for (seg, index) in &segments {
        let literal_seg =
            if matches!(seg, ArrayBracketSegment::ElementAccess) && has_shape_member(&current) {
                literal_variable_index(index, ctx)
            } else {
                None
            };
        let seg = literal_seg.as_ref().unwrap_or(seg);
        let Some(element) = index_segment(&current, seg, ctx) else {
            return vec![];
        };
        current = element;

        // After each segment, the resulting type might itself be an
        // alias (e.g. a shape value defined as another alias).
        if let Some(expanded) = crate::type_engine::type_resolution::resolve_type_alias_typed(
            &current,
            &ctx.current_class.name,
            ctx.all_classes,
            ctx.class_loader,
        ) {
            current = expanded;
        }
    }

    let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
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

/// Whether `ty` is, or has a union member that is, an array shape: the
/// only kind of array whose offset reads depend on which key is read.
fn has_shape_member(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::ArrayShape(_) => true,
        TypeKind::Nullable(inner) => has_shape_member(inner),
        TypeKind::Union(members) => members.iter().any(has_shape_member),
        _ => false,
    }
}

/// The literal key a variable index holds, as the segment a literal
/// written in its place would classify to.
///
/// `$k = 'classmap'; $data[$k]` reads the same entry `$data['classmap']`
/// does, so a variable whose type is a single string or int literal
/// addresses that one shape entry rather than any of them.
fn literal_variable_index(
    index: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<ArrayBracketSegment> {
    let Expression::Variable(Variable::Direct(_)) = index else {
        return None;
    };
    let resolved = resolve_rhs_expression(index, ctx);
    let [only] = resolved.as_slice() else {
        return None;
    };
    let literal = only.type_string.as_literal()?;
    match literal {
        LiteralValue::String(_) => Some(ArrayBracketSegment::StringKey(
            literal.string_content()?.into_owned(),
        )),
        LiteralValue::Int(_) => Some(ArrayBracketSegment::IntKey(
            literal.parse_i64()?.to_string(),
        )),
        LiteralValue::Float(_) => None,
    }
}

/// Read one bracket segment off `base` and return the value type it
/// yields, or `None` when the offset read cannot be typed at all.
///
/// A union is indexed member-wise and the results joined, because each
/// member answers the offset read differently: an array yields its
/// element or shape value type, a string yields `string`, and `null` (or
/// any other non-array scalar) yields `null`, which is what PHP itself
/// produces for an offset read on such a value. Indexing the union as a
/// whole instead would find no element type and resolve to nothing, and a
/// caller unioning that away turns a value it knows nothing about into a
/// type narrower than the truth.
///
/// For the same reason a member the walk cannot type widens the join to
/// `mixed` rather than dropping out of it.
///
/// The one member that defers instead of widening is an array that names
/// no element type (`array`, `iterable`, an unparameterised `list`).
/// Indexing it can only answer `mixed`, and on its own that is the honest
/// answer. Beside a sibling array that *does* name an element type it is
/// the same alternative spelled vaguer, though, so letting it through
/// would drown the sibling's answer out of the join.
fn index_segment(
    base: &PhpType,
    seg: &ArrayBracketSegment,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    match base.kind() {
        TypeKind::Union(members) => {
            let indexed: Vec<(bool, PhpType)> = members
                .iter()
                .map(|member| {
                    let value = index_segment(member, seg, ctx).unwrap_or_else(PhpType::mixed);
                    (member.is_array_like(), value)
                })
                .collect();
            let specific_array = indexed
                .iter()
                .any(|(array_like, value)| *array_like && !value.is_mixed());
            let joined: Vec<PhpType> = indexed
                .into_iter()
                .filter(|(array_like, value)| !(specific_array && *array_like && value.is_mixed()))
                .map(|(_, value)| value)
                .collect();
            return Some(PhpType::join_runtime_value_types(joined));
        }
        // `?T` is `T|null`, and the `null` half yields `null`.
        TypeKind::Nullable(inner) => {
            let indexed = index_segment(inner, seg, ctx).unwrap_or_else(PhpType::mixed);
            return Some(PhpType::join_runtime_value_types(vec![
                indexed,
                PhpType::null(),
            ]));
        }
        _ => {}
    }

    // Try pure-type extraction first (array shapes, generics).
    let extracted = match seg {
        // An optional entry (`array{0?: string}`, which is what a branch
        // merge leaves where only one path wrote the key) may not be there,
        // and an offset read of a key an array lacks is `null` — the same
        // answer the empty-shape case below gives for a read that is a
        // guaranteed miss.
        ArrayBracketSegment::StringKey(key) | ArrayBracketSegment::IntKey(key) => base
            .shape_entry(key)
            .map(|entry| {
                if entry.optional {
                    entry.value_type.clone().or_null()
                } else {
                    entry.value_type.clone()
                }
            })
            .or_else(|| base.extract_element_type().cloned()),
        // A dynamic (non-literal) key can address any entry, so a shape
        // yields the union of its value types (via
        // `iterable_element_type`); generic arrays yield their value type
        // as before.
        ArrayBracketSegment::ElementAccess => base.iterable_element_type(),
    };
    if let Some(element) = extracted {
        return Some(element);
    }

    // An empty shape has no entry any key could address, so the read is a
    // guaranteed miss and yields `null`, exactly like an offset read on a
    // non-array.  Widening to `mixed` instead loses the answer to
    // `$a[$k] ?? 0` on the `[]` a loop is about to fill, which then makes
    // every accumulated `+` an `int|float`.
    if base.is_empty_array_shape() {
        return Some(PhpType::null());
    }

    // Fallback: when the base type is a plain class name (e.g.
    // `OpeningHours`), resolve the class and check its iterable generics
    // (`@extends`, `@implements`) for the element type. This handles
    // `$obj->prop['key']` where `prop` is a collection class like
    // `OpeningHours extends DataCollection<string, Day>`.
    let class_element = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        base,
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
        crate::type_engine::variable::foreach_resolution::extract_iterable_element_type_from_class(
            &merged,
            ctx.class_loader,
        )
    });
    if let Some(element) = class_element {
        return Some(element);
    }

    if base.is_bare_array() || base.is_mixed() {
        // Bare `array` and `mixed` have unknown element types; accessing
        // any key yields `mixed`.
        return Some(PhpType::mixed());
    }
    // A string offset is itself a one-character string.
    if base.is_string_subtype() {
        return Some(PhpType::string());
    }
    // Reading an offset off anything else that is not an object yields
    // `null` at runtime (with a warning), rather than being untypeable.
    if base.is_null() || base.is_int_subtype() || base.is_float_subtype() || base.is_bool() {
        return Some(PhpType::null());
    }
    None
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
            let key = match s.value {
                // A value that is not UTF-8 (`$a["\x8b"]`) cannot be
                // written as a shape key, so the access stays opaque.
                Some(bytes) => match literal_bytes_to_str(bytes) {
                    Some(key) => key.to_string(),
                    None => return ArrayBracketSegment::ElementAccess,
                },
                None => {
                    let raw_str = bytes_to_str(s.raw);
                    crate::text_scan::unquote_php_string(raw_str)
                        .unwrap_or(raw_str)
                        .to_string()
                }
            };
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
///
/// `never` is the identity for the union: an empty array literal binds
/// its element template to `never`, and `never|int` is just `int`.
pub(crate) fn insert_or_union(subs: &mut HashMap<String, PhpType>, key: String, value: PhpType) {
    use std::collections::hash_map::Entry;
    if value.is_never() && subs.contains_key(&key) {
        return;
    }
    match subs.entry(key) {
        Entry::Vacant(e) => {
            e.insert(value);
        }
        Entry::Occupied(mut e) => {
            let existing = e.get().clone();
            if existing == value {
                return;
            }
            if existing.is_never() {
                e.insert(value);
                return;
            }
            let mut parts = match existing.kind() {
                TypeKind::Union(parts) => parts.to_vec(),
                _ => vec![existing],
            };
            match value.kind() {
                TypeKind::Union(new_parts) => {
                    for p in new_parts {
                        if !parts.contains(p) {
                            parts.push(p.clone());
                        }
                    }
                }
                _ => {
                    if !parts.contains(&value) {
                        parts.push(value);
                    }
                }
            }
            // A literal from one site and its base type from another bind
            // the base type: `array_reduce(..., 0)` with an `int` callback
            // carries an `int`, not a `0|int`.
            e.insert(PhpType::join_runtime_value_types(parts));
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
/// - `X::class` resolves to `PhpType::named("X")` — bound directly to the
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
    ctx: &crate::type_engine::resolver::ResolutionCtx<'_>,
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
        return Some(PhpType::named(atom(&fqn)));
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
    match ty.kind() {
        TypeKind::ClassString(Some(inner)) => Some(inner.clone()),
        TypeKind::ClassString(None) => Some(PhpType::named(atom("object"))),
        // A class name binds directly; a scalar keyword (`string`, `int`,
        // …) is not a class, so it must not bind `T` — otherwise a plain
        // `string` argument would produce `class-string<string>`.
        TypeKind::Named(name) => {
            if crate::php_type::is_builtin_non_class_type(name) {
                None
            } else {
                Some(PhpType::named(*name))
            }
        }
        // A union of class-strings binds `T` to the union of the inner
        // classes.  Every member must yield a class; if any member is not
        // a class-string the whole binding is abandoned so `T` falls back
        // to its declared bound.
        TypeKind::Union(members) => {
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
                _ => Some(PhpType::union(parts)),
            }
        }
        _ => None,
    }
}
