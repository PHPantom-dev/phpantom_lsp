use super::*;

use mago_span::HasSpan;

use crate::type_engine::types::narrowing;

// ─── Control flow handling ──────────────────────────────────────────────────

/// Process an `if` statement with branch merging.
pub(crate) fn process_if<'b>(
    if_stmt: &'b If<'b>,
    enclosing_stmt: &'b Statement<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Assignment in condition: `if ($x = expr())`, pass-by-reference
    // (`if (preg_match(..., $matches))`), and the narrowing a `&&` / `||`
    // chain proves for its later operands: `if ($x !== null &&
    // $x->method())`.
    let condition = process_condition(if_stmt.condition, scope, ctx);

    // A cursor inside the condition is answered by the scope the condition
    // walk reached it with; nothing the branches do is in force there yet.
    let cond_span = if_stmt.condition.span();
    if ctx.cursor_offset >= cond_span.start.offset && ctx.cursor_offset <= cond_span.end.offset {
        return;
    }

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
            process_if_statement_body(if_stmt, body, enclosing_stmt, &condition, scope, ctx);
        }
        IfBody::ColonDelimited(body) => {
            process_if_colon_body(if_stmt, body, enclosing_stmt, &condition, scope, ctx);
        }
    }
}

/// The scope an arm of an `if` chain is reached on: every condition
/// before it was evaluated and came out false.
///
/// `condition` is the leading `if`'s; `prior` are the `elseif` conditions
/// between it and the arm, each evaluated on the scope the one before it
/// failed in.
fn falsey_through<'b>(
    condition: &Condition<'_>,
    prior: impl Iterator<Item = &'b Expression<'b>>,
    ctx: &ForwardWalkCtx<'_>,
) -> ScopeState {
    let mut path = condition.falsey(ctx).clone();
    for prior in prior {
        let prior = process_condition(prior, &mut path, ctx);
        path = prior.falsey(ctx).clone();
    }
    path
}

/// Process if with statement body (brace-style).
pub(crate) fn process_if_statement_body<'b>(
    if_stmt: &'b If<'b>,
    body: &'b IfStatementBody<'b>,
    enclosing_stmt: &'b Statement<'b>,
    condition: &Condition<'b>,
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
    // strictly preceding elseif condition were false to reach here, and
    // this elseif's own condition is walked from there, so the scope it
    // reaches the cursor with answers.  The if/preceding-elseif bodies
    // never ran: without this case the cursor falls through to the "after
    // the whole chain" merge below, which pulls in assignments from the
    // if-body (e.g. `if (...) { $value = true; } elseif (foo($value)) { ...
    // }` must not see `$value` as `T|bool` while evaluating `foo($value)`).
    for (idx, ei) in body.else_if_clauses.iter().enumerate() {
        let cond_span = ei.condition.span();
        if ctx.cursor_offset >= cond_span.start.offset && ctx.cursor_offset <= cond_span.end.offset
        {
            *scope = falsey_through(
                condition,
                body.else_if_clauses.iter().take(idx).map(|ei| ei.condition),
                ctx,
            );
            process_condition(ei.condition, scope, ctx);
            return;
        }
    }

    if cursor_in_then {
        // Cursor is inside the then-branch: walk only this branch, on the
        // scope where the condition held.
        *scope = condition.truthy(ctx).clone();
        walk_body_forward(std::iter::once(body.statement), scope, ctx);
        return;
    }

    if cursor_in_elseif {
        // Find which elseif contains the cursor.
        for (idx, ei) in body.else_if_clauses.iter().enumerate() {
            let sp = ei.statement.span();
            if ctx.cursor_offset >= sp.start.offset && ctx.cursor_offset <= sp.end.offset {
                // Every condition above this one failed, and this one held.
                let mut path = falsey_through(
                    condition,
                    body.else_if_clauses.iter().take(idx).map(|ei| ei.condition),
                    ctx,
                );
                let arm = process_condition(ei.condition, &mut path, ctx);
                *scope = arm.truthy(ctx).clone();
                walk_body_forward(std::iter::once(ei.statement), scope, ctx);
                return;
            }
        }
        return;
    }

    if cursor_in_else && let Some(ref else_clause) = body.else_clause {
        // Every condition in the chain failed.
        *scope = falsey_through(
            condition,
            body.else_if_clauses.iter().map(|ei| ei.condition),
            ctx,
        );
        walk_body_forward(std::iter::once(else_clause.statement), scope, ctx);
        return;
    }

    // Cursor is AFTER the if/else block.  We need to merge all branches.
    let branches = fork_if_branches(
        condition,
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
    condition: &Condition<'b>,
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
        *scope = condition.truthy(ctx).clone();
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
            *scope = falsey_through(
                condition,
                body.else_if_clauses.iter().take(idx).map(|ei| ei.condition),
                ctx,
            );
            process_condition(ei.condition, scope, ctx);
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
            let mut path = falsey_through(
                condition,
                body.else_if_clauses.iter().take(idx).map(|ei| ei.condition),
                ctx,
            );
            let arm = process_condition(ei.condition, &mut path, ctx);
            *scope = arm.truthy(ctx).clone();
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
            *scope = falsey_through(
                condition,
                body.else_if_clauses.iter().map(|ei| ei.condition),
                ctx,
            );
            walk_body_forward(else_clause.statements.iter(), scope, ctx);
            return;
        }
    }

    // Cursor is after the if — merge branches.
    let branches = fork_if_branches(
        condition,
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

/// Walk every arm of an `if` chain the cursor sits after, each from the
/// scope it is reached on, so [`merge_if_branches`] can join them.
///
/// Both spellings of an `if` (braced and `:`-delimited) fork the same way
/// and differ only in how they reach each arm's statements, which is what
/// the `I` iterator abstracts over.  A branch the guard rules out is still
/// walked (the cursor may be inside it), but it is marked so the join drops
/// what it established.
///
/// An `elseif` is evaluated only when every condition before it failed, so
/// it runs on the scope the one before it failed in, and so does whatever
/// follows it: the `else`, or the fall-through when there is none.
fn fork_if_branches<'b, I>(
    condition: &Condition<'_>,
    dead: &DeadIfBranches,
    then_stmts: I,
    else_ifs: Vec<ElseIfArm<'b, I>>,
    else_arm: Option<ElseArm<I>>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> IfBranchScopes
where
    I: Iterator<Item = &'b Statement<'b>> + std::clone::Clone,
{
    let mut then_scope = condition.truthy(ctx).clone();
    then_scope.unreachable |= dead.then_branch;
    walk_body_forward(then_stmts.clone(), &mut then_scope, ctx);
    let then_exits = branch_exits_stmts(then_stmts, &then_scope, ctx);

    let has_else_ifs = !else_ifs.is_empty();
    let mut path = condition.falsey(ctx).clone();
    let mut elseif_scopes: Vec<(ScopeState, bool)> = Vec::with_capacity(else_ifs.len());
    for (ei_idx, arm) in else_ifs.into_iter().enumerate() {
        // Record a scope snapshot at the elseif condition boundary so
        // that diagnostic variable lookups inside the condition don't
        // pick up assignments from preceding if/elseif bodies.
        if is_diagnostic_scope_active() {
            record_scope_snapshot(arm.condition.span().start.offset, &path);
        }
        let arm_condition = process_condition(arm.condition, &mut path, ctx);
        let mut ei_scope = arm_condition.truthy(ctx).clone();
        ei_scope.unreachable |= dead.else_if_clauses[ei_idx];
        walk_body_forward(arm.stmts.clone(), &mut ei_scope, ctx);
        let exits = branch_exits_stmts(arm.stmts, &ei_scope, ctx);
        elseif_scopes.push((ei_scope, exits));
        path = arm_condition.falsey(ctx).clone();
    }

    let else_branch = else_arm.map(|arm| {
        let mut else_scope = path.clone();
        else_scope.unreachable |= dead.else_clause;
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
        pre_if_unreachable: scope.unreachable,
        then_branch: (then_scope, then_exits),
        else_ifs: elseif_scopes,
        else_branch,
        fall_through: path,
        has_else_ifs,
    }
}

/// The branch scopes an `if` chain produced, with whether each of them
/// reaches the statement after the chain.
struct IfBranchScopes {
    /// Whether the `if` itself could not be reached.
    pre_if_unreachable: bool,
    /// The then-body's scope, and whether it exits.
    then_branch: (ScopeState, bool),
    /// One entry per `elseif`, in source order.
    else_ifs: Vec<(ScopeState, bool)>,
    /// `None` when the chain has no `else` clause.
    else_branch: Option<(ScopeState, bool)>,
    /// The scope where every condition in the chain failed, which is the
    /// path out of the bottom when there is no `else`.
    fall_through: ScopeState,
    /// Whether the chain has an `elseif`.
    has_else_ifs: bool,
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
    branches: IfBranchScopes,
    enclosing_stmt: &Statement<'_>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let IfBranchScopes {
        pre_if_unreachable,
        then_branch: (then_scope, then_exits),
        else_ifs,
        else_branch,
        fall_through,
        has_else_ifs,
    } = branches;

    // A guard clause: the then-body leaves and nothing else is written, so
    // the code after the `if` is reached only where the condition failed.
    let guard = then_exits && !has_else_ifs && else_branch.is_none();

    // The arms whose condition held, in source order.
    let mut arms: Vec<&ScopeState> = Vec::new();
    if !then_exits {
        arms.push(&then_scope);
    }
    for (ei_scope, ei_exits) in else_ifs.iter() {
        if !ei_exits {
            arms.push(ei_scope);
        }
    }
    // The path on which every condition failed: the `else`, or falling out
    // of the bottom when there is none (e.g. `$a["test"] === null` →
    // `$a["test"]` is NOT null in the implicit else path).
    let mut rest: Option<&ScopeState> = match &else_branch {
        Some((es, else_exits)) => (!else_exits).then_some(es),
        None => Some(&fall_through),
    };

    // A branch whose condition proved impossible describes a run that
    // cannot happen.  Dropping it is what makes a reassignment inside
    // `if ($v instanceof AbstractNode) { $v = $v->getNode(); }` the
    // post-if type of `$v` when `$v` was already an `AbstractNode`: the
    // implicit else has no value to carry.  If every path is impossible
    // the whole `if` is, and the pre-if scope is the least surprising
    // answer.
    if arms.iter().chain(&rest).any(|s| !s.unreachable) {
        arms.retain(|s| !s.unreachable);
        rest = rest.filter(|s| !s.unreachable);
    }

    // The implicit else path precedes the then-body in source order, so it
    // goes first: the join below preserves this order in each variable's
    // type list, and hover renders the first entry as the headline type.
    let rest_first = else_branch.is_none();
    let surviving_scopes: Vec<&ScopeState> = match rest {
        Some(rest) if rest_first => std::iter::once(rest).chain(arms.iter().copied()).collect(),
        _ => arms.iter().copied().chain(rest).collect(),
    };

    let Some(mut merged) = join_if_paths(&arms, rest, rest_first) else {
        // Every branch returns, throws, or jumps, and the branches cover
        // every case: nothing falls out of the bottom of this `if`.  The
        // pre-if types (still in `scope`) are the least surprising answer
        // for a cursor in the dead code that follows, but a join further
        // out must not count this path — an enclosing loop whose body
        // always `break`s has no fall-through edge, only the break edges.
        scope.unreachable = true;
        return;
    };
    if surviving_scopes.len() > 1 {
        // Simplify unions where a child class is merged with its
        // parent — e.g. `ClassResolvesBackChild | ClassResolvesBack`
        // collapses to `ClassResolvesBack`.
        simplify_class_hierarchy_unions(&mut merged, &surviving_scopes[0].locals, ctx.class_loader);
    }
    *scope = merged;

    // Drop synthetic property access keys that only some branches
    // established: those represent narrowing (or an assignment) that
    // holds within one branch and says nothing about the others.  Keys
    // every surviving path carries are kept, so their merged union is
    // the type the property has once the branches reconverge.  A guard
    // clause's fall-through is the only surviving path, so what it proved
    // (e.g. `$this->model` narrowed to `Order` after
    // `if (!$this->model instanceof Order) { return; }`) survives into the
    // post-if scope.
    retain_synthetic_keys_common_to_all(scope, &surviving_scopes);

    if guard {
        // What the failed condition proves impossible is a property of the
        // continuation, not of a branch that was dropped, so the
        // fall-through's reachability stands.
        //
        // When the if body unconditionally exits and there are no
        // elseif/else branches, the code after the `if` does not run on
        // that path.  This applies to ALL exit types (return, throw, break,
        // continue).
        if enclosing_stmt.span().end.offset < ctx.cursor_offset {
            apply_guard_clause_null_narrowing(if_stmt, scope, ctx);
        }
    } else {
        // Impossibility is a property of one branch's path conditions, not
        // of the join: the statement after the `if` is reached by whichever
        // branch *was* possible.  Restoring the pre-if reachability keeps a
        // dropped branch from erasing the rest of the walk.
        scope.unreachable = pre_if_unreachable;
    }
}

/// Join the paths out of an `if` chain: the arms whose condition held, in
/// source order, and `rest`, the path on which every condition failed.
/// `None` when no path reaches the end of the chain.
///
/// The arms are joined with one another before `rest` is joined with the
/// result. A join learns proofs from whatever tells its two sides apart,
/// so joining `rest` with every arm at once is what lets a later test that
/// the chain did not fall through re-apply what its conditions proved
/// together: past `if ($c === 'C0') { $name = 'N0'; } elseif …
/// else { $name = 'Unknown'; }`, `$name !== 'Unknown'` narrows `$c` to the
/// literals the arms compared it with.
///
/// The arms themselves are joined pairwise (see [`ScopeState::join_all`]).
/// Folding them into one accumulator instead would recheck every proof it
/// had gathered at each arm, which made a lookup table written as a few
/// thousand `elseif`s take minutes.
///
/// `rest_first` puts `rest` ahead of the arms, as the fall-through of a
/// chain with no `else` comes before them in source order.
fn join_if_paths(
    arms: &[&ScopeState],
    rest: Option<&ScopeState>,
    rest_first: bool,
) -> Option<ScopeState> {
    let arms = ScopeState::join_all(arms.iter().map(|&arm| arm.clone()).collect());
    match (arms, rest) {
        (Some(arms), Some(rest)) if rest_first => {
            let mut joined = rest.clone();
            joined.merge_branch(&arms);
            Some(joined)
        }
        (Some(mut arms), Some(rest)) => {
            arms.merge_branch(rest);
            Some(arms)
        }
        (arms, rest) => arms.or_else(|| rest.cloned()),
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
/// must not be merged into the post-if scope.  A branch exits when
/// control is gone by its end (see `statements_unconditionally_exit`); a
/// braced branch is the one (possibly block) statement it holds.
///
/// The branch's own scope is passed along so that a `never`-returning
/// method called on a local variable (`$aborter->fail()`) is recognised,
/// not just `$this->fail()`.
pub(crate) fn branch_exits_stmts<'s>(
    stmts: impl Iterator<Item = &'s Statement<'s>>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let var_types = |var_name: &str| scope.get(var_name).to_vec();
    let receiver_resolver = |expr: &Expression<'_>| resolved_receiver_class_names(expr, scope, ctx);
    let exit_ctx = narrowing::ExitCtx {
        current_class: ctx.current_class,
        all_classes: ctx.all_classes,
        class_loader: ctx.class_loader,
        function_loader: ctx.loaders.function_loader,
        resolved_class_cache: ctx.resolved_class_cache,
        var_types: Some(&var_types),
        receiver_resolver: Some(&receiver_resolver),
    };
    narrowing::statements_unconditionally_exit(stmts, &exit_ctx)
}
