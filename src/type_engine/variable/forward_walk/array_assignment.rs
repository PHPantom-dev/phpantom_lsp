//! Destructuring assignments and writes into an array variable.

use super::*;

use mago_span::HasSpan;

use crate::atom::bytes_to_str;
use crate::php_type::PhpType;
use crate::type_engine::types::narrowing;
use crate::types::ResolvedType;

/// Process array destructuring assignments.
///
/// Resolves the RHS type once, then walks the LHS pattern to assign
/// types to each destructured variable.  Handles nested patterns like
/// `[$a, [$b, $c]] = $nested` by recursing into inner array/list
/// expressions.
pub(crate) fn process_destructuring_assignment<'b>(
    assignment: &'b Assignment<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let scope_resolver = scope.snapshot_resolver();

    // Build a temporary VarResolutionCtx just to resolve the RHS type.
    // The var_name doesn't matter here since we're resolving the RHS
    // expression, not looking up a specific variable.
    let dummy_name = String::from("$__destructuring_rhs");
    let var_ctx = ctx.var_ctx_for_with_scope(
        &dummy_name,
        assignment.span().start.offset,
        &scope_resolver,
        Some(scope.proofs()),
    );

    // Try inline @var docblock first, then fall back to RHS expression.
    let stmt_offset = assignment.span().start.offset as usize;
    let raw_type: Option<PhpType> =
        crate::docblock::find_inline_var_docblock(ctx.content, stmt_offset)
            .map(|(vt, _)| crate::util::resolve_php_type_names(&vt, ctx.class_loader))
            .or_else(|| {
                super::super::foreach_resolution::resolve_expression_type(assignment.rhs, &var_ctx)
            });

    // Expand type aliases before shape/generic extraction.
    let raw_type = raw_type.map(|rt| {
        crate::type_engine::type_resolution::resolve_type_alias_typed(
            &rt,
            &ctx.current_class.name,
            ctx.all_classes,
            ctx.class_loader,
        )
        .unwrap_or(rt)
    });

    if let Some(ref rhs_type) = raw_type {
        bind_destructured_pattern(assignment.lhs, rhs_type, scope, ctx);
    }

    // Ensure every destructured variable is present in scope even when the
    // RHS type (or an individual element's type) could not be resolved.  A
    // plain assignment from an unresolvable RHS records the variable with an
    // empty type list via `set_empty`, which lets later assert narrowing seed
    // a type for it.  Without this, list-destructuring from an unresolvable
    // RHS leaves the variables absent from scope entirely, so the assert
    // narrowing loop never visits them and the asserted type is dropped.
    seed_destructured_vars_empty(assignment.lhs, scope);
}

/// Walk a destructuring LHS pattern and record every direct variable in
/// scope with an empty type list, unless it is already present.  Used so
/// that variables destructured from an unresolvable RHS still participate
/// in later narrowing (`set_empty` leaves any already-bound type intact).
pub(crate) fn seed_destructured_vars_empty<'b>(lhs: &'b Expression<'b>, scope: &mut ScopeState) {
    let elements: Vec<&ArrayElement<'b>> = match lhs {
        Expression::Array(arr) => arr.elements.iter().collect(),
        Expression::List(list) => list.elements.iter().collect(),
        _ => return,
    };

    for elem in elements {
        let value_expr = match elem {
            ArrayElement::KeyValue(kv) => kv.value,
            ArrayElement::Value(val) => val.value,
            _ => continue,
        };
        match value_expr {
            Expression::Variable(Variable::Direct(dv)) => {
                scope.set_empty(bytes_to_str(dv.name));
            }
            Expression::Array(_) | Expression::List(_) => {
                seed_destructured_vars_empty(value_expr, scope);
            }
            _ => {}
        }
    }
}

/// Recursively bind types from a destructuring LHS pattern against a
/// resolved RHS type.  For each variable in the pattern, extracts the
/// corresponding type from the RHS type (via shape key or positional
/// index) and sets it in scope.  For nested array/list sub-patterns,
/// recurses with the extracted element type.
pub(crate) fn bind_destructured_pattern<'b>(
    lhs: &'b Expression<'b>,
    rhs_type: &PhpType,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let elements: Vec<&ArrayElement<'b>> = match lhs {
        Expression::Array(arr) => arr.elements.iter().collect(),
        Expression::List(list) => list.elements.iter().collect(),
        _ => return,
    };

    let mut positional_index: usize = 0;
    for elem in elements {
        let (value_expr, shape_key) = match elem {
            ArrayElement::KeyValue(kv) => {
                let key = extract_foreach_destr_key(kv.key);
                (kv.value, key)
            }
            ArrayElement::Value(val) => {
                let key = Some(positional_index.to_string());
                positional_index += 1;
                (val.value, key)
            }
            // A hole (`[, $second]`) names nothing but still consumes the
            // position, so every later element shifts along with it.
            ArrayElement::Missing(_) => {
                positional_index += 1;
                continue;
            }
            _ => continue,
        };

        // Determine the type for this element position.
        let elem_type: Option<PhpType> = shape_key
            .as_ref()
            .and_then(|k| rhs_type.shape_value_type(k).cloned())
            .or_else(|| rhs_type.extract_value_type(false).cloned());

        match value_expr {
            // Direct variable: bind the type.
            Expression::Variable(Variable::Direct(dv)) => {
                if let Some(ref vt) = elem_type {
                    scope.set(bytes_to_str(dv.name), ctx.resolved_types_for(vt.clone()));
                }
            }
            // Nested pattern: recurse with the extracted element type.
            Expression::Array(_) | Expression::List(_) => {
                if let Some(ref vt) = elem_type {
                    bind_destructured_pattern(value_expr, vt, scope, ctx);
                }
            }
            _ => {}
        }
    }
}

/// Process array key assignment: `$var['key'] = expr;`
pub(crate) fn process_array_key_assignment<'b>(
    array_access: &'b ArrayAccess<'b>,
    assignment: &'b Assignment<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
    process_array_key_write(array_access, rhs_types, scope, ctx);
}

/// Store `value_types` at the element `$var['key']…` names, the write both a
/// plain `=` and a compound assignment (`+=`, `.=`, …) perform.
pub(crate) fn process_array_key_write<'b>(
    array_access: &'b ArrayAccess<'b>,
    value_types: Vec<ResolvedType>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    if let Some((base_name, key_chain)) =
        super::super::array_shape_writes::extract_nested_array_access_chain(array_access)
    {
        apply_array_write(&base_name, &key_chain, false, value_types, scope, ctx);
    }
}

/// Process array append: `$var[] = expr;` and `$var['a'][$i][] = expr;`
pub(crate) fn process_array_append<'b>(
    array_append: &'b ArrayAppend<'b>,
    assignment: &'b Assignment<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    match array_append.array {
        Expression::Variable(Variable::Direct(dv)) => {
            let base_name = bytes_to_str(dv.name).to_string();
            let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
            apply_array_write(&base_name, &[], true, rhs_types, scope, ctx);
        }
        // `$var['a'][$i][] = …` — the append lands on the innermost level
        // of an array-access chain rather than on the variable itself.
        Expression::ArrayAccess(inner) => {
            if let Some((base_name, key_chain)) =
                super::super::array_shape_writes::extract_nested_array_access_chain(inner)
            {
                let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
                apply_array_write(&base_name, &key_chain, true, rhs_types, scope, ctx);
            }
        }
        _ => {}
    }
}

/// Merge the value of an element write into the base variable's type.
///
/// `key_chain` holds the array-access keys from outermost to innermost;
/// `append` marks a trailing `[]` past the last key. Literal-string keys
/// become shape entries, dynamic keys become generic `array<K, V>`
/// levels, and missing intermediate levels auto-vivify.
fn apply_array_write<'b>(
    base_name: &str,
    key_chain: &[&Expression<'b>],
    append: bool,
    rhs_types: Vec<ResolvedType>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // An append with no inferable element type leaves the variable alone
    // rather than widening a tracked `list<T>` with `mixed`. A keyed write
    // records `mixed` so the key itself still shows up in the shape.
    if append && rhs_types.is_empty() {
        return;
    }
    let value_php_type = if rhs_types.is_empty() {
        PhpType::mixed()
    } else {
        ResolvedType::types_joined(&rhs_types)
    };
    // A property is written through with its declared type as the
    // starting point.  One with no type to start from (undeclared, or
    // reached through `__get`) is left to its declaration rather than
    // pinned to what one write put into it.
    if narrowing::is_member_path_key(base_name) {
        seed_synthetic_key_if_needed(base_name, scope, ctx);
        if scope.get(base_name).is_empty() {
            return;
        }
    }
    let base_type = scope
        .get(base_name)
        .last()
        .map(|rt| rt.type_string.clone())
        .unwrap_or_else(PhpType::array);

    // If the base variable is an object (e.g. SplObjectStorage, ArrayAccess),
    // array-access syntax invokes offsetSet, not actual array mutation.
    // Preserve the original object type instead of overwriting it with an array shape.
    // A read of the same offset is still taken to return what was written,
    // the same assumption an `===` check on that read narrows under.
    if base_type.is_object_like() && !base_type.is_array_like() {
        if !append {
            overwrite_written_offset(base_name, key_chain, rhs_types, scope);
        }
        return;
    }

    // If the base variable is a string, bracket-indexed assignment
    // (`$str[0] = 'z'`) modifies the string in-place — the variable
    // remains a string, it does NOT become an array.
    if base_type.is_string_subtype() {
        scope.set(
            base_name,
            vec![ResolvedType::from_type_string(PhpType::string())],
        );
        return;
    }

    let mut write_keys: Vec<super::super::array_shape_writes::ArrayWriteKey> = key_chain
        .iter()
        .map(
            |idx| match super::super::array_shape_writes::extract_array_key_for_shape(idx) {
                Some(key) => super::super::array_shape_writes::ArrayWriteKey::Shape(key),
                None => {
                    let index_types = resolve_rhs_with_scope(idx, scope, ctx);
                    super::super::array_shape_writes::ArrayWriteKey::Keyed {
                        key_type: super::super::array_shape_writes::infer_array_key_type(
                            idx,
                            &index_types,
                        ),
                        slot: super::super::array_shape_writes::extract_array_write_index(idx),
                    }
                }
            },
        )
        .collect();
    if append {
        write_keys.push(super::super::array_shape_writes::ArrayWriteKey::Append);
    }

    let merged = super::super::array_shape_writes::merge_nested_array_write(
        &base_type,
        &write_keys,
        &value_php_type,
        ctx.in_loop,
    );
    scope.set(base_name, vec![ResolvedType::from_type_string(merged)]);

    if !append {
        overwrite_written_offset(base_name, key_chain, rhs_types, scope);
    }
}

/// Record the value a keyed write stored as the type of the offset it
/// targets.
///
/// A keyed write is authoritative for the element it targets, so it
/// must overwrite any synthetic scope key (`$tmp[$key]`, `$a["x"]`)
/// narrowing left behind for that same subject. Left stale, a
/// narrowed-to-null entry from an `isset`/`!isset` guard survives past
/// the write that just proved the key present, and resurfaces when
/// this branch's scope merges back with one where the key was proven
/// present a different way — see `apply_null_narrowing_truthy`'s
/// `extract_not_isset_vars` arm, which narrows the synthetic key to
/// null before the guarded body ever runs. An append (`$var[] = …`)
/// has no addressable key to overwrite, so callers skip it.
fn overwrite_written_offset(
    base_name: &str,
    key_chain: &[&Expression<'_>],
    rhs_types: Vec<ResolvedType>,
    scope: &mut ScopeState,
) {
    if let Some(key) = array_write_synthetic_key(base_name, key_chain) {
        // `rhs_types`, not a flattened `PhpType`: the shape merge's plain
        // type string drops the `class_info` a member-access completion on
        // the synthetic key (`$result["user"]->`) needs.
        let synthetic_types = if rhs_types.is_empty() {
            vec![ResolvedType::from_type_string(PhpType::mixed())]
        } else {
            rhs_types
        };
        scope.set(&key, synthetic_types);
    }
}

/// Render the synthetic scope key a keyed write targets, matching the key
/// text [`narrowing::expr_to_subject_key`] builds for a read of the same
/// subject (`$tmp[$key]`, `$a["x"][$i]`), so a write can find and
/// overwrite whatever narrowing recorded under that key.
fn array_write_synthetic_key(base_name: &str, key_chain: &[&Expression<'_>]) -> Option<String> {
    let mut key = base_name.to_string();
    for index in key_chain {
        if let Some(literal) = narrowing::array_index_literal_key(index) {
            key.push_str(&format!("[\"{literal}\"]"));
        } else {
            let index_key = narrowing::array_index_key(index)?;
            // `expr_to_subject_key`'s `array_access_subject_key` only
            // renders a non-literal index that reads a variable
            // (`contains('$')`); an index that writes, concatenates, or
            // compares is not the same subject a read of it renders, so
            // there is no synthetic key to find.
            if !index_key.contains('$') {
                return None;
            }
            key.push_str(&format!("[{index_key}]"));
        }
    }
    Some(key)
}
