//! The fixed-point walk shared by every loop statement.
//!
//! A loop body is re-walked until the types it assigns stop changing, so
//! that a variable whose type depends on an earlier iteration settles
//! instead of being read from the first pass alone.

use super::*;
use std::collections::HashMap;

/// Walk a loop body until its loop-carried types stop changing.
///
/// The caller has already seeded `scope` for the first iteration (bound
/// the `foreach` target, narrowed by the `while` condition, run the `for`
/// initialisers).  `seed` advances the loop for every later walk: at
/// `AfterBody` it applies whatever runs between two body executions, and
/// at `Entry` it re-applies the caller's first-iteration narrowing to the
/// merged entry scope.
///
/// A walk that uses `discovery_ctx` ignores the cursor so that
/// assignments written *below* it are still discovered.  That leaves the
/// end-of-body types in `scope`, which is wrong for a caller asking about
/// a position inside the body: a read written above a reassignment of the
/// same variable would be answered with the reassigned type instead of
/// the one the loop entry established.  So whenever the discovery context
/// suppressed the cursor and no walk has honoured it yet, a last walk
/// runs with the real one.
///
/// [`LoopWalk::fold_exit_edges`] says whether the `continue` states
/// collected by the body walk belong in `scope`.  They join at the *end*
/// of the body, so a caller asking about a position above that point must
/// not see them — after `if (!$line) { continue; }` the guard has already
/// ruled the falsy `$line` out.
pub(crate) fn walk_loop_body_to_fixed_point<'b>(
    body_stmts: &[&'b Statement<'b>],
    scope: &mut ScopeState,
    walk: LoopWalk<'_>,
    mut seed: impl FnMut(&mut ScopeState, LoopSeedPoint),
) {
    let LoopWalk {
        pre_loop_scope,
        assignment_depth,
        fold_exit_edges,
        ctx,
        discovery_ctx,
    } = walk;
    let re_walks = assignment_depth.saturating_sub(1);

    // ── Initial walk (always performed) ─────────────────────────
    let initial_ctx = if re_walks > 0 { discovery_ctx } else { ctx };
    clear_exit_frame();
    walk_body_forward(body_stmts.iter().copied(), scope, initial_ctx);
    if fold_exit_edges {
        drain_continue_edges(scope);
    }
    let mut walked_at_cursor = re_walks == 0;

    // ── Re-walk iterations (only if types changed) ──────────────
    for iteration in 0..re_walks {
        // Check for changes BEFORE re-walking: compare post-walk
        // scope against the pre-loop scope.  If no variable has a
        // type that differs from what was known before the loop,
        // there's nothing new to propagate — skip the re-walk.
        if !scope_has_changes(pre_loop_scope, scope) {
            break;
        }

        *scope = merged_loop_entry_scope(pre_loop_scope, scope, &mut seed);

        // Use the real context on the final iteration so diagnostic
        // snapshots and cursor handling are correct.
        let is_final = iteration + 1 >= re_walks;
        clear_exit_frame();
        walk_body_forward(
            body_stmts.iter().copied(),
            scope,
            if is_final { ctx } else { discovery_ctx },
        );
        if fold_exit_edges {
            drain_continue_edges(scope);
        }
        walked_at_cursor = is_final;
    }

    if !walked_at_cursor && discovery_ctx.cursor_offset != ctx.cursor_offset {
        *scope = merged_loop_entry_scope(pre_loop_scope, scope, &mut seed);
        clear_exit_frame();
        walk_body_forward(body_stmts.iter().copied(), scope, ctx);
        if fold_exit_edges {
            drain_continue_edges(scope);
        }
    }
}

/// How one loop's body should be walked.
pub(crate) struct LoopWalk<'a> {
    /// The types that were known before the loop, which the body may not
    /// have run at all.
    pub(crate) pre_loop_scope: &'a ScopeState,
    /// How many walks it takes for the body's assignment chain to settle.
    pub(crate) assignment_depth: u32,
    /// Whether the loop's own exit edges belong in the answer — false when
    /// the caller is asking about a position inside the body.
    pub(crate) fold_exit_edges: bool,
    /// The real walk context, cursor and all.
    pub(crate) ctx: &'a ForwardWalkCtx<'a>,
    /// The context the discovery walks use, which may ignore the cursor.
    pub(crate) discovery_ctx: &'a ForwardWalkCtx<'a>,
}

/// The entry scope of the next walk of a loop body: the previous walk
/// advanced past the end of the body, merged with what was known before
/// the loop (the body may not have run yet), then narrowed the way the
/// loop narrows its first iteration.
fn merged_loop_entry_scope(
    pre_loop_scope: &ScopeState,
    walked: &mut ScopeState,
    seed: &mut impl FnMut(&mut ScopeState, LoopSeedPoint),
) -> ScopeState {
    seed(walked, LoopSeedPoint::AfterBody);

    let mut next_scope = pre_loop_scope.clone();
    next_scope.merge_branch(walked);
    seed(&mut next_scope, LoopSeedPoint::Entry);
    next_scope
}

/// The condition of a leading `if (…) { <jump past the loop> }` guard.
///
/// An `elseif` or `else` means the `if` is a branch rather than a guard, so
/// falling out of its bottom proves nothing about the condition.
pub(crate) fn guard_past_loop_condition<'b>(stmt: &'b Statement<'b>) -> Option<&'b Expression<'b>> {
    let Statement::If(if_stmt) = stmt else {
        return None;
    };
    let IfBody::Statement(body) = &if_stmt.body else {
        return None;
    };
    if !body.else_if_clauses.is_empty() || body.else_clause.is_some() {
        return None;
    }
    // A condition that assigns changes the entry the guard then talks
    // about, so what it proves is not a claim about what the collection
    // holds.
    let mut writes = HashMap::new();
    collect_expr_assignment_deps(if_stmt.condition, &mut writes);
    if !writes.is_empty() {
        return None;
    }
    statement_leaves_loop(body.statement).then_some(if_stmt.condition)
}

/// Whether a statement jumps somewhere that the code right after the
/// enclosing loop cannot be reached from.
///
/// Any `break`/`continue` level above 1 qualifies: it leaves the loop plus
/// at least one structure the code after the loop is itself inside.
fn statement_leaves_loop(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::Return(_) => true,
        Statement::Break(brk) => exit_level(brk.level).is_some_and(|level| level >= 2),
        Statement::Continue(cont) => exit_level(cont.level).is_some_and(|level| level >= 2),
        Statement::Expression(es) => matches!(
            es.expression,
            Expression::Throw(_)
                | Expression::Construct(mago_syntax::cst::Construct::Exit(_))
                | Expression::Construct(mago_syntax::cst::Construct::Die(_))
        ),
        Statement::Block(block) => block.statements.iter().any(statement_leaves_loop),
        _ => false,
    }
}
