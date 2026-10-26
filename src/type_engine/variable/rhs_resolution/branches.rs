//! Expressions whose value comes from one of several branches: the
//! ternary and the chains of them nested in each other's `else`, `??`
//! chains, and `match`.
//!
//! Each branch is resolved with the narrowing its condition establishes, as
//! an `if` body is, and the branches that can be reached are unioned.

use std::collections::HashMap;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::{atom, bytes_to_str};
use crate::php_type::PhpType;
use crate::type_engine::resolver::VarResolutionCtx;
use crate::type_engine::type_resolution;
use crate::types::ResolvedType;

use super::{peel_type_transparent, resolve_rhs_expression};

/// Collapse only semantically redundant alternatives after joining control-flow
/// expression branches. Exact literal-only unions remain untouched.
fn simplify_branch_results(results: Vec<ResolvedType>) -> Vec<ResolvedType> {
    let mut results = ResolvedType::collapse_redundant_runtime_literals(results);
    // `mixed_absorbs_siblings: false` — a ternary/match arm that only
    // resolves to `mixed` (an unresolved call) must not erase a sibling
    // arm's real, narrower answer; see `drop_subsumed_entries`.
    ResolvedType::drop_subsumed_entries(&mut results, false);
    results
}

/// PHP's runtime truthiness for a ternary condition, when it is a literal
/// whose truthiness is knowable at parse time.
///
/// Returns `None` for anything that isn't a bare scalar literal (a variable,
/// a call, a comparison, ...), so the caller falls back to unioning both arms.
fn static_condition_truthiness(expr: &Expression) -> Option<bool> {
    match expr {
        Expression::Parenthesized(inner) => static_condition_truthiness(inner.expression),
        Expression::Literal(Literal::True(_)) => Some(true),
        Expression::Literal(Literal::False(_)) => Some(false),
        Expression::Literal(Literal::Null(_)) => Some(false),
        Expression::Literal(Literal::Integer(integer)) => integer.value.map(|value| value != 0),
        Expression::Literal(Literal::Float(float)) => Some(float.value.into_inner() != 0.0),
        Expression::Literal(Literal::String(string)) => string.value.and_then(|bytes| {
            std::str::from_utf8(bytes)
                .ok()
                .map(|value| !value.is_empty() && value != "0")
        }),
        _ => None,
    }
}

/// Resolve a `??` chain, unioning every operand that can still be
/// reached at runtime.
///
/// `??` is right-associative, so `$a ?? $b ?? $c` nests as
/// `$a ?? ($b ?? $c)`; the chain is walked down its right spine rather
/// than recursed into.  Each operand contributes its type with `null`
/// stripped (the next operand covers the null case), an operand that
/// resolves to nothing contributes `mixed` (at runtime it holds *some*
/// value), and an operand that cannot be null at all makes the rest of
/// the chain dead code.
pub(super) fn resolve_null_coalesce_chain<'b>(
    binary: &'b Binary<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let mut combined: Vec<ResolvedType> = Vec::new();
    let mut current = binary;
    let base_ctx = ctx;
    let mut operand_ctx: Option<VarResolutionCtx<'_>> = None;
    let mut carried: Option<HashMap<String, Vec<ResolvedType>>> = None;
    loop {
        let ctx = operand_ctx.as_ref().unwrap_or(base_ctx);
        // Syntactically non-nullable operands: `new Foo()`, a non-null
        // literal, an array literal, `clone $x`.
        let non_nullable = match peel_type_transparent(current.lhs) {
            Expression::Literal(Literal::Null(_)) => false,
            Expression::Instantiation(_)
            | Expression::Literal(_)
            | Expression::Array(_)
            | Expression::LegacyArray(_)
            | Expression::Clone(_) => true,
            _ => false,
        };
        let lhs_results = resolve_rhs_expression(current.lhs, ctx);
        if lhs_results.is_empty() && operand_is_null_only(current.lhs, ctx) {
            // A bare variable with no entry in scope has not been
            // assigned on any path reaching this point, so it is
            // undefined here — which PHP evaluates as `null` (with a
            // notice), not "could be anything".  It contributes nothing
            // to the union, the same as a resolved-but-stripped `null`
            // a few lines down.
        } else if lhs_results.is_empty() {
            // A genuinely unresolvable operand. At runtime it could hold
            // any value, so represent it as `mixed` and keep unioning
            // the rest of the chain.
            ResolvedType::extend_unique(
                &mut combined,
                vec![ResolvedType::from_type_string(PhpType::mixed())],
            );
        } else if non_nullable {
            // The remaining operands are unreachable.
            ResolvedType::extend_unique(&mut combined, lhs_results);
            return simplify_branch_results(combined);
        } else {
            ResolvedType::extend_unique(&mut combined, strip_null_alternatives(lhs_results));
        }
        // Always union with the next operand.  Even when the type string
        // looks non-nullable, the user wrote `??` defensively and both
        // branches are valid candidates.
        // An operand after a plain variable runs where that variable was
        // `null`, which the forward walker records at the operand's start
        // (inside `if ($a || $b)`, the `$b` of `$a ?? $b` is truthy).
        if matches!(
            peel_type_transparent(current.lhs),
            Expression::Variable(Variable::Direct(_))
        ) {
            let (own, impossible) =
                crate::type_engine::variable::forward_walk::condition_arm_narrowing(
                    current.lhs,
                    false,
                    ctx,
                );
            let mut next = base_ctx.with_cursor_offset(current.rhs.span().start.offset);
            if !impossible && !own.is_empty() {
                carried
                    .get_or_insert_with(|| base_ctx.match_arm_narrowing.clone())
                    .extend(own);
            }
            if let Some(overrides) = &carried {
                next = next.with_match_arm_narrowing(overrides.clone());
            }
            operand_ctx = Some(next);
        }
        let ctx = operand_ctx.as_ref().unwrap_or(base_ctx);
        match peel_type_transparent(current.rhs) {
            Expression::Binary(next) if next.operator.is_null_coalesce() => current = next,
            last => {
                ResolvedType::extend_unique(&mut combined, resolve_rhs_expression(last, ctx));
                return simplify_branch_results(combined);
            }
        }
    }
}

/// Whether an empty-resolving `??` operand is a bare variable that was
/// never assigned on any path reaching this point, rather than one that
/// was assigned but whose type the resolver failed to work out.
///
/// `resolve_rhs_expression` returns an empty `Vec` for both: the forward
/// walker's `ScopeState` answers an unassigned name and an
/// assigned-but-unresolved one the same way through `scope_var_resolver`
/// (see `ScopeState::snapshot_resolver`).  `scope_contains_resolver` is
/// the one signal that tells them apart, so this only fires when it is
/// available; a caller without a forward-walker scope (e.g. the
/// backward-scan cold path) keeps the existing `mixed`-widening
/// behaviour.
fn operand_is_null_only(expr: &Expression<'_>, ctx: &VarResolutionCtx<'_>) -> bool {
    let Expression::Variable(Variable::Direct(dv)) = peel_type_transparent(expr) else {
        return false;
    };
    let Some(contains) = ctx.scope_contains_resolver else {
        return false;
    };
    !contains(bytes_to_str(dv.name))
}

/// Turn a branch that resolved to nothing into `mixed`.
///
/// A union built from branches must never come out narrower than the
/// truth. A branch the resolver could not type still holds *some* value
/// at runtime, so letting it vanish leaves the surviving branches
/// claiming to be the complete set of what the expression can hold, and
/// every consumer reads that as a confident answer: hover names one
/// value, completion offers its members, and a diagnostic that treats the
/// type as exhaustive contradicts the code.
///
/// Branches that produce no value at all (`throw`, `exit`, `die`) are the
/// exception. They never reach the union, so they widen nothing.
fn widen_unresolved_branch(expr: &Expression<'_>, results: Vec<ResolvedType>) -> Vec<ResolvedType> {
    if !results.is_empty() || branch_yields_no_value(expr) {
        return results;
    }
    vec![ResolvedType::from_type_string(PhpType::mixed())]
}

/// Whether a branch expression aborts instead of producing a value.
fn branch_yields_no_value(expr: &Expression<'_>) -> bool {
    matches!(
        peel_type_transparent(expr),
        Expression::Throw(_) | Expression::Construct(Construct::Exit(_) | Construct::Die(_))
    )
}

/// Drop `null` from each resolved alternative of a `??` operand.
///
/// Example: `?Foo ?? Bar` → `Foo|Bar`.  A bare `null` alternative is
/// dropped entirely; anything else (including `mixed`) passes through.
fn strip_null_alternatives(results: Vec<ResolvedType>) -> Vec<ResolvedType> {
    results
        .into_iter()
        .filter_map(|mut resolved| match resolved.type_string.non_null_type() {
            Some(non_null) => {
                resolved.type_string = non_null;
                Some(resolved)
            }
            None if resolved.type_string == PhpType::null() => None,
            None => Some(resolved),
        })
        .collect()
}

/// Keep only what can be truthy in each resolved alternative.
///
/// A short ternary (`$x ?: $default`) yields the condition's own value,
/// but only on the branch where that value was truthy, so `string|false`
/// contributes `string` and nothing more.  An alternative with no truthy
/// part at all (`false`, `null`) is dropped: reaching it is impossible.
fn truthy_alternatives(results: Vec<ResolvedType>) -> Vec<ResolvedType> {
    results
        .into_iter()
        .filter_map(|mut resolved| {
            resolved.type_string = resolved.type_string.truthy_type()?;
            Some(resolved)
        })
        .collect()
}

/// Resolve a ternary, or a chain of ternaries nested in each other's
/// `else` branch (`$a ? 1 : ($b ? 2 : ($c ? 3 : 4))`), to the union of
/// the branches that are reachable.
///
/// Each branch is resolved with the cursor positioned inside it so that
/// instanceof / guard narrowing from the condition applies to variable
/// and property subjects within the branch.  Without this,
/// `$x instanceof Foo ? $x->m() : null` would resolve `$x->m()` against
/// the un-narrowed type, the then branch would fail, and the whole
/// ternary would collapse to the else branch instead of unioning both.
///
/// A condition whose PHP truthiness is statically known (`true`,
/// `false`, `0`, `''`, …) makes one branch unreachable, and the dead
/// branch must not widen the result into a union.  PHPStan prunes the
/// same way.
///
/// A condition that rules out every value one of its subjects holds prunes
/// the same way even though its truthiness is not a constant:
/// `$acc === null ? seed() : $acc->merge()` cannot take its else arm while
/// `$acc` is still exactly `null`, so folding that arm's unresolvable
/// receiver into the union would erase the seed the then arm produces.
pub(super) fn resolve_conditional_chain<'b>(
    conditional: &'b Conditional<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let mut skipped_impossible = false;
    let results = resolve_conditional_arms(conditional, ctx, true, &mut skipped_impossible);
    // Pruning must never be the reason the whole ternary has no type: when
    // every arm that was left looked impossible, the narrowing that said so
    // was the less trustworthy of the two answers.
    if results.is_empty() && skipped_impossible {
        return resolve_conditional_arms(conditional, ctx, false, &mut false);
    }
    results
}

fn resolve_conditional_arms<'b>(
    conditional: &'b Conditional<'b>,
    ctx: &VarResolutionCtx<'_>,
    prune_impossible: bool,
    skipped_impossible: &mut bool,
) -> Vec<ResolvedType> {
    let mut combined: Vec<ResolvedType> = Vec::new();
    let mut current = conditional;
    // Narrowing carried down the else spine: reaching link N means every
    // earlier condition was false, so their inverse specifications all hold.
    let mut carried = ctx.match_arm_narrowing.clone();
    loop {
        // A short ternary (`$a ?: $b`) reuses the condition as its then
        // branch.
        let is_short = current.then.is_none();
        let then_expr = current.then.unwrap_or(current.condition);
        let truthiness = static_condition_truthiness(current.condition);
        if truthiness != Some(false) {
            let then_ctx = ctx.with_cursor_offset(then_expr.span().start.offset);
            // The then arm only runs when the condition held, so it sees the
            // condition's positive narrowing — the same specification an
            // `if` body gets.
            let (then_ctx, impossible) =
                with_arm_narrowing(&then_ctx, &carried, current.condition, true);
            if impossible && prune_impossible {
                *skipped_impossible = true;
            } else {
                let then_results = resolve_rhs_expression(then_expr, &then_ctx);
                let then_results = widen_unresolved_branch(then_expr, then_results);
                // The short form yields the condition's value, and reaching it
                // means the condition was truthy, so its falsy members are not
                // part of the result.
                let then_results = if is_short {
                    truthy_alternatives(then_results)
                } else {
                    then_results
                };
                ResolvedType::extend_unique(&mut combined, then_results);
            }
        }
        if truthiness == Some(true) {
            return simplify_branch_results(combined);
        }
        match peel_type_transparent(current.r#else) {
            Expression::Conditional(next) => {
                merge_arm_narrowing(&mut carried, current.condition, false, ctx);
                current = next;
            }
            _ => {
                let else_ctx = ctx.with_cursor_offset(current.r#else.span().start.offset);
                // Reaching the else arm proves the condition was false, so it
                // sees the inverse narrowing.
                let (else_ctx, impossible) =
                    with_arm_narrowing(&else_ctx, &carried, current.condition, false);
                if impossible && prune_impossible {
                    *skipped_impossible = true;
                } else {
                    let else_results = resolve_rhs_expression(current.r#else, &else_ctx);
                    ResolvedType::extend_unique(
                        &mut combined,
                        widen_unresolved_branch(current.r#else, else_results),
                    );
                }
                return simplify_branch_results(combined);
            }
        }
    }
}

/// Derive a context whose variable lookups see `condition`'s narrowing for
/// the given polarity, layered on top of `carried`, and say whether the
/// polarity is possible at all.
///
/// The condition's own specification wins over `carried`: it is evaluated
/// against the same declared types and is the more specific of the two.
fn with_arm_narrowing<'a>(
    ctx: &VarResolutionCtx<'a>,
    carried: &HashMap<String, Vec<ResolvedType>>,
    condition: &Expression<'_>,
    truthy: bool,
) -> (VarResolutionCtx<'a>, bool) {
    let mut overrides = carried.clone();
    let (own, impossible) =
        crate::type_engine::variable::forward_walk::condition_arm_narrowing(condition, truthy, ctx);
    overrides.extend(own);
    (ctx.with_match_arm_narrowing(overrides), impossible)
}

/// Fold `condition`'s narrowing for one polarity into an override map.
fn merge_arm_narrowing(
    overrides: &mut HashMap<String, Vec<ResolvedType>>,
    condition: &Expression<'_>,
    truthy: bool,
    ctx: &VarResolutionCtx<'_>,
) {
    for (name, types) in crate::type_engine::variable::forward_walk::condition_narrowing_overrides(
        condition, truthy, ctx,
    ) {
        overrides.insert(name, types);
    }
}

/// Resolve a `match` to the union of the arms that can be reached, each
/// resolved with the narrowing its conditions establish.
pub(super) fn resolve_match_expression(
    match_expr: &Match<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    // Two subject shapes carry narrowing information: `match (true)`
    // with `instanceof` conditions, and `match ($x::class)` with
    // `Foo::class` conditions.
    let subject_var = crate::type_engine::type_ops::condition_facts::match_class_subject_var(
        match_expr.expression,
    );
    let is_true_subject = match_expr.expression.is_true();
    let narrows = is_true_subject || subject_var.is_some();
    let mut combined = Vec::new();
    // Reaching an arm means every arm above it was tested and
    // failed, so the inverse of their conditions holds in its body
    // — the proof a ternary chain carries down its `else` spine,
    // and what makes `default => $x` see the `$x === null` arm.
    let mut carried: HashMap<String, Vec<ResolvedType>> = HashMap::new();
    // `default` runs only once every other arm failed, wherever it is
    // written, so it is resolved last.
    let (default_arms, expression_arms): (Vec<_>, Vec<_>) = match_expr
        .arms
        .iter()
        .partition(|arm| matches!(arm, MatchArm::Default(_)));
    for arm in expression_arms.into_iter().chain(default_arms) {
        // Narrowed variable types, so that the arm expression
        // resolves against the narrowed class.
        let mut overrides = carried.clone();
        if narrows && let MatchArm::Expression(expr_arm) = arm {
            // The arm's own conditions describe its body more
            // precisely than the carried inverses, so they win.
            overrides.extend(extract_match_arm_narrowings(expr_arm, subject_var, ctx));
            if subject_var.is_none() {
                // The shared pipeline needs the forward walker's
                // scope to seed the subjects; where there is none
                // the `instanceof` extractor above is all we get.
                overrides.extend(match_true_arm_narrowings(expr_arm, ctx));
            }
        }
        // The arm is resolved with the cursor inside it, as a
        // ternary's branches are: a lookup that has no scope to read
        // walks to the cursor, and the walk applies the narrowing
        // the arm runs under there.
        let arm_expr = arm.expression();
        let mut arm_ctx = ctx.with_cursor_offset(arm_expr.span().start.offset);
        if !overrides.is_empty() {
            arm_ctx.match_arm_narrowing = overrides;
        }
        let arm_results = resolve_rhs_expression(arm_expr, &arm_ctx);
        ResolvedType::extend_unique(
            &mut combined,
            widen_unresolved_branch(arm_expr, arm_results),
        );
        if is_true_subject && let MatchArm::Expression(expr_arm) = arm {
            // Every condition of this arm was not `true` for the
            // arms below it, so all of their inverses hold together,
            // on top of what the arms above already proved.
            for condition in expr_arm.conditions.iter() {
                if !crate::type_engine::variable::forward_walk::match_true_condition_is_boolean(
                    condition,
                    || resolve_rhs_expression(condition, ctx),
                ) {
                    continue;
                }
                let carried_ctx =
                    (!carried.is_empty()).then(|| ctx.with_match_arm_narrowing(carried.clone()));
                carried.extend(
                    crate::type_engine::variable::forward_walk::condition_narrowing_overrides(
                        condition,
                        false,
                        carried_ctx.as_ref().unwrap_or(ctx),
                    ),
                );
            }
        }
    }
    simplify_branch_results(combined)
}

// ── Match-arm narrowing override ────────────────────────────────────
//
// When resolving the RHS of a `match(true)` arm like:
//
//   match (true) {
//       $model instanceof Customer => $model->country,
//       …
//   }
//
// the arm expression `$model->country` must resolve `$model` as
// `Customer`, not its declared parameter type `?Model`.  The normal
// variable resolution pipeline doesn't know about the match-arm
// condition, so we propagate narrowings via the `match_arm_narrowing`
// field on `VarResolutionCtx`.  When entering a `match(true)` arm
// body, a new context is created with the narrowed types; callers in
// `resolve_rhs_method_call_inner` and `resolve_rhs_property_access`
// consult `ctx.match_arm_narrowing` when the object is a bare variable.

/// Extract instanceof narrowings from a `match(true)` arm's conditions.
///
/// A condition like `$var instanceof ClassName` maps `"$var"` → the
/// resolved `ClassInfo` for `ClassName`.  The arm runs when any one of its
/// conditions holds, so a variable is narrowed only when every condition
/// names it, to the union of the classes they name: `$p instanceof Cat,
/// $p instanceof Dog` narrows `$p` to `Cat|Dog`, while `$p instanceof Cat,
/// $q instanceof Dog` narrows neither.
fn extract_match_arm_narrowings(
    expr_arm: &MatchExpressionArm<'_>,
    subject_var: Option<&str>,
    ctx: &VarResolutionCtx<'_>,
) -> HashMap<String, Vec<ResolvedType>> {
    let mut narrowed: Option<(String, Vec<ResolvedType>)> = None;
    let names = ctx.written_names();
    for condition in expr_arm.conditions.iter() {
        // `match ($x::class) { Foo::class, Bar::class => … }` — each
        // condition names one class the subject may be, so the arm body
        // sees the union of them.
        let pair = match subject_var {
            Some(var) => {
                crate::type_engine::type_ops::condition_facts::class_match_condition_class(
                    condition, &names,
                )
                .map(|ty| (var.to_owned(), ty))
            }
            None => extract_instanceof_pair(condition, &names),
        };
        // A condition that says nothing this one can read about the
        // variable the others narrow lets the arm run whatever it holds.
        let Some((var_name, class_type)) = pair else {
            return HashMap::new();
        };
        if narrowed.as_ref().is_some_and(|(name, _)| *name != var_name) {
            return HashMap::new();
        }
        let resolved = type_resolution::type_hint_to_classes_typed(
            &class_type,
            &ctx.current_class.fqn(),
            ctx.file.all_classes,
            ctx.file.class_loader,
        );
        if resolved.is_empty() {
            return HashMap::new();
        }
        let results = ResolvedType::from_classes_with_hint(resolved, class_type);
        match &mut narrowed {
            Some((_, existing)) => ResolvedType::extend_unique(existing, results),
            None => narrowed = Some((var_name, results)),
        }
    }
    narrowed.into_iter().collect()
}

/// The narrowing a `match (true)` arm's conditions establish for its body.
///
/// This runs the same condition pipeline an `if` body gets, so every rule
/// in it (`!== null`, type guards, assertions, …) reaches a match arm, not
/// just the `instanceof` shape [`extract_match_arm_narrowings`] knows.
///
/// An arm runs when *any* of its conditions matched, so a fact only holds
/// inside the body when every condition proves it: the per-condition maps
/// are intersected by subject and unioned by type.
fn match_true_arm_narrowings(
    expr_arm: &MatchExpressionArm<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> HashMap<String, Vec<ResolvedType>> {
    let mut conditions = expr_arm.conditions.iter();
    let Some(first) = conditions.next() else {
        return HashMap::new();
    };
    let mut merged =
        crate::type_engine::variable::forward_walk::condition_narrowing_overrides(first, true, ctx);
    for condition in conditions {
        if merged.is_empty() {
            break;
        }
        let next = crate::type_engine::variable::forward_walk::condition_narrowing_overrides(
            condition, true, ctx,
        );
        merged.retain(|name, types| match next.get(name) {
            Some(other) => {
                ResolvedType::extend_unique(types, other.clone());
                true
            }
            None => false,
        });
    }
    merged
}

/// Extract `($var_name, ClassName)` from `$var instanceof ClassName`.
fn extract_instanceof_pair(
    expr: &Expression<'_>,
    names: &crate::type_engine::resolver::WrittenNames<'_>,
) -> Option<(String, PhpType)> {
    let expr = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    if let Expression::Binary(bin) = expr
        && bin.operator.is_instanceof()
    {
        // LHS: the variable
        let var_name = match bin.lhs {
            Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
            _ => return None,
        };
        // RHS: the class name
        let class_type = match bin.rhs {
            Expression::Identifier(ident) => names.class_type(bytes_to_str(ident.value())),
            Expression::Self_(_) => PhpType::named(atom("self")),
            Expression::Static(_) => PhpType::named(atom("static")),
            Expression::Parent(_) => PhpType::named(atom("parent")),
            _ => return None,
        };
        Some((var_name, class_type))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_simplification_keeps_nested_mixed_alternatives() {
        let nested_mixed = PhpType::union(vec![
            PhpType::mixed(),
            PhpType::named(atom("CompletionFallback")),
        ]);
        let results = vec![
            ResolvedType::from_type_string(nested_mixed.clone()),
            ResolvedType::from_type_string(PhpType::literal_string_raw("'tag'")),
        ];

        let simplified = simplify_branch_results(results);
        assert_eq!(simplified.len(), 2);
        assert_eq!(simplified[0].type_string, nested_mixed);
        assert_eq!(
            simplified[1].type_string,
            PhpType::literal_string_raw("'tag'")
        );
    }

    #[test]
    fn branch_simplification_preserves_class_metadata_while_absorbing_scalar_literal() {
        let class = crate::test_fixtures::make_class("Example");
        let results = vec![
            ResolvedType::from_class(class),
            ResolvedType::from_type_string(PhpType::literal_string_raw("'fixed'")),
            ResolvedType::from_type_string(PhpType::string()),
        ];

        let simplified = simplify_branch_results(results);
        assert_eq!(simplified.len(), 2);
        assert!(simplified.iter().any(|result| result.class_info.is_some()));
        assert!(simplified.iter().any(|result| {
            result.class_info.is_none() && result.type_string == PhpType::string()
        }));
    }
}
