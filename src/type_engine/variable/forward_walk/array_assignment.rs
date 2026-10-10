//! Destructuring assignments and writes into an array variable.

use super::*;

use mago_span::HasSpan;

use crate::atom::bytes_to_str;
use crate::php_type::{PhpType, TypeKind};
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

    match raw_type {
        Some(ref rhs_type) => bind_destructured_pattern(assignment.lhs, rhs_type, scope, ctx),
        None => forget_destructured_vars(assignment.lhs, scope),
    }
}

/// Record every direct variable in a destructuring LHS pattern as holding
/// an unknown value.  A destructuring writes each of its targets whatever
/// the RHS turns out to be, so one whose element could not be typed loses
/// the value it held before, the way a plain assignment from an
/// unresolvable RHS does.  The entry stays in scope, which lets later
/// assert narrowing seed a type for it.
fn forget_destructured_vars<'b>(lhs: &'b Expression<'b>, scope: &mut ScopeState) {
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
                scope.set_unknown(bytes_to_str(dv.name));
            }
            Expression::Array(_) | Expression::List(_) => {
                forget_destructured_vars(value_expr, scope);
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

        let elem_type = destructured_element_type(rhs_type, shape_key.as_deref());

        match (value_expr, elem_type) {
            (Expression::Variable(Variable::Direct(dv)), Some(vt)) => {
                scope.set(bytes_to_str(dv.name), ctx.resolved_types_for(vt));
            }
            (Expression::Array(_) | Expression::List(_), Some(vt)) => {
                bind_destructured_pattern(value_expr, &vt, scope, ctx);
            }
            (Expression::Variable(Variable::Direct(dv)), None) => {
                scope.set_unknown(bytes_to_str(dv.name));
            }
            (Expression::Array(_) | Expression::List(_), None) => {
                forget_destructured_vars(value_expr, scope);
            }
            _ => {}
        }
    }
}

/// The type the element under `key` of a destructured `rhs_type` holds.
///
/// A union is one of its members at runtime, so each target holds what it
/// would have read off any of them. Destructuring `null` or another scalar
/// assigns `null` to every target, so a nullable RHS hands each target its
/// element type or `null`.
fn destructured_element_type(rhs_type: &PhpType, key: Option<&str>) -> Option<PhpType> {
    match rhs_type.kind() {
        TypeKind::Nullable(inner) => destructured_element_type(inner, key).map(PhpType::or_null),
        TypeKind::Union(members) => members
            .iter()
            .map(|member| destructured_element_type(member, key))
            .collect::<Option<Vec<_>>>()
            .map(PhpType::union),
        _ if rhs_type.is_null()
            || rhs_type.is_bool()
            || rhs_type.is_true()
            || rhs_type.is_false()
            || rhs_type.is_int_subtype()
            || rhs_type.is_float_subtype()
            || rhs_type.is_string_subtype() =>
        {
            Some(PhpType::null())
        }
        _ => key
            .and_then(|k| rhs_type.shape_value_type(k).cloned())
            .or_else(|| rhs_type.extract_value_type(false).cloned()),
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

/// Process `array_push($var, …)` and `array_unshift($var, …)`: the values
/// land in `$var` the way a `$var[] = …` append for each of them puts them
/// there, in front of the existing entries for `array_unshift()`.
///
/// Returns whether `expr` was such a call, so the by-reference pass leaves
/// the result alone instead of resetting `$var` to the parameter's `array`
/// hint.
pub(crate) fn process_array_push_call<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    use super::super::array_shape_writes::{PushedValues, apply_array_push, apply_array_unshift};

    let Expression::Call(Call::Function(call)) = expr else {
        return false;
    };
    let Expression::Identifier(ident) = call.function else {
        return false;
    };
    let name = bytes_to_str(ident.value()).trim_start_matches('\\');
    let unshift = if name.eq_ignore_ascii_case("array_push") {
        false
    } else if name.eq_ignore_ascii_case("array_unshift") {
        true
    } else {
        return false;
    };
    let mut args = call.argument_list.arguments.iter();
    let Some(Argument::Positional(target)) = args.next() else {
        return false;
    };
    let Expression::Variable(Variable::Direct(dv)) = target.value else {
        return false;
    };
    if target.ellipsis.is_some() {
        return false;
    }
    let base_name = bytes_to_str(dv.name);
    let base_types = scope.get(base_name);
    let base_type = if base_types.is_empty() {
        PhpType::array()
    } else {
        ResolvedType::types_joined(base_types)
    };
    if !base_type.is_array_like() {
        return false;
    }

    let resolve = |value: &'b Expression<'b>| {
        let types = resolve_rhs_with_scope(value, scope, ctx);
        if types.is_empty() {
            PhpType::mixed()
        } else {
            ResolvedType::types_joined(&types)
        }
    };
    let mut values = Vec::new();
    for arg in args {
        let Argument::Positional(arg) = arg else {
            return false;
        };
        let value = resolve(arg.value);
        if arg.ellipsis.is_none() {
            values.push(PushedValues::One(value));
            continue;
        }
        // A spread of a shape adds exactly its entries; one of anything
        // else adds an unknown number of its elements.
        match value.kind() {
            TypeKind::ArrayShape(entries) if entries.iter().all(|entry| !entry.optional) => {
                values.extend(
                    entries
                        .iter()
                        .map(|entry| PushedValues::One(entry.value_type.clone())),
                );
            }
            _ => values.push(PushedValues::Any(
                value.iterable_element_type().unwrap_or_else(PhpType::mixed),
            )),
        }
    }

    let result = if unshift {
        apply_array_unshift(&base_type, &values, ctx.in_loop)
    } else {
        apply_array_push(&base_type, &values, ctx.in_loop)
    };
    scope.set(base_name, vec![ResolvedType::from_type_string(result)]);
    // The entries already there keep their values; the array as a whole
    // does not.
    scope.invalidate_whole_value_proofs(base_name);
    scope.note_element_write(base_name, false);
    true
}

/// Process the calls that take an array by reference and either leave its
/// value alone (`reset()`, `end()`, `next()`, `prev()` only move the
/// internal pointer) or drop one entry from an end (`array_shift()`,
/// `array_pop()`).
///
/// Returns whether `expr` was handled, so the by-reference pass does not
/// reset the variable to the parameter's `array|object` hint. A removal
/// from something that is not an array is left to that pass.
pub(crate) fn process_array_cursor_call<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let Some((name, call)) = called_function(expr) else {
        return false;
    };
    let from_front = if is_array_pointer_function(name) {
        None
    } else if name.eq_ignore_ascii_case("array_shift") {
        Some(true)
    } else if name.eq_ignore_ascii_case("array_pop") {
        Some(false)
    } else {
        return false;
    };
    let mut args = call.argument_list.arguments.iter();
    let (Some(Argument::Positional(target)), None) = (args.next(), args.next()) else {
        return false;
    };
    let Expression::Variable(Variable::Direct(dv)) = target.value else {
        return false;
    };
    if target.ellipsis.is_some() {
        return false;
    }
    let Some(from_front) = from_front else {
        return true;
    };
    let base_name = bytes_to_str(dv.name);
    let base_types = scope.get(base_name);
    if base_types.is_empty() {
        return false;
    }
    let base_type = ResolvedType::types_joined(base_types);
    if !base_type.is_array_like() {
        return false;
    }
    // The walker re-walks a loop body until it settles, and a shape that
    // loses an entry on every pass never does.
    let result = (!ctx.in_loop)
        .then(|| super::super::array_shape_writes::apply_array_remove_end(&base_type, from_front))
        .flatten()
        .unwrap_or_else(|| super::super::array_shape_writes::after_unknown_removal(&base_type));
    scope.set(base_name, vec![ResolvedType::from_type_string(result)]);
    scope.invalidate_proofs(base_name);
    true
}

/// Process the calls that sort an array in place.
///
/// Sorting reorders the entries without changing which values the array
/// holds, so the argument keeps its value type and, when it had none,
/// stays empty. The sorts that keep keys (`asort()`, `ksort()`, …) leave
/// the type alone; the ones that renumber (`sort()`, `usort()`,
/// `shuffle()`, …) make it a list of the same values.
///
/// Returns whether `expr` was handled, so the by-reference pass does not
/// reset the variable to the parameter's bare `array` hint.
pub(crate) fn process_array_sort_call(expr: &Expression<'_>, scope: &mut ScopeState) -> bool {
    const RENUMBERING: [&str; 4] = ["sort", "rsort", "usort", "shuffle"];
    const KEY_PRESERVING: [&str; 8] = [
        "asort",
        "arsort",
        "ksort",
        "krsort",
        "uasort",
        "uksort",
        "natsort",
        "natcasesort",
    ];
    let Some((name, call)) = called_function(expr) else {
        return false;
    };
    let renumbers = if RENUMBERING.iter().any(|f| f.eq_ignore_ascii_case(name)) {
        true
    } else if KEY_PRESERVING.iter().any(|f| f.eq_ignore_ascii_case(name)) {
        false
    } else {
        return false;
    };
    let Some(Argument::Positional(target)) = call.argument_list.arguments.first() else {
        return false;
    };
    let Expression::Variable(Variable::Direct(dv)) = target.value else {
        return false;
    };
    if target.ellipsis.is_some() {
        return false;
    }
    let base_name = bytes_to_str(dv.name);
    let base_types = scope.get(base_name);
    if base_types.is_empty() {
        return false;
    }
    let base_type = ResolvedType::types_joined(base_types);
    if !base_type.is_array_like() {
        return false;
    }
    if !renumbers || base_type.is_empty_array_shape() {
        return true;
    }
    let Some(value) = base_type.iterable_element_type() else {
        return false;
    };
    let list = PhpType::list(value);
    let result = if base_type.is_provably_non_empty() {
        list.non_empty_array_form()
    } else {
        list
    };
    scope.set(base_name, vec![ResolvedType::from_type_string(result)]);
    scope.invalidate_proofs(base_name);
    true
}

/// Whether `expr` calls `reset()`, `end()`, `next()` or `prev()`, which
/// take their array by reference only to move its internal pointer.
pub(crate) fn is_array_pointer_call(expr: &Expression<'_>) -> bool {
    called_function(expr).is_some_and(|(name, _)| is_array_pointer_function(name))
}

fn is_array_pointer_function(name: &str) -> bool {
    ["reset", "end", "next", "prev"]
        .iter()
        .any(|f| f.eq_ignore_ascii_case(name))
}

/// The bare name and call node of a plain `name(…)` function call.
fn called_function<'a, 'b>(expr: &'a Expression<'b>) -> Option<(&'b str, &'a FunctionCall<'b>)> {
    let Expression::Call(Call::Function(call)) = expr else {
        return None;
    };
    let Expression::Identifier(ident) = call.function else {
        return None;
    };
    Some((bytes_to_str(ident.value()).trim_start_matches('\\'), call))
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
        forget_overwritten_offsets(base_name, key_chain, append, scope);
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
        scope.invalidate_proofs(base_name);
        return;
    }

    // The subject each segment writes into: `$a`, then `$a[$k]`, and so on.
    // `None` once a segment is not one a subject key can spell. Only worth
    // spelling out while some key fact could be about one of them.
    let subjects: Vec<Option<String>> = if scope.key_facts.is_none() {
        Vec::new()
    } else {
        (0..=key_chain.len())
            .map(|depth| array_write_synthetic_key(base_name, &key_chain[..depth]))
            .collect()
    };
    let mut write_keys: Vec<super::super::array_shape_writes::ArrayWriteKey> = key_chain
        .iter()
        .enumerate()
        .map(|(depth, idx)| {
            match super::super::array_shape_writes::extract_array_key_for_shape(idx) {
                Some(key) => super::super::array_shape_writes::ArrayWriteKey::Shape(key),
                None => {
                    let index_types = resolve_rhs_with_scope(idx, scope, ctx);
                    let key_type =
                        super::super::array_shape_writes::infer_array_key_type(idx, &index_types);
                    let literal_slot =
                        match super::super::array_shape_writes::literal_write_key(&key_type) {
                            Some(Ok(key)) => {
                                return super::super::array_shape_writes::ArrayWriteKey::Shape(key);
                            }
                            Some(Err(slot)) => Some(slot),
                            None => None,
                        };
                    super::super::array_shape_writes::ArrayWriteKey::Keyed {
                        key_type,
                        slot: super::super::array_shape_writes::extract_array_write_index(idx)
                            .or(literal_slot),
                        existing: is_existing_key_write(
                            subjects.get(depth).and_then(Option::as_deref),
                            idx,
                            scope,
                        ),
                    }
                }
            }
        })
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
    note_element_write(base_name, &subjects, &write_keys, append, scope);
    forget_proofs_about_written_values(base_name, key_chain, append, scope);

    forget_overwritten_offsets(base_name, key_chain, append, scope);
    if !append {
        overwrite_written_offset(base_name, key_chain, rhs_types, scope);
    }
}

/// Drop the proofs an element write made stale: the ones about the array
/// and each offset on the way down to the element, which all hold a
/// different value now, and every one about the element it replaced.
fn forget_proofs_about_written_values(
    base_name: &str,
    key_chain: &[&Expression<'_>],
    append: bool,
    scope: &mut ScopeState,
) {
    scope.invalidate_whole_value_proofs(base_name);
    let changed = if append {
        key_chain.len()
    } else {
        key_chain.len().saturating_sub(1)
    };
    for depth in 1..=changed {
        let Some(level) = array_write_synthetic_key(base_name, &key_chain[..depth]) else {
            return;
        };
        scope.invalidate_whole_value_proofs(&level);
    }
    if !append && let Some(written) = array_write_synthetic_key(base_name, key_chain) {
        scope.invalidate_proofs(&written);
    }
}

/// Drop the synthetic offset keys a write made stale.
///
/// A write below an offset changes what that offset holds, so a key
/// recorded for it (`$a["d"]`, by an earlier `$a["d"] = [...]`) would be
/// read back instead of the updated slot in `$a`'s own type. An append
/// changes the offset it appends to the same way. And a write that
/// replaces an offset outright takes every key recorded below it
/// (`$a["d"]["e"]`) with the old value.
fn forget_overwritten_offsets(
    base_name: &str,
    key_chain: &[&Expression<'_>],
    append: bool,
    scope: &mut ScopeState,
) {
    let changed = if append {
        key_chain.len()
    } else {
        key_chain.len().saturating_sub(1)
    };
    for depth in 1..=changed {
        if let Some(prefix) = array_write_synthetic_key(base_name, &key_chain[..depth])
            && scope.contains(&prefix)
        {
            scope.remove(&prefix);
        }
    }
    if !append && let Some(written) = array_write_synthetic_key(base_name, key_chain) {
        scope.invalidate_dependent_keys(&written);
    }
}

/// Whether the index `idx` writing into `subject` is a variable holding a
/// key the array is known to have.
fn is_existing_key_write(subject: Option<&str>, idx: &Expression<'_>, scope: &ScopeState) -> bool {
    let (Some(subject), Expression::Variable(Variable::Direct(dv))) = (subject, idx) else {
        return false;
    };
    scope.is_existing_key(subject, bytes_to_str(dv.name))
}

/// Drop the key facts an element write invalidated: each level it added a
/// key to no longer holds the same keys as anything else, and the value it
/// replaced says nothing about the keys it held before.
fn note_element_write(
    base_name: &str,
    subjects: &[Option<String>],
    write_keys: &[super::super::array_shape_writes::ArrayWriteKey],
    append: bool,
    scope: &mut ScopeState,
) {
    if scope.key_facts.is_none() {
        return;
    }
    let depth = write_keys.len() - usize::from(append);
    for (level, key) in write_keys.iter().enumerate() {
        let adds_key = !matches!(
            key,
            super::super::array_shape_writes::ArrayWriteKey::Keyed { existing: true, .. }
        );
        if !adds_key {
            continue;
        }
        match subjects.get(level).cloned().flatten() {
            Some(subject) => scope.note_element_write(&subject, false),
            None => {
                scope.note_element_write(base_name, true);
                return;
            }
        }
    }
    if !append {
        match subjects.get(depth).cloned().flatten() {
            Some(subject) => scope.note_element_write(&subject, true),
            None => scope.note_element_write(base_name, true),
        }
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
pub(super) fn array_write_synthetic_key(
    base_name: &str,
    key_chain: &[&Expression<'_>],
) -> Option<String> {
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
