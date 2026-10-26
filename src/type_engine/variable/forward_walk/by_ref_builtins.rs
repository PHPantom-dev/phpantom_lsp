//! What a call writes into the variables it passes by reference, once its
//! callee is known: the type each by-reference parameter leaves behind,
//! the shape `preg_match()` and its relatives fill `$matches` with, and
//! the locals `extract()` defines for the keys of its array.

use mago_syntax::cst::argument::Argument;

use crate::atom::{atom, bytes_to_str};
use crate::php_type::{PhpType, ShapeEntry, TypeKind};
use crate::type_engine::call_resolution::{effective_out_type, resolve_out_type_for_call};
use crate::types::ResolvedType;

use super::*;

/// Whether `expr` is a call to `extract()`.
pub(super) fn is_extract_call(expr: &Expression<'_>) -> bool {
    let Expression::Call(Call::Function(call)) = expr else {
        return false;
    };
    let Expression::Identifier(ident) = call.function else {
        return false;
    };
    bytes_to_str(ident.value())
        .trim_start_matches('\\')
        .eq_ignore_ascii_case("extract")
}

/// How `extract()` treats a key, from its `flags` argument (with the
/// `EXTR_REFS` bit, which changes no types, masked off).
#[derive(Clone, Copy, PartialEq)]
enum ExtractMode {
    Overwrite,
    Skip,
    PrefixSame,
    PrefixAll,
    PrefixInvalid,
    PrefixIfExists,
    IfExists,
}

impl ExtractMode {
    fn from_flags(flags: i64) -> Option<Self> {
        Some(match flags & !256 {
            0 => Self::Overwrite,
            1 => Self::Skip,
            2 => Self::PrefixSame,
            3 => Self::PrefixAll,
            4 => Self::PrefixInvalid,
            5 => Self::PrefixIfExists,
            6 => Self::IfExists,
            _ => return None,
        })
    }

    fn is_prefixed(self) -> bool {
        matches!(
            self,
            Self::PrefixSame | Self::PrefixAll | Self::PrefixInvalid | Self::PrefixIfExists
        )
    }

    /// The local a key is written to, if any.  `exists` says whether a
    /// local of that name is in scope, which is what the conditional
    /// modes test.
    fn target(self, key: &str, prefix: &str, exists: impl Fn(&str) -> bool) -> Option<String> {
        let prefixed = || format!("{prefix}_{key}");
        let name = match self {
            Self::Overwrite => key.to_string(),
            Self::Skip if exists(key) => return None,
            Self::Skip => key.to_string(),
            Self::PrefixSame if is_extractable_variable_name(key) && exists(key) => prefixed(),
            Self::PrefixSame => key.to_string(),
            Self::PrefixAll => prefixed(),
            Self::PrefixInvalid if is_extractable_variable_name(key) => key.to_string(),
            Self::PrefixInvalid => prefixed(),
            Self::PrefixIfExists if exists(key) => prefixed(),
            Self::IfExists if exists(key) => key.to_string(),
            Self::PrefixIfExists | Self::IfExists => return None,
        };
        is_extractable_variable_name(&name).then_some(name)
    }

    /// Whether a call whose keys are not known may write the local `name`.
    fn may_write_any(self, name: &str, prefix: &str) -> bool {
        match self {
            Self::Skip => false,
            // A key that collides is written under the prefix, and one that
            // does not names a variable that is not there yet.
            Self::PrefixSame | Self::PrefixAll | Self::PrefixIfExists => name
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('_')),
            Self::Overwrite | Self::PrefixInvalid | Self::IfExists => true,
        }
    }
}

/// The value of an `extract()` flags argument: `EXTR_*` constants and
/// integer literals, combined with `|`.
fn extract_flags_value(expr: &Expression<'_>) -> Option<i64> {
    match expr {
        Expression::Parenthesized(inner) => extract_flags_value(inner.expression),
        Expression::Binary(binary) if matches!(binary.operator, BinaryOperator::BitwiseOr(_)) => {
            Some(extract_flags_value(binary.lhs)? | extract_flags_value(binary.rhs)?)
        }
        Expression::Literal(Literal::Integer(int)) => int.value.map(|v| v as i64),
        Expression::ConstantAccess(access) => {
            let name = bytes_to_str(access.name.value());
            Some(match name.rsplit('\\').next().unwrap_or(name) {
                "EXTR_OVERWRITE" => 0,
                "EXTR_SKIP" => 1,
                "EXTR_PREFIX_SAME" => 2,
                "EXTR_PREFIX_ALL" => 3,
                "EXTR_PREFIX_INVALID" => 4,
                "EXTR_PREFIX_IF_EXISTS" => 5,
                "EXTR_IF_EXISTS" => 6,
                "EXTR_REFS" => 256,
                _ => return None,
            })
        }
        _ => None,
    }
}

/// Define the locals an `extract()` call writes.
///
/// An array shape writes one local per key, under the name and on the
/// condition its flags say, so a required key replaces whatever the
/// variable held and an optional one may.  An array whose keys are not
/// known may write any local the flags allow with anything, so each of
/// those widens to `mixed`.  Returns whether the call was an `extract()`.
pub(super) fn process_extract_call<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    if !is_extract_call(expr) {
        return false;
    }
    let Expression::Call(Call::Function(call)) = expr else {
        return false;
    };
    let mut array_arg = None;
    let mut flags_arg = None;
    let mut prefix_arg = None;
    let mut unpacked = false;
    let mut next_positional = 0;
    for arg in call.argument_list.arguments.iter() {
        if let Argument::Positional(pos) = arg
            && pos.ellipsis.is_some()
        {
            unpacked = true;
        }
        let (value, selector) = arg_expr_and_selector(arg, &mut next_positional);
        let slot = match selector {
            ArgSelector::Position(0) => &mut array_arg,
            ArgSelector::Position(1) => &mut flags_arg,
            ArgSelector::Position(2) => &mut prefix_arg,
            ArgSelector::Name(name) if name == "array" => &mut array_arg,
            ArgSelector::Name(name) if name == "flags" => &mut flags_arg,
            ArgSelector::Name(name) if name == "prefix" => &mut prefix_arg,
            _ => continue,
        };
        *slot = Some(value);
    }
    let Some(array_arg) = array_arg else {
        return true;
    };
    let mode = match flags_arg {
        None => Some(ExtractMode::Overwrite),
        Some(flags) => extract_flags_value(flags).and_then(ExtractMode::from_flags),
    };
    let prefix =
        prefix_arg.and_then(crate::type_engine::type_ops::condition_facts::string_literal_value);
    let mode = match mode {
        Some(mode) if !unpacked && (prefix.is_some() || !mode.is_prefixed()) => mode,
        _ => {
            widen_extractable_locals(scope, |_| true);
            return true;
        }
    };
    let prefix = prefix.as_deref().unwrap_or("");

    let types = super::assignment::resolve_rhs_with_scope(array_arg, scope, ctx);
    let array = ResolvedType::types_joined(&types);
    // Each alternative of a union of shapes writes its own keys, so a key
    // the others lack is only possibly written.
    let shapes: Option<Vec<&[ShapeEntry]>> = match array.kind() {
        _ if types.is_empty() => None,
        TypeKind::ArrayShape(entries) => Some(vec![&entries[..]]),
        TypeKind::Union(members) => members
            .iter()
            .map(|m| match m.kind() {
                TypeKind::ArrayShape(entries) => Some(&entries[..]),
                _ => None,
            })
            .collect(),
        _ => None,
    };
    let Some(shapes) = shapes else {
        widen_extractable_locals(scope, |name| mode.may_write_any(name, prefix));
        return true;
    };
    let exists = |name: &str| scope.contains(&format!("${name}"));
    // (name, value, optional, alternatives writing it), in key order.
    let mut written: Vec<(String, Vec<PhpType>, bool, usize)> = Vec::new();
    for entries in &shapes {
        for (position, entry) in entries.iter().enumerate() {
            let key = match entry.key.as_deref() {
                Some(key) => key.trim_matches(|c| c == '\'' || c == '"').to_string(),
                None => position.to_string(),
            };
            let Some(name) = mode.target(&key, prefix, exists) else {
                continue;
            };
            match written.iter_mut().find(|(n, ..)| *n == name) {
                Some((_, values, optional, count)) => {
                    values.push(entry.value_type.clone());
                    *optional |= entry.optional;
                    *count += 1;
                }
                None => written.push((name, vec![entry.value_type.clone()], entry.optional, 1)),
            }
        }
    }
    for (name, mut values, optional, count) in written {
        let var_name = format!("${name}");
        if optional || count < shapes.len() {
            let existing = scope.get(&var_name);
            if !existing.is_empty() {
                values.insert(0, ResolvedType::types_joined(existing));
            }
        }
        scope.invalidate_dependent_keys(&var_name);
        scope.invalidate_proofs(&var_name);
        scope.set(
            &var_name,
            vec![ResolvedType::from_type_string(PhpType::union(values))],
        );
    }
    true
}

/// Widen to `mixed` every local an `extract()` of unknown keys may have
/// written, as `may_write` (given the name without its `$`) allows.
fn widen_extractable_locals(scope: &mut ScopeState, may_write: impl Fn(&str) -> bool) {
    let names: Vec<String> = scope
        .locals
        .keys()
        .filter_map(|key| key.strip_prefix('$'))
        .filter(|name| is_extractable_variable_name(name) && may_write(name))
        .map(|name| format!("${name}"))
        .collect();
    for var_name in names {
        scope.invalidate_dependent_keys(&var_name);
        scope.invalidate_proofs(&var_name);
        scope.set(
            &var_name,
            vec![ResolvedType::from_type_string(PhpType::mixed())],
        );
    }
}

/// Whether `extract()` turns an array key into a local: a valid PHP
/// variable name other than `this`, which it refuses to overwrite.
fn is_extractable_variable_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first == b'_' || first.is_ascii_alphabetic() || first >= 0x80)
        && bytes.all(|b| b == b'_' || b.is_ascii_alphanumeric() || b >= 0x80)
        && name != "this"
}

/// Whether `value_expr` is passed to the call via `...value_expr` (argument
/// unpacking), rather than as a direct positional or named argument.
fn arg_is_unpacked<'b>(arg_list: &ArgumentList<'b>, value_expr: &Expression<'b>) -> bool {
    arg_list.arguments.iter().any(|arg| {
        matches!(arg, Argument::Positional(pos) if pos.ellipsis.is_some() && std::ptr::eq(pos.value, value_expr))
    })
}

/// Give each variable `call` passes to a by-reference parameter the type
/// the call leaves in it: the classes the parameter's out type names, or
/// else the out type itself where it is a primitive one (`array &$matches`),
/// or `array` for a new variable handed to an untyped parameter. Existing
/// exact values must be invalidated because the callee may assign any value
/// allowed by the parameter type.
pub(super) fn seed_by_ref_parameters<'b>(
    expr: &'b Expression<'b>,
    call: &ByRefCall<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Most callees take nothing by reference, and so write nothing back.
    // Moving the internal pointer leaves the array's value as it was, so
    // the `array|object` hint describes nothing the call wrote.
    if !call.parameters.iter().any(|param| param.is_reference)
        || super::array_assignment::is_array_pointer_call(expr)
    {
        return;
    }

    let ByRefCall {
        arguments: arg_list,
        parameters,
        callee: template_owner,
    } = call;

    // Bind arguments to parameters following PHP's rules so a named argument
    // seeds the parameter it actually targets, not the one at its ordinal
    // position in the call.
    let bound = crate::call_args::bind_args_to_params(parameters, arg_list);

    // A conditional out type is decided by the arguments as the call
    // receives them, before it writes any of them.
    let pre_call = scope.snapshot_resolver();

    // An out type written in the callee's own `@template` params
    // (`usort`'s `array<TKey, TValue> &$array`) describes the caller's
    // variable only once those params are bound, and this pass has no
    // binding for them. Applied raw it would replace a precise argument
    // type (`TargetClass<ContainerExtension>[]`) with an array of a name
    // nothing can resolve, so the variable is left as it was instead.
    let callee_templates = template_owner.template_params();

    for (param_index, (param, arg_expr)) in parameters.iter().zip(bound.iter()).enumerate() {
        let arg_expr = match arg_expr {
            Some(expr) => *expr,
            None => continue,
        };

        // Only handle direct variable arguments.
        let var_name = match arg_expr {
            Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
            _ => continue,
        };

        // Check if the corresponding parameter is pass-by-reference.
        if !param.is_reference {
            continue;
        }

        // `example(...$z)` unpacks each element of `$z` into a separate
        // by-ref argument; the callee writes through the *elements*, not
        // through `$z` itself, so `$z` stays an array whose values take
        // the parameter's type rather than becoming that type directly.
        let is_spread = param.is_variadic && arg_is_unpacked(arg_list, arg_expr);

        let already_in_scope = !scope.get(&var_name).is_empty();
        // An explicit `@param-out` states what the callee writes, so the
        // argument's type from before the call has no say in the result. A
        // plain hint only bounds what the callee may write, and precision
        // it does not contradict survives the call.
        let keep_existing = param.param_out_type.is_none();
        let mut seeded = false;
        if let Some(out_hint) =
            effective_out_type(param, param_index, template_owner, ctx.file.backend)
        {
            // A PHPStan conditional out type (`@param-out ($arg is null ?
            // A&I : A) $arg`) is call-site-agnostic up to this point; only
            // this call's own arguments say which branch it actually takes.
            let out_hint = if !matches!(out_hint.kind(), TypeKind::Conditional(_)) {
                out_hint
            } else {
                let var_ctx = ctx.var_ctx_for_with_scope(
                    "",
                    ctx.cursor_offset,
                    &pre_call,
                    Some(scope.proofs()),
                );
                let call_var_resolver =
                    super::super::resolution::build_var_resolver_from_ctx(&var_ctx);
                resolve_out_type_for_call(
                    out_hint,
                    parameters,
                    template_owner,
                    arg_list,
                    ctx.file.content,
                    &var_ctx.as_resolution_ctx(),
                    Some(&call_var_resolver),
                )
            };
            // Checked once the call has picked a branch, since the branch it
            // takes (`array` in `$x is array ? array : T`) may name none.
            if out_hint.references_any_name(callee_templates) {
                continue;
            }
            // A variadic parameter's stored PHPDoc type may describe the
            // collected argument array (`string[] &$values`), while each
            // call-site variable is one element of that collection. Native
            // element hints such as `string &...$values` are already scalar
            // and therefore pass through unchanged.
            let effective_hint = if param.is_variadic {
                out_hint
                    .iterable_element_type()
                    .unwrap_or_else(|| out_hint.clone())
            } else {
                out_hint
            };
            let primitive_hint = match effective_hint.kind() {
                TypeKind::Union(members) | TypeKind::Intersection(members) => {
                    !members.is_empty() && members.iter().all(PhpType::is_scalar)
                }
                _ => effective_hint.is_scalar(),
            };
            let classes = if primitive_hint {
                Vec::new()
            } else {
                crate::type_engine::type_resolution::type_hint_to_classes_typed(
                    &effective_hint,
                    &ctx.current_class.fqn(),
                    ctx.file.all_classes,
                    ctx.file.class_loader,
                )
            };
            if is_spread && (primitive_hint || !classes.is_empty()) {
                // The existing array's own element type plays the same role
                // here that the whole variable's type plays below: kept
                // when it already agrees with the hint (minus literal
                // precision), replaced by the hint when it disagrees or is
                // unknown. The array's keys are untouched either way.
                let existing = scope.get(&var_name);
                let existing_joined =
                    (!existing.is_empty()).then(|| ResolvedType::types_joined(existing));
                let key_type = existing_joined
                    .as_ref()
                    .and_then(PhpType::iterable_key_type)
                    .unwrap_or_else(PhpType::int);
                let value_type = existing_joined
                    .as_ref()
                    .filter(|_| keep_existing)
                    .and_then(PhpType::iterable_element_type)
                    .filter(|value| !value.is_null())
                    .filter(|value| value.is_subtype_of(&effective_hint))
                    .map(|value| value.widen_scalar_literals())
                    .unwrap_or(effective_hint);
                scope.set(
                    &var_name,
                    vec![ResolvedType::from_type_string(PhpType::generic_array(
                        key_type, value_type,
                    ))],
                );
                seeded = true;
            } else if !classes.is_empty() {
                // `function fill(Foo &$out)`: the variable holds a `Foo`
                // after the call, whatever it held before.
                let written = ResolvedType::from_classes_with_hint(classes, effective_hint);
                if resolved_types_differ(&written, scope.get(&var_name)) {
                    scope.set(&var_name, written);
                }
                seeded = true;
            } else if primitive_hint {
                // The callee may assign any value the parameter type allows,
                // so an exact value observed before the call is stale. What
                // the call cannot invalidate is precision the hint does not
                // contradict: `array_shift(array &$array)` says nothing about
                // the element type of a `Node[]` argument. Keep a value the
                // hint already covers, minus its literal precision, and fall
                // back to the hint only when the two genuinely disagree.
                let existing = scope.get(&var_name);
                let refined = (keep_existing && !existing.is_empty())
                    .then(|| ResolvedType::types_joined(existing))
                    // An empty array shape (`$matches = [];` before
                    // `preg_match_all(…, $matches)`) records no element
                    // precision, only that nothing had been written yet —
                    // which is exactly what the call invalidates. It is a
                    // subtype of every array hint, so without this it would
                    // survive the call and claim the result is still empty.
                    // A bare `null` (`$key = null;` before the call) says the
                    // same thing about a nullable out type, and keeping it
                    // would claim the callee wrote nothing at all.
                    .filter(|existing| !existing.shape_entries().is_some_and(<[_]>::is_empty))
                    .filter(|existing| !existing.is_null())
                    .filter(|existing| existing.is_subtype_of(&effective_hint))
                    .map(|existing| existing.widen_scalar_literals())
                    .unwrap_or(effective_hint);
                scope.set(&var_name, vec![ResolvedType::from_type_string(refined)]);
                seeded = true;
            }
        }
        if !seeded && !already_in_scope && param.type_hint.is_none() {
            // Untyped pass-by-reference parameters (e.g. `&$matches`
            // in `preg_match`, `&$result` in `parse_str`) are most
            // commonly arrays. Seed only new variables as `array`; an
            // existing value has no sounder replacement without a hint,
            // and neither has one the callee's body did not give up.
            scope.set(
                &var_name,
                vec![ResolvedType::from_type_string(PhpType::named(atom(
                    "array",
                )))],
            );
        }
    }
}

/// Type `$matches` from the capture groups of the pattern a
/// `preg_match`/`preg_match_all` call passes.
///
/// The parameter is declared `?array &$matches`, and a bare `array` is all
/// the generic by-reference seeding above can offer. A literal pattern says
/// more: which keys the array has, and which of them a successful match may
/// leave out.
///
/// The call site is not the place that knows whether the match succeeded, so
/// what lands in the scope here is what the call leaves either way. A branch
/// guarded on the outcome narrows it down (see
/// [`apply_preg_match_narrowing`]).
///
/// Returns whether the variable was typed. A pattern the group walk refuses,
/// or a `$flags` argument that does not resolve to a constant, leaves the
/// call to the generic path.
pub(super) fn seed_preg_matches<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let Some(call) = crate::type_engine::regex_shape::preg_call(expr) else {
        return false;
    };
    let Some(matched) = preg_matched_type(&call, scope, ctx) else {
        return false;
    };
    scope.set(
        call.matches_var,
        vec![ResolvedType::from_type_string(
            crate::type_engine::regex_shape::or_no_match(matched, call.matches_all),
        )],
    );
    true
}

/// The type a *successful* match leaves in the out-parameter of `call`.
///
/// `None` when the analysis refuses the call: a pattern whose group list the
/// walk cannot read, or a `$flags` argument that does not resolve to a
/// constant whose bits the shape analysis models.
pub(crate) fn preg_matched_type<'b>(
    call: &crate::type_engine::regex_shape::PregCall<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<PhpType> {
    let flags = match call.flags {
        None => 0,
        Some(flags) => preg_flag_bits(flags, scope, ctx)?,
    };
    call.pattern
        .as_deref()
        .and_then(|pattern| {
            crate::type_engine::regex_shape::matches_type(pattern, flags, call.matches_all)
        })
        .or_else(|| crate::type_engine::regex_shape::opaque_matches_type(flags, call.matches_all))
}

/// The flag mask a `preg_match` `$flags` argument holds.
///
/// Resolved through the shared pipeline so a named constant, a class
/// constant, and a variable holding one all read the same. Returns `None`
/// when the argument does not resolve to a single integer value, or to one
/// whose bits the shape analysis does not model — the result's shape depends
/// on the flags, so a mask that cannot be read is not one to guess at.
fn preg_flag_bits<'b>(
    expr: &'b Expression<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<i64> {
    let scope_resolver = scope.snapshot_resolver();
    let var_ctx =
        ctx.var_ctx_for_with_scope("", ctx.cursor_offset, &scope_resolver, Some(scope.proofs()));
    let flags =
        crate::type_engine::variable::iterable_elements::resolve_expression_type(expr, &var_ctx)
            .as_ref()
            .and_then(crate::type_engine::type_ops::const_fold::literal_int_value)?;
    crate::type_engine::regex_shape::flags_are_modelled(flags).then_some(flags)
}
