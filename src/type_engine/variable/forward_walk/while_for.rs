use super::*;

use mago_span::HasSpan;

/// Process a `while` loop.
///
/// Uses the same two-pass strategy as `process_foreach` and
/// `process_for`: the first pass discovers all variable assignments
/// inside the loop body, the results are merged back into the
/// pre-loop scope, and the final pass re-walks with full visibility
/// of loop-carried assignments.
pub(crate) fn process_while<'b>(
    while_stmt: &'b While<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let depth_guard = LoopDepthGuard::enter();
    let loop_depth = depth_guard.depth();

    // Hard limit: skip the body entirely at excessive nesting depth.
    if loop_depth > MAX_LOOP_DEPTH {
        return;
    }

    let pre_loop_scope = scope.clone();

    // The condition runs before every iteration, and the body only when
    // it held: `while (($line = fgets($h)) !== false)` assigns `$line` and
    // strips the sentinel from it in one go, and `while (preg_match(...,
    // $matches))` seeds `$matches` for the body to read.
    let entry = process_condition(while_stmt.condition, scope, ctx);
    *scope = entry.truthy(ctx).clone();

    // When the cursor is inside the loop body (completion path), discovery
    // passes must walk the ENTIRE body; the final pass uses the real
    // cursor_offset so it stops at the cursor as usual.
    let body_span = match &while_stmt.body {
        WhileBody::Statement(inner) => inner.span(),
        WhileBody::ColonDelimited(body) => body.span(),
    };
    let cursor_in_body =
        ctx.cursor_offset >= body_span.start.offset && ctx.cursor_offset <= body_span.end.offset;
    let discovery_ctx = (if cursor_in_body && !is_diagnostic_scope_active() {
        ctx.with_cursor_offset(u32::MAX)
    } else {
        ctx.with_cursor_offset(ctx.cursor_offset)
    })
    .with_in_loop(true);
    let loop_body_ctx = ctx.with_in_loop(true);

    // Record a snapshot after condition processing (same reasoning as
    // the corresponding snapshot in `process_if`).
    if is_diagnostic_scope_active() {
        let body_start = match &while_stmt.body {
            WhileBody::Statement(inner) => inner.span().start.offset,
            WhileBody::ColonDelimited(body) => body.colon.start.offset,
        };
        record_scope_snapshot(body_start, scope);
    }

    // ── Assignment-depth-bounded loop iteration ─────────────────
    let body_stmts: Vec<&Statement<'b>> = match &while_stmt.body {
        WhileBody::Statement(inner) => vec![*inner],
        WhileBody::ColonDelimited(body) => body.statements.iter().collect(),
    };
    let assignment_depth =
        clamp_iterations_for_depth(assignment_map_depth(&body_stmts), loop_depth);

    let exit_frame = ExitFrameGuard::push();
    walk_loop_body_to_fixed_point(
        &body_stmts,
        scope,
        LoopWalk {
            pre_loop_scope: &pre_loop_scope,
            assignment_depth,
            fold_exit_edges: !cursor_in_body,
            ctx: &loop_body_ctx,
            discovery_ctx: &discovery_ctx,
        },
        |next_scope, point| {
            if point != LoopSeedPoint::Entry {
                return;
            }
            let entry = process_condition(while_stmt.condition, next_scope, ctx);
            *next_scope = entry.truthy(ctx).clone();
        },
    );
    let exits = exit_frame.pop();

    // When the cursor is inside the loop body (completion path), keep
    // the scope with condition narrowing applied.  The post-loop
    // merge would erase the narrowing (since the loop might not execute),
    // but the cursor IS inside the body, so the condition is true.
    if cursor_in_body && !is_diagnostic_scope_active() {
        return;
    }

    // The loop leaves when the condition fails, which it does either on
    // the first test or on the one after an iteration: `while ($a) { $a =
    // $a->parent; }` leaves `$a` null.  The test after the last iteration
    // is the same expression at the same offsets, and what the walk
    // recorded there (diagnostic snapshots, the scope at a cursor) already
    // describes every time it runs.
    let mut exited = {
        let _suspend = suspend_snapshot_recording();
        let retest_ctx = ctx.with_cursor_offset(u32::MAX);
        let retest = process_condition(while_stmt.condition, scope, &retest_ctx);
        retest.falsey(&retest_ctx).clone()
    };
    // A loop that started out unreachable has no live "never ran"
    // alternative to protect: the first test's failure is exactly as dead
    // as what the body produced, so joining it would only let
    // `merge_branch`'s "an unreachable side contributes nothing" rule
    // discard whatever the body actually assigned.  See the matching
    // comment in `process_foreach`.
    if !pre_loop_scope.unreachable {
        let mut never_ran = entry.falsey(ctx).clone();
        never_ran.merge_branch(&exited);
        exited = never_ran;
    }
    *scope = exited;

    // A `break` leaves without re-testing the condition, so its state
    // joins *after* the inverse narrowing rather than being narrowed by it.
    merge_exit_edges(scope, &exits.breaks);

    // Remove synthetic property access keys that were seeded by
    // condition narrowing.  These represent narrowed types that only
    // hold inside the loop body (where the condition is true).
    // After the loop, the condition may be false, so the narrowing
    // no longer applies.
    strip_synthetic_property_keys(scope);
}

/// Process a `for` loop.
///
/// Uses the same assignment-depth-bounded iteration as `process_foreach`:
/// a cheap AST walk determines the dependency chain depth, then the body
/// is re-walked up to that many times with fixed-point early exit.
pub(crate) fn process_for<'b>(
    for_stmt: &'b For<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let depth_guard = LoopDepthGuard::enter();
    let loop_depth = depth_guard.depth();

    // Hard limit: skip the body entirely at excessive nesting depth.
    if loop_depth > MAX_LOOP_DEPTH {
        return;
    }

    // Process initializer expressions (e.g. `$i = 0`).
    for init_expr in for_stmt.initializations.iter() {
        process_assignment_expr(init_expr, scope, ctx);
    }

    // Process condition assignments (e.g. `for (; $x = nextItem(); )`),
    // the narrowing a clause's `&&` / `||` chain proves for its own later
    // operands (`for (; $n && $n->next(); )`), and pass-by-ref in
    // conditions (e.g. `for (; preg_match(..., $m); )`).
    let entry = process_for_conditions(for_stmt, scope, ctx);

    let pre_loop_scope = scope.clone();
    let always_enters = for_condition_holds_on_entry(for_stmt, scope, ctx);

    // The body executes when the last condition clause is truthy; the
    // earlier clauses are evaluated for their side effects alone.
    if let Some(entry) = &entry {
        *scope = entry.truthy(ctx).clone();
    }

    // When the cursor is inside the loop body (completion path), discovery
    // passes must walk the ENTIRE body; the final pass uses the real
    // cursor_offset so it stops at the cursor as usual.
    let body_span = match &for_stmt.body {
        ForBody::Statement(inner) => inner.span(),
        ForBody::ColonDelimited(body) => body.span(),
    };
    let cursor_in_body =
        ctx.cursor_offset >= body_span.start.offset && ctx.cursor_offset <= body_span.end.offset;
    let discovery_ctx = (if cursor_in_body && !is_diagnostic_scope_active() {
        ctx.with_cursor_offset(u32::MAX)
    } else {
        ctx.with_cursor_offset(ctx.cursor_offset)
    })
    .with_in_loop(true);
    let loop_body_ctx = ctx.with_in_loop(true);

    // ── Assignment-depth-bounded loop iteration ─────────────────
    let body_stmts: Vec<&Statement<'b>> = match &for_stmt.body {
        ForBody::Statement(inner) => vec![*inner],
        ForBody::ColonDelimited(body) => body.statements.iter().collect(),
    };
    // The update clause is part of the loop's assignment graph: a
    // hand-walked iterator (`for (…; …; $node = $node->next)`) carries its
    // type from one iteration to the next through the increment alone, so a
    // body with no assignments of its own still needs a re-walk.
    let assignment_depth = clamp_iterations_for_depth(
        assignment_map_depth_with_updates(&body_stmts, for_stmt.increments.iter().copied()),
        loop_depth,
    );

    let exit_frame = ExitFrameGuard::push();
    walk_loop_body_to_fixed_point(
        &body_stmts,
        scope,
        LoopWalk {
            pre_loop_scope: &pre_loop_scope,
            assignment_depth,
            fold_exit_edges: !cursor_in_body,
            ctx: &loop_body_ctx,
            discovery_ctx: &discovery_ctx,
        },
        |next_scope, point| match point {
            // The update clause runs after the body, so its reassignments
            // are what the next iteration starts from.  The initialisers
            // are deliberately *not* re-run: they execute once, and
            // `pre_loop_scope` (which the entry scope is merged from)
            // already holds the types they bound.  Re-running them would
            // overwrite a loop-carried type with the first iteration's.
            LoopSeedPoint::AfterBody => {
                process_for_updates(for_stmt, next_scope, ctx);
            }
            LoopSeedPoint::Entry => {
                if let Some(entry) = process_for_conditions(for_stmt, next_scope, ctx) {
                    *next_scope = entry.truthy(ctx).clone();
                }
            }
        },
    );
    let exits = exit_frame.pop();

    // Record a snapshot at each increment expression so that member
    // accesses in the update clause (e.g. `$p = $p->next()`, also on the
    // `for` line) see the variables bound by the init clause and the loop
    // body.  The increments run after the body, so `scope` here reflects
    // both; recording before the post-loop merge keeps the in-loop types
    // rather than the widened post-loop union.
    if is_diagnostic_scope_active() {
        for increment in for_stmt.increments.iter() {
            record_scope_snapshot(increment.span().start.offset, scope);
        }
    }

    // When the cursor is inside the loop body (completion path), keep the
    // scope with condition narrowing applied.  The post-loop merge would
    // erase the narrowing (since the loop might not execute), but the
    // cursor IS inside the body, so the conditions are true there.
    if cursor_in_body && !is_diagnostic_scope_active() {
        return;
    }

    // A loop that ran its body at least once exited from the condition
    // check that follows the update clause, so the reassignments the update
    // clause makes are part of the post-loop state.
    process_for_updates(for_stmt, scope, ctx);

    // The loop leaves when its last condition clause fails (the earlier
    // clauses are evaluated for their side effects but don't gate
    // continuation), on the first test or on the one after an iteration:
    // `for (; ($row = fgetcsv($h)) !== false; )` leaves `$row` false.  As
    // in `process_while`, the re-test records no snapshots of its own.
    if let Some(entry) = entry {
        let mut exited = {
            let _suspend = suspend_snapshot_recording();
            let retest_ctx = ctx.with_cursor_offset(u32::MAX);
            match process_for_conditions(for_stmt, scope, &retest_ctx) {
                Some(retest) => retest.falsey(&retest_ctx).clone(),
                None => scope.clone(),
            }
        };
        // Unless the conditions hold on entry, the loop body might not
        // execute at all.  As in `process_while` and `process_foreach`, a
        // loop that started out unreachable has nothing live to protect by
        // keeping that path.
        if !always_enters && !pre_loop_scope.unreachable {
            let mut never_ran = entry.falsey(ctx).clone();
            never_ran.merge_branch(&exited);
            exited = never_ran;
        }
        *scope = exited;
    }

    // A `break` leaves without re-testing the condition, so its state
    // joins after the inverse narrowing.
    merge_exit_edges(scope, &exits.breaks);

    // Remove synthetic property access keys that were seeded by condition
    // narrowing; they only hold inside the loop body where the conditions
    // were true.
    strip_synthetic_property_keys(scope);
}

/// Evaluate a `for` loop's condition clauses left to right, and return
/// the last one's outcomes: only the last clause decides whether the body
/// runs.  `None` when there is no clause.
///
/// A snapshot at each clause lets member accesses in it (which live on the
/// `for` line, before any body statement) see the variables bound by the
/// init clause and by the clauses before it.
fn process_for_conditions<'b>(
    for_stmt: &'b For<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<Condition<'b>> {
    let mut last = None;
    for cond_expr in for_stmt.conditions.iter() {
        record_scope_snapshot(cond_expr.span().start.offset, scope);
        last = Some(process_condition(cond_expr, scope, ctx));
    }
    last
}

/// Whether a `for` loop's body is certain to run at least once: its last
/// condition clause (the one that decides entry) resolves to `true` in the
/// scope the initialisers leave behind, or there is no condition at all.
fn for_condition_holds_on_entry<'b>(
    for_stmt: &'b For<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let Some(last_cond) = for_stmt.conditions.iter().last() else {
        return true;
    };
    // Only a comparison or a literal can come out as `true`; anything else
    // is not worth resolving here.
    match crate::parser::unwrap_parens(last_cond) {
        Expression::Binary(binary) if binary.operator.is_comparison() => {}
        Expression::Literal(Literal::True(_)) => return true,
        _ => return false,
    }
    let resolved = resolve_rhs_with_scope(last_cond, scope, ctx);
    !resolved.is_empty() && resolved.iter().all(|rt| rt.type_string.is_true())
}

/// Apply a `for` loop's update clause to `scope`.  The clause is usually
/// an increment (`$i++`) rather than an assignment, and an increment is
/// what widens a counter's initial literal to `int` for every iteration
/// after the first.
fn process_for_updates<'b>(
    for_stmt: &'b For<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    for increment in for_stmt.increments.iter() {
        process_assignment_expr(increment, scope, ctx);
    }
}

/// Process a `do-while` loop.
///
/// Uses the same assignment-depth-bounded iteration as `process_foreach`:
/// a cheap AST walk determines the dependency chain depth, then the body
/// is re-walked up to that many times with fixed-point early exit.
///
/// Unlike `for`/`while`, the body of a `do-while` always executes at
/// least once, so we do NOT merge with a pre-loop scope at the end.
pub(crate) fn process_do_while<'b>(
    dw: &'b DoWhile<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let depth_guard = LoopDepthGuard::enter();
    let loop_depth = depth_guard.depth();

    // Hard limit: skip the body entirely at excessive nesting depth.
    if loop_depth > MAX_LOOP_DEPTH {
        return;
    }

    let pre_loop_scope = scope.clone();

    // A caller asking about a position inside the body is asking about a
    // point the loop's exit edges have not reached yet.
    let body_span = dw.statement.span();
    let cursor_in_body =
        ctx.cursor_offset >= body_span.start.offset && ctx.cursor_offset <= body_span.end.offset;

    // ── Assignment-depth-bounded loop iteration ─────────────────
    let body_stmts: Vec<&Statement<'b>> = vec![dw.statement];
    let assignment_depth =
        clamp_iterations_for_depth(assignment_map_depth(&body_stmts), loop_depth);

    let loop_body_ctx = ctx.with_in_loop(true);

    let exit_frame = ExitFrameGuard::push();
    walk_loop_body_to_fixed_point(
        &body_stmts,
        scope,
        LoopWalk {
            pre_loop_scope: &pre_loop_scope,
            assignment_depth,
            fold_exit_edges: !cursor_in_body,
            ctx: &loop_body_ctx,
            discovery_ctx: &loop_body_ctx,
        },
        |next_scope, point| match point {
            // The condition is tested after the body and the loop only
            // re-enters when it held, so every iteration past the first
            // starts from a body-exit state the condition has narrowed:
            // `do { … $c = $c->getParent(); } while ($c !== null);` reads
            // a non-null `$c` at the top of iteration two onwards. This
            // narrows the body-exit state alone, before it is merged with
            // the pre-loop state the first iteration ran on.
            LoopSeedPoint::AfterBody => {
                apply_condition_narrowing(dw.condition, next_scope, ctx);
            }
            LoopSeedPoint::Entry => {
                // Re-evaluating the condition re-runs `$c = $c->getParent()`
                // on the merged scope, which puts the declared `?Category`
                // back over the narrowing `AfterBody` applied. The loop
                // only re-enters when the condition held, so the entry is
                // where it held.
                let entry = process_condition(dw.condition, next_scope, ctx);
                *next_scope = entry.truthy(ctx).clone();
            }
        },
    );
    let exits = exit_frame.pop();

    // A caller asking about a position inside the body has its answer: the
    // walk stopped there.
    if cursor_in_body && !is_diagnostic_scope_active() {
        return;
    }

    // The condition runs after the body, so it is walked on the scope the
    // body leaves behind: that is where
    // `do { $n = next(); } while ($n && $n->ok());` reads `$n` from, and
    // what it writes is in force once the loop exits.  The loop exits
    // where it failed: `do { $a = getA(); } while ($a !== null);` leaves
    // `$a` null.
    let exit = process_condition(dw.condition, scope, ctx);
    *scope = exit.falsey(ctx).clone();

    // A `break` leaves without re-testing the condition, so its state
    // joins after the inverse narrowing.  This is the only way the state
    // before an early `break` reaches the code after the loop: the body
    // always runs, so there is no pre-loop scope to merge with.
    merge_exit_edges(scope, &exits.breaks);
}
