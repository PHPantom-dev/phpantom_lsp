use super::*;

use mago_span::HasSpan;

use crate::type_engine::types::narrowing;

// ─── Control flow handling ──────────────────────────────────────────────────

/// The conditions of an `if`'s `elseif` clauses, in source order.
///
/// Both body styles carry the same clauses under different types, and
/// several passes need to treat an `elseif`'s condition exactly as they
/// treat the leading `if`'s.
pub(crate) fn elseif_conditions<'b>(body: &'b IfBody<'b>) -> Vec<&'b Expression<'b>> {
    match body {
        IfBody::Statement(body) => body
            .else_if_clauses
            .iter()
            .map(|clause| clause.condition)
            .collect(),
        IfBody::ColonDelimited(body) => body
            .else_if_clauses
            .iter()
            .map(|clause| clause.condition)
            .collect(),
    }
}

/// Process an `if` statement with branch merging.
pub(crate) fn process_if<'b>(
    if_stmt: &'b If<'b>,
    enclosing_stmt: &'b Statement<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Record `&&` chain snapshots for the condition expression so that
    // member accesses after an instanceof/null guard within the condition
    // see the narrowed type.  E.g. `if ($x !== null && $x->method())`
    // — the `$x->method()` span needs `$x` narrowed to non-null.
    // The `||` variant handles the short-circuit guard idiom
    // `!$x instanceof Foo || $x->method()`.
    record_short_circuit_snapshots(if_stmt.condition, scope, ctx);

    // Cursor inside the condition: narrowing for member accesses there
    // was already recorded above via the chain snapshots (diagnostics),
    // or is applied by the caller after this returns (mod.rs's cursor
    // narrowing pass for hover/completion), so leave scope untouched.
    let cond_span = if_stmt.condition.span();
    if ctx.cursor_offset >= cond_span.start.offset && ctx.cursor_offset <= cond_span.end.offset {
        return;
    }

    // Assignment in condition: `if ($x = expr())`
    process_nested_assignments(if_stmt.condition, scope, ctx);

    // Pass-by-reference in condition: `if (preg_match(..., $matches))`
    seed_pass_by_ref_in_condition(if_stmt.condition, scope, ctx);

    // Record a snapshot after condition processing so that variables
    // seeded by pass-by-reference (e.g. `$matches` from `preg_match`)
    // are visible in the then-body and elseif/else bodies.  Without
    // this, the pre-statement snapshot (recorded by the outer
    // `walk_body_forward` before `process_if` runs) would be the
    // nearest floor entry, and it predates the seeding.
    if is_diagnostic_scope_active() {
        let body_start = match &if_stmt.body {
            IfBody::Statement(body) => body.statement.span().start.offset,
            IfBody::ColonDelimited(body) => body.colon.start.offset,
        };
        record_scope_snapshot(body_start, scope);
    }

    match &if_stmt.body {
        IfBody::Statement(body) => {
            process_if_statement_body(if_stmt, body, enclosing_stmt, scope, ctx);
        }
        IfBody::ColonDelimited(body) => {
            process_if_colon_body(if_stmt, body, enclosing_stmt, scope, ctx);
        }
    }
}

/// Process if with statement body (brace-style).
pub(crate) fn process_if_statement_body<'b>(
    if_stmt: &'b If<'b>,
    body: &'b IfStatementBody<'b>,
    enclosing_stmt: &'b Statement<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let then_span = body.statement.span();
    let cursor_in_then =
        ctx.cursor_offset >= then_span.start.offset && ctx.cursor_offset <= then_span.end.offset;

    // Which branches a decidable guard rules out.  Recorded before the
    // cursor dispatch below so the ranges are collected whichever branch
    // the walk goes on to take.
    let dead = dead_if_branches(
        if_stmt.condition,
        body.else_if_clauses.iter().map(|ei| ei.condition),
        body.else_clause.is_some(),
        ctx,
    );
    if dead.any() {
        if dead.then_branch {
            record_unreachable_range((then_span.start.offset, then_span.end.offset));
        }
        for (ei, ei_dead) in body.else_if_clauses.iter().zip(dead.else_if_clauses.iter()) {
            if *ei_dead {
                let sp = ei.statement.span();
                record_unreachable_range((sp.start.offset, sp.end.offset));
            }
        }
        if dead.else_clause
            && let Some(ref else_clause) = body.else_clause
        {
            let sp = else_clause.statement.span();
            record_unreachable_range((sp.start.offset, sp.end.offset));
        }
    }

    let cursor_in_elseif = body.else_if_clauses.iter().any(|ei| {
        let sp = ei.statement.span();
        ctx.cursor_offset >= sp.start.offset && ctx.cursor_offset <= sp.end.offset
    });

    let cursor_in_else = body.else_clause.as_ref().is_some_and(|ec| {
        let sp = ec.statement.span();
        ctx.cursor_offset >= sp.start.offset && ctx.cursor_offset <= sp.end.offset
    });

    // Cursor inside an elseif's own condition (as opposed to its body,
    // handled by `cursor_in_elseif` below): the if condition and every
    // strictly preceding elseif condition were false to reach here, but
    // this elseif's own condition is still being evaluated — it hasn't
    // been narrowed on yet, and the if/preceding-elseif bodies never ran.
    // Without this case the cursor falls through to the "after the whole
    // chain" merge below, which pulls in assignments from the if-body
    // (e.g. `if (...) { $value = true; } elseif (foo($value)) { ... }`
    // must not see `$value` as `T|bool` while evaluating `foo($value)`).
    for (idx, ei) in body.else_if_clauses.iter().enumerate() {
        let cond_span = ei.condition.span();
        if ctx.cursor_offset >= cond_span.start.offset && ctx.cursor_offset <= cond_span.end.offset
        {
            apply_condition_narrowing_inverse(if_stmt.condition, scope, ctx);
            for prev_ei in body.else_if_clauses.iter().take(idx) {
                apply_condition_narrowing_inverse(prev_ei.condition, scope, ctx);
            }
            return;
        }
    }

    if cursor_in_then {
        // Cursor is inside the then-branch.  Apply instanceof narrowing
        // and walk only this branch.
        apply_condition_narrowing(if_stmt.condition, scope, ctx);
        walk_body_forward(std::iter::once(body.statement), scope, ctx);
        return;
    }

    if cursor_in_elseif {
        // Find which elseif contains the cursor.
        for ei in body.else_if_clauses.iter() {
            let sp = ei.statement.span();
            if ctx.cursor_offset >= sp.start.offset && ctx.cursor_offset <= sp.end.offset {
                // Apply negated narrowing from the if condition, then
                // positive narrowing from this elseif condition.
                apply_condition_narrowing_inverse(if_stmt.condition, scope, ctx);
                // Also apply inverse narrowing for preceding elseifs.
                for prev_ei in body.else_if_clauses.iter() {
                    if std::ptr::eq(prev_ei, ei) {
                        break;
                    }
                    apply_condition_narrowing_inverse(prev_ei.condition, scope, ctx);
                }
                // The assignment and the by-reference seeding run before the
                // narrowing, exactly as they do for the leading `if`:
                // `elseif ($x = f())` has to put `$x` in scope before the
                // truthy test can strip its falsy members, and
                // `elseif (preg_match(…, $m))` has to seed `$m` before the
                // test can rule out the failed match.
                process_nested_assignments(ei.condition, scope, ctx);
                seed_pass_by_ref_in_condition(ei.condition, scope, ctx);
                apply_condition_narrowing(ei.condition, scope, ctx);
                walk_body_forward(std::iter::once(ei.statement), scope, ctx);
                return;
            }
        }
        return;
    }

    if cursor_in_else && let Some(ref else_clause) = body.else_clause {
        // Apply inverse narrowing from all conditions.
        apply_condition_narrowing_inverse(if_stmt.condition, scope, ctx);
        for ei in body.else_if_clauses.iter() {
            apply_condition_narrowing_inverse(ei.condition, scope, ctx);
        }
        walk_body_forward(std::iter::once(else_clause.statement), scope, ctx);
        return;
    }

    // Cursor is AFTER the if/else block.  We need to merge all branches.
    let branches = fork_if_branches(
        if_stmt,
        &dead,
        std::iter::once(body.statement),
        body.else_if_clauses
            .iter()
            .map(|ei| ElseIfArm {
                condition: ei.condition,
                stmts: std::iter::once(ei.statement),
            })
            .collect(),
        body.else_clause.as_ref().map(|else_clause| ElseArm {
            stmts: std::iter::once(else_clause.statement),
            snapshot_offset: Some(else_clause.statement.span().start.offset),
        }),
        scope,
        ctx,
    );
    merge_if_branches(if_stmt, branches, enclosing_stmt, scope, ctx);
}

/// Process if with colon-delimited body.
pub(crate) fn process_if_colon_body<'b>(
    if_stmt: &'b If<'b>,
    body: &'b IfColonDelimitedBody<'b>,
    enclosing_stmt: &'b Statement<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Simplified handling for colon-delimited if.
    // Check if cursor is inside the then-body.
    let then_end = if !body.else_if_clauses.is_empty() {
        body.else_if_clauses
            .first()
            .unwrap()
            .elseif
            .span()
            .start
            .offset
    } else if let Some(ref ec) = body.else_clause {
        ec.r#else.span().start.offset
    } else {
        body.endif.span().start.offset
    };

    let then_start = body.colon.start.offset;
    let cursor_in_then = ctx.cursor_offset >= then_start && ctx.cursor_offset < then_end;

    // Which branches a decidable guard rules out.  See the brace-body
    // variant for why this runs before the cursor dispatch.
    let dead = dead_if_branches(
        if_stmt.condition,
        body.else_if_clauses.iter().map(|ei| ei.condition),
        body.else_clause.is_some(),
        ctx,
    );
    if dead.any() {
        if dead.then_branch {
            record_statements_unreachable(body.statements.iter());
        }
        for (ei, ei_dead) in body.else_if_clauses.iter().zip(dead.else_if_clauses.iter()) {
            if *ei_dead {
                record_statements_unreachable(ei.statements.iter());
            }
        }
        if dead.else_clause
            && let Some(ref else_clause) = body.else_clause
        {
            record_statements_unreachable(else_clause.statements.iter());
        }
    }

    if cursor_in_then {
        apply_condition_narrowing(if_stmt.condition, scope, ctx);
        walk_body_forward(body.statements.iter(), scope, ctx);
        return;
    }

    // Cursor inside an elseif's own condition (before its `:`): only the
    // if condition and strictly preceding elseif conditions are known
    // false here — this elseif's own condition and every branch body are
    // not yet in effect.  See the brace-body variant above for why this
    // case must be handled separately from the body case below.
    for (idx, ei) in body.else_if_clauses.iter().enumerate() {
        let cond_span = ei.condition.span();
        if ctx.cursor_offset >= cond_span.start.offset && ctx.cursor_offset <= cond_span.end.offset
        {
            apply_condition_narrowing_inverse(if_stmt.condition, scope, ctx);
            for prev_ei in body.else_if_clauses.iter().take(idx) {
                apply_condition_narrowing_inverse(prev_ei.condition, scope, ctx);
            }
            return;
        }
    }

    for (idx, ei) in body.else_if_clauses.iter().enumerate() {
        let ei_start = ei.colon.start.offset;
        let ei_end = ei
            .statements
            .last()
            .map(|s| s.span().end.offset)
            .unwrap_or(ei_start);
        if ctx.cursor_offset >= ei_start && ctx.cursor_offset <= ei_end {
            apply_condition_narrowing_inverse(if_stmt.condition, scope, ctx);
            for prev_ei in body.else_if_clauses.iter().take(idx) {
                apply_condition_narrowing_inverse(prev_ei.condition, scope, ctx);
            }
            process_nested_assignments(ei.condition, scope, ctx);
            seed_pass_by_ref_in_condition(ei.condition, scope, ctx);
            apply_condition_narrowing(ei.condition, scope, ctx);
            walk_body_forward(ei.statements.iter(), scope, ctx);
            return;
        }
    }

    if let Some(ref else_clause) = body.else_clause {
        let ec_start = else_clause.colon.start.offset;
        let ec_end = else_clause
            .statements
            .last()
            .map(|s| s.span().end.offset)
            .unwrap_or(ec_start);
        if ctx.cursor_offset >= ec_start && ctx.cursor_offset <= ec_end {
            apply_condition_narrowing_inverse(if_stmt.condition, scope, ctx);
            for ei in body.else_if_clauses.iter() {
                apply_condition_narrowing_inverse(ei.condition, scope, ctx);
            }
            walk_body_forward(else_clause.statements.iter(), scope, ctx);
            return;
        }
    }

    // Cursor is after the if — merge branches.
    let branches = fork_if_branches(
        if_stmt,
        &dead,
        body.statements.iter(),
        body.else_if_clauses
            .iter()
            .map(|ei| ElseIfArm {
                condition: ei.condition,
                stmts: ei.statements.iter(),
            })
            .collect(),
        body.else_clause.as_ref().map(|else_clause| ElseArm {
            stmts: else_clause.statements.iter(),
            snapshot_offset: else_clause
                .statements
                .first()
                .map(|first_stmt| first_stmt.span().start.offset),
        }),
        scope,
        ctx,
    );
    merge_if_branches(if_stmt, branches, enclosing_stmt, scope, ctx);
}

/// An `elseif` arm of an `if` chain, as either body spelling presents it.
struct ElseIfArm<'b, I> {
    condition: &'b Expression<'b>,
    stmts: I,
}

/// The `else` arm of an `if` chain, as either body spelling presents it.
struct ElseArm<I> {
    stmts: I,
    /// Where a diagnostic-scope snapshot of the arm's entry state is
    /// recorded: the arm's first statement, when it has one.
    snapshot_offset: Option<u32>,
}

/// Walk every arm of an `if` chain the cursor sits after, each from its
/// own copy of `scope`, so [`merge_if_branches`] can join them.
///
/// Both spellings of an `if` (braced and `:`-delimited) fork the same way
/// and differ only in how they reach each arm's statements, which is what
/// the `I` iterator abstracts over.  A branch the guard rules out is still
/// walked (the cursor may be inside it), but it is marked so the join drops
/// what it established.
fn fork_if_branches<'b, I>(
    if_stmt: &'b If<'b>,
    dead: &DeadIfBranches,
    then_stmts: I,
    else_ifs: Vec<ElseIfArm<'b, I>>,
    else_arm: Option<ElseArm<I>>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> IfBranchScopes<'b>
where
    I: Iterator<Item = &'b Statement<'b>> + std::clone::Clone,
{
    let pre_if_scope = scope.clone();

    let mut then_scope = scope.clone();
    then_scope.unreachable |= dead.then_branch;
    apply_condition_narrowing(if_stmt.condition, &mut then_scope, ctx);
    walk_body_forward(then_stmts.clone(), &mut then_scope, ctx);
    let then_exits = branch_exits_stmts(then_stmts, &then_scope, ctx);

    let mut elseif_scopes: Vec<(ScopeState, bool)> = Vec::with_capacity(else_ifs.len());
    let mut else_if_conditions: Vec<&'b Expression<'b>> = Vec::with_capacity(else_ifs.len());
    for (ei_idx, arm) in else_ifs.into_iter().enumerate() {
        let mut ei_scope = pre_if_scope.clone();
        ei_scope.unreachable |= dead.else_if_clauses[ei_idx];
        // The elseif branch only runs when the if condition and every
        // preceding elseif condition were false, so apply their inverse
        // narrowing before walking this branch.
        apply_condition_narrowing_inverse(if_stmt.condition, &mut ei_scope, ctx);
        for prev_condition in &else_if_conditions {
            apply_condition_narrowing_inverse(prev_condition, &mut ei_scope, ctx);
        }
        // Record a scope snapshot at the elseif condition boundary so
        // that diagnostic variable lookups inside the condition don't
        // pick up assignments from preceding if/elseif bodies.
        if is_diagnostic_scope_active() {
            record_scope_snapshot(arm.condition.span().start.offset, &ei_scope);
        }
        // An `elseif`'s own `&&` / `||` chain narrows its later operands
        // just as the leading `if`'s does.
        record_short_circuit_snapshots(arm.condition, &ei_scope, ctx);
        process_nested_assignments(arm.condition, &mut ei_scope, ctx);
        seed_pass_by_ref_in_condition(arm.condition, &mut ei_scope, ctx);
        apply_condition_narrowing(arm.condition, &mut ei_scope, ctx);
        walk_body_forward(arm.stmts.clone(), &mut ei_scope, ctx);
        let exits = branch_exits_stmts(arm.stmts, &ei_scope, ctx);
        elseif_scopes.push((ei_scope, exits));
        else_if_conditions.push(arm.condition);
    }

    let else_branch = else_arm.map(|arm| {
        let mut else_scope = pre_if_scope.clone();
        else_scope.unreachable |= dead.else_clause;
        // The else branch only runs when the if condition and every
        // elseif condition were false, so apply the inverse of all of
        // them.
        apply_condition_narrowing_inverse(if_stmt.condition, &mut else_scope, ctx);
        for condition in &else_if_conditions {
            apply_condition_narrowing_inverse(condition, &mut else_scope, ctx);
        }
        // Record a scope snapshot at the else boundary so that
        // diagnostic variable lookups inside the else body don't
        // pick up assignments from the if/elseif bodies.
        if is_diagnostic_scope_active()
            && let Some(offset) = arm.snapshot_offset
        {
            record_scope_snapshot(offset, &else_scope);
        }
        walk_body_forward(arm.stmts.clone(), &mut else_scope, ctx);
        let exits = branch_exits_stmts(arm.stmts, &else_scope, ctx);
        (else_scope, exits)
    });

    IfBranchScopes {
        pre_if: pre_if_scope,
        then_branch: (then_scope, then_exits),
        else_ifs: elseif_scopes,
        else_branch,
        else_if_conditions,
    }
}

/// The branch scopes an `if` chain produced, with whether each of them
/// reaches the statement after the chain.
struct IfBranchScopes<'e> {
    /// The scope as it stood before the `if`.
    pre_if: ScopeState,
    /// The then-body's scope, and whether it exits.
    then_branch: (ScopeState, bool),
    /// One entry per `elseif`, in source order.
    else_ifs: Vec<(ScopeState, bool)>,
    /// `None` when the chain has no `else` clause.
    else_branch: Option<(ScopeState, bool)>,
    /// Each `elseif` condition, for the inverse narrowing the implicit
    /// fall-through path carries.
    else_if_conditions: Vec<&'e Expression<'e>>,
}

/// Join the branch scopes of an `if` chain back into `scope`, and apply
/// the narrowing a guard clause leaves behind.
///
/// Both spellings of an `if` (braced and `:`-delimited) reconverge the
/// same way, so they share this half; they differ only in how they reach
/// the branches in the first place.
///
/// A branch that returns, throws, or jumps out of the enclosing loop does
/// not reach the statement after the `if`, so it contributes nothing
/// here; a `break`/`continue` branch reaches the loop's own join instead,
/// which `record_exit_edge` has already been handed.
fn merge_if_branches(
    if_stmt: &If<'_>,
    branches: IfBranchScopes<'_>,
    enclosing_stmt: &Statement<'_>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let IfBranchScopes {
        pre_if,
        then_branch: (then_scope, then_exits),
        else_ifs,
        else_branch,
        else_if_conditions,
    } = branches;
    let pre_if_unreachable = pre_if.unreachable;

    let mut implicit_else_scope;
    let mut surviving_scopes: Vec<&ScopeState> = Vec::new();

    if !then_exits {
        surviving_scopes.push(&then_scope);
    }
    for (ei_scope, ei_exits) in else_ifs.iter() {
        if !ei_exits {
            surviving_scopes.push(ei_scope);
        }
    }
    match &else_branch {
        Some((es, else_exits)) => {
            if !else_exits {
                surviving_scopes.push(es);
            }
        }
        None => {
            // No else clause — the pre-if scope is an implicit surviving
            // path.  Falling out of the bottom means every condition in
            // the chain was false, so each one's inverse narrowing holds
            // here (e.g. `$a["test"] === null` → `$a["test"]` is NOT null
            // in the implicit else path).
            //
            // The leading condition is the exception: when the then-body
            // exits and there is no `elseif`, the dedicated guard clause
            // section below applies its inverse to the merged scope, and
            // applying it in both places would double-narrow.  With an
            // `elseif` present that section bails out, so this is the only
            // place the fall-through path learns the leading condition was
            // false.
            implicit_else_scope = pre_if.clone();
            if !then_exits || !else_if_conditions.is_empty() {
                apply_condition_narrowing_inverse(if_stmt.condition, &mut implicit_else_scope, ctx);
            }
            for condition in &else_if_conditions {
                apply_condition_narrowing_inverse(condition, &mut implicit_else_scope, ctx);
            }
            // The implicit else path precedes the then-body in source
            // order, so it goes first: the merge below preserves this
            // order in each variable's type list, and hover renders the
            // first entry as the headline type.
            surviving_scopes.insert(0, &implicit_else_scope);
        }
    }

    // A branch whose condition proved impossible describes a run that
    // cannot happen.  Dropping it is what makes a reassignment inside
    // `if ($v instanceof AbstractNode) { $v = $v->getNode(); }` the
    // post-if type of `$v` when `$v` was already an `AbstractNode`: the
    // implicit else has no value to carry.  If every path is impossible
    // the whole `if` is, and the pre-if scope is the least surprising
    // answer.
    if surviving_scopes.iter().any(|s| !s.unreachable) {
        surviving_scopes.retain(|s| !s.unreachable);
    }

    if surviving_scopes.is_empty() {
        // Every branch returns, throws, or jumps, and the branches cover
        // every case: nothing falls out of the bottom of this `if`.  The
        // pre-if types are the least surprising answer for a cursor in
        // the dead code that follows, but a join further out must not
        // count this path — an enclosing loop whose body always `break`s
        // has no fall-through edge, only the break edges.
        *scope = pre_if;
        scope.unreachable = true;
        return;
    } else if surviving_scopes.len() == 1 {
        *scope = surviving_scopes[0].clone();
    } else {
        let mut merged = surviving_scopes[0].clone();
        for s in &surviving_scopes[1..] {
            merged.merge_branch(s);
        }
        // Simplify unions where a child class is merged with its
        // parent — e.g. `ClassResolvesBackChild | ClassResolvesBack`
        // collapses to `ClassResolvesBack`.
        simplify_class_hierarchy_unions(&mut merged, ctx.class_loader);
        *scope = merged;
    }

    // Drop synthetic property access keys that only some branches
    // established: those represent narrowing (or an assignment) that
    // holds within one branch and says nothing about the others.  Keys
    // every surviving path carries are kept, so their merged union is
    // the type the property has once the branches reconverge.  This
    // must run BEFORE guard clause narrowing so that
    // guard-clause-narrowed property keys (e.g. `$this->model`
    // narrowed to `Order` after
    // `if (!$this->model instanceof Order) { return; }`) survive into
    // the post-if scope.
    retain_synthetic_keys_common_to_all(scope, &surviving_scopes);

    // Impossibility is a property of one branch's path conditions, not of
    // the join: the statement after the `if` is reached by whichever branch
    // *was* possible.  Restoring the pre-if reachability keeps a dropped
    // branch from erasing the rest of the walk.  The guard clause narrowing
    // below runs after the restore because what *it* proves impossible is a
    // property of the continuation, not of a branch that was dropped.
    scope.unreachable = pre_if_unreachable;

    // Guard clause narrowing: when the if body unconditionally exits
    // and there are no elseif/else branches, apply inverse narrowing.
    // This applies to ALL exit types (return, throw, break, continue)
    // because the code after the if in the current scope does not
    // execute in that path.
    if enclosing_stmt.span().end.offset < ctx.cursor_offset
        && then_exits
        && else_if_conditions.is_empty()
        && else_branch.is_none()
    {
        apply_condition_narrowing_inverse(if_stmt.condition, scope, ctx);
        apply_guard_clause_null_narrowing(if_stmt, scope, ctx);
    }
}

/// Type a method-call receiver that is not a plain variable, so that a
/// guard body ending in `app()->abort()` or `$this->aborter->fail()`
/// terminates the branch.
///
/// The scope is read as a snapshot: resolution goes through the shared
/// RHS pipeline with the walker's in-progress scope injected as the
/// variable resolver, so it answers from types already established
/// rather than re-walking the body it was called from.
fn resolved_receiver_class_names(
    expr: &Expression<'_>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<String> {
    narrowing::class_names_of(&resolve_rhs_with_scope(expr, scope, ctx))
}

/// Check whether an if/elseif/else branch terminates, so its assignments
/// must not be merged into the post-if scope.  A branch exits if any
/// statement in it exits, matching `if_body_unconditionally_exits`; a
/// braced branch is the one (possibly block) statement it holds.
///
/// The branch's own scope is passed along so that a `never`-returning
/// method called on a local variable (`$aborter->fail()`) is recognised,
/// not just `$this->fail()`.
pub(crate) fn branch_exits_stmts<'s>(
    mut stmts: impl Iterator<Item = &'s Statement<'s>>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let var_types = |var_name: &str| scope.get(var_name).to_vec();
    let receiver_resolver = |expr: &Expression<'_>| resolved_receiver_class_names(expr, scope, ctx);
    let exit_ctx = narrowing::ExitCtx {
        current_class: ctx.current_class,
        class_loader: ctx.class_loader,
        function_loader: ctx.loaders.function_loader,
        resolved_class_cache: ctx.resolved_class_cache,
        var_types: Some(&var_types),
        receiver_resolver: Some(&receiver_resolver),
    };
    stmts.any(|s| narrowing::statement_unconditionally_exits(s, &exit_ctx))
}
