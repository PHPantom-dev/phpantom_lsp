//! Narrowing that a call proves for the code after it: `assert()` and the
//! guard functions that work like it (`abort_unless()`, `throw_if()`, …),
//! `@phpstan-assert` and PHPUnit's assertions, a conditional return type's
//! `never` branch, and `@phpstan-self-out` re-binding the receiver.

use super::*;

use mago_span::HasSpan;
use mago_syntax::cst::argument::Argument;

use crate::atom::{atom, bytes_to_str};
use crate::php_type::PhpType;
use crate::type_engine::type_ops::condition_facts;
use crate::types::ResolvedType;

/// The guard functions whose first argument is a condition the call
/// leaves proven, paired with that parameter's name and with whether the
/// condition still holds once the call returns.
///
/// `assert()` proves its argument true outright.  Laravel's
/// `abort_unless()` / `throw_unless()` prove it true by bailing out when
/// it is false, and `abort_if()` / `throw_if()` prove its *negation* the
/// same way.  Neither the framework nor any stub annotates these four
/// with `@phpstan-assert`, so the name is the only signal there is, and
/// this is the set the Laravel PHPStan extensions special-case too.
const CONDITION_GUARD_FUNCTIONS: [(&str, &str, bool); 5] = [
    ("assert", "assertion", true),
    ("abort_if", "boolean", false),
    ("abort_unless", "boolean", true),
    ("throw_if", "condition", false),
    ("throw_unless", "condition", true),
];

/// The condition a [guard call](CONDITION_GUARD_FUNCTIONS) proves and
/// whether it holds after the call, or `None` when `expr` is not one.
///
/// Matches every spelling PHP accepts: unqualified, fully-qualified
/// (`\assert`), and any letter case.
fn guard_call_condition<'b>(expr: &'b Expression<'b>) -> Option<(&'b Expression<'b>, bool)> {
    let Expression::Call(Call::Function(fc)) = crate::parser::unwrap_parens(expr) else {
        return None;
    };
    let Expression::Identifier(ident) = fc.function else {
        return None;
    };
    let raw = bytes_to_str(ident.value());
    let called = raw.strip_prefix('\\').unwrap_or(raw);
    let (_, param, holds_after) = CONDITION_GUARD_FUNCTIONS
        .iter()
        .find(|(name, _, _)| called.eq_ignore_ascii_case(name))?;

    // A named argument may sit anywhere in the list, so `abort_if(code:
    // 404, boolean: $x === null)` still has to reach the condition.
    let condition = match fc.argument_list.arguments.first()? {
        Argument::Positional(pos) => pos.value,
        Argument::Named(_) => fc
            .argument_list
            .arguments
            .iter()
            .find_map(|arg| match arg {
                Argument::Named(named)
                    if bytes_to_str(named.name.value).eq_ignore_ascii_case(param) =>
                {
                    Some(named.value)
                }
                _ => None,
            })?,
    };
    Some((condition, *holds_after))
}

/// Process assert narrowing (assert($x instanceof Foo), @phpstan-assert, etc.)
pub(crate) fn process_assert_narrowing<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Every narrowing path below only fires for a (possibly parenthesized)
    // call expression, so a non-call statement can never be an assert() /
    // custom type-guard call. Bail out before the scope clone below, which
    // otherwise runs once per in-scope variable for every statement.
    let unwrapped = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    if !matches!(unwrapped, Expression::Call(_)) {
        return;
    }

    let guard = guard_call_condition(expr);

    // ── Handle assert($x instanceof Foo) for variables NOT yet in scope ──
    // When a foreach binds a variable but the iterable element type is
    // unknown, the variable won't be in the scope map.  A subsequent
    // `assert($x instanceof Foo)` (or `abort_unless($x instanceof Foo,
    // 403)`) should add it with the asserted type.
    if let Some((condition, true)) = guard
        && let Expression::Binary(bin) = condition
        && bin.operator.is_instanceof()
        && let Expression::Variable(Variable::Direct(dv)) = bin.lhs
    {
        let var_name = bytes_to_str(dv.name).to_string();
        if scope.get(&var_name).is_empty() {
            // Variable not in scope — seed it with the asserted type.
            if let Some(name) = ctx.written_names().static_receiver_class_name(bin.rhs) {
                let resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
                    &PhpType::named(atom(&name)),
                    &ctx.current_class.fqn(),
                    ctx.file.all_classes,
                    ctx.file.class_loader,
                );
                if !resolved.is_empty() {
                    scope.set(
                        &var_name,
                        ResolvedType::from_classes_with_hint(resolved, PhpType::named(atom(&name))),
                    );
                } else {
                    scope.set(
                        &var_name,
                        vec![ResolvedType::from_type_string(PhpType::named(atom(&name)))],
                    );
                }
            }
        }
    }

    // Seed property/array-access subject keys that appear as arguments
    // to the assert call (e.g. `assertInstanceOf(X::class, $view->component)`
    // or a `@phpstan-assert` helper called on `$arg->value`) so the
    // narrowing loop below can find and narrow them.
    seed_assert_arg_subject_keys(expr, scope, ctx);

    // Re-export narrowing: PHPUnit's `assertTrue()` / `assertFalse()` carry
    // `@psalm-assert true/false $condition`.  When the argument is a boolean
    // condition expression (e.g. `property_exists($x, 'p')`), proving it
    // true/false is equivalent to a guard on that condition, so run the
    // standard condition-narrowing pipeline on the argument.
    let reexport_conditions = {
        let reexport_snapshot = scope.locals.clone();
        let reexport_resolver = |vn: &str| -> Vec<ResolvedType> {
            reexport_snapshot
                .get(&atom(vn))
                .cloned()
                .unwrap_or_default()
        };
        let reexport_ctx = build_var_ctx("", ctx, &reexport_resolver);
        condition_facts::collect_assert_reexport_conditions(expr, &reexport_ctx)
    };
    for (condition, asserts_true) in reexport_conditions {
        if asserts_true {
            apply_condition_narrowing(condition, scope, ctx);
        } else {
            apply_condition_narrowing_inverse(condition, scope, ctx);
        }
    }

    // A conditional return type whose `never` branch some argument value
    // would have selected proves that value never reached the call.
    apply_never_branch_narrowing(unwrapped, scope, ctx);

    // `assert(<condition>)` proves its argument true for everything that
    // follows in the same scope, exactly the way entering `if (<condition>)`
    // proves it for the block body.  `abort_if(<condition>, 404)` and its
    // siblings prove the same thing about the branch that survives them,
    // just in the polarity their name picks.  Feeding the argument into the
    // same pipeline the `if` takes means every guard form is honoured in
    // both places: `$x !== null`, `$x !== false`, `is_string($x)`, `&&`
    // chains, and so on.
    if let Some((condition, holds_after)) = guard {
        if holds_after {
            apply_condition_narrowing(condition, scope, ctx);
        } else {
            apply_condition_narrowing_inverse(condition, scope, ctx);
        }
    }

    // PHPUnit's `assertSame()`, `assertCount()`, `assertArrayHasKey()` and
    // `assertNotEmpty()` (and their negations) prove the `===`, `count()`,
    // `array_key_exists()` and `!empty()` checks they stand for, so those
    // are narrowed as if they were written out in an `assert()`.
    if let Expression::Call(call) = unwrapped {
        let phpunit_assertion = {
            let snapshot = scope.locals.clone();
            let resolver = |vn: &str| -> Vec<ResolvedType> {
                snapshot.get(&atom(vn)).cloned().unwrap_or_default()
            };
            let var_ctx = build_var_ctx("", ctx, &resolver);
            condition_facts::phpunit_assertion(call, &var_ctx)
        };
        match phpunit_assertion {
            Some(condition_facts::PhpunitAssertion::Same {
                expected,
                actual,
                identical,
            }) => {
                let span = call.span();
                let comparison = Expression::Binary(Binary {
                    lhs: expected,
                    operator: if identical {
                        BinaryOperator::Identical(span)
                    } else {
                        BinaryOperator::NotIdentical(span)
                    },
                    rhs: actual,
                });
                apply_condition_narrowing(&comparison, scope, ctx);
            }
            Some(condition_facts::PhpunitAssertion::Count {
                expected,
                haystack,
                equal,
            }) => apply_count_assertion(haystack, expected, equal, scope, ctx),
            Some(condition_facts::PhpunitAssertion::HasKey {
                key,
                array,
                present,
            }) => apply_array_key_assertion(key, array, present, scope, ctx),
            Some(condition_facts::PhpunitAssertion::Truthy { actual, empty }) => {
                if empty {
                    apply_condition_narrowing_inverse(actual, scope, ctx);
                } else {
                    apply_condition_narrowing(actual, scope, ctx);
                }
            }
            None => {}
        }
    }

    // Apply assert narrowing to each variable the call names.
    let scope_resolver = scope.snapshot_resolver();
    let var_names = scope_keys_named_by(expr, scope);
    for var_name in var_names {
        let var_ctx = ctx.var_ctx_for_with_scope(
            &var_name,
            ctx.cursor_offset,
            &scope_resolver,
            Some(scope.proofs()),
        );
        let before = scope.get(&var_name).to_vec();
        let mut results = before.clone();

        // @phpstan-assert / @psalm-assert
        let mut type_guard: Option<(condition_facts::TypeGuardKind, bool)> = None;
        let mut intersected = false;
        ResolvedType::apply_narrowing(&mut results, |classes| {
            condition_facts::try_apply_custom_assert_narrowing(
                expr,
                &var_ctx,
                classes,
                &mut type_guard,
                &mut intersected,
            )
        });
        // The assertion proved a class the subject does not nominally
        // implement, so the entries describe one value that is all of them
        // rather than a choice between them.  Untagged they join as a
        // union, which satisfies neither half's declared type.
        if intersected {
            ResolvedType::tag_as_intersection(&mut results);
        }

        // A scalar / pseudo-type assertion (`assertIsString`, `assertIsObject`,
        // `assertIsArray`, their `assertIsNot*` negations, or the `object`
        // fallback for an unresolvable `assertInstanceOf` class argument) is a
        // type guard, not a class narrowing.  Apply it on the full resolved
        // types so union members are kept or dropped by category — e.g.
        // `assertIsObject` drops null/scalar members while keeping the class,
        // and `assertIsNotObject` drops the class.
        if let Some((kind, exclude)) = type_guard {
            if exclude {
                condition_facts::apply_type_guard_exclusion(
                    kind,
                    &mut results,
                    Some(ctx.file.class_loader),
                );
            } else {
                condition_facts::apply_type_guard_inclusion(
                    kind,
                    &mut results,
                    Some(ctx.file.class_loader),
                );
            }
        }

        // A not-null assertion (`@phpstan-assert !null $x`, e.g. PHPUnit's
        // `assertNotNull`) removes the `null` pseudo-type, which the
        // class-based exclusion above cannot express.  Strip null from the
        // subject's resolved types directly so a value that was tracked as
        // exactly `null` (e.g. after `$obj->prop = null;`) no longer reads
        // as null after the assertion.
        if condition_facts::call_asserts_not_null(expr, &var_ctx) {
            results.retain_mut(|rt| match rt.type_string.non_null_type() {
                Some(non_null) => {
                    rt.type_string = non_null;
                    true
                }
                None => rt.type_string != PhpType::null(),
            });
        }

        if resolved_types_differ(&results, &before) {
            if results.is_empty() {
                // Narrowing removed all types (e.g. assert($x instanceof
                // UnresolvableClass)).  Explicitly clear the variable so
                // that diagnostics see "unknown type" and suppress false
                // positives.  `scope.set()` is a no-op for empty vecs.
                scope.set_untyped(&var_name);
            } else {
                scope.set(&var_name, results);
            }
        }
    }
}

/// `@psalm-this-out` / `@phpstan-self-out`: a call to a method carrying
/// this annotation changes the type the walker tracks for its receiver,
/// the way an assignment changes a variable's type.  Method-level
/// template parameters bound from the call's arguments are substituted
/// into the annotation's type before it replaces the receiver's tracked
/// type: `$box->replace('x')` on a `MutableBox<int> $box`, where
/// `replace(U $value)` declares `@psalm-this-out self<U>`, re-binds
/// `$box` to `MutableBox<string>` for whatever runs after the call, in
/// the same expression or after it.
///
/// Only fires for a receiver that is a plain variable already in scope
/// with a resolved class — `$this` is excluded because there is no
/// receiver variable to re-bind.
pub(crate) fn process_self_out_narrowing<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let (object, method, argument_list) = match expr {
        Expression::Call(Call::Method(mc)) => (mc.object, &mc.method, &mc.argument_list),
        Expression::Call(Call::NullSafeMethod(mc)) => (mc.object, &mc.method, &mc.argument_list),
        _ => return,
    };
    let ClassLikeMemberSelector::Identifier(ident) = method else {
        return;
    };
    let Expression::Variable(Variable::Direct(dv)) = object else {
        return;
    };
    let var_name = bytes_to_str(dv.name);
    if var_name == "$this" {
        return;
    }
    let method_name = bytes_to_str(ident.value).to_string();

    let before = scope.get(var_name).to_vec();
    if before.is_empty() {
        return;
    }
    // Cheap check before the template substitution machinery below runs:
    // bail out unless at least one branch's class actually declares a
    // self-out type for this method.
    if !before.iter().any(|rt| {
        rt.class_info
            .as_ref()
            .and_then(|c| c.get_method_ci(&method_name))
            .is_some_and(|m| m.self_out().is_some())
    }) {
        return;
    }

    let arg_texts = crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast(
        argument_list,
        ctx.file.content,
    );
    let arg_refs: Vec<&str> = arg_texts.iter().map(|s| s.as_str()).collect();

    let scope_resolver = scope.snapshot_resolver();
    let var_ctx = ctx.var_ctx_for_with_scope(
        var_name,
        ctx.cursor_offset,
        &scope_resolver,
        Some(scope.proofs()),
    );
    let rctx = var_ctx.as_resolution_ctx();

    let mut changed = false;
    let mut results: Vec<ResolvedType> = Vec::with_capacity(before.len());
    for rt in &before {
        let mutated = rt.class_info.as_ref().and_then(|owner| {
            let method = owner.get_method_ci(&method_name)?;
            let self_out = method.self_out().cloned()?;
            let mut template_subs = crate::type_engine::call_resolution::build_call_template_subs(
                owner,
                &method_name,
                &arg_refs,
                Some(&rt.type_string),
                &rctx,
            );
            for (name, ty) in template_subs.iter_mut() {
                let bound = method.template_param_bounds().get(&crate::atom::atom(name));
                *ty =
                    crate::type_engine::call_resolution::generalize_object_template_arg(ty, bound);
            }
            let substituted = self_out.substitute(&template_subs).simplified();
            let final_ty = if substituted.contains_self_ref() {
                substituted.replace_self_with_type(&rt.type_string)
            } else {
                substituted
            };
            // Re-resolve the class for the new type rather than keeping the
            // receiver's existing `class_info`: that one still carries the
            // *old* template substitution, so members typed by a template
            // parameter would keep resolving to the pre-call binding.
            let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
                &final_ty,
                &ctx.current_class.fqn(),
                ctx.file.all_classes,
                ctx.file.class_loader,
            );
            Some(if classes.is_empty() {
                vec![ResolvedType::from_type_string(final_ty)]
            } else {
                ResolvedType::from_classes_with_hint(classes, final_ty)
            })
        });
        match mutated {
            Some(new_rts) => {
                changed = true;
                results.extend(new_rts);
            }
            None => results.push(rt.clone()),
        }
    }

    if changed {
        scope.set(var_name, results);
    }
}

/// Subtract from each argument the values a `never` branch of the callee's
/// conditional return type rules out.
///
/// `throw_unless()`, `throw_if()`, `abort_unless()` and their family carry
/// no `@phpstan-assert` tag; they declare their effect in the return type
/// itself:
///
/// ```text
/// @return ($condition is false ? never : ($condition is non-empty-mixed ? TValue : never))
/// ```
///
/// A `null` argument lands on a `never` branch there, so a run that gets
/// past the call did not pass one, and the following scope can drop it.
/// This is the same subtraction the `if (!$x) { throw …; }` form already
/// gets, derived from the declaration instead of from a body.
fn apply_never_branch_narrowing<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let Expression::Call(call) = expr else {
        return;
    };

    let snapshot = scope.locals.clone();
    let resolver =
        |vn: &str| -> Vec<ResolvedType> { snapshot.get(&atom(vn)).cloned().unwrap_or_default() };
    let var_ctx = build_var_ctx("", ctx, &resolver);
    let Some(info) = condition_facts::extract_conditional_return_call(call, &var_ctx) else {
        return;
    };

    // A subject named by an argument is keyed the same way a condition's
    // subject is, so a property path or a call argument narrows too.
    seed_assert_arg_subject_keys(expr, scope, ctx);

    for (index, parameter) in info.parameters.iter().enumerate() {
        let Some(argument) = info.argument_list.arguments.iter().nth(index) else {
            continue;
        };
        let arg_expr = condition_facts::argument_value(argument);
        let Some(key) = condition_facts::expr_to_subject_key(arg_expr) else {
            continue;
        };
        let current = scope.get(&key).to_vec();
        if current.is_empty() {
            continue;
        }
        let joined = ResolvedType::types_joined(&current);
        let ruled_out = condition_facts::never_ruled_out_members(
            &info.return_type,
            &parameter.name,
            &joined,
            ctx.file.class_loader,
        );
        if ruled_out.is_empty() {
            continue;
        }
        let kept: Vec<ResolvedType> = current
            .into_iter()
            .filter(|rt| !ruled_out.iter().any(|out| out.equivalent(&rt.type_string)))
            .map(|mut rt| {
                if let Some(narrowed) = subtract_members(&rt.type_string, &ruled_out) {
                    rt.type_string = narrowed;
                }
                rt
            })
            .collect();
        if !kept.is_empty() {
            scope.set(&key, kept);
        }
    }
}

/// `ty` with every alternative `ruled_out` names removed, or `None` when
/// it names none of them.
///
/// A type with a single alternative is left alone: removing its only
/// member would leave nothing to describe the value with.
fn subtract_members(ty: &PhpType, ruled_out: &[PhpType]) -> Option<PhpType> {
    let members = condition_facts::split_into_runtime_members(ty);
    if members.len() < 2 {
        return None;
    }
    let kept: Vec<PhpType> = members
        .iter()
        .filter(|member| !ruled_out.iter().any(|out| out.equivalent(member)))
        .cloned()
        .collect();
    if kept.is_empty() || kept.len() == members.len() {
        return None;
    }
    Some(PhpType::union(kept))
}
