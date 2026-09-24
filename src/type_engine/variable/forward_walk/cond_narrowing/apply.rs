//! The orchestrators that turn a condition expression into scope
//! narrowing, for the truthy branch and for its inverse.
//!
//! The extractors each sibling module owns answer what one check
//! proves; these walk the condition, combine what the operands of a
//! logical chain prove, and write the result into the scope.

use super::*;

/// Narrow a `match ($x::class)` subject to the classes one arm names.
///
/// `match ($node::class) { ASTClass::class, ASTEnum::class => … }` proves
/// the subject is one of the listed classes inside that arm, exactly like
/// a chain of `instanceof` checks would — except the identity is exact, so
/// no subclass survives.
pub(crate) fn apply_class_match_arm_narrowing<'b>(
    subject_var: &str,
    expr_arm: &'b MatchExpressionArm<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let classes: Vec<PhpType> = expr_arm
        .conditions
        .iter()
        .filter_map(|c| narrowing::class_match_condition_class(c))
        .collect();
    if classes.is_empty() {
        return;
    }

    let scope_resolver = scope.snapshot_resolver();
    let var_ctx = build_var_ctx(subject_var, ctx, &scope_resolver);
    let union = narrowing::resolve_class_names_to_union(&classes, &var_ctx);
    if union.is_empty() {
        return;
    }
    scope.set(
        subject_var,
        union.into_iter().map(ResolvedType::from_class).collect(),
    );
}

/// Where one subject's accumulated classes came from while walking a
/// condition's `&&` operands, which decides whether they are
/// alternatives or members of an intersection.
#[derive(Default)]
pub(super) struct Conjuncts {
    /// How many operands contributed a positive `instanceof` naming a
    /// single class.
    pub(super) operands: usize,
    /// At least one contributing operand was `is_a($x, C::class, true)` —
    /// a string alternative on the subject must survive the narrowing.
    pub(super) allow_string: bool,
    /// At least one contributing operand pinned the class exactly
    /// (`get_class($x) === C::class`), so a subclass of `C` does not pass.
    pub(super) exact: bool,
}

/// What a condition's `instanceof`-style checks concluded about one
/// subject, beyond the classes themselves.
#[derive(Default, Clone, Copy)]
pub(super) struct CheckShape {
    /// The classes describe one value that is all of them at once.
    pub(super) intersected: bool,
    /// A string alternative on the subject must survive the narrowing.
    pub(super) allow_string: bool,
    /// The classes are exact identities rather than subtype bounds.
    pub(super) exact: bool,
}

impl Conjuncts {
    /// Whether the accumulated classes describe one value that is all of
    /// them at once.
    ///
    /// `$x instanceof A && $x instanceof B` proves both, so the value is
    /// `A&B`.  One operand on its own proves a single class, so it does
    /// not conclude an intersection.
    pub(super) fn is_intersection(&self) -> bool {
        self.operands > 1
    }
}

/// Apply condition-based narrowing (instanceof, null check, type guard)
/// to the scope.  This narrows types for the "truthy" branch.
pub(crate) fn apply_condition_narrowing<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // `!(!$x)` says exactly what `$x` says, so cancel the pair before any
    // extractor looks at it.  The chain collectors fold each operand of an
    // `&&` / `||` the same way.
    let condition = narrowing::fold_negation_pairs(condition);

    // A `!` over a logical chain proves what the *inverse* pass proves
    // about the chain itself.
    if let Some(inner) = negated_logical_chain(condition) {
        apply_condition_narrowing_inverse(inner, scope, ctx);
        return;
    }

    // Seed property access keys from conditions into the scope so that
    // narrowing functions can find and narrow them.
    seed_property_keys_into_scope(condition, scope, ctx);

    // Decompose `&&` chains so that `$x instanceof Foo && $x instanceof Bar`
    // applies both narrowings as a union (intersection semantics: the
    // variable satisfies both checks, so members from both types are
    // available).
    //
    // An operand that is itself a disjunction is held back from that
    // decomposition: entering the branch proves the disjunction, not any
    // one of its legs, and every pass below narrows what it is handed as
    // though it had held.  The join at the end of this function owns them
    // instead, so a leg's own conclusion never reaches the branch body
    // unless every other leg proves it too.
    let (disjunctions, operands): (Vec<_>, Vec<_>) = collect_and_chain_operands(condition)
        .into_iter()
        .partition(|operand| collect_or_chain_operands(unwrap_parens(operand)).len() > 1);

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
    // Expand operands that are a bare boolean standing for a check
    // (`$isHtml` from `$isHtml = $raw instanceof HtmlString`) into the
    // check itself, and make sure its subject is narrowed below even
    // when the condition never names it.
    let alias_extractions: Vec<Vec<AliasExtraction>> = operands
        .iter()
        .map(|operand| assertion_alias_extractions(operand, scope))
        .collect();
    for subject in alias_extractions.iter().flatten().map(|a| &a.subject) {
        if !var_names.contains(subject) {
            var_names.push(subject.clone());
        }
    }

    let mut pinned = commit_chain_instanceof(&operands, &alias_extractions, &var_names, scope, ctx);

    // A key read through a receiver the same chain narrows is only
    // resolvable once that narrowing has landed:
    // `$expr instanceof FuncCall && !$expr->name instanceof Name` cannot
    // look `name` up on `FuncCall` until the first operand has proved that
    // is what `$expr` is.  Seeding is left until here for those keys, and
    // the extraction runs again over them alone — the subjects the first
    // run committed are already narrowed and must not be narrowed twice.
    let late_keys: Vec<String> = collect_condition_property_keys(condition)
        .into_iter()
        .filter(|key| !scope.contains(key))
        .collect();
    if !late_keys.is_empty() {
        for key in &late_keys {
            seed_synthetic_key_if_needed(key, scope, ctx);
        }
        let seeded: Vec<String> = late_keys
            .into_iter()
            .filter(|key| scope.contains(key))
            .collect();
        if !seeded.is_empty() {
            pinned.extend(commit_chain_instanceof(
                &operands,
                &alias_extractions,
                &seeded,
                scope,
                ctx,
            ));
        }
    }

    // The passes below still read the whole condition, disjunctions and
    // all.  Each of them either decomposes `&&` and stops at an operand it
    // does not recognise, or reads the condition as one expression — so a
    // disjunction is opaque to them and none of a leg's conclusions can
    // escape through them.  A new pass that looks *inside* an `||` belongs
    // in the leg walk below, not here.

    // Type guard narrowing: `is_object($x)`, `is_array($x)`, etc.
    apply_type_guard_narrowing_truthy(condition, scope, ctx);

    // A check on `$x->prop` discriminates a union of objects when only
    // some of them declare a `prop` that could have passed it.
    apply_property_discriminant_narrowing(condition, scope, ctx, true);

    // `is_a($x, Class::class, true)` / `class_exists($x)` narrowing:
    // narrow a string-typed `$x` to `class-string<Class>` / `class-string`.
    apply_class_string_guard_narrowing(condition, scope, ctx, true);

    // Null narrowing: `if ($x !== null)` — remove null from scope.
    apply_null_narrowing_truthy(condition, scope, ctx);

    // A proof about a `?->` chain's value is a proof about its receivers.
    apply_nullsafe_receiver_narrowing(condition, scope, ctx, true);

    // @phpstan-assert-if-true / -if-false narrowing.
    apply_phpstan_assert_condition_narrowing(condition, scope, ctx, false);

    // in_array($var, $haystack, true) narrowing.
    apply_in_array_narrowing(condition, scope, ctx, false);

    // property_exists($var, 'name') / method_exists($var, 'name') narrowing.
    apply_member_exists_narrowing(condition, scope, false);

    // array_key_exists('k', $arr) narrowing on an optional shape key.
    apply_array_key_exists_narrowing(condition, scope, ctx, false);

    // `if (preg_match(…, $matches))` — the body runs on a successful match,
    // so `$matches` has the keys the pattern describes.
    apply_preg_match_narrowing(condition, scope, ctx, true);

    // Each disjunction the chain held back proves only that one of its
    // legs held.  Splitting it re-uses the branch join, so the scope ends
    // up carrying both the union of what the legs prove and the record of
    // which leg proved what.
    apply_disjunct_operand_narrowing(&disjunctions, &pinned, scope, ctx);

    // Whatever the passes above proved about one value's null, they
    // proved about every value whose null it stands for.  Last, so it
    // sees the narrowed state rather than the state on the way in.
    apply_non_null_implication_narrowing(scope, ctx);
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

    let scope_resolver = scope.snapshot_resolver();
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
    // A bare boolean standing for a check inverts along with the rest of
    // the condition: `if (!$isHtml) { return; }` leaves `$raw` narrowed
    // to `HtmlString` after the guard.
    let alias_extractions = assertion_alias_extractions(condition, scope);
    for a in &alias_extractions {
        if !var_names.contains(&a.subject) {
            var_names.push(a.subject.clone());
        }
    }
    for var_name in &var_names {
        // The inverse of a disjunction the condition names, or of one a
        // boolean stands for: neither leg held, so every class it lists
        // is excluded.
        let alias_or = alias_extractions
            .iter()
            .find(|a| a.subject == *var_name && !a.alternatives.is_empty());
        if let Some(alias) = alias_or.filter(|a| !a.extraction.negated) {
            let var_ctx = build_var_ctx(var_name, ctx, &scope_resolver);
            if exclude_classes_in_scope(var_name, &alias_classes(alias), &var_ctx, scope) {
                scope.unreachable = true;
            }
            continue;
        }
        // The inverse of `!$isNode`: the chain did hold, so the subject
        // is one of the classes it lists.
        if let Some(alias) = alias_or {
            let var_ctx = build_var_ctx(var_name, ctx, &scope_resolver);
            let union = narrowing::resolve_class_names_to_union(&alias_classes(alias), &var_ctx);
            if !union.is_empty() {
                commit_instanceof_narrowing(
                    var_name,
                    union.into_iter().map(ResolvedType::from_class).collect(),
                    CheckShape::default(),
                    scope,
                    ctx,
                    &scope_resolver,
                );
            }
            continue;
        }

        if let Some(classes) = narrowing::try_extract_compound_or_instanceof(condition, var_name)
            && !classes.is_empty()
        {
            let var_ctx = build_var_ctx(var_name, ctx, &scope_resolver);
            if exclude_classes_in_scope(var_name, &classes, &var_ctx, scope) {
                scope.unreachable = true;
            }
            continue;
        }

        // `if (!$x instanceof $stmtClass) { continue; }` — the
        // fall-through proves the dynamic check held.
        if let Some((rhs, negated)) = narrowing::try_extract_dynamic_instanceof(condition, var_name)
        {
            let targets = dynamic_instanceof_targets(rhs, scope, ctx);
            if !targets.is_empty() {
                let var_ctx = build_var_ctx(var_name, ctx, &scope_resolver);
                if negated {
                    let mut narrowed = Vec::new();
                    for target in &targets {
                        let mut single = Vec::new();
                        ResolvedType::apply_narrowing(&mut single, |classes| {
                            narrowing::apply_instanceof_inclusion(target, false, &var_ctx, classes)
                        });
                        ResolvedType::extend_unique(&mut narrowed, single);
                    }
                    // An operand that names no loadable class proves
                    // nothing; committing an empty result would instead
                    // mark the subject unresolvable.
                    if !narrowed.is_empty() {
                        commit_instanceof_narrowing(
                            var_name,
                            narrowed,
                            CheckShape::default(),
                            scope,
                            ctx,
                            &scope_resolver,
                        );
                    }
                } else if exclude_classes_in_scope(var_name, &targets, &var_ctx, scope) {
                    scope.unreachable = true;
                }
                continue;
            }
        }

        if let Some(extraction) =
            narrowing::try_extract_instanceof_with_negation(condition, var_name).or_else(|| {
                alias_extractions
                    .iter()
                    .find(|a| a.subject == *var_name)
                    .map(|a| a.extraction.clone())
            })
        {
            let var_ctx = build_var_ctx(var_name, ctx, &scope_resolver);
            if extraction.negated {
                // Inverse of negated instanceof → positive instanceof,
                // which proves exactly what the truthy branch of the
                // un-negated check proves.  Resolve the asserted classes
                // on their own and hand them to the shared commit so the
                // existing union is *filtered* down to them rather than
                // extended with them.
                let mut narrowed = Vec::new();
                ResolvedType::apply_narrowing(&mut narrowed, |classes| {
                    narrowing::apply_instanceof_inclusion(
                        &extraction.class_type,
                        extraction.exact,
                        &var_ctx,
                        classes,
                    )
                });
                commit_instanceof_narrowing(
                    var_name,
                    narrowed,
                    CheckShape {
                        allow_string: extraction.allow_string,
                        exact: extraction.exact,
                        ..CheckShape::default()
                    },
                    scope,
                    ctx,
                    &scope_resolver,
                );
            } else {
                // Inverse of positive instanceof → exclusion.
                // Exclusion does NOT strip null (`!instanceof` is
                // true for null values).
                if exclude_classes_in_scope(
                    var_name,
                    std::slice::from_ref(&extraction.class_type),
                    &var_ctx,
                    scope,
                ) {
                    // Every alternative the variable had was excluded, so
                    // nothing can reach this path: `$v` was already an
                    // `AbstractNode` and this is the else of
                    // `if ($v instanceof AbstractNode)`.
                    scope.unreachable = true;
                }
            }
        }
    }

    // Inverse member-existence narrowing: after a guard clause like
    // `if (!property_exists($x, 'name')) { return; }`, the member is
    // known to exist.
    apply_member_exists_narrowing(condition, scope, true);

    // Inverse `array_key_exists` narrowing: after
    // `if (!array_key_exists('k', $arr)) { return; }` the key is present.
    apply_array_key_exists_narrowing(condition, scope, ctx, true);

    // A union of objects the check on `$x->prop` could only have failed
    // for some of.  Callers hand this one operand at a time, so the
    // `&&` / `||` decomposition is already done.
    apply_property_discriminant_narrowing(condition, scope, ctx, false);
}

/// Apply inverse condition-based narrowing (for else branches and
/// guard clauses).
pub(crate) fn apply_condition_narrowing_inverse<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // As in the truthy pass: `!(!$x)` is `$x`, so cancel the pair first.
    let condition = narrowing::fold_negation_pairs(condition);

    // The mirror of the truthy pass: the fall-through of
    // `if (!($t instanceof CallableType || $t instanceof ClosureType)) { return; }`
    // is what the chain itself proves, so hand it to the truthy pass.
    if let Some(inner) = negated_logical_chain(condition) {
        apply_condition_narrowing(inner, scope, ctx);
        return;
    }

    // De Morgan over `||`: NOT (A || B) = !A && !B.  Every operand's inverse
    // holds at the same time, so they apply sequentially to one scope.  This
    // is what makes the `if (!guard1 || !guard2) { return; }` idiom narrow
    // its fall-through by each conjunct.
    let or_operands = collect_or_chain_operands(condition);
    if or_operands.len() > 1 {
        for operand in &or_operands {
            // Recurse rather than calling the single-operand form directly,
            // so a nested `&&` inside one `||` operand is decomposed too.
            apply_condition_narrowing_inverse(operand, scope, ctx);
        }
        return;
    }

    // De Morgan over `&&`: NOT (A && B) = !A || !B.  The operands are
    // alternatives, not simultaneous facts, so each contributes one branch
    // of a union: narrow a clone per operand, then merge.
    let and_operands = collect_and_chain_operands(condition);
    if and_operands.len() > 1 {
        let base_scope = scope.clone();
        let mut branch_scopes: Vec<ScopeState> = Vec::new();
        for operand in &and_operands {
            let mut branch = base_scope.clone();
            apply_condition_narrowing_inverse(operand, &mut branch, ctx);
            branch_scopes.push(branch);
        }
        if let Some(first) = branch_scopes.first() {
            let mut merged = first.clone();
            for branch in &branch_scopes[1..] {
                merged.merge_branch(branch);
            }
            // A synthetic key only one branch established is not a fact
            // about the merge: the other branches say nothing about it, so
            // the declared type still stands.  Without this,
            // `!($n === 'self' && $s->isInClass())` left
            // `$s->getClassReflection()` narrowed to the `null` that one
            // alternative implies, and a sibling `elseif` that proves the
            // opposite could no longer widen it back.
            let branch_refs: Vec<&ScopeState> = branch_scopes.iter().collect();
            retain_synthetic_keys_common_to_all(&mut merged, &branch_refs);
            *scope = merged;
        }
        return;
    }

    apply_condition_narrowing_inverse_operand(condition, scope, ctx);
}

/// The variable overrides a condition establishes for one polarity, ready to
/// hand to [`VarResolutionCtx::with_match_arm_narrowing`].
///
/// This is how the expression resolvers (ternary arms, short-circuit
/// operands) get their narrowing: they run the *same* pipeline `if`/`else`
/// bodies use, over a scope seeded with just the subjects the condition
/// names, and keep whatever it changed. Every rule added to the pipeline
/// therefore reaches every expression position for free.
///
/// Returns an empty map when the condition narrows nothing, which callers
/// use to skip building a derived context at all.
pub(crate) fn condition_narrowing_overrides<'b>(
    condition: &'b Expression<'b>,
    truthy: bool,
    ctx: &VarResolutionCtx<'_>,
) -> HashMap<String, Vec<ResolvedType>> {
    condition_arm_narrowing(condition, truthy, ctx).0
}

/// [`condition_narrowing_overrides`] plus whether the arm can run at all.
///
/// The flag is `true` when the condition rules out every value one of its
/// subjects could hold, which makes the arm dead code: the else of
/// `$acc === null ? seed($x) : $acc->merge($x)` on the run where `$acc` is
/// still exactly `null`. An arm that cannot run contributes no type, so a
/// caller that unions the arms must leave it out rather than fold in the
/// unresolvable receiver it would have had.
pub(crate) fn condition_arm_narrowing<'b>(
    condition: &'b Expression<'b>,
    truthy: bool,
    ctx: &VarResolutionCtx<'_>,
) -> (HashMap<String, Vec<ResolvedType>>, bool) {
    let Some(resolver) = ctx.scope_var_resolver else {
        return (HashMap::new(), false);
    };

    let mut subjects: Vec<String> = Vec::new();
    collect_condition_subject_vars(condition, &mut subjects);
    for key in collect_condition_property_keys(condition) {
        if !subjects.contains(&key) {
            subjects.push(key);
        }
    }
    if subjects.is_empty() {
        return (HashMap::new(), false);
    }

    let mut scope = ScopeState::new();
    // A condition that tests a boolean standing for an earlier check
    // (`$isFoo ? … : …`) names only the boolean, so the check's own
    // subject has to be seeded before there is anything to narrow.
    if let Some(proofs) = ctx.scope_proofs.filter(|p| !p.is_empty()) {
        scope.adopt_proofs(&proofs);
        let holders: Vec<Atom> = subjects.iter().map(|s| atom(s)).collect();
        for holder in &holders {
            proofs.subjects_of(holder, &mut subjects);
        }
    }
    for subject in &subjects {
        let types = resolver(subject);
        if !types.is_empty() {
            scope.set(subject, types);
        }
    }
    if scope.locals.is_empty() {
        return (HashMap::new(), false);
    }

    let seeded = scope.locals.clone();
    let walk_ctx = ForwardWalkCtx::from_var_ctx(ctx);
    if truthy {
        apply_condition_narrowing(condition, &mut scope, &walk_ctx);
    } else {
        apply_condition_narrowing_inverse(condition, &mut scope, &walk_ctx);
    }

    let impossible = scope.unreachable;
    let overrides = scope
        .locals
        .into_iter()
        .filter(|(name, types)| {
            !types.is_empty()
                && seeded
                    .get(name)
                    .is_none_or(|before| narrowing_changed_types(before, types))
        })
        .map(|(name, types)| (name.to_string(), types))
        .collect();
    (overrides, impossible)
}

/// Apply every inverse narrowing rule to a condition that is no longer a
/// `&&`/`||` chain.
///
/// [`apply_condition_narrowing_inverse`] does the De Morgan decomposition and
/// hands each leaf here, so every rule sees the operand it can actually match
/// instead of the compound expression wrapping it.
fn apply_condition_narrowing_inverse_operand<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    apply_condition_narrowing_inverse_single(condition, scope, ctx);

    // Inverse type guard narrowing: `if (is_object($x))` in else → exclude object.
    apply_type_guard_narrowing_inverse(condition, scope, ctx);

    // Inverse class-string guard narrowing: `if (!is_a($x, Class::class, true))`
    // guard clause → after it, `$x` is a class-string of `Class`.
    apply_class_string_guard_narrowing(condition, scope, ctx, false);

    // Inverse null narrowing: `if ($x === null)` after guard → remove null.
    apply_null_narrowing_inverse(condition, scope, ctx);

    // A proof about a `?->` chain's value is a proof about its receivers.
    apply_nullsafe_receiver_narrowing(condition, scope, ctx, false);

    // Inverse @phpstan-assert-if-true / -if-false narrowing.
    apply_phpstan_assert_condition_narrowing(condition, scope, ctx, true);

    // Inverse in_array narrowing: exclude the element type in the else branch.
    apply_in_array_narrowing(condition, scope, ctx, true);

    // Inverse `preg_match` narrowing: the else branch (and the fall-through of
    // an `if (!preg_match(…, $matches)) { return; }` guard) knows the opposite
    // outcome of the one the condition tests for.
    apply_preg_match_narrowing(condition, scope, ctx, false);

    // Whatever the passes above proved about one value's null, they
    // proved about every value whose null it stands for.  Last, so it
    // sees the narrowed state rather than the state on the way in.
    apply_non_null_implication_narrowing(scope, ctx);
}

/// Build a [`VarResolutionCtx`] from a variable name and forward-walk context.
///
/// Shared helper used by the narrowing functions in this module to avoid
/// repeating the struct construction at every call site.  It carries no
/// scope proofs: the callers use it to resolve the class names a check
/// mentions, which is a question about declarations rather than about
/// what the values flowing through the scope have been proven to be.
pub(crate) fn build_var_ctx<'a>(
    var_name: &'a str,
    ctx: &'a ForwardWalkCtx<'_>,
    scope_resolver: &'a dyn Fn(&str) -> Vec<ResolvedType>,
) -> VarResolutionCtx<'a> {
    VarResolutionCtx {
        backend: ctx.backend,
        loaders: ctx.loaders,
        resolved_class_cache: ctx.resolved_class_cache,
        enclosing_return_type: ctx.enclosing_return_type.clone(),
        top_level_scope: ctx.top_level_scope.clone(),
        scope_var_resolver: Some(scope_resolver),
        ..VarResolutionCtx::new(
            var_name,
            ctx.current_class,
            ctx.all_classes,
            ctx.content,
            ctx.cursor_offset,
            ctx.class_loader,
        )
    }
}
