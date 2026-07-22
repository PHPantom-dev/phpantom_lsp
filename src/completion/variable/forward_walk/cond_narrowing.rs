use super::*;
use std::collections::HashMap;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::argument::Argument;

use crate::atom::{Atom, atom, bytes_to_str};
use crate::completion::resolver::VarResolutionCtx;
use crate::completion::types::narrowing;
use crate::php_type::PhpType;
use crate::types::{MethodInfo, PropertyInfo, ResolvedType};

// ─── Completion-path ternary/match(true) narrowing ──────────────────────────

/// Walk an expression tree looking for a `match(true)` arm or ternary
/// `instanceof` branch that contains the cursor.  When found, apply
/// the appropriate narrowing to `scope` so that variable lookups see
/// the narrowed type.
///
/// This is the completion-path counterpart of
/// [`record_match_ternary_snapshots`], which records scope snapshots
/// for the diagnostic path.  Here we modify the live scope in-place
/// because the completion path only needs one variable's type at one
/// cursor position.
pub(crate) fn apply_cursor_ternary_narrowing<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let cursor = ctx.cursor_offset;
    let span = expr.span();
    if cursor < span.start.offset || cursor > span.end.offset {
        return;
    }

    match expr {
        Expression::Match(match_expr) if match_expr.expression.is_true() => {
            for arm in match_expr.arms.iter() {
                match arm {
                    MatchArm::Expression(expr_arm) => {
                        let arm_span = expr_arm.expression.span();
                        if cursor >= arm_span.start.offset && cursor <= arm_span.end.offset {
                            for condition in expr_arm.conditions.iter() {
                                apply_condition_narrowing(condition, scope, ctx);
                            }
                            // Recurse into the arm body for nested patterns.
                            apply_cursor_ternary_narrowing(expr_arm.expression, scope, ctx);
                            return;
                        }
                    }
                    MatchArm::Default(def_arm) => {
                        let arm_span = def_arm.expression.span();
                        if cursor >= arm_span.start.offset && cursor <= arm_span.end.offset {
                            apply_cursor_ternary_narrowing(def_arm.expression, scope, ctx);
                            return;
                        }
                    }
                }
            }
        }
        Expression::Conditional(conditional) => {
            // Check if the condition contains an instanceof check or a
            // member-existence proof
            // (`property_exists`/`method_exists`/`isset($x->prop)`) for
            // any variable currently in scope.
            let has_narrowing = {
                let var_names: Vec<Atom> = scope.locals.keys().copied().collect();
                var_names.iter().any(|vn| {
                    narrowing::try_extract_instanceof(conditional.condition, vn).is_some()
                        || narrowing::try_extract_instanceof_with_negation(
                            conditional.condition,
                            vn,
                        )
                        .is_some()
                        || narrowing::try_extract_compound_or_instanceof(conditional.condition, vn)
                            .is_some()
                })
            } || condition_proves_member(conditional.condition, scope);
            if has_narrowing {
                if let Some(then_expr) = conditional.then {
                    let then_span = then_expr.span();
                    if cursor >= then_span.start.offset && cursor <= then_span.end.offset {
                        apply_condition_narrowing(conditional.condition, scope, ctx);
                        apply_cursor_ternary_narrowing(then_expr, scope, ctx);
                        return;
                    }
                }
                let else_span = conditional.r#else.span();
                if cursor >= else_span.start.offset && cursor <= else_span.end.offset {
                    apply_condition_narrowing_inverse(conditional.condition, scope, ctx);
                    apply_cursor_ternary_narrowing(conditional.r#else, scope, ctx);
                }
            } else {
                // No instanceof — just recurse for nested patterns.
                if let Some(then_expr) = conditional.then {
                    apply_cursor_ternary_narrowing(then_expr, scope, ctx);
                }
                apply_cursor_ternary_narrowing(conditional.r#else, scope, ctx);
            }
        }
        Expression::Assignment(assignment) => {
            apply_cursor_ternary_narrowing(assignment.rhs, scope, ctx);
        }
        Expression::Parenthesized(inner) => {
            apply_cursor_ternary_narrowing(inner.expression, scope, ctx);
        }
        Expression::Call(call) => {
            let args = match call {
                Call::Function(fc) => {
                    apply_cursor_ternary_narrowing(fc.function, scope, ctx);
                    &fc.argument_list
                }
                Call::Method(mc) => {
                    apply_cursor_ternary_narrowing(mc.object, scope, ctx);
                    &mc.argument_list
                }
                Call::NullSafeMethod(mc) => {
                    apply_cursor_ternary_narrowing(mc.object, scope, ctx);
                    &mc.argument_list
                }
                Call::StaticMethod(_) => return,
            };
            for arg in args.arguments.iter() {
                let arg_expr = match arg {
                    Argument::Positional(a) => a.value,
                    Argument::Named(a) => a.value,
                };
                apply_cursor_ternary_narrowing(arg_expr, scope, ctx);
            }
        }
        Expression::Binary(bin)
            if matches!(
                bin.operator,
                BinaryOperator::And(_) | BinaryOperator::LowAnd(_)
            ) =>
        {
            // `&&` chain: apply narrowing from LHS operands when the
            // cursor is in the RHS.  E.g. `$x instanceof Foo && $x->bar()`
            // narrows `$x` to `Foo` for the `$x->bar()` operand.
            let operands = collect_and_chain_operands(expr);
            if operands.len() >= 2 {
                let mut narrowed = false;
                for (i, operand) in operands.iter().enumerate() {
                    let op_span = operand.span();
                    if cursor >= op_span.start.offset && cursor <= op_span.end.offset {
                        // Cursor is inside this operand — apply
                        // narrowing from all preceding operands.
                        // (Already applied cumulatively in the loop.)
                        narrowed = true;
                        apply_cursor_ternary_narrowing(operand, scope, ctx);
                        break;
                    }
                    // Apply this operand's narrowing for subsequent operands.
                    if i < operands.len() - 1 {
                        apply_condition_narrowing(operand, scope, ctx);
                    }
                }
                if !narrowed {
                    // Cursor not inside any operand — just recurse.
                    apply_cursor_ternary_narrowing(bin.lhs, scope, ctx);
                    apply_cursor_ternary_narrowing(bin.rhs, scope, ctx);
                }
            } else {
                apply_cursor_ternary_narrowing(bin.lhs, scope, ctx);
                apply_cursor_ternary_narrowing(bin.rhs, scope, ctx);
            }
        }
        Expression::Binary(bin)
            if matches!(
                bin.operator,
                BinaryOperator::Or(_) | BinaryOperator::LowOr(_)
            ) =>
        {
            // `||` chain: the right operand executes only when the
            // preceding operands are false, so apply the *inverse*
            // narrowing from those operands when the cursor is in a
            // later operand.  E.g. `!$x instanceof Foo || $x->bar()`
            // narrows `$x` to `Foo` for the `$x->bar()` operand.
            let operands = collect_or_chain_operands(expr);
            if operands.len() >= 2 {
                let mut narrowed = false;
                for (i, operand) in operands.iter().enumerate() {
                    let op_span = operand.span();
                    if cursor >= op_span.start.offset && cursor <= op_span.end.offset {
                        narrowed = true;
                        apply_cursor_ternary_narrowing(operand, scope, ctx);
                        break;
                    }
                    // Apply this operand's inverse narrowing for the
                    // subsequent operands.
                    if i < operands.len() - 1 {
                        apply_condition_narrowing_inverse(operand, scope, ctx);
                    }
                }
                if !narrowed {
                    apply_cursor_ternary_narrowing(bin.lhs, scope, ctx);
                    apply_cursor_ternary_narrowing(bin.rhs, scope, ctx);
                }
            } else {
                apply_cursor_ternary_narrowing(bin.lhs, scope, ctx);
                apply_cursor_ternary_narrowing(bin.rhs, scope, ctx);
            }
        }
        Expression::Binary(bin) => {
            apply_cursor_ternary_narrowing(bin.lhs, scope, ctx);
            apply_cursor_ternary_narrowing(bin.rhs, scope, ctx);
        }
        // Non-`true` match expressions — recurse into arms.
        Expression::Match(match_expr) => {
            for arm in match_expr.arms.iter() {
                let arm_expr = match arm {
                    MatchArm::Expression(e) => e.expression,
                    MatchArm::Default(d) => d.expression,
                };
                apply_cursor_ternary_narrowing(arm_expr, scope, ctx);
            }
        }
        _ => {}
    }
}

// ─── Narrowing helpers ──────────────────────────────────────────────────────

/// Apply condition-based narrowing (instanceof, null check, type guard)
/// to the scope.  This narrows types for the "truthy" branch.
pub(crate) fn apply_condition_narrowing<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Seed property access keys from conditions into the scope so that
    // narrowing functions can find and narrow them.
    seed_property_keys_into_scope(condition, scope, ctx);

    // Decompose `&&` chains so that `$x instanceof Foo && $x instanceof Bar`
    // applies both narrowings as a union (intersection semantics: the
    // variable satisfies both checks, so members from both types are
    // available).
    let operands = collect_and_chain_operands(condition);

    // First pass: collect all instanceof extractions per variable across
    // all `&&` operands.  This prevents later operands from overwriting
    // earlier ones when both narrow the same variable.
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = |vn: &str| -> Vec<ResolvedType> {
        scope_snapshot.get(&atom(vn)).cloned().unwrap_or_default()
    };
    let mut var_names: Vec<String> = scope.locals.keys().map(|k| k.to_string()).collect();
    // Include variables from instanceof conditions that may not be in
    // scope yet (e.g. undeclared variables used in instanceof checks).
    for name in collect_condition_var_names(condition) {
        if !var_names.contains(&name) {
            var_names.push(name);
        }
    }
    // Include property access keys from conditions (e.g. `$a->foo`
    // from `$a->foo instanceof Foo`) so instanceof narrowing applies.
    for key in collect_condition_property_keys(condition) {
        if !var_names.contains(&key) {
            var_names.push(key);
        }
    }

    // Track which variables have been narrowed by instanceof across
    // `&&` operands so we can merge them into a union.
    let mut instanceof_results: HashMap<String, Vec<ResolvedType>> = HashMap::new();

    for operand in &operands {
        for var_name in &var_names {
            // Compound OR instanceof: `$x instanceof A || $x instanceof B`
            if let Some(classes) = narrowing::try_extract_compound_or_instanceof(operand, var_name)
                && !classes.is_empty()
            {
                let var_ctx = build_var_ctx(var_name, ctx, &scope_resolver);
                let union = narrowing::resolve_class_names_to_union(&classes, &var_ctx);
                if !union.is_empty() {
                    let entry = instanceof_results.entry(var_name.clone()).or_default();
                    ResolvedType::extend_unique(
                        entry,
                        union.into_iter().map(ResolvedType::from_class).collect(),
                    );
                }
                continue;
            }

            // Single instanceof (including negated, is_a, get_class).
            if let Some(extraction) =
                narrowing::try_extract_instanceof_with_negation(operand, var_name)
            {
                let var_ctx = build_var_ctx(var_name, ctx, &scope_resolver);
                if extraction.negated {
                    // Negated instanceof: apply exclusion to the current
                    // scope immediately (each negation removes one type).
                    let mut results = scope.get(var_name).to_vec();
                    ResolvedType::apply_narrowing(&mut results, |classes| {
                        narrowing::apply_instanceof_exclusion(
                            &extraction.class_type,
                            &var_ctx,
                            classes,
                        )
                    });
                    // Negated instanceof exclusion does NOT eliminate
                    // null — `!$x instanceof Foo` is true when $x is
                    // null, so null stays in the union.  No stripping.
                    if !results.is_empty() {
                        scope.set(var_name, results);
                    }
                } else {
                    // Positive instanceof: resolve and accumulate into
                    // the per-variable union.  For a single operand this
                    // produces `[Foo]`; for `&& instanceof Bar` it
                    // accumulates `[Foo, Bar]`.
                    let mut single = Vec::new();
                    ResolvedType::apply_narrowing(&mut single, |classes| {
                        narrowing::apply_instanceof_inclusion(
                            &extraction.class_type,
                            extraction.exact,
                            &var_ctx,
                            classes,
                        )
                    });
                    if !single.is_empty() {
                        let entry = instanceof_results.entry(var_name.clone()).or_default();
                        ResolvedType::extend_unique(entry, single);
                    } else {
                        // Target class is unresolvable — mark variable
                        // as empty so diagnostics suppress false positives.
                        instanceof_results.entry(var_name.clone()).or_default();
                    }
                }
            }
        }
    }

    // Apply the accumulated instanceof narrowing results to the scope.
    for (var_name, narrowed) in instanceof_results {
        if !narrowed.is_empty() {
            let existing = scope.get(&var_name);
            if existing.is_empty() {
                // Untyped variable — instanceof provides the type.
                scope.set(&var_name, narrowed);
            } else {
                // When the existing type is entirely `mixed` or
                // `object`, instanceof replaces it — there is no
                // useful information to preserve or intersect.
                let all_broad = existing.iter().all(|rt| {
                    rt.class_info.is_none()
                        && matches!(
                            rt.type_string.unwrap_nullable(),
                            PhpType::Named(n) if n.eq_ignore_ascii_case("mixed") || n.eq_ignore_ascii_case("object")
                        )
                });
                if all_broad {
                    scope.set(&var_name, narrowed);
                    continue;
                }

                // Typed variable — filter the existing union to only
                // types present in the narrowed set.  This correctly
                // handles both single instanceof (`Dog|Cat` → `Dog`)
                // and OR instanceof (`Dog|Cat|Other` → `Dog|Cat`).
                //
                // When the narrowed type is NOT in the existing union
                // (e.g. `MockInterface` narrowed to `MolliePayment`),
                // this is an intersection case — apply via
                // apply_instanceof_inclusion which has interface
                // intersection logic.
                let narrowed_fqns: Vec<String> = narrowed
                    .iter()
                    .filter_map(|rt| rt.class_info.as_ref().map(|c| c.fqn().to_string()))
                    .collect();

                // Try filtering: keep existing entries whose class is
                // in the narrowed set.  Strip null from the type_string
                // because a successful instanceof check guarantees the
                // value is non-null (e.g. `?Foo` → `Foo`).
                let filtered: Vec<ResolvedType> = existing
                    .iter()
                    .filter(|rt| {
                        rt.class_info
                            .as_ref()
                            .is_some_and(|c| narrowed_fqns.contains(&c.fqn().to_string()))
                    })
                    .map(|rt| {
                        if let Some(non_null) = rt.type_string.non_null_type() {
                            ResolvedType {
                                type_string: non_null,
                                class_info: rt.class_info.clone(),
                            }
                        } else {
                            rt.clone()
                        }
                    })
                    .collect();

                if !filtered.is_empty() {
                    // Filter matched — use the filtered results
                    // (preserves richer type info from original resolution).
                    // Also strip bare `null` entries: a successful
                    // instanceof check guarantees non-null, so `null`
                    // entries added by `from_classes_with_hint` must
                    // be removed.
                    let filtered: Vec<ResolvedType> = filtered
                        .into_iter()
                        .filter(|rt| !rt.type_string.is_null())
                        .collect();
                    if filtered.is_empty() {
                        scope.set(&var_name, narrowed);
                    } else {
                        scope.set(&var_name, filtered);
                    }
                } else {
                    // No overlap between existing and narrowed types.
                    // This is the intersection case (e.g. MockInterface
                    // narrowed to MolliePayment).  Use
                    // apply_instanceof_inclusion which produces the
                    // intersection when one side is an interface.
                    let mut results = existing.to_vec();
                    // Apply all narrowed classes as a single group by
                    // building a union type.
                    let union_type = if narrowed_fqns.len() == 1 {
                        PhpType::Named(narrowed_fqns[0].clone())
                    } else {
                        PhpType::Union(
                            narrowed_fqns
                                .iter()
                                .map(|n| PhpType::Named(n.clone()))
                                .collect(),
                        )
                    };
                    let var_ctx = build_var_ctx(&var_name, ctx, &scope_resolver);
                    ResolvedType::apply_narrowing(&mut results, |classes| {
                        narrowing::apply_instanceof_inclusion(&union_type, false, &var_ctx, classes)
                    });
                    // Instanceof guarantees non-null — strip bare
                    // `null` entries that were preserved by
                    // `apply_narrowing`'s `None => true` rule.
                    results.retain(|rt| !rt.type_string.is_null());
                    if !results.is_empty() {
                        scope.set(&var_name, results);
                    } else {
                        // Fallback: use the narrowed types directly.
                        scope.set(&var_name, narrowed);
                    }
                }
            }
        } else {
            // Empty narrowed list means the target was unresolvable.
            scope.locals.insert(atom(&var_name), vec![]);
        }
    }

    // Type guard narrowing: `is_object($x)`, `is_array($x)`, etc.
    apply_type_guard_narrowing_truthy(condition, scope);

    // `is_a($x, Class::class, true)` / `class_exists($x)` narrowing:
    // narrow a string-typed `$x` to `class-string<Class>` / `class-string`.
    apply_class_string_guard_narrowing(condition, scope, ctx, true);

    // Null narrowing: `if ($x !== null)` — remove null from scope.
    apply_null_narrowing_truthy(condition, scope, ctx);

    // @phpstan-assert-if-true / -if-false narrowing.
    apply_phpstan_assert_condition_narrowing(condition, scope, ctx, false);

    // in_array($var, $haystack, true) narrowing.
    apply_in_array_narrowing(condition, scope, ctx, false);

    // property_exists($var, 'name') / method_exists($var, 'name') narrowing.
    apply_member_exists_narrowing(condition, scope, false);
}

/// Apply inverse narrowing for a single condition expression (not
/// decomposed).  Called by [`apply_condition_narrowing_inverse`] for
/// each operand in a `&&` chain, or for the whole condition when it
/// is not a chain.
pub(crate) fn apply_condition_narrowing_inverse_single<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Seed property access keys from conditions into the scope so that
    // narrowing functions can find and narrow them.
    seed_property_keys_into_scope(condition, scope, ctx);

    let scope_snapshot = scope.locals.clone();
    let scope_resolver = |vn: &str| -> Vec<ResolvedType> {
        scope_snapshot.get(&atom(vn)).cloned().unwrap_or_default()
    };
    // Include variables from instanceof conditions that may not be in
    // scope yet (e.g. `if (!$foobar instanceof Foobar) { break; }`
    // where `$foobar` was never assigned).  After the guard clause,
    // `$foobar` must be `Foobar`.
    let mut var_names: Vec<String> = scope.locals.keys().map(|k| k.to_string()).collect();
    for name in collect_condition_var_names(condition) {
        if !var_names.contains(&name) {
            var_names.push(name);
        }
    }
    // Include property access keys from conditions (e.g. `$a->foo`
    // from `$a->foo instanceof Foo`) so instanceof narrowing applies.
    for key in collect_condition_property_keys(condition) {
        if !var_names.contains(&key) {
            var_names.push(key);
        }
    }
    for var_name in &var_names {
        if let Some(classes) = narrowing::try_extract_compound_or_instanceof(condition, var_name)
            && !classes.is_empty()
        {
            let var_ctx = build_var_ctx(var_name, ctx, &scope_resolver);
            let mut results = scope.get(var_name).to_vec();
            for cls_type in &classes {
                ResolvedType::apply_narrowing(&mut results, |class_list| {
                    narrowing::apply_instanceof_exclusion(cls_type, &var_ctx, class_list)
                });
            }
            if !results.is_empty() {
                scope.set(var_name, results);
            }
            continue;
        }

        if let Some(extraction) =
            narrowing::try_extract_instanceof_with_negation(condition, var_name)
        {
            let var_ctx = build_var_ctx(var_name, ctx, &scope_resolver);
            let mut results = scope.get(var_name).to_vec();
            if extraction.negated {
                // Inverse of negated instanceof → positive instanceof.
                // Instanceof guarantees non-null, so strip null entries.
                ResolvedType::apply_narrowing(&mut results, |classes| {
                    narrowing::apply_instanceof_inclusion(
                        &extraction.class_type,
                        extraction.exact,
                        &var_ctx,
                        classes,
                    )
                });
                results.retain(|rt| !rt.type_string.is_null());
            } else {
                // Inverse of positive instanceof → exclusion.
                // Exclusion does NOT strip null (`!instanceof` is
                // true for null values).
                ResolvedType::apply_narrowing(&mut results, |classes| {
                    narrowing::apply_instanceof_exclusion(&extraction.class_type, &var_ctx, classes)
                });
            }
            if !results.is_empty() {
                scope.set(var_name, results);
            }
        }
    }

    // Inverse member-existence narrowing: after a guard clause like
    // `if (!property_exists($x, 'name')) { return; }`, the member is
    // known to exist.
    apply_member_exists_narrowing(condition, scope, true);
}

/// Apply inverse condition-based narrowing (for else branches and
/// guard clauses).
pub(crate) fn apply_condition_narrowing_inverse<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Decompose `||` chains: NOT (A || B) = !A && !B.
    // Each operand's inverse is applied sequentially (intersection
    // semantics: all must hold simultaneously).
    let or_operands = collect_or_chain_operands(condition);
    if or_operands.len() > 1 {
        for operand in &or_operands {
            apply_condition_narrowing_inverse_single(operand, scope, ctx);
        }
        // Type guard, null, phpstan-assert, and in_array narrowing
        // operate on the full condition expression.
        apply_type_guard_narrowing_inverse(condition, scope);
        apply_class_string_guard_narrowing(condition, scope, ctx, false);
        apply_null_narrowing_inverse(condition, scope, ctx);
        apply_phpstan_assert_condition_narrowing(condition, scope, ctx, true);
        apply_in_array_narrowing(condition, scope, ctx, true);
        return;
    }

    // Decompose `&&` chains so that each operand is processed
    // individually.  For guard clauses like
    // `if (!$x instanceof A && !$x instanceof B) { return; }`,
    // the inverse (code after the guard) means `$x IS A || $x IS B`.
    //
    // De Morgan: NOT (!A && !B) = A || B.  Each operand's inverse
    // produces one branch of the union.  We clone the scope for each
    // operand, apply the inverse, then merge (union) all results back
    // into the main scope.
    let operands = collect_and_chain_operands(condition);
    if operands.len() > 1 {
        let base_scope = scope.clone();
        let mut branch_scopes: Vec<ScopeState> = Vec::new();
        for operand in &operands {
            let mut branch = base_scope.clone();
            apply_condition_narrowing_inverse_single(operand, &mut branch, ctx);
            branch_scopes.push(branch);
        }
        // Merge all branch scopes (union of all narrowed types).
        if let Some(first) = branch_scopes.first() {
            let mut merged = first.clone();
            for branch in &branch_scopes[1..] {
                merged.merge_branch(branch);
            }
            *scope = merged;
        }
        // Type guard, null, phpstan-assert, and in_array narrowing
        // operate on the full condition expression.
        apply_type_guard_narrowing_inverse(condition, scope);
        apply_class_string_guard_narrowing(condition, scope, ctx, false);
        apply_null_narrowing_inverse(condition, scope, ctx);
        apply_phpstan_assert_condition_narrowing(condition, scope, ctx, true);
        apply_in_array_narrowing(condition, scope, ctx, true);
        return;
    }

    apply_condition_narrowing_inverse_single(condition, scope, ctx);

    // Inverse type guard narrowing: `if (is_object($x))` in else → exclude object.
    apply_type_guard_narrowing_inverse(condition, scope);

    // Inverse class-string guard narrowing: `if (!is_a($x, Class::class, true))`
    // guard clause → after it, `$x` is a class-string of `Class`.
    apply_class_string_guard_narrowing(condition, scope, ctx, false);

    // Inverse null narrowing: `if ($x === null)` after guard → remove null.
    apply_null_narrowing_inverse(condition, scope, ctx);

    // Inverse @phpstan-assert-if-true / -if-false narrowing.
    apply_phpstan_assert_condition_narrowing(condition, scope, ctx, true);

    // Inverse in_array narrowing: exclude the element type in the else branch.
    apply_in_array_narrowing(condition, scope, ctx, true);
}

/// Report whether `condition` contains a member-existence proof for any
/// variable currently in `scope`: `property_exists($x, 'name')`,
/// `method_exists($x, 'name')`, or `isset($x->name)` (all recognised by
/// [`narrowing::try_extract_member_exists_guard`]).
///
/// Ternary branch narrowing runs only for conditions that add information
/// the guarded branch relies on.  Like `instanceof`, these guards qualify:
/// the then-branch of `property_exists($x, 'p') ? $x->p : …` depends on the
/// proof that `$x->p` exists.
pub(crate) fn condition_proves_member(condition: &Expression<'_>, scope: &ScopeState) -> bool {
    let var_names: Vec<Atom> = scope.locals.keys().copied().collect();
    collect_and_chain_operands(condition).iter().any(|operand| {
        var_names
            .iter()
            .any(|vn| narrowing::try_extract_member_exists_guard(operand, vn.as_str()).is_some())
    })
}

/// Apply `property_exists($var, 'name')` / `method_exists($var, 'name')`
/// narrowing to the scope.
///
/// In the branch where the guard holds, each class in the variable's
/// resolved union gains a virtual member of the guarded name (unknown
/// type), mirroring PHPStan's `object&hasProperty('name')` intersection.
/// Member access, completion, and hover inside the branch then treat the
/// member as present instead of reporting it unknown.
///
/// `inverted` is `false` for the truthy branch (a bare guard proves the
/// member exists) and `true` for the inverse path (else branch / after an
/// exiting guard clause), where the *negated* form proves it.
pub(crate) fn apply_member_exists_narrowing<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    inverted: bool,
) {
    for operand in collect_and_chain_operands(condition) {
        let var_names: Vec<Atom> = scope.locals.keys().copied().collect();
        for var_name in &var_names {
            let Some((member, is_method, negated)) =
                narrowing::try_extract_member_exists_guard(operand, var_name)
            else {
                continue;
            };
            // Only the direction where the guard is known TRUE adds
            // information — "the member does not exist" removes nothing
            // we model.
            if negated != inverted {
                continue;
            }

            let mut results = scope.get(var_name).to_vec();
            let mut changed = false;
            for rt in &mut results {
                let Some(class_info) = &rt.class_info else {
                    continue;
                };
                // Skip when the member is already declared on the class
                // itself — nothing to add, and injecting an untyped
                // virtual member would shadow the declared type.  Only
                // own members are checked (resolving ancestors here
                // would be expensive); guarding a *statically declared
                // inherited* member with `property_exists` is rare, and
                // the cost is an unknown member type inside the branch,
                // never a false diagnostic.
                let already_present = if is_method {
                    class_info.get_method_ci(&member).is_some()
                } else {
                    class_info
                        .properties
                        .iter()
                        .any(|p| p.name.as_str() == member)
                };
                if already_present {
                    continue;
                }
                let mut narrowed = (**class_info).clone();
                if is_method {
                    narrowed
                        .methods
                        .push(Arc::new(MethodInfo::virtual_method(&member, None)));
                } else {
                    narrowed
                        .properties
                        .push(PropertyInfo::virtual_property(&member, None));
                }
                rt.class_info = Some(Arc::new(narrowed));
                changed = true;
            }
            if changed {
                scope.set(var_name, results);
            }
        }
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
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = |vn: &str| -> Vec<ResolvedType> {
        scope_snapshot.get(&atom(vn)).cloned().unwrap_or_default()
    };

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
            } else {
                ResolvedType::apply_narrowing(&mut results, |classes| {
                    narrowing::apply_instanceof_inclusion(&element_type, false, &var_ctx, classes)
                });
            }

            if !results.is_empty() {
                scope.set(var_name, results);
            }
        }
    }
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
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = |vn: &str| -> Vec<ResolvedType> {
        scope_snapshot.get(&atom(vn)).cloned().unwrap_or_default()
    };
    let var_ctx = build_var_ctx("", ctx, &scope_resolver);
    let raw_type =
        crate::completion::variable::resolution::resolve_arg_raw_type(haystack_expr, &var_ctx);
    raw_type.and_then(|t| t.extract_element_type().cloned())
}

/// Apply null narrowing for the truthy branch.
/// Build a [`VarResolutionCtx`] from a variable name and forward-walk context.
///
/// Shared helper used by the narrowing functions in this module to avoid
/// repeating the struct construction at every call site.
/// Apply `@phpstan-assert-if-true` / `@phpstan-assert-if-false` narrowing
/// from a function or static/instance method call used as a condition.
///
/// When `inverted` is false we are in the truthy branch (then-body or
/// while-body).  When `inverted` is true we are in the else branch or
/// applying guard-clause inverse narrowing.
pub(crate) fn apply_phpstan_assert_condition_narrowing<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    inverted: bool,
) {
    use crate::types::AssertionKind;

    // Unwrap parentheses and detect negation (`!func($var)`).
    let (func_call_expr, condition_negated) = narrowing::unwrap_condition_negation(condition);

    let call = match func_call_expr {
        Expression::Call(c) => c,
        _ => return,
    };

    // Determine whether the function returned true in this branch.
    let function_returned_true = !(inverted ^ condition_negated);

    let scope_snapshot = scope.locals.clone();
    let scope_resolver = |vn: &str| -> Vec<ResolvedType> {
        scope_snapshot.get(&atom(vn)).cloned().unwrap_or_default()
    };

    // Try to extract assertion info from function calls and static method calls.
    match call {
        Call::Function(func_call) => {
            let func_name = match func_call.function {
                Expression::Identifier(ident) => bytes_to_str(ident.value()).to_string(),
                _ => return,
            };
            let func_info = match ctx.loaders.function_loader {
                Some(fl) => match fl(&func_name) {
                    Some(fi) => fi,
                    None => return,
                },
                None => return,
            };
            if func_info.type_assertions.is_empty() {
                return;
            }
            for assertion in &func_info.type_assertions {
                let applies_positively = match assertion.kind {
                    AssertionKind::IfTrue => function_returned_true,
                    AssertionKind::IfFalse => !function_returned_true,
                    AssertionKind::Always => continue,
                };
                if let Some(arg_var) = narrowing::find_assertion_arg_variable(
                    &func_call.argument_list,
                    &assertion.param_name,
                    &func_info.parameters,
                ) {
                    let should_exclude = assertion.negated ^ !applies_positively;
                    let var_ctx = build_var_ctx(&arg_var, ctx, &scope_resolver);
                    let mut results = scope.get(&arg_var).to_vec();
                    if should_exclude {
                        ResolvedType::apply_narrowing(&mut results, |classes| {
                            narrowing::apply_instanceof_exclusion(
                                &assertion.asserted_type,
                                &var_ctx,
                                classes,
                            )
                        });
                    } else {
                        ResolvedType::apply_narrowing(&mut results, |classes| {
                            narrowing::apply_instanceof_inclusion(
                                &assertion.asserted_type,
                                false,
                                &var_ctx,
                                classes,
                            )
                        });
                    }
                    if !results.is_empty() {
                        scope.set(&arg_var, results);
                    }
                }
            }
        }
        Call::StaticMethod(static_call) => {
            let method_name = match &static_call.method {
                ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value).to_string(),
                _ => return,
            };
            // Resolve the receiver to a class, handling `self`, `static`,
            // `parent`, and subclass names.
            let receiver = match static_call.class {
                Expression::Identifier(ident) => {
                    let name = bytes_to_str(ident.value());
                    let fqn = crate::util::resolve_name_via_loader(name, ctx.class_loader);
                    (ctx.class_loader)(&fqn).or_else(|| (ctx.class_loader)(name))
                }
                Expression::Self_(_) | Expression::Static(_) => {
                    (ctx.class_loader)(&ctx.current_class.name)
                }
                Expression::Parent(_) => match ctx.current_class.parent_class.as_ref() {
                    Some(parent) => (ctx.class_loader)(parent),
                    None => return,
                },
                _ => return,
            };
            let class_info = match receiver {
                Some(ci) => ci,
                None => return,
            };
            // Search the trait/parent chain so assertions declared on an
            // ancestor (e.g. PHPUnit's `Assert`) are found.  Uses raw class
            // loads only, avoiding a full merge that would poison the shared
            // resolved-class cache mid-walk.
            let method = match narrowing::find_assertion_method_in_chain(
                &class_info,
                &method_name,
                ctx.class_loader,
                &mut Vec::new(),
                0,
            ) {
                Some(m) => m,
                None => return,
            };
            for assertion in &method.type_assertions {
                let applies_positively = match assertion.kind {
                    AssertionKind::IfTrue => function_returned_true,
                    AssertionKind::IfFalse => !function_returned_true,
                    AssertionKind::Always => continue,
                };
                if let Some(arg_var) = narrowing::find_assertion_arg_variable(
                    &static_call.argument_list,
                    &assertion.param_name,
                    &method.parameters,
                ) {
                    let should_exclude = assertion.negated ^ !applies_positively;
                    // Resolve `self`/`static`/`$this` in the asserted type
                    // against the declaring class, not the enclosing class.
                    let resolved_assert_type = if assertion.asserted_type.contains_self_ref() {
                        assertion.asserted_type.replace_self(&class_info.fqn())
                    } else {
                        assertion.asserted_type.clone()
                    };
                    let var_ctx = build_var_ctx(&arg_var, ctx, &scope_resolver);
                    let mut results = scope.get(&arg_var).to_vec();
                    if should_exclude {
                        ResolvedType::apply_narrowing(&mut results, |classes| {
                            narrowing::apply_instanceof_exclusion(
                                &resolved_assert_type,
                                &var_ctx,
                                classes,
                            )
                        });
                    } else {
                        ResolvedType::apply_narrowing(&mut results, |classes| {
                            narrowing::apply_instanceof_inclusion(
                                &resolved_assert_type,
                                false,
                                &var_ctx,
                                classes,
                            )
                        });
                    }
                    if !results.is_empty() {
                        scope.set(&arg_var, results);
                    }
                }
            }
        }
        Call::Method(method_call) => {
            // Instance method: `$var->method()` with `@phpstan-assert-if-true Type $this`
            let receiver_var = match method_call.object {
                Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
                _ => return,
            };
            let method_name = match &method_call.method {
                ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value).to_string(),
                _ => return,
            };
            // Resolve the receiver's type to find the method's assertions.
            let receiver_types = scope.get(&receiver_var);
            if receiver_types.is_empty() {
                return;
            }
            // Collect assertions from all candidate classes.
            let mut to_apply: Vec<(crate::php_type::PhpType, bool, String)> = Vec::new();
            for rt in receiver_types {
                let receiver = match (ctx.class_loader)(&rt.type_string.to_string()) {
                    Some(ci) => ci,
                    None => {
                        continue;
                    }
                };
                // Search the trait/parent chain for the method's assertions
                // using raw class loads only (a full merge would poison the
                // shared resolved-class cache mid-walk).
                let method = match narrowing::find_assertion_method_in_chain(
                    &receiver,
                    &method_name,
                    ctx.class_loader,
                    &mut Vec::new(),
                    0,
                ) {
                    Some(m) => m,
                    None => continue,
                };
                for assertion in &method.type_assertions {
                    let applies_positively = match assertion.kind {
                        AssertionKind::IfTrue => function_returned_true,
                        AssertionKind::IfFalse => !function_returned_true,
                        AssertionKind::Always => continue,
                    };
                    let should_exclude = assertion.negated ^ !applies_positively;
                    // Resolve `self`/`static`/`$this` in the asserted type
                    // against the *declaring* class (e.g. `Decimal`), not the
                    // enclosing class (e.g. `Monetary`).  Without this,
                    // `@phpstan-assert-if-false self<true> $this` on
                    // `Decimal::isZero()` would narrow $denominator to
                    // `Monetary` instead of `Decimal`.
                    let resolved_type = if assertion.asserted_type.contains_self_ref() {
                        assertion.asserted_type.replace_self(&receiver.fqn())
                    } else {
                        assertion.asserted_type.clone()
                    };
                    if assertion.param_name == "$this" {
                        // Narrows the receiver variable itself.
                        to_apply.push((resolved_type, should_exclude, receiver_var.clone()));
                    } else if let Some(arg_var) = narrowing::find_assertion_arg_variable(
                        &method_call.argument_list,
                        &assertion.param_name,
                        &method.parameters,
                    ) {
                        to_apply.push((resolved_type, should_exclude, arg_var));
                    }
                }
            }
            for (asserted_type, should_exclude, target_var) in to_apply {
                let var_ctx = build_var_ctx(&target_var, ctx, &scope_resolver);
                let mut results = scope.get(&target_var).to_vec();
                if should_exclude {
                    ResolvedType::apply_narrowing(&mut results, |classes| {
                        narrowing::apply_instanceof_exclusion(&asserted_type, &var_ctx, classes)
                    });
                } else {
                    ResolvedType::apply_narrowing(&mut results, |classes| {
                        narrowing::apply_instanceof_inclusion(
                            &asserted_type,
                            false,
                            &var_ctx,
                            classes,
                        )
                    });
                }
                if !results.is_empty() {
                    scope.set(&target_var, results);
                }
            }
        }
        _ => {}
    }
}

pub(crate) fn build_var_ctx<'a>(
    var_name: &'a str,
    ctx: &'a ForwardWalkCtx<'_>,
    scope_resolver: &'a dyn Fn(&str) -> Vec<ResolvedType>,
) -> VarResolutionCtx<'a> {
    VarResolutionCtx {
        var_name,
        current_class: ctx.current_class,
        all_classes: ctx.all_classes,
        content: ctx.content,
        cursor_offset: ctx.cursor_offset,
        class_loader: ctx.class_loader,
        loaders: ctx.loaders,
        resolved_class_cache: ctx.resolved_class_cache,
        enclosing_return_type: ctx.enclosing_return_type.clone(),
        top_level_scope: ctx.top_level_scope.clone(),
        branch_aware: false,
        match_arm_narrowing: HashMap::new(),
        scope_var_resolver: Some(scope_resolver),
    }
}

///
/// Handles `$x !== null`, `$x != null`, `isset($x)`, `!empty($x)`,
/// `!is_null($x)`, and truthiness checks.
/// Apply type-guard narrowing in the truthy branch.
///
/// When `is_object($var)` (or `is_array`, `is_string`, etc.) appears
/// in a condition, narrow the variable's type.  For `mixed` variables,
/// this replaces `mixed` with the guard's canonical type (e.g. `object`).
/// For union types, it filters to only the members that match the guard.
///
/// Handles compound `&&` conditions by decomposing them into individual
/// operands and applying each type guard found.  For example,
/// `is_object($data) && property_exists($data, 'error_link')` applies
/// the `is_object` guard to `$data`.
pub(crate) fn apply_type_guard_narrowing_truthy(
    condition: &Expression<'_>,
    scope: &mut ScopeState,
) {
    apply_type_guard_on_operands(condition, scope, true);
}

/// Apply type-guard narrowing in the inverse (else) branch.
///
/// When `is_object($var)` appears in a condition, the else branch
/// knows the variable is NOT an object — filter out object-like
/// members from the union type.
pub(crate) fn apply_type_guard_narrowing_inverse(
    condition: &Expression<'_>,
    scope: &mut ScopeState,
) {
    apply_type_guard_on_operands(condition, scope, false);
}

/// Shared implementation for truthy and inverse type-guard narrowing.
///
/// Decomposes `&&` chains into individual operands and applies each
/// type guard found.  When `truthy` is `true`, applies inclusion
/// narrowing (then-body); when `false`, applies exclusion (else-body).
pub(crate) fn apply_type_guard_on_operands(
    condition: &Expression<'_>,
    scope: &mut ScopeState,
    truthy: bool,
) {
    // Decompose `&&` chains so that `is_object($x) && is_string($y)`
    // applies both guards.
    let operands = collect_and_chain_operands(condition);
    let mut var_names: Vec<String> = scope.locals.keys().map(|k| k.to_string()).collect();
    // Include property access keys from conditions (e.g. `$a->foo`
    // from `is_string($a->foo)`) so they can be narrowed.
    for key in collect_condition_property_keys(condition) {
        if !var_names.contains(&key) {
            var_names.push(key);
        }
    }
    for operand in &operands {
        for var_name in &var_names {
            if let Some((kind, negated)) = narrowing::try_extract_type_guard(operand, var_name) {
                // When the guard is negated (e.g. `!is_object($x)`),
                // flip the inclusion/exclusion logic: the truthy branch
                // of a negated guard means the variable is NOT the
                // guarded type, and vice versa.
                let effective_truthy = if negated { !truthy } else { truthy };
                let mut results = scope.get(var_name).to_vec();
                if !results.is_empty() {
                    if effective_truthy {
                        narrowing::apply_type_guard_inclusion(kind, &mut results);
                    } else {
                        narrowing::apply_type_guard_exclusion(kind, &mut results);
                    }
                    if !results.is_empty() {
                        scope.set(var_name, results);
                    }
                }
            }
        }
    }
}

/// Apply `is_a($x, Class::class, true)` / `class_exists($x)` (and the
/// other `*_exists()` forms) class-string narrowing.
///
/// When the guard's effective truth value is `true`, narrows string-like
/// (and `mixed`) entries in `$x`'s type to `class-string<Class>` (or
/// bare `class-string` for the generic `*_exists()` forms, which don't
/// name a specific class).  Negation is resolved by
/// `try_extract_class_string_guard`, so passing `truthy = false` here
/// from a guard-clause inverse correctly re-derives the truthy narrowing
/// for a negated condition (`if (!is_a(...)) { throw; }`).
///
/// Object-typed entries (with `class_info` set) are left untouched —
/// `is_a()`'s object side is already narrowed by the existing
/// instanceof-style handling, which operates independently on the
/// class-bearing entries.
pub(crate) fn apply_class_string_guard_narrowing<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    truthy: bool,
) {
    let operands = collect_and_chain_operands(condition);
    let mut var_names: Vec<String> = scope.locals.keys().map(|k| k.to_string()).collect();
    for key in collect_condition_property_keys(condition) {
        if !var_names.contains(&key) {
            var_names.push(key);
        }
    }
    for operand in &operands {
        for var_name in &var_names {
            if let Some((target, negated)) =
                narrowing::try_extract_class_string_guard(operand, var_name)
            {
                let effective_truthy = if negated { !truthy } else { truthy };
                if !effective_truthy {
                    continue;
                }
                // Seed compound subject keys (`$arr['class']`, `$obj->prop`)
                // so a class-string guard on an array-index or property
                // subject narrows just like one on a plain variable.  An
                // untyped array index seeds as `mixed`, which the loop below
                // narrows to `class-string<Class>`.
                seed_synthetic_key_if_needed(var_name, scope, ctx);
                let mut results = scope.get(var_name).to_vec();
                if results.is_empty() {
                    continue;
                }
                let resolved_fqn = target
                    .as_deref()
                    .map(|name| crate::util::resolve_name_via_loader(name, ctx.class_loader));
                let class_string_type = match &resolved_fqn {
                    Some(fqn) => PhpType::parse(&format!("class-string<{}>", fqn)),
                    None => PhpType::parse("class-string"),
                };
                let mut changed = false;
                for rt in results.iter_mut() {
                    if rt.class_info.is_some() {
                        continue;
                    }
                    // Never widen a type that is already at least as
                    // specific as the guard's result. The generic
                    // `*_exists()` forms narrow to bare `class-string`; a
                    // variable already typed `class-string<Foo>` must keep
                    // its type argument rather than be downgraded (a bare
                    // `class-string` is a supertype, so `new $var` could no
                    // longer recover the concrete class).
                    if rt.type_string.is_subtype_of(&class_string_type) {
                        continue;
                    }
                    if rt.type_string.is_subtype_of(&PhpType::string()) || rt.type_string.is_mixed()
                    {
                        rt.type_string = class_string_type.clone();
                        changed = true;
                    }
                }
                if changed {
                    scope.set(var_name, results);
                }
            }
        }
    }
}

pub(crate) fn apply_null_narrowing_truthy<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Decompose `&&` chains so that `isset($a) && isset($b)` narrows
    // both variables, and `$x !== null && $y !== null` works too.
    let operands = collect_and_chain_operands(condition);
    if operands.len() > 1 {
        for operand in &operands {
            apply_null_narrowing_truthy(operand, scope, ctx);
        }
        return;
    }

    // Check for `$x !== null` or `$x != null` or `null !== $x` etc.
    if let Some(var_name) = extract_non_null_check_var(condition) {
        // For array access keys, narrow the shape on the base variable.
        if let Some((base, key)) = split_array_access_key(&var_name) {
            strip_null_from_array_shape_key(base, key, scope);
        } else {
            seed_synthetic_key_if_needed(&var_name, scope, ctx);
            strip_null_from_scope(&var_name, scope);
        }
    }
    // `isset($x)` — truthy branch means $x is not null: strip null.
    // Handles multiple args: `isset($a, $b)` strips null from both.
    for var_name in extract_isset_vars(condition) {
        if let Some((base, key)) = split_array_access_key(&var_name) {
            strip_null_from_array_shape_key(base, key, scope);
        } else {
            seed_synthetic_key_if_needed(&var_name, scope, ctx);
            strip_null_from_scope(&var_name, scope);
        }
    }
    // `!isset($x)` — truthy branch means $x is null: narrow to null.
    for var_name in extract_not_isset_vars(condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        narrow_to_null_in_scope(&var_name, scope);
    }
    // Check for `$x === null` or `$x == null` — narrow to null only.
    if let Some(var_name) = extract_null_equality_check_var(condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        narrow_to_null_in_scope(&var_name, scope);
    }
    // `!empty($x)` — truthy branch means $x is non-empty (truthy):
    // strip null (and false) from the type.
    if let Some(var_name) = extract_not_empty_var(condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        strip_null_from_scope(&var_name, scope);
    }
    // Bare truthy check: `if ($x) { ... }` — $x is truthy in the
    // then-body, so strip null and false from its type.
    if let Some(var_name) =
        expr_to_var_name(condition).or_else(|| narrowing::expr_to_subject_key(condition))
    {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        strip_falsy_from_scope(&var_name, scope);
    }
}

/// Apply inverse null narrowing (for guard clause: `if ($x === null) { return; }`).
pub(crate) fn apply_null_narrowing_inverse<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // When the condition is `$x === null` (equality check for null),
    // the inverse (else/guard) means $x is NOT null.
    if let Some(var_name) = extract_null_equality_check_var(condition) {
        // For array access keys like `$a["test"]`, narrow the array
        // shape on the base variable directly rather than using a
        // synthetic scope entry.  This ensures the narrowed shape
        // survives scope merges.
        if let Some((base, key)) = split_array_access_key(&var_name) {
            strip_null_from_array_shape_key(base, key, scope);
        } else {
            seed_synthetic_key_if_needed(&var_name, scope, ctx);
            strip_null_from_scope(&var_name, scope);
        }
    }
    // When the condition is `$x !== null`, the inverse (else/guard)
    // means $x IS null — narrow to null only.
    if let Some(var_name) = extract_non_null_check_var(condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        narrow_to_null_in_scope(&var_name, scope);
    }
    // When the condition is `!$x` or `empty($x)`, the inverse means
    // $x is truthy — remove null.
    if let Some(var_name) = extract_falsy_check_var(condition) {
        strip_null_from_scope(&var_name, scope);
    }
    // When the condition is a bare `$x` (truthy check), the inverse means
    // $x is falsy.  For nullable types (`T|null`), narrow to null.
    // This handles `while ($a) { ... }` => after loop, $a is null.
    if let Some(var_name) = expr_to_var_name(condition) {
        narrow_to_null_in_scope(&var_name, scope);
    }
    // `isset($x)` — inverse (else) means $x was null: narrow to null.
    for var_name in extract_isset_vars(condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        narrow_to_null_in_scope(&var_name, scope);
    }
    // `!isset($x)` — inverse (guard after `!isset` return) means $x
    // is not null: strip null.
    for var_name in extract_not_isset_vars(condition) {
        if let Some((base, key)) = split_array_access_key(&var_name) {
            strip_null_from_array_shape_key(base, key, scope);
        } else {
            seed_synthetic_key_if_needed(&var_name, scope, ctx);
            strip_null_from_scope(&var_name, scope);
        }
    }
}

/// Extract variable name from `$x !== null` or `null !== $x` patterns.
pub(crate) fn extract_non_null_check_var(expr: &Expression<'_>) -> Option<String> {
    let (inner, negated) = narrowing::unwrap_condition_negation(expr);
    match inner {
        Expression::Binary(bin) => {
            let is_not_identical = matches!(bin.operator, BinaryOperator::NotIdentical(_));
            let is_not_equal = matches!(bin.operator, BinaryOperator::NotEqual(_));
            let is_identical = matches!(bin.operator, BinaryOperator::Identical(_));
            let is_equal = matches!(bin.operator, BinaryOperator::Equal(_));

            // `$x !== null` or `null !== $x`
            if (is_not_identical || is_not_equal) && !negated
                || (is_identical || is_equal) && negated
            {
                if is_null_expr(bin.rhs) {
                    return expr_to_var_name(bin.lhs)
                        .or_else(|| narrowing::expr_to_subject_key(bin.lhs));
                }
                if is_null_expr(bin.lhs) {
                    return expr_to_var_name(bin.rhs)
                        .or_else(|| narrowing::expr_to_subject_key(bin.rhs));
                }
            }
            None
        }
        _ => None,
    }
}

/// Extract all variable names from an `isset(…)` call (non-negated).
/// Handles simple variables (`$x`) and property/array access keys
/// (`$obj->prop`, `$arr["key"]`).  Returns an empty vec when the
/// expression is not an `isset()` call, or when it is negated.
pub(crate) fn extract_isset_vars(expr: &Expression<'_>) -> Vec<String> {
    let (inner, negated) = narrowing::unwrap_condition_negation(expr);
    if negated {
        return vec![];
    }
    // `isset()` is a language construct, parsed as Expression::Construct(Construct::Isset).
    let Expression::Construct(Construct::Isset(isset)) = inner else {
        return vec![];
    };
    let mut vars = Vec::new();
    for value in isset.values.iter() {
        if let Some(name) =
            expr_to_var_name(value).or_else(|| narrowing::expr_to_subject_key(value))
        {
            vars.push(name);
        }
    }
    vars
}

/// Extract all variable names from a `!isset(…)` call (negated isset).
/// Returns an empty vec when the expression is not a negated `isset()`.
pub(crate) fn extract_not_isset_vars(expr: &Expression<'_>) -> Vec<String> {
    let (inner, negated) = narrowing::unwrap_condition_negation(expr);
    if !negated {
        return vec![];
    }
    // `isset()` is a language construct, parsed as Expression::Construct(Construct::Isset).
    let Expression::Construct(Construct::Isset(isset)) = inner else {
        return vec![];
    };
    let mut vars = Vec::new();
    for value in isset.values.iter() {
        if let Some(name) =
            expr_to_var_name(value).or_else(|| narrowing::expr_to_subject_key(value))
        {
            vars.push(name);
        }
    }
    vars
}

/// Extract variable name from `$x === null` or `null === $x` patterns.
pub(crate) fn extract_null_equality_check_var(expr: &Expression<'_>) -> Option<String> {
    let (inner, negated) = narrowing::unwrap_condition_negation(expr);
    match inner {
        Expression::Binary(bin) => {
            let is_identical = matches!(bin.operator, BinaryOperator::Identical(_));
            let is_equal = matches!(bin.operator, BinaryOperator::Equal(_));

            if (is_identical || is_equal) && !negated {
                if is_null_expr(bin.rhs) {
                    return expr_to_var_name(bin.lhs)
                        .or_else(|| narrowing::expr_to_subject_key(bin.lhs));
                }
                if is_null_expr(bin.lhs) {
                    return expr_to_var_name(bin.rhs)
                        .or_else(|| narrowing::expr_to_subject_key(bin.rhs));
                }
            }
            None
        }
        _ => None,
    }
}

/// Extract variable name from `!empty($x)` (negated empty check).
pub(crate) fn extract_not_empty_var(expr: &Expression<'_>) -> Option<String> {
    if let Expression::UnaryPrefix(prefix) = expr
        && prefix.operator.is_not()
        && let Expression::Construct(Construct::Empty(empty)) = prefix.operand
    {
        return expr_to_var_name(empty.value);
    }
    None
}

/// Extract variable name from falsy checks: `!$x`, `empty($x)`.
pub(crate) fn extract_falsy_check_var(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            expr_to_var_name(prefix.operand)
        }
        // `empty($x)` — language construct, parsed as Expression::Construct(Construct::Empty).
        Expression::Construct(Construct::Empty(empty)) => expr_to_var_name(empty.value),
        _ => None,
    }
}

/// Check if an expression is `null`.
pub(crate) fn is_null_expr(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Literal(Literal::Null(_)) => true,
        Expression::ConstantAccess(ca) => {
            let name = ca.name.value();
            let clean = crate::util::strip_fqn_prefix(bytes_to_str(name));
            clean.eq_ignore_ascii_case("null")
        }
        _ => false,
    }
}

/// Extract a direct variable name from an expression.
pub(crate) fn expr_to_var_name(expr: &Expression<'_>) -> Option<String> {
    if let Expression::Variable(Variable::Direct(dv)) = expr {
        Some(bytes_to_str(dv.name).to_string())
    } else {
        None
    }
}

/// Strip `null` from a variable's type in the scope.
/// Narrow a variable in scope to `null` only.
///
/// Used when a condition like `$x === null` is true: the variable must
/// be null.  Replaces the variable's type with `null` if it currently
/// contains a nullable type, or sets it to `null` if the variable has
/// any type at all.
pub(crate) fn narrow_to_null_in_scope(var_name: &str, scope: &mut ScopeState) {
    let types = scope.get(var_name).to_vec();
    if types.is_empty() {
        return;
    }
    // Check whether any existing type contains null (Nullable, Union
    // with null member, or bare null).  `non_null_type()` returns
    // `Some` for `?T` and `T|null` unions; `is_null()` catches bare
    // `null`.
    let has_null = types
        .iter()
        .any(|rt| rt.type_string.non_null_type().is_some() || rt.type_string.is_null());
    if has_null {
        scope.set(
            var_name,
            vec![ResolvedType::from_type_string(PhpType::null())],
        );
    }
}

pub(crate) fn strip_null_from_scope(var_name: &str, scope: &mut ScopeState) {
    let types = scope.get(var_name).to_vec();
    if types.is_empty() {
        return;
    }

    let stripped: Vec<ResolvedType> = types
        .into_iter()
        .filter_map(|mut rt| match rt.type_string.non_null_type() {
            Some(non_null) => {
                rt.type_string = non_null;
                Some(rt)
            }
            None if rt.type_string == PhpType::null() => None,
            None => Some(rt),
        })
        .collect();

    if !stripped.is_empty() {
        scope.set(var_name, stripped);
    }
}

/// Strip both `null` and `false` from a variable's type in the scope.
///
/// Used after falsy guard clauses (`if (!$var) { throw; }`) where the
/// variable is known to be truthy (non-null and non-false) after the guard.
pub(crate) fn strip_falsy_from_scope(var_name: &str, scope: &mut ScopeState) {
    let types = scope.get(var_name).to_vec();
    if types.is_empty() {
        return;
    }

    let is_false = |t: &PhpType| matches!(t, PhpType::Named(n) if n == "false");

    let stripped: Vec<ResolvedType> = types
        .into_iter()
        .filter_map(|mut rt| {
            // Strip null
            let ty = match rt.type_string.non_null_type() {
                Some(non_null) => non_null,
                None if rt.type_string == PhpType::null() => return None,
                None => rt.type_string.clone(),
            };
            // Strip false
            if is_false(&ty) {
                return None;
            }
            let ty = match &ty {
                PhpType::Union(members) => {
                    let non_false: Vec<PhpType> =
                        members.iter().filter(|m| !is_false(m)).cloned().collect();
                    match non_false.len() {
                        0 => return None,
                        1 => non_false.into_iter().next().unwrap(),
                        _ => PhpType::Union(non_false),
                    }
                }
                _ => ty,
            };
            rt.type_string = ty;
            Some(rt)
        })
        .collect();

    if !stripped.is_empty() {
        scope.set(var_name, stripped);
    }
}

/// Split a single-level array access key like `$a["test"]` into base
/// variable and key name.  Returns `None` for non-array-access keys and
/// for multi-level access (`$a["x"]["y"]`), which this single-key
/// narrowing cannot represent and would otherwise mis-split.
pub(crate) fn split_array_access_key(key: &str) -> Option<(&str, &str)> {
    let bracket_pos = key.find("[\"")?;
    let base = &key[..bracket_pos];
    // The base must be a plain expression with no earlier array access.
    if base.contains('[') {
        return None;
    }
    let key_name = key[bracket_pos + 2..].strip_suffix("\"]")?;
    // A nested access leaves bracket characters inside the extracted key
    // (e.g. `x"]["y`); reject it rather than narrowing a bogus key.
    if key_name.contains('[') || key_name.contains(']') {
        return None;
    }
    Some((base, key_name))
}

/// Strip `null` from a specific array shape key on a variable.
///
/// Given variable `$a` typed as `array{test: ?int}` and key `"test"`,
/// rewrites the variable's type to `array{test: int}`.  This modifies
/// the base variable's type directly so the narrowed shape survives
/// scope merges (unlike synthetic scope entries which are stripped).
pub(crate) fn strip_null_from_array_shape_key(
    base_var: &str,
    key_name: &str,
    scope: &mut ScopeState,
) {
    let types = scope.get(base_var).to_vec();
    if types.is_empty() {
        return;
    }
    let narrowed: Vec<ResolvedType> = types
        .into_iter()
        .map(|mut rt| {
            rt.type_string = strip_null_from_shape_key(&rt.type_string, key_name);
            rt
        })
        .collect();
    scope.set(base_var, narrowed);
}

/// Recursively strip `null` from a specific key in an array shape type.
pub(crate) fn strip_null_from_shape_key(
    ty: &crate::php_type::PhpType,
    key: &str,
) -> crate::php_type::PhpType {
    use crate::php_type::{PhpType, ShapeEntry};
    match ty {
        PhpType::ArrayShape(entries) => {
            let new_entries: Vec<ShapeEntry> = entries
                .iter()
                .map(|e| {
                    if e.key.as_deref() == Some(key) {
                        let non_null = e
                            .value_type
                            .non_null_type()
                            .unwrap_or_else(|| e.value_type.clone());
                        ShapeEntry {
                            key: e.key.clone(),
                            value_type: non_null,
                            optional: false, // known to be present (was checked)
                        }
                    } else {
                        e.clone()
                    }
                })
                .collect();
            PhpType::ArrayShape(new_entries)
        }
        PhpType::Nullable(inner) => {
            // `?array{test: ?int}` → `?array{test: int}`
            PhpType::Nullable(Box::new(strip_null_from_shape_key(inner, key)))
        }
        PhpType::Union(members) => {
            let new_members: Vec<PhpType> = members
                .iter()
                .map(|m| strip_null_from_shape_key(m, key))
                .collect();
            PhpType::Union(new_members)
        }
        other => other.clone(),
    }
}

pub(crate) fn apply_guard_clause_null_narrowing<'b>(
    if_stmt: &'b If<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // When `if ($x === null) { return; }`, strip null from $x after.
    // When `if (!$x) { return; }`, strip null from $x after.
    if let Some(var_name) = extract_null_equality_check_var(if_stmt.condition) {
        if let Some((base, key)) = split_array_access_key(&var_name) {
            strip_null_from_array_shape_key(base, key, scope);
        } else {
            seed_synthetic_key_if_needed(&var_name, scope, ctx);
            strip_null_from_scope(&var_name, scope);
        }
    }
    if let Some(var_name) = extract_falsy_check_var(if_stmt.condition) {
        strip_falsy_from_scope(&var_name, scope);
    }
    // `if (!isset($x)) { return; }` — after the guard, $x is not null.
    for var_name in extract_not_isset_vars(if_stmt.condition) {
        if let Some((base, key)) = split_array_access_key(&var_name) {
            strip_null_from_array_shape_key(base, key, scope);
        } else {
            seed_synthetic_key_if_needed(&var_name, scope, ctx);
            strip_null_from_scope(&var_name, scope);
        }
    }
    // `if ($x !== null)` with return doesn't narrow after — the
    // remaining code is the null path.  This is handled by the
    // inverse narrowing in the guard clause logic.
}

/// Process assignment in a condition: `if ($x = expr())`
pub(crate) fn process_condition_assignment<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Direct assignment: `if ($x = expr())`
    if let Expression::Assignment(assignment) = condition
        && assignment.operator.is_assign()
        && let Expression::Variable(Variable::Direct(dv)) = assignment.lhs
    {
        let var_name = bytes_to_str(dv.name).to_string();
        let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
        if !rhs_types.is_empty() {
            scope.set(&var_name, rhs_types);
        }
        return;
    }
    // Parenthesized conditions: `if (($x = expr()))`
    if let Expression::Parenthesized(inner) = condition {
        process_condition_assignment(inner.expression, scope, ctx);
        return;
    }
    // Negated (or otherwise unary-prefixed) conditions:
    //   `if (!$x = expr()) { return; }` — PHP parses this as
    //   `!($x = expr())`.  Recurse into the operand.
    if let Expression::UnaryPrefix(prefix) = condition {
        process_condition_assignment(prefix.operand, scope, ctx);
        return;
    }
    // Assignment inside a binary comparison or logical chain:
    //   `if (($x = expr()) !== null)`, `if (null !== ($x = expr()))`,
    //   `while (($x = next()) && $x->valid())`.  Recurse into both
    //   operands so the assignment on either side is seen.
    if let Expression::Binary(bin) = condition {
        process_condition_assignment(bin.lhs, scope, ctx);
        process_condition_assignment(bin.rhs, scope, ctx);
        return;
    }
    // Assignment wrapped in a call argument:
    //   `while (is_object($token = $tokenizer->next()))`.  Recurse
    //   into each argument value so the assignment is registered.
    if let Expression::Call(call) = condition {
        let arg_list = match call {
            Call::Function(fc) => &fc.argument_list,
            Call::Method(mc) => &mc.argument_list,
            Call::NullSafeMethod(mc) => &mc.argument_list,
            Call::StaticMethod(sc) => &sc.argument_list,
        };
        for arg in arg_list.arguments.iter() {
            let arg_expr = match arg {
                Argument::Positional(a) => a.value,
                Argument::Named(a) => a.value,
            };
            process_condition_assignment(arg_expr, scope, ctx);
        }
    }
}

/// Extract variable names referenced in instanceof / is_a / get_class
/// conditions.  This catches variables that are not yet in scope but
/// are used in guard clauses like `if (!$x instanceof Foo) { return; }`.
pub(crate) fn collect_condition_var_names(expr: &Expression<'_>) -> Vec<String> {
    let mut names = Vec::new();
    collect_condition_var_names_inner(expr, &mut names);
    names
}

/// Remove synthetic property/array access keys from the scope.
/// Called after loop merges and other scope transitions where
/// condition-based narrowing no longer holds.
///
/// Synthetic keys contain `->` (property access) or `["` (array access).
pub(crate) fn strip_synthetic_property_keys(scope: &mut ScopeState) {
    scope
        .locals
        .retain(|key, _| !key.contains("->") && !key.contains("[\""));
}

/// Seed a synthetic scope entry for a compound key (property access
/// or array access) if it isn't already present.  Simple variable
/// names (no `->` or `["`) are skipped since they are already tracked.
pub(crate) fn seed_synthetic_key_if_needed(
    key: &str,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Only seed compound keys (property access or array access).
    let is_property = key.contains("->");
    let is_array = key.contains("[\"");
    if !is_property && !is_array {
        return;
    }
    if scope.contains(key) {
        return;
    }

    if is_property {
        // Property access: delegate to existing seeding logic via a
        // one-key call (seed_property_keys_into_scope expects a
        // condition expression, but we already have the key).
        if let Some(arrow_pos) = key.rfind("->") {
            let obj_var = &key[..arrow_pos];
            let prop_name = &key[arrow_pos + 2..];
            let obj_types = scope.get(obj_var);
            if obj_types.is_empty() {
                return;
            }
            let mut prop_results: Vec<ResolvedType> = Vec::new();
            for rt in obj_types {
                if let Some(ref cls) = rt.class_info {
                    let type_hint = crate::inheritance::resolve_property_type_hint(
                        cls,
                        prop_name,
                        ctx.class_loader,
                    );
                    if let Some(hint) = type_hint {
                        let resolved_classes =
                            crate::completion::type_resolution::type_hint_to_classes_typed(
                                &hint,
                                &ctx.current_class.name,
                                ctx.all_classes,
                                ctx.class_loader,
                            );
                        if resolved_classes.is_empty() {
                            ResolvedType::extend_unique(
                                &mut prop_results,
                                vec![ResolvedType::from_type_string(hint)],
                            );
                        } else {
                            ResolvedType::extend_unique(
                                &mut prop_results,
                                ResolvedType::from_classes_with_hint(resolved_classes, hint),
                            );
                        }
                    }
                }
            }
            if !prop_results.is_empty() {
                scope.set(key, prop_results);
            }
        }
    } else if is_array {
        // Array access key: `$a["test"]`.
        // Extract the base variable and key name.
        if let Some(bracket_pos) = key.find("[\"") {
            let base_var = &key[..bracket_pos];
            let key_name = key[bracket_pos + 2..]
                .strip_suffix("\"]")
                .unwrap_or(&key[bracket_pos + 2..]);
            let base_types = scope.get(base_var);
            if base_types.is_empty() {
                return;
            }
            // Look up the array key's type.  Prefer a precise shape entry
            // (`array{class: Foo}`); fall back to the generic element type
            // (`array<string, Foo>` → `Foo`); and finally to `mixed` for an
            // untyped array (plain `array`).  Seeding the untyped case is
            // what lets assertion / class-string narrowing apply to an
            // array-index subject whose element type is otherwise unknown
            // (e.g. `assertInstanceOf(X::class, $arr['k'])`).
            let mut key_results: Vec<ResolvedType> = Vec::new();
            for rt in base_types {
                let element_type = rt
                    .type_string
                    .extract_shape_key_type(key_name)
                    .or_else(|| rt.type_string.extract_value_type(false).cloned())
                    .or_else(|| rt.type_string.is_array_like().then(PhpType::mixed));
                let Some(element_type) = element_type else {
                    continue;
                };
                let resolved_classes =
                    crate::completion::type_resolution::type_hint_to_classes_typed(
                        &element_type,
                        &ctx.current_class.name,
                        ctx.all_classes,
                        ctx.class_loader,
                    );
                if resolved_classes.is_empty() {
                    ResolvedType::extend_unique(
                        &mut key_results,
                        vec![ResolvedType::from_type_string(element_type)],
                    );
                } else {
                    ResolvedType::extend_unique(
                        &mut key_results,
                        ResolvedType::from_classes_with_hint(resolved_classes, element_type),
                    );
                }
            }
            if !key_results.is_empty() {
                scope.set(key, key_results);
            }
        }
    }
}

/// Seed property/array-access subject keys that appear as arguments to a
/// call expression into the scope.
///
/// Used for assertion narrowing on non-variable subjects, e.g.
/// `assertInstanceOf(X::class, $view->component)` or a `@phpstan-assert`
/// helper invoked on `$arg->value`.  Each argument that resolves to a
/// compound subject key (property path or array access) is seeded with
/// its current type so the assertion narrowing loop can narrow it.
pub(crate) fn seed_assert_arg_subject_keys(
    expr: &Expression<'_>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let expr = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    let Expression::Call(call) = expr else {
        return;
    };
    let argument_list = match call {
        Call::Function(fc) => &fc.argument_list,
        Call::Method(mc) => &mc.argument_list,
        Call::NullSafeMethod(mc) => &mc.argument_list,
        Call::StaticMethod(sc) => &sc.argument_list,
    };
    for arg in argument_list.arguments.iter() {
        let arg_expr = match arg {
            Argument::Positional(pos) => pos.value,
            Argument::Named(named) => named.value,
        };
        if let Some(key) = narrowing::expr_to_subject_key(arg_expr)
            && (key.contains("->") || key.contains("[\""))
        {
            seed_synthetic_key_if_needed(&key, scope, ctx);
        }
    }
}

/// Collect property access keys (e.g. `$a->foo`) from conditions that
/// contain type guards or instanceof checks on property accesses.
/// These keys are injected into the scope so that narrowing applies.
pub(crate) fn collect_condition_property_keys(expr: &Expression<'_>) -> Vec<String> {
    let mut keys = Vec::new();
    collect_condition_property_keys_inner(expr, &mut keys);
    keys
}

pub(crate) fn collect_condition_property_keys_inner(expr: &Expression<'_>, keys: &mut Vec<String>) {
    match expr {
        // instanceof: `$a->foo instanceof Foo` or `$row["page"] instanceof Foo`
        Expression::Binary(bin) if bin.operator.is_instanceof() => {
            if let Some(key) = narrowing::expr_to_subject_key(bin.lhs)
                && (key.contains("->") || key.contains("[\""))
                && !keys.contains(&key)
            {
                keys.push(key);
            }
        }
        // Negation: `!is_string($a->foo)`, `!($a->foo instanceof Foo)`
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            collect_condition_property_keys_inner(prefix.operand, keys);
        }
        Expression::Parenthesized(p) => {
            collect_condition_property_keys_inner(p.expression, keys);
        }
        // Logical connectives
        Expression::Binary(bin)
            if matches!(
                bin.operator,
                BinaryOperator::And(_)
                    | BinaryOperator::LowAnd(_)
                    | BinaryOperator::Or(_)
                    | BinaryOperator::LowOr(_)
            ) =>
        {
            collect_condition_property_keys_inner(bin.lhs, keys);
            collect_condition_property_keys_inner(bin.rhs, keys);
        }
        // Type guard functions: `is_string($a->foo)`, `is_int($a->foo)`, etc.
        Expression::Call(Call::Function(func_call)) => {
            if let Expression::Identifier(ident) = func_call.function {
                let func_name = bytes_to_str(ident.value());
                let is_type_guard = matches!(
                    func_name,
                    "is_array"
                        | "is_string"
                        | "is_int"
                        | "is_integer"
                        | "is_long"
                        | "is_float"
                        | "is_double"
                        | "is_real"
                        | "is_bool"
                        | "is_object"
                        | "is_numeric"
                        | "is_callable"
                        | "is_null"
                        | "is_scalar"
                        | "is_a"
                        | "class_exists"
                        | "interface_exists"
                        | "enum_exists"
                        | "trait_exists"
                );
                if is_type_guard && let Some(first_arg) = func_call.argument_list.arguments.first()
                {
                    let arg_expr = match first_arg {
                        Argument::Positional(pos) => pos.value,
                        Argument::Named(named) => named.value,
                    };
                    if let Some(key) = narrowing::expr_to_subject_key(arg_expr)
                        && (key.contains("->") || key.contains("[\""))
                        && !keys.contains(&key)
                    {
                        keys.push(key);
                    }
                }
            }
        }
        _ => {}
    }
}

/// Resolve the type of a property access key (e.g. `$a->foo`) from
/// the current scope and seed it into the scope as a synthetic entry.
/// This allows subsequent narrowing functions to find and narrow
/// property access expressions.
pub(crate) fn seed_property_keys_into_scope(
    condition: &Expression<'_>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let keys = collect_condition_property_keys(condition);
    if keys.is_empty() {
        return;
    }
    for key in &keys {
        // Skip if already seeded (e.g. from a prior elseif condition).
        if scope.contains(key) {
            continue;
        }
        // Parse the key to extract object variable and property name.
        // Key format: `$var->prop` (possibly chained like `$a->b->c`).
        if let Some(arrow_pos) = key.rfind("->") {
            let obj_var = &key[..arrow_pos];
            let prop_name = &key[arrow_pos + 2..];

            // Resolve the object variable's type from scope.
            let obj_types = scope.get(obj_var);
            if obj_types.is_empty() {
                continue;
            }

            // Look up the property type on the resolved class(es).
            let mut prop_results: Vec<ResolvedType> = Vec::new();
            for rt in obj_types {
                if let Some(ref cls) = rt.class_info {
                    let type_hint = crate::inheritance::resolve_property_type_hint(
                        cls,
                        prop_name,
                        ctx.class_loader,
                    );
                    if let Some(hint) = type_hint {
                        let resolved_classes =
                            crate::completion::type_resolution::type_hint_to_classes_typed(
                                &hint,
                                &ctx.current_class.name,
                                ctx.all_classes,
                                ctx.class_loader,
                            );
                        if resolved_classes.is_empty() {
                            ResolvedType::extend_unique(
                                &mut prop_results,
                                vec![ResolvedType::from_type_string(hint)],
                            );
                        } else {
                            ResolvedType::extend_unique(
                                &mut prop_results,
                                ResolvedType::from_classes_with_hint(resolved_classes, hint),
                            );
                        }
                    }
                }
            }

            if !prop_results.is_empty() {
                scope.set(key, prop_results);
            }
        }
    }
}

pub(crate) fn collect_condition_var_names_inner(expr: &Expression<'_>, names: &mut Vec<String>) {
    match expr {
        Expression::Binary(bin) if bin.operator.is_instanceof() => {
            if let Expression::Variable(Variable::Direct(dv)) = bin.lhs {
                let name = bytes_to_str(dv.name).to_string();
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            collect_condition_var_names_inner(prefix.operand, names);
        }
        Expression::Parenthesized(p) => {
            collect_condition_var_names_inner(p.expression, names);
        }
        Expression::Binary(bin)
            if matches!(
                bin.operator,
                BinaryOperator::And(_)
                    | BinaryOperator::LowAnd(_)
                    | BinaryOperator::Or(_)
                    | BinaryOperator::LowOr(_)
            ) =>
        {
            collect_condition_var_names_inner(bin.lhs, names);
            collect_condition_var_names_inner(bin.rhs, names);
        }
        // is_a($var, ...) and get_class($var) === ...
        Expression::Call(Call::Function(func_call)) => {
            let func_name = match func_call.function {
                Expression::Identifier(ident) => bytes_to_str(ident.value()),
                _ => return,
            };
            if matches!(
                func_name,
                "is_a"
                    | "get_class"
                    | "class_exists"
                    | "interface_exists"
                    | "enum_exists"
                    | "trait_exists"
            ) && let Some(first_arg) = func_call.argument_list.arguments.first()
            {
                let arg_expr = match first_arg {
                    Argument::Positional(pos) => pos.value,
                    Argument::Named(named) => named.value,
                };
                if let Expression::Variable(Variable::Direct(dv)) = arg_expr {
                    let name = bytes_to_str(dv.name).to_string();
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
            }
        }
        _ => {}
    }
}

/// Check if a statement unconditionally exits (return/throw/continue/break).
pub(crate) fn statement_unconditionally_exits(stmt: &Statement<'_>) -> bool {
    narrowing::statement_unconditionally_exits(stmt)
}

/// Check whether a statement exits via `break` or `continue` (loop-local
/// exit) rather than `return` or `throw` (function exit).
///
/// When an if-branch exits via `break`/`continue`, the variable
/// assignments made in that branch still flow to the post-loop scope.
/// The if-merge should include these branch scopes in the surviving
/// set so that the merged post-if scope reflects the assignments.
pub(crate) fn exits_via_loop_control(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::Break(_) | Statement::Continue(_) => true,
        Statement::Block(block) => block.statements.last().is_some_and(exits_via_loop_control),
        _ => false,
    }
}
#[cfg(test)]
mod tests {
    use super::split_array_access_key;

    #[test]
    fn splits_single_level_string_key() {
        assert_eq!(split_array_access_key("$a[\"test\"]"), Some(("$a", "test")));
    }

    #[test]
    fn rejects_non_array_access() {
        assert_eq!(split_array_access_key("$a"), None);
    }

    #[test]
    fn rejects_nested_array_access() {
        // `$a["x"]["y"]` must not be mis-split into base `$a` and key
        // `x"]["y`; single-key narrowing cannot represent it.
        assert_eq!(split_array_access_key("$a[\"x\"][\"y\"]"), None);
    }

    #[test]
    fn rejects_base_with_earlier_access() {
        assert_eq!(split_array_access_key("$a[0][\"y\"]"), None);
    }
}
