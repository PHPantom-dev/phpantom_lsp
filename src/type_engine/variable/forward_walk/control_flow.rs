use super::*;
use std::collections::{HashMap, HashSet};

use mago_span::HasSpan;
use mago_syntax::cst::argument::Argument;

use crate::atom::{atom, bytes_to_str, literal_bytes_to_str};
use crate::parser::extract_hint_type;
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::types::narrowing;
use crate::type_engine::variable::foreach_resolution::{
    is_unsubstituted_template_param, resolve_iterable_element_via_class,
};
use crate::types::ResolvedType;

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
    let pre_if_scope = scope.clone();

    // Walk each branch independently and merge results.  A branch the
    // guard rules out is still walked (the cursor may be inside it), but
    // it is marked so the merge below drops what it established.
    let mut then_scope = scope.clone();
    then_scope.unreachable |= dead.then_branch;
    apply_condition_narrowing(if_stmt.condition, &mut then_scope, ctx);
    walk_body_forward(std::iter::once(body.statement), &mut then_scope, ctx);
    let then_exits = branch_exits(body.statement, &then_scope, ctx);

    let mut elseif_scopes: Vec<(ScopeState, bool)> = Vec::new();
    for (ei_idx, ei) in body.else_if_clauses.iter().enumerate() {
        let mut ei_scope = pre_if_scope.clone();
        ei_scope.unreachable |= dead.else_if_clauses[ei_idx];
        apply_condition_narrowing_inverse(if_stmt.condition, &mut ei_scope, ctx);
        for prev_ei in body.else_if_clauses.iter().take(ei_idx) {
            apply_condition_narrowing_inverse(prev_ei.condition, &mut ei_scope, ctx);
        }
        // Record a scope snapshot at the elseif condition boundary so
        // that diagnostic variable lookups inside the condition don't
        // pick up assignments from preceding if/elseif bodies.
        if is_diagnostic_scope_active() {
            record_scope_snapshot(ei.condition.span().start.offset, &ei_scope);
        }
        // An `elseif`'s own `&&` / `||` chain narrows its later operands
        // just as the leading `if`'s does.
        record_short_circuit_snapshots(ei.condition, &ei_scope, ctx);
        process_nested_assignments(ei.condition, &mut ei_scope, ctx);
        seed_pass_by_ref_in_condition(ei.condition, &mut ei_scope, ctx);
        apply_condition_narrowing(ei.condition, &mut ei_scope, ctx);
        walk_body_forward(std::iter::once(ei.statement), &mut ei_scope, ctx);
        let exits = branch_exits(ei.statement, &ei_scope, ctx);
        elseif_scopes.push((ei_scope, exits));
    }

    let (else_scope, else_exits) = if let Some(ref else_clause) = body.else_clause {
        let mut else_scope = pre_if_scope.clone();
        else_scope.unreachable |= dead.else_clause;
        apply_condition_narrowing_inverse(if_stmt.condition, &mut else_scope, ctx);
        for ei in body.else_if_clauses.iter() {
            apply_condition_narrowing_inverse(ei.condition, &mut else_scope, ctx);
        }
        // Record a scope snapshot at the else boundary so that
        // diagnostic variable lookups inside the else body don't
        // pick up assignments from the if/elseif bodies.
        if is_diagnostic_scope_active() {
            record_scope_snapshot(else_clause.statement.span().start.offset, &else_scope);
        }
        walk_body_forward(std::iter::once(else_clause.statement), &mut else_scope, ctx);
        let exits = branch_exits(else_clause.statement, &else_scope, ctx);
        (Some(else_scope), exits)
    } else {
        (None, false)
    };

    merge_if_branches(
        if_stmt,
        IfBranchScopes {
            pre_if: pre_if_scope,
            then_branch: (then_scope, then_exits),
            else_ifs: elseif_scopes,
            else_branch: else_scope.map(|s| (s, else_exits)),
            else_if_conditions: body.else_if_clauses.iter().map(|ei| ei.condition).collect(),
        },
        enclosing_stmt,
        scope,
        ctx,
    );
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
    let pre_if_scope = scope.clone();

    let mut then_scope = scope.clone();
    then_scope.unreachable |= dead.then_branch;
    apply_condition_narrowing(if_stmt.condition, &mut then_scope, ctx);
    walk_body_forward(body.statements.iter(), &mut then_scope, ctx);
    let then_exits = branch_exits_stmts(body.statements.iter(), &then_scope, ctx);

    let mut elseif_scopes: Vec<(ScopeState, bool)> = Vec::new();
    for (ei_idx, ei) in body.else_if_clauses.iter().enumerate() {
        let mut ei_scope = pre_if_scope.clone();
        ei_scope.unreachable |= dead.else_if_clauses[ei_idx];
        // The elseif branch only runs when the if condition and every
        // preceding elseif condition were false, so apply their inverse
        // narrowing before walking this branch.
        apply_condition_narrowing_inverse(if_stmt.condition, &mut ei_scope, ctx);
        for prev_ei in body.else_if_clauses.iter().take(ei_idx) {
            apply_condition_narrowing_inverse(prev_ei.condition, &mut ei_scope, ctx);
        }
        // Record a scope snapshot at the elseif condition boundary so
        // that diagnostic variable lookups inside the condition don't
        // pick up assignments from preceding if/elseif bodies.
        if is_diagnostic_scope_active() {
            record_scope_snapshot(ei.condition.span().start.offset, &ei_scope);
        }
        // An `elseif`'s own `&&` / `||` chain narrows its later operands
        // just as the leading `if`'s does.
        record_short_circuit_snapshots(ei.condition, &ei_scope, ctx);
        process_nested_assignments(ei.condition, &mut ei_scope, ctx);
        seed_pass_by_ref_in_condition(ei.condition, &mut ei_scope, ctx);
        apply_condition_narrowing(ei.condition, &mut ei_scope, ctx);
        walk_body_forward(ei.statements.iter(), &mut ei_scope, ctx);
        let exits = branch_exits_stmts(ei.statements.iter(), &ei_scope, ctx);
        elseif_scopes.push((ei_scope, exits));
    }

    let (else_scope, else_exits) = if let Some(ref else_clause) = body.else_clause {
        let mut else_scope = pre_if_scope.clone();
        else_scope.unreachable |= dead.else_clause;
        // The else branch only runs when the if condition and every
        // elseif condition were false, so apply the inverse of all of
        // them.
        apply_condition_narrowing_inverse(if_stmt.condition, &mut else_scope, ctx);
        for ei in body.else_if_clauses.iter() {
            apply_condition_narrowing_inverse(ei.condition, &mut else_scope, ctx);
        }
        // Record a scope snapshot at the else boundary.
        if is_diagnostic_scope_active()
            && let Some(first_stmt) = else_clause.statements.first()
        {
            record_scope_snapshot(first_stmt.span().start.offset, &else_scope);
        }
        walk_body_forward(else_clause.statements.iter(), &mut else_scope, ctx);
        let exits = branch_exits_stmts(else_clause.statements.iter(), &else_scope, ctx);
        (Some(else_scope), exits)
    } else {
        (None, false)
    };

    merge_if_branches(
        if_stmt,
        IfBranchScopes {
            pre_if: pre_if_scope,
            then_branch: (then_scope, then_exits),
            else_ifs: elseif_scopes,
            else_branch: else_scope.map(|s| (s, else_exits)),
            else_if_conditions: body.else_if_clauses.iter().map(|ei| ei.condition).collect(),
        },
        enclosing_stmt,
        scope,
        ctx,
    );
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

/// Check whether an if/elseif/else branch terminates, so its
/// assignments must not be merged into the post-if scope.
///
/// The branch's own scope is passed along so that a `never`-returning
/// method called on a local variable (`$aborter->fail()`) is recognised,
/// not just `$this->fail()`.
fn branch_exits(stmt: &Statement<'_>, scope: &ScopeState, ctx: &ForwardWalkCtx<'_>) -> bool {
    let var_types = |var_name: &str| scope.get(var_name).to_vec();
    let receiver_resolver = |expr: &Expression<'_>| resolved_receiver_class_names(expr, scope, ctx);
    narrowing::statement_unconditionally_exits(
        stmt,
        &narrowing::ExitCtx {
            current_class: ctx.current_class,
            class_loader: ctx.class_loader,
            function_loader: ctx.loaders.function_loader,
            resolved_class_cache: ctx.resolved_class_cache,
            var_types: Some(&var_types),
            receiver_resolver: Some(&receiver_resolver),
        },
    )
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

/// Check whether a colon-delimited if/elseif/else branch terminates, so
/// its assignments must not be merged into the post-if scope.  Mirrors
/// `branch_exits` for a top-level statement list rather than a single
/// (possibly block) statement: a branch exits if any statement in it
/// exits, matching `if_body_unconditionally_exits`'s colon-delimited
/// handling.
fn branch_exits_stmts<'s>(
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

/// Compute the assignment dependency depth for a loop body.
///
/// Does a cheap AST walk (no type resolution) to find which variables
/// are assigned and which other variables appear on the RHS.  Then
/// follows the dependency chain to compute the longest path.
///
/// For example, in:
///   $a = $input;
///   $b = transform($a);
///   $c = $b + 1;
///
/// The dependency map is {$a → {$input}, $b → {$a}, $c → {$b}} and
/// the longest chain is 3 ($input → $a → $b → $c).
///
/// This determines how many loop iterations are needed for types to
/// propagate through the entire chain.  Typically 1-3 for real PHP.
pub(crate) fn assignment_map_depth(statements: &[&Statement<'_>]) -> u32 {
    assignment_map_depth_with_updates(statements, std::iter::empty())
}

/// `assignment_map_depth` for a loop whose header also assigns: the `for`
/// update clause reassigns variables between two body executions, so those
/// assignments belong in the same dependency graph as the body's.
pub(crate) fn assignment_map_depth_with_updates<'a>(
    statements: &[&Statement<'_>],
    updates: impl Iterator<Item = &'a Expression<'a>>,
) -> u32 {
    // Build dependency map: assigned_var → set of RHS variables
    let mut deps: HashMap<String, HashSet<String>> = HashMap::new();

    for stmt in statements {
        collect_assignment_deps(stmt, &mut deps);
    }
    for update in updates {
        collect_expr_assignment_deps(update, &mut deps);
    }

    if deps.is_empty() {
        return 1;
    }

    // Compute longest dependency chain via DFS with cycle detection.
    let mut cache: HashMap<String, u32> = HashMap::new();
    let mut max_depth: u32 = 1;
    let keys: Vec<String> = deps.keys().cloned().collect();
    for key in &keys {
        let d = chain_depth(key, &deps, &mut cache, &mut HashSet::new());
        max_depth = max_depth.max(d);
    }

    // The chain depth tells us how many levels of variable-to-variable
    // propagation exist.  But even a single assignment needs 2 iterations:
    // one to discover the assignment, one to re-walk with the discovered
    // type visible from the start.  So: iterations = depth + 1.
    // Clamp to a reasonable maximum to avoid pathological cases.
    (max_depth + 1).min(3)
}

/// Recursively compute the dependency chain depth for a variable.
pub(crate) fn chain_depth(
    var: &str,
    deps: &HashMap<String, HashSet<String>>,
    cache: &mut HashMap<String, u32>,
    visiting: &mut HashSet<String>,
) -> u32 {
    if let Some(&cached) = cache.get(var) {
        return cached;
    }
    if !visiting.insert(var.to_string()) {
        // Cycle detected — break it.
        return 1;
    }
    let depth = if let Some(rhs_vars) = deps.get(var) {
        let mut max_child: u32 = 0;
        for dep in rhs_vars {
            max_child = max_child.max(chain_depth(dep, deps, cache, visiting));
        }
        max_child + 1
    } else {
        1
    };
    visiting.remove(var);
    cache.insert(var.to_string(), depth);
    depth
}

/// Collect assignment dependencies from a statement (cheap AST walk).
pub(crate) fn collect_assignment_deps(
    stmt: &Statement<'_>,
    deps: &mut HashMap<String, HashSet<String>>,
) {
    match stmt {
        Statement::Expression(expr_stmt) => {
            collect_expr_assignment_deps(expr_stmt.expression, deps);
        }
        Statement::If(if_stmt) => {
            // Walk all branches via the IfBody enum.
            match &if_stmt.body {
                IfBody::Statement(body) => {
                    collect_assignment_deps(body.statement, deps);
                    for ei in body.else_if_clauses.iter() {
                        collect_assignment_deps(ei.statement, deps);
                    }
                    if let Some(ref else_clause) = body.else_clause {
                        collect_assignment_deps(else_clause.statement, deps);
                    }
                }
                IfBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        collect_assignment_deps(s, deps);
                    }
                    for ei in body.else_if_clauses.iter() {
                        for s in ei.statements.iter() {
                            collect_assignment_deps(s, deps);
                        }
                    }
                    if let Some(ref else_clause) = body.else_clause {
                        for s in else_clause.statements.iter() {
                            collect_assignment_deps(s, deps);
                        }
                    }
                }
            }
        }
        Statement::Block(block) => {
            for s in block.statements.iter() {
                collect_assignment_deps(s, deps);
            }
        }
        Statement::Try(try_stmt) => {
            for s in try_stmt.block.statements.iter() {
                collect_assignment_deps(s, deps);
            }
            for catch in try_stmt.catch_clauses.iter() {
                for s in catch.block.statements.iter() {
                    collect_assignment_deps(s, deps);
                }
            }
            if let Some(ref finally) = try_stmt.finally_clause {
                for s in finally.block.statements.iter() {
                    collect_assignment_deps(s, deps);
                }
            }
        }
        Statement::Switch(switch) => {
            for case in switch.body.cases().iter() {
                for s in case.statements().iter() {
                    collect_assignment_deps(s, deps);
                }
            }
        }
        // Nested loops: walk their bodies too.
        Statement::Foreach(f) => {
            collect_foreach_header_deps(f, deps);
            match &f.body {
                ForeachBody::Statement(s) => {
                    collect_assignment_deps(s, deps);
                }
                ForeachBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        collect_assignment_deps(s, deps);
                    }
                }
            }
        }
        Statement::While(w) => match &w.body {
            WhileBody::Statement(s) => {
                collect_assignment_deps(s, deps);
            }
            WhileBody::ColonDelimited(body) => {
                for s in body.statements.iter() {
                    collect_assignment_deps(s, deps);
                }
            }
        },
        Statement::For(f) => match &f.body {
            ForBody::Statement(s) => {
                collect_assignment_deps(s, deps);
            }
            ForBody::ColonDelimited(body) => {
                for s in body.statements.iter() {
                    collect_assignment_deps(s, deps);
                }
            }
        },
        Statement::DoWhile(dw) => {
            collect_assignment_deps(dw.statement, deps);
        }
        _ => {}
    }
}

/// Extract assignment dependencies from an expression.
pub(crate) fn collect_expr_assignment_deps(
    expr: &Expression<'_>,
    deps: &mut HashMap<String, HashSet<String>>,
) {
    let Expression::Assignment(assign) = expr else {
        return;
    };

    let mut rhs_vars = HashSet::new();
    collect_rhs_variables(assign.rhs, &mut rhs_vars);

    // A write through an index or a property (`$a[$k] = …`, `$a->p = …`)
    // keeps everything the target already held, so the new type of the
    // base variable depends on its own previous type as well as on the
    // RHS.  Recording that self-edge is what makes a loop that both
    // reads and writes the same array iterate until the element type
    // settles, instead of stopping after the first walk.
    let mut targets = HashSet::new();
    collect_assignment_target_vars(assign.lhs, &mut targets);
    let indexed_write = !matches!(
        assign.lhs,
        Expression::Variable(mago_syntax::cst::variable::Variable::Direct(_))
            | Expression::Array(_)
            | Expression::List(_)
    );
    if indexed_write {
        // The index/receiver sub-expressions are reads, not writes.
        collect_lhs_index_variables(assign.lhs, &mut rhs_vars);
    }

    for target in targets {
        let entry = deps.entry(target.clone()).or_default();
        entry.extend(rhs_vars.iter().cloned());
        if indexed_write {
            entry.insert(target);
        }
    }
}

/// Collect the variables an assignment target writes to.
///
/// A direct variable writes itself, a destructuring pattern writes each
/// variable it binds, and an indexed or property write is attributed to
/// the variable at the base of the chain (`$a[$k][0] = …` writes `$a`).
pub(crate) fn collect_assignment_target_vars(target: &Expression<'_>, out: &mut HashSet<String>) {
    use mago_syntax::cst::access::Access;
    use mago_syntax::cst::variable::Variable;

    match target {
        Expression::Variable(Variable::Direct(dv)) => {
            out.insert(bytes_to_str(dv.name).to_string());
        }
        Expression::Array(_) | Expression::List(_) => {
            for value_expr in destructuring_element_exprs(target) {
                collect_assignment_target_vars(value_expr, out);
            }
        }
        Expression::ArrayAccess(aa) => collect_assignment_target_vars(aa.array, out),
        Expression::ArrayAppend(aa) => collect_assignment_target_vars(aa.array, out),
        Expression::Access(access) => match access {
            Access::Property(pa) => collect_assignment_target_vars(pa.object, out),
            Access::NullSafeProperty(pa) => collect_assignment_target_vars(pa.object, out),
            _ => {}
        },
        Expression::Parenthesized(p) => collect_assignment_target_vars(p.expression, out),
        _ => {}
    }
}

/// Collect the variables read by the index expressions of a write target.
///
/// `$a[$k] = …` reads `$k` to decide where to write, so `$k` belongs in
/// the dependency set even though `$a` is what gets assigned.
fn collect_lhs_index_variables(target: &Expression<'_>, vars: &mut HashSet<String>) {
    use mago_syntax::cst::access::Access;

    match target {
        Expression::ArrayAccess(aa) => {
            collect_rhs_variables(aa.index, vars);
            collect_lhs_index_variables(aa.array, vars);
        }
        Expression::ArrayAppend(aa) => collect_lhs_index_variables(aa.array, vars),
        Expression::Access(access) => match access {
            Access::Property(pa) => collect_lhs_index_variables(pa.object, vars),
            Access::NullSafeProperty(pa) => collect_lhs_index_variables(pa.object, vars),
            _ => {}
        },
        Expression::Parenthesized(p) => collect_lhs_index_variables(p.expression, vars),
        _ => {}
    }
}

/// The value expressions of a destructuring pattern's elements.
fn destructuring_element_exprs<'b>(pattern: &'b Expression<'b>) -> Vec<&'b Expression<'b>> {
    let elements: Vec<&ArrayElement<'b>> = match pattern {
        Expression::Array(arr) => arr.elements.iter().collect(),
        Expression::List(list) => list.elements.iter().collect(),
        _ => return Vec::new(),
    };
    elements
        .into_iter()
        .filter_map(|elem| match elem {
            ArrayElement::KeyValue(kv) => Some(kv.value),
            ArrayElement::Value(val) => Some(val.value),
            _ => None,
        })
        .collect()
}

/// Record the dependency a `foreach` header creates: every variable the
/// target binds takes its type from the iterated expression.
///
/// Without this edge a loop that destructures an array it also writes to
/// looks dependency-free, so the fixed-point walk stops before the
/// element type it wrote has been read back.
fn collect_foreach_header_deps(foreach: &Foreach<'_>, deps: &mut HashMap<String, HashSet<String>>) {
    let mut iter_vars = HashSet::new();
    collect_rhs_variables(foreach.expression, &mut iter_vars);
    if iter_vars.is_empty() {
        return;
    }

    let mut bound = HashSet::new();
    match &foreach.target {
        ForeachTarget::Value(val) => collect_foreach_bound_vars(val.value, &mut bound),
        ForeachTarget::KeyValue(kv) => {
            collect_foreach_bound_vars(kv.key, &mut bound);
            collect_foreach_bound_vars(kv.value, &mut bound);
        }
    }

    for name in bound {
        deps.entry(name)
            .or_default()
            .extend(iter_vars.iter().cloned());
    }
}

/// Collect the variables a `foreach` target binds, unwrapping `&$v` and
/// recursing through destructuring patterns.
fn collect_foreach_bound_vars(target: &Expression<'_>, out: &mut HashSet<String>) {
    let target = if let Expression::UnaryPrefix(up) = target
        && matches!(up.operator, UnaryPrefixOperator::Reference(_))
    {
        up.operand
    } else {
        target
    };
    collect_assignment_target_vars(target, out);
}

/// Collect all variable references from an expression (cheap, no type resolution).
pub(crate) fn collect_rhs_variables(expr: &Expression<'_>, vars: &mut HashSet<String>) {
    use mago_syntax::cst::variable::Variable;

    match expr {
        Expression::Variable(Variable::Direct(dv)) => {
            vars.insert(bytes_to_str(dv.name).to_string());
        }
        Expression::Binary(binary) => {
            collect_rhs_variables(binary.lhs, vars);
            collect_rhs_variables(binary.rhs, vars);
        }
        Expression::UnaryPrefix(unary) => {
            collect_rhs_variables(unary.operand, vars);
        }
        Expression::UnaryPostfix(unary) => {
            collect_rhs_variables(unary.operand, vars);
        }
        Expression::Parenthesized(p) => {
            collect_rhs_variables(p.expression, vars);
        }
        Expression::Call(call) => {
            // Collect variables from call arguments.
            match call {
                Call::Function(fc) => {
                    collect_rhs_variables(fc.function, vars);
                    collect_arglist_variables(&fc.argument_list, vars);
                }
                Call::Method(mc) => {
                    collect_rhs_variables(mc.object, vars);
                    collect_arglist_variables(&mc.argument_list, vars);
                }
                Call::NullSafeMethod(mc) => {
                    collect_rhs_variables(mc.object, vars);
                    collect_arglist_variables(&mc.argument_list, vars);
                }
                Call::StaticMethod(sc) => {
                    collect_rhs_variables(sc.class, vars);
                    collect_arglist_variables(&sc.argument_list, vars);
                }
            }
        }
        Expression::Access(access) => match access {
            mago_syntax::cst::access::Access::Property(pa) => {
                collect_rhs_variables(pa.object, vars);
            }
            mago_syntax::cst::access::Access::NullSafeProperty(pa) => {
                collect_rhs_variables(pa.object, vars);
            }
            mago_syntax::cst::access::Access::StaticProperty(sp) => {
                collect_rhs_variables(sp.class, vars);
            }
            mago_syntax::cst::access::Access::ClassConstant(cc) => {
                collect_rhs_variables(cc.class, vars);
            }
        },
        Expression::ArrayAccess(aa) => {
            collect_rhs_variables(aa.array, vars);
        }
        Expression::Conditional(cond) => {
            collect_rhs_variables(cond.condition, vars);
            if let Some(then_expr) = cond.then {
                collect_rhs_variables(then_expr, vars);
            }
            collect_rhs_variables(cond.r#else, vars);
        }

        Expression::Instantiation(inst) => {
            collect_rhs_variables(inst.class, vars);
            if let Some(ref args) = inst.argument_list {
                collect_arglist_variables(args, vars);
            }
        }
        Expression::Assignment(assign) => {
            // Nested assignments like `$a = $b = expr`.
            collect_rhs_variables(assign.rhs, vars);
        }
        _ => {}
    }
}

/// Collect variable references from an argument list.
pub(crate) fn collect_arglist_variables(
    args: &mago_syntax::cst::argument::ArgumentList<'_>,
    vars: &mut HashSet<String>,
) {
    for arg in args.arguments.iter() {
        let expr = match arg {
            Argument::Positional(a) => a.value,
            Argument::Named(a) => a.value,
        };
        collect_rhs_variables(expr, vars);
    }
}

/// Check whether the post-walk scope has any NEW or CHANGED variable
/// types compared to the pre-loop scope.  This is the Mago-style
/// fixed-point check that runs BEFORE a re-walk: if nothing changed,
/// there's no point walking the body again.
///
/// This is asymmetric: new variables in `after` that weren't in
/// `before` count as changes, but variables in `before` that aren't
/// in `after` do not (they were just not assigned in the loop body).
pub(crate) fn scope_has_changes(before: &ScopeState, after: &ScopeState) -> bool {
    for (name, after_types) in &after.locals {
        match before.locals.get(name) {
            None => {
                // New variable assigned in the loop body.
                if !after_types.is_empty() {
                    return true;
                }
            }
            Some(before_types) => {
                if after_types.len() != before_types.len() {
                    return true;
                }
                for (at, bt) in after_types.iter().zip(before_types.iter()) {
                    if at.type_string != bt.type_string {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Where in a loop iteration `walk_loop_body_to_fixed_point` is invoking
/// its seeding callback.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopSeedPoint {
    /// Straight after a walk of the body, on the types that walk left
    /// behind.  This is where a `for` loop's increment clause runs.
    AfterBody,
    /// On the entry scope of the next walk, once the previous walk's types
    /// have been merged back in: re-applies the narrowing the caller did
    /// for the first iteration.
    Entry,
}

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
fn walk_loop_body_to_fixed_point<'b>(
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
struct LoopWalk<'a> {
    /// The types that were known before the loop, which the body may not
    /// have run at all.
    pre_loop_scope: &'a ScopeState,
    /// How many walks it takes for the body's assignment chain to settle.
    assignment_depth: u32,
    /// Whether the loop's own exit edges belong in the answer — false when
    /// the caller is asking about a position inside the body.
    fold_exit_edges: bool,
    /// The real walk context, cursor and all.
    ctx: &'a ForwardWalkCtx<'a>,
    /// The context the discovery walks use, which may ignore the cursor.
    discovery_ctx: &'a ForwardWalkCtx<'a>,
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

/// Process a `foreach` statement.
pub(crate) fn process_foreach<'b>(
    foreach: &'b Foreach<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let loop_depth = enter_loop();

    // Hard limit: skip the body entirely at excessive nesting depth.
    if loop_depth > MAX_LOOP_DEPTH {
        leave_loop(loop_depth);
        return;
    }

    // Apply any standalone `/** @var Type $var */` docblocks that precede
    // the foreach keyword.  These are not separate AST statements (the
    // parser attaches them as comments to the foreach), so they won't be
    // processed by `process_expression_statement`.  Without this, variables
    // typed only via docblock (common in Blade templates) won't be in scope
    // when the iterable expression is resolved.
    //
    // We extract all variables referenced in the foreach expression and
    // check for @var annotations for each one.
    let foreach_offset = foreach.foreach.span().start.offset as usize;
    if let Expression::Variable(Variable::Direct(dv)) = foreach.expression {
        // `bytes_to_str(dv.name)` already includes the leading `$`, which
        // is how scope keys and `find_var_raw_type_in_source` expect it.
        let var_name = bytes_to_str(dv.name);
        if let Some(var_type) =
            crate::docblock::find_var_raw_type_in_source(ctx.content, foreach_offset, var_name)
        {
            let php_type = crate::util::resolve_php_type_names(&var_type, ctx.class_loader);
            // An explicit inline `@var` seeds an empty scope entry, and it
            // also refines a non-informative pre-existing type such as a
            // `mixed` closure/function parameter or a bare `array`.  Without
            // the second case, a `mixed` parameter would occupy the scope
            // slot and shadow the developer's `@var iterable<T> $x`
            // annotation, leaving the loop variable untyped.
            let current = scope.get(var_name);
            let should_apply = current.is_empty()
                || current.iter().all(|rt| {
                    crate::docblock::should_override_type_typed(&php_type, &rt.type_string)
                });
            if should_apply {
                let resolved = resolve_type_to_resolved_types(&php_type, ctx);
                scope.set(var_name, resolved);
            }
        }
    } else {
        // For complex expressions like `$users->active()->byName()`,
        // extract the base variable and resolve its type from @var.
        let expr_start = foreach.expression.span().start.offset as usize;
        let expr_end = foreach.expression.span().end.offset as usize;
        if let Some(expr_text) = ctx.content.get(expr_start..expr_end) {
            // Extract the base variable (e.g. "$users" from "$users->active()->byName()")
            if let Some(base_end) = expr_text.find("->").or_else(|| expr_text.find("::")) {
                let base_var = expr_text[..base_end].trim();
                // Scope keys retain the leading `$` (e.g. "$users"), so the
                // lookup and the insert must both use the `$`-prefixed name,
                // matching the direct-variable branch above.
                if base_var.starts_with('$')
                    && let Some(var_type) = crate::docblock::find_var_raw_type_in_source(
                        ctx.content,
                        foreach_offset,
                        base_var,
                    )
                {
                    let php_type = crate::util::resolve_php_type_names(&var_type, ctx.class_loader);
                    // As in the direct-variable branch: seed an unknown base
                    // variable, or refine a non-informative pre-existing type
                    // (e.g. a `mixed` parameter), but never clobber a more
                    // precise type inferred from an assignment.
                    let current = scope.get(base_var);
                    let should_apply = current.is_empty()
                        || current.iter().all(|rt| {
                            crate::docblock::should_override_type_typed(&php_type, &rt.type_string)
                        });
                    if should_apply {
                        let resolved = resolve_type_to_resolved_types(&php_type, ctx);
                        scope.set(base_var, resolved);
                    }
                }
            }
        }
    }

    // The iterable expression is an expression position like any other,
    // so the narrowing its own short-circuit chains, ternary branches and
    // `match (true)` arms prove has to reach the code inside them.
    // `foreach ($t instanceof UnionType ? $t->getTypes() : [$t] as $inner)`
    // reads `getTypes()` off the narrowed subject, not the declared one.
    record_short_circuit_snapshots(foreach.expression, scope, ctx);
    if is_diagnostic_scope_active() {
        record_match_ternary_snapshots(foreach.expression, scope, ctx);
    }

    // Resolve the iterable expression's type.
    let iter_type = resolve_foreach_iterable_type(foreach, scope, ctx);

    let pre_loop_scope = scope.clone();

    // When the cursor is inside the loop body (completion path), discovery
    // passes must walk the ENTIRE body; the final pass uses the real
    // cursor_offset so it stops at the cursor as usual.
    let body_span = match &foreach.body {
        ForeachBody::Statement(inner) => inner.span(),
        ForeachBody::ColonDelimited(body) => body.span(),
    };
    let cursor_in_body =
        ctx.cursor_offset >= body_span.start.offset && ctx.cursor_offset <= body_span.end.offset;
    let discovery_ctx = if cursor_in_body && !is_diagnostic_scope_active() {
        ctx.with_cursor_offset(u32::MAX)
    } else {
        ctx.with_cursor_offset(ctx.cursor_offset)
    };

    // Bind the value variable (and optionally the key variable).
    match &foreach.target {
        ForeachTarget::Value(val) => {
            bind_foreach_value(val.value, &iter_type, scope, ctx);
        }
        ForeachTarget::KeyValue(kv) => {
            bind_foreach_key(kv.key, &iter_type, scope, ctx);
            bind_foreach_value(kv.value, &iter_type, scope, ctx);
        }
    }

    // Docblock fallback: when `bind_foreach_value`/`bind_foreach_key`
    // could not determine the element type from the iterable (e.g. the
    // iterable is `mixed` or a bare `array`), check for inline
    // `/** @var Type $var */` docblock(s) preceding the foreach keyword
    // and use them to seed the key and/or value variables.  @var
    // annotations are explicit developer overrides that take priority
    // over types inferred from the iterable.
    let value_var_name = match &foreach.target {
        ForeachTarget::Value(val) => extract_foreach_var_name(val.value),
        ForeachTarget::KeyValue(kv) => extract_foreach_var_name(kv.value),
    };
    let key_var_name = match &foreach.target {
        ForeachTarget::Value(_) => None,
        ForeachTarget::KeyValue(kv) => extract_foreach_var_name(kv.key),
    };

    // Collect resolved docblock overrides for key/value variables.
    let mut value_docblock_override: Option<Vec<ResolvedType>> = None;
    let mut key_docblock_override: Option<Vec<ResolvedType>> = None;
    let foreach_offset = foreach.foreach.span().start.offset as usize;
    let before = &ctx.content[..foreach_offset.min(ctx.content.len())];
    let trimmed = before.trim_end();
    if trimmed.ends_with("*/")
        && let Some(doc_start) = trimmed.rfind("/**")
    {
        let doc_text = &trimmed[doc_start..trimmed.len()];
        let var_annotations = parse_var_docblock_pairs(doc_text);
        for (doc_var, php_type) in &var_annotations {
            if let Some(ref vn) = value_var_name
                && doc_var == vn
            {
                value_docblock_override = Some(resolve_type_to_resolved_types(php_type, ctx));
            }
            if let Some(ref kn) = key_var_name
                && doc_var == kn
            {
                key_docblock_override = Some(resolve_type_to_resolved_types(php_type, ctx));
            }
        }
    }

    // Apply docblock overrides (overwrites bind_foreach_key/value results).
    if let Some(ref resolved) = value_docblock_override
        && let Some(ref vn) = value_var_name
    {
        scope.set(vn, resolved.clone());
    }
    if let Some(ref resolved) = key_docblock_override
        && let Some(ref kn) = key_var_name
    {
        scope.set(kn, resolved.clone());
    }
    // When the iterable is a bare `array` (no generic parameters)
    // and no @var docblock provided a concrete type, the element
    // type is `mixed`.  Seed it so that assignments from the loop
    // variable propagate `mixed` correctly through the body.
    if let Some(ref vn) = value_var_name
        && value_docblock_override.is_none()
        && scope.get(vn).is_empty()
        && iter_type.as_ref().is_some_and(|it| it.is_bare_array())
    {
        scope.set(vn, vec![ResolvedType::from_type_string(PhpType::mixed())]);
    }

    // What one entry looks like before the body has said anything about
    // it, which is the baseline the loop's own guards narrow.
    let entry_value_types: Option<Vec<ResolvedType>> =
        value_var_name.as_ref().map(|vn| scope.get(vn).to_vec());

    // ── Assignment-depth-bounded loop iteration ─────────────────
    //
    // Walk the body once (always needed).  Then check whether any
    // variable types changed compared to the pre-loop scope.  Only
    // re-walk if there are actual changes AND the assignment depth
    // requires further propagation.  This matches Mago's approach:
    // the fixed-point check happens BEFORE the expensive re-walk,
    // not after.
    let body_stmts: Vec<&Statement<'b>> = match &foreach.body {
        ForeachBody::Statement(inner) => vec![*inner],
        ForeachBody::ColonDelimited(body) => body.statements.iter().collect(),
    };
    let assignment_depth =
        clamp_iterations_for_depth(assignment_map_depth(&body_stmts), loop_depth);

    // A `foreach` over an array the engine watched being built and knows
    // is still empty runs zero times, so it cannot change any type.  The
    // body is still walked so that a cursor or diagnostic inside it is
    // answered, but its writes are dropped afterwards.
    //
    // Keeping them would poison an enclosing loop's fixed point: the
    // first walk of an outer loop reaches an inner `foreach` over the
    // accumulator before anything has been written to it, and the
    // unresolved element types that walk produces would be unioned into
    // the accumulator for good, so the element type never converges.
    if iter_type
        .as_ref()
        .is_some_and(|it| it.is_empty_array_shape())
    {
        let restored = pre_loop_scope.clone();
        push_exit_frame();
        walk_body_forward(body_stmts.iter().copied(), scope, ctx);
        pop_exit_frame();
        *scope = restored;
        leave_loop(loop_depth);
        return;
    }

    push_exit_frame();
    walk_loop_body_to_fixed_point(
        &body_stmts,
        scope,
        LoopWalk {
            pre_loop_scope: &pre_loop_scope,
            assignment_depth,
            fold_exit_edges: !cursor_in_body,
            ctx,
            discovery_ctx: &discovery_ctx,
        },
        |next_scope, point| {
            if point != LoopSeedPoint::Entry {
                return;
            }
            // Re-bind the foreach variables for the next iteration,
            // discarding what the previous one wrote to them.
            match &foreach.target {
                ForeachTarget::Value(val) => {
                    reset_foreach_target(val.value, next_scope, &pre_loop_scope);
                    bind_foreach_value(val.value, &iter_type, next_scope, ctx);
                }
                ForeachTarget::KeyValue(kv) => {
                    reset_foreach_target(kv.key, next_scope, &pre_loop_scope);
                    reset_foreach_target(kv.value, next_scope, &pre_loop_scope);
                    bind_foreach_key(kv.key, &iter_type, next_scope, ctx);
                    bind_foreach_value(kv.value, &iter_type, next_scope, ctx);
                }
            }
            // Re-apply docblock overrides after re-binding.
            if let Some(ref resolved) = value_docblock_override
                && let Some(ref vn) = value_var_name
            {
                next_scope.set(vn, resolved.clone());
            }
            if let Some(ref resolved) = key_docblock_override
                && let Some(ref kn) = key_var_name
            {
                next_scope.set(kn, resolved.clone());
            }
        },
    );

    let exits = pop_exit_frame();

    // An iterable that proves it has entries — a non-empty array literal,
    // or a type refined to `non-empty-array`/`non-empty-list`/a required
    // shape entry — runs the body at least once, so what was known before
    // the loop is not an alternative to what the body left behind.  The
    // pre-loop sentinel (`$max = null` ahead of a loop that always
    // assigns) would otherwise survive the whole loop.  A body that never
    // falls out of its own bottom has no fall-through state to keep, so
    // it still takes the merge.
    let body_always_runs = !scope.unreachable
        && (is_non_empty_array_literal(foreach.expression)
            || iter_type
                .as_ref()
                .is_some_and(PhpType::is_provably_non_empty));

    if !body_always_runs {
        // The iterable might be empty, so the loop body might not execute
        // at all.  Merge with the pre-loop scope.
        let post_loop = scope.clone();
        *scope = pre_loop_scope;
        scope.merge_branch(&post_loop);
    }

    // A path that broke out never reached the end of the body, so the
    // fall-through alone does not describe it.
    if !cursor_in_body {
        merge_exit_edges(scope, &exits.breaks);
        let _ = narrow_iterated_collection(
            foreach,
            &body_stmts,
            iter_type.as_ref(),
            entry_value_types.as_deref(),
            scope,
            ctx,
        );
    }

    leave_loop(loop_depth);
}

/// Narrow the collection a loop iterated to what the loop proved about
/// every one of its entries.
///
/// `foreach ($conds as $cond) { if (!$cond instanceof C) { break 2; } … }`
/// only falls out of its own bottom once every entry has passed the guard,
/// so the code after it may treat the whole collection as `C[]` — which is
/// what a second loop over the same expression, the idiom this exists for,
/// then reads its own variable from.  An empty collection makes the claim
/// vacuously true, so whether the body ran does not matter.
///
/// A `break` or `continue` naming only this loop proves nothing: the first
/// jumps straight to the code being narrowed, and the second skips the
/// entry rather than the rest of the program.
fn narrow_iterated_collection<'b>(
    foreach: &'b Foreach<'b>,
    body_stmts: &[&'b Statement<'b>],
    iter_type: Option<&PhpType>,
    entry_value_types: Option<&[ResolvedType]>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<()> {
    // A braced body arrives as a single `Block` statement, and the guards
    // are its children rather than the body's.  Checked before anything is
    // allocated: a loop that does not open with a guard is the common case
    // and there is nothing here for it.
    let leading = match body_stmts {
        [Statement::Block(block)] => block.statements.first(),
        _ => body_stmts.first().copied(),
    };
    guard_past_loop_condition(leading?)?;

    let entry_value_types = entry_value_types.filter(|types| !types.is_empty())?;
    let iter_type = iter_type?;
    let value_expr = match &foreach.target {
        ForeachTarget::Value(val) => val.value,
        ForeachTarget::KeyValue(kv) => kv.value,
    };
    // A by-reference loop writes through the entries it visits, so what a
    // guard proved about one need not still hold afterwards.
    if let Expression::UnaryPrefix(up) = value_expr
        && matches!(up.operator, UnaryPrefixOperator::Reference(_))
    {
        return None;
    }
    let Expression::Variable(Variable::Direct(dv)) = value_expr else {
        return None;
    };
    let var_name = bytes_to_str(dv.name).to_string();
    let collection_key = narrowing::expr_to_subject_key(foreach.expression)?;

    // Replay the leading guards against the entry binding alone: what
    // survives all of them is what every entry had to be to get here.
    let mut guard_scope = ScopeState::new();
    guard_scope.set(&var_name, entry_value_types.to_vec());
    let unwrapped: Vec<&Statement<'_>> = match body_stmts {
        [Statement::Block(block)] => block.statements.iter().collect(),
        _ => body_stmts.to_vec(),
    };
    for stmt in &unwrapped {
        let Some(condition) = guard_past_loop_condition(stmt) else {
            break;
        };
        apply_condition_narrowing_inverse(condition, &mut guard_scope, ctx);
        if guard_scope.unreachable {
            return None;
        }
    }

    let narrowed = guard_scope.get(&var_name);
    if narrowed.is_empty() || !narrowing_changed_types(entry_value_types, narrowed) {
        return None;
    }
    let element = ResolvedType::types_joined(narrowed);
    let collection_type =
        crate::type_engine::variable::array_func_rules::with_element_type(iter_type, element)?;
    // Keep whatever class backs the container itself (a `Collection` object
    // rather than a plain array); only its element type changed.
    let mut entry = scope
        .get(&collection_key)
        .first()
        .cloned()
        .unwrap_or_else(|| ResolvedType::from_type_string(collection_type.clone()));
    entry.type_string = collection_type;
    scope.set(&collection_key, vec![entry]);
    Some(())
}

/// The condition of a leading `if (…) { <jump past the loop> }` guard.
///
/// An `elseif` or `else` means the `if` is a branch rather than a guard, so
/// falling out of its bottom proves nothing about the condition.
fn guard_past_loop_condition<'b>(stmt: &'b Statement<'b>) -> Option<&'b Expression<'b>> {
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

/// Resolve the iterable expression's type for a foreach.
///
/// Every answer is run through `resolve_type_alias_typed` so a
/// `@phpstan-type` / `@phpstan-import-type` alias is expanded to the array
/// type it names before the caller reads a key or element type off it.
/// The expansion lives here rather than in each branch of
/// [`resolve_foreach_iterable_type_raw`] so a new branch cannot forget it.
pub(crate) fn resolve_foreach_iterable_type<'b>(
    foreach: &'b Foreach<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<PhpType> {
    let raw = resolve_foreach_iterable_type_raw(foreach, scope, ctx)?;
    Some(
        crate::type_engine::type_resolution::resolve_type_alias_typed(
            &raw,
            &ctx.current_class.name,
            ctx.all_classes,
            ctx.class_loader,
        )
        .unwrap_or(raw),
    )
}

/// The unexpanded iterable type, tried source by source.
fn resolve_foreach_iterable_type_raw<'b>(
    foreach: &'b Foreach<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<PhpType> {
    // Try direct scope lookup for bare variable iterators.
    if let Expression::Variable(Variable::Direct(dv)) = foreach.expression {
        let var_name = bytes_to_str(dv.name).to_string();
        let from_scope = scope.get(&var_name);
        if !from_scope.is_empty() {
            return Some(ResolvedType::types_joined(from_scope));
        }
    }

    // Fall back to resolve_rhs_expression for complex expressions.
    let resolved = resolve_rhs_with_scope(foreach.expression, scope, ctx);
    if !resolved.is_empty() {
        return Some(ResolvedType::types_joined(&resolved));
    }

    // Fallback: for simple `$variable` iterators, check for an inline
    // `/** @var Type $var */` or `@param` annotation near the foreach.
    // Handles cases where the variable's type comes from a docblock
    // rather than an assignment.
    if let Expression::Variable(Variable::Direct(dv)) = foreach.expression {
        let var_name = bytes_to_str(dv.name).to_string();
        let foreach_offset = foreach.foreach.span().start.offset as usize;
        if let Some(docblock_type) = crate::docblock::find_iterable_raw_type_in_source(
            ctx.content,
            foreach_offset,
            &var_name,
        )
        .map(|t| crate::util::resolve_php_type_names(&t, ctx.class_loader))
        {
            return Some(docblock_type);
        }
    }

    // Final fallback: resolve the foreach expression as a "subject"
    // through the full resolver pipeline (SubjectExpr::parse →
    // property/method chain resolution).  Handles cases like
    // `$this->getItems()` or `self::fetchAll()` where the expression
    // type wasn't captured by scope lookup or resolve_rhs_expression
    // above.
    if let Some(iter_type) = resolve_foreach_expr_via_subject(foreach.expression, scope, ctx) {
        return Some(iter_type);
    }

    None
}

/// Resolve a foreach expression to a `PhpType` by treating it as a
/// subject string and going through the full resolver pipeline.
///
/// It extracts the expression text, calls `resolve_target_classes` to
/// get `ClassInfo` objects, and constructs a `TypeKind::Named` from the
/// first resolved class.
pub(crate) fn resolve_foreach_expr_via_subject<'b>(
    expression: &'b Expression<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<PhpType> {
    let expr_span = expression.span();
    let expr_start = expr_span.start.offset as usize;
    let expr_end = expr_span.end.offset as usize;
    let expr_text = ctx.content.get(expr_start..expr_end)?.trim();
    if expr_text.is_empty() {
        return None;
    }

    // Build a ResolutionCtx from the forward walker's context.
    let scope_resolver = scope.snapshot_resolver();
    let var_ctx = ctx.var_ctx_for_with_scope(
        "$__foreach",
        expr_span.start.offset,
        &scope_resolver,
        Some(scope.proofs()),
    );
    let rctx = var_ctx.as_resolution_ctx();

    let resolved = crate::type_engine::resolver::resolve_target_classes(
        expr_text,
        crate::types::AccessKind::Arrow,
        &rctx,
    );

    if resolved.is_empty() {
        return None;
    }

    // Construct a PhpType from the resolved classes.  If any resolved
    // type has a structured type_string (e.g. `list<User>`,
    // `Collection<int, Product>`), prefer that — it carries generic
    // parameters that `extract_value_type` can use.
    for rt in &resolved {
        if rt.type_string.has_type_structure() {
            let expanded = crate::type_engine::type_resolution::resolve_type_alias_typed(
                &rt.type_string,
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            )
            .unwrap_or_else(|| rt.type_string.clone());
            return Some(expanded);
        }
    }

    // Fall back to the class name — `bind_foreach_value` Strategy 2
    // will resolve it through inheritance to find element types.
    // Use `fqn()` (not `name`) so that the returned `TypeKind::Named`
    // carries the fully-qualified class name.  `ClassInfo.name` is
    // always the short name (e.g. `OrderProductCollection`), while
    // `fqn()` combines namespace + name into the FQN that the class
    // loader needs to find and merge the class.
    let first = resolved.first()?;
    let name = first
        .class_info
        .as_ref()
        .map(|c| c.fqn().to_string())
        .or_else(|| first.type_string.base_name().map(|s| s.to_string()))?;

    Some(PhpType::named(atom(&name)))
}

/// The pieces of a forward-walk context the shared iterable element/key
/// derivation reads.
fn iterable_ctx<'a>(
    ctx: &'a ForwardWalkCtx<'_>,
) -> crate::type_engine::variable::foreach_resolution::IterableCtx<'a> {
    crate::type_engine::variable::foreach_resolution::IterableCtx {
        current_class: ctx.current_class,
        all_classes: ctx.all_classes,
        class_loader: ctx.class_loader,
        resolved_class_cache: ctx.resolved_class_cache,
    }
}

/// Bind a foreach value variable from the iterable's element type.
///
/// Resolution strategy:
/// 1. Try `PhpType::extract_value_type` — works for types that already
///    carry generic parameters (e.g. `list<User>`, `array<int, Order>`,
///    `Collection<int, Product>`).
/// 2. Class-based fallback — when the type is a bare class name (e.g.
///    `OrderProductCollection`), resolve it to `ClassInfo`, merge
///    inheritance, and extract the element type from `@extends` /
///    `@implements` generics.
pub(crate) fn bind_foreach_value<'b>(
    value_expr: &'b Expression<'b>,
    iter_type: &Option<PhpType>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Unwrap `&$value` (by-reference foreach) to get the inner variable.
    let value_expr = if let Expression::UnaryPrefix(up) = value_expr
        && matches!(up.operator, UnaryPrefixOperator::Reference(_))
    {
        up.operand
    } else {
        value_expr
    };
    if let Expression::Variable(Variable::Direct(dv)) = value_expr {
        let var_name = bytes_to_str(dv.name).to_string();
        if let Some(it) = iter_type {
            // Strategy 1: extract from the type's own generic parameters
            // (or, for tuple-style shapes, the union of positional values).
            let value_php_type = it.iterable_element_type();
            if let Some(vt) = value_php_type {
                scope.set(&var_name, ctx.resolved_types_for(vt.clone()));
                return;
            }

            // Strategy 2: class-based fallback for bare collection names.
            let element_via_class = resolve_iterable_element_via_class(it, &iterable_ctx(ctx));
            if let Some(element_type) = element_via_class
                && !is_unsubstituted_template_param(&element_type)
            {
                scope.set(&var_name, ctx.resolved_types_for(element_type));
            }

            // Strategy 3: union type fallback — try each member individually.
            // When the iterable is a union like `ProductCollection|Product`,
            // neither `extract_value_type` nor `resolve_iterable_element_via_class`
            // works on the union as a whole.  Walk each member and use the
            // first one that yields an element type.
            if let TypeKind::Union(members) = it.kind() {
                for member in members {
                    // Try extract_value_type on each member (handles generic collections).
                    if let Some(vt) = member.extract_value_type(false) {
                        let resolved =
                            crate::type_engine::type_resolution::type_hint_to_classes_typed(
                                vt,
                                &ctx.current_class.name,
                                ctx.all_classes,
                                ctx.class_loader,
                            );
                        if !resolved.is_empty() {
                            scope.set(
                                &var_name,
                                ResolvedType::from_classes_with_hint(resolved, vt.clone()),
                            );
                        } else {
                            scope.set(&var_name, vec![ResolvedType::from_type_string(vt.clone())]);
                        }
                        return;
                    }
                    // Try class-based element extraction on each member.
                    if let Some(element_type) =
                        resolve_iterable_element_via_class(member, &iterable_ctx(ctx))
                        && !is_unsubstituted_template_param(&element_type)
                    {
                        let resolved =
                            crate::type_engine::type_resolution::type_hint_to_classes_typed(
                                &element_type,
                                &ctx.current_class.name,
                                ctx.all_classes,
                                ctx.class_loader,
                            );
                        if !resolved.is_empty() {
                            scope.set(
                                &var_name,
                                ResolvedType::from_classes_with_hint(resolved, element_type),
                            );
                        } else {
                            scope.set(
                                &var_name,
                                vec![ResolvedType::from_type_string(element_type)],
                            );
                        }
                        return;
                    }
                }
            }
        }
        // Couldn't determine the element type (untyped/unknown iterable).
        // Seed `mixed` so body assignments like `$x = $value` after
        // `$x = null` overwrite pure-null and participate in post-loop
        // merge + `is_null` early-return narrowing.  Bare `array` is
        // already seeded as `mixed` above; fully untyped parameters
        // hit this path with `iter_type = None`.
        if scope.get(&var_name).is_empty() {
            scope.set(
                &var_name,
                vec![ResolvedType::from_type_string(PhpType::mixed())],
            );
        }
    } else if let Expression::Array(_) | Expression::List(_) = value_expr {
        // Array/list destructuring in foreach: `foreach ($items as [$a, $b])`
        // Extract the element type from the iterable, then resolve each
        // destructured variable's type from that element type using shape
        // keys or positional indices.
        let element_type: Option<PhpType> = iter_type.as_ref().and_then(|it| {
            crate::type_engine::variable::foreach_resolution::iteration_value_type(
                it,
                &iterable_ctx(ctx),
            )
        });

        if let Some(ref elem_type) = element_type {
            let elements_iter: Vec<&ArrayElement<'_>> = match value_expr {
                Expression::Array(arr) => arr.elements.iter().collect(),
                Expression::List(list) => list.elements.iter().collect(),
                _ => vec![],
            };

            let mut positional_index: usize = 0;
            for elem in elements_iter {
                let (var_name, shape_key) = match elem {
                    ArrayElement::KeyValue(kv) => {
                        if let Expression::Variable(Variable::Direct(dv)) = kv.value {
                            (
                                bytes_to_str(dv.name).to_string(),
                                extract_foreach_destr_key(kv.key),
                            )
                        } else {
                            continue;
                        }
                    }
                    ArrayElement::Value(val) => {
                        let key = Some(positional_index.to_string());
                        positional_index += 1;
                        if let Expression::Variable(Variable::Direct(dv)) = val.value {
                            (bytes_to_str(dv.name).to_string(), key)
                        } else {
                            continue;
                        }
                    }
                    // A hole (`foreach ($x as [, $parameter])`) names nothing
                    // but still consumes the position.
                    ArrayElement::Missing(_) => {
                        positional_index += 1;
                        continue;
                    }
                    _ => continue,
                };

                // Try shape key lookup first, then fall back to generic element type.
                let resolved_type = shape_key
                    .as_ref()
                    .and_then(|k| elem_type.shape_value_type(k).cloned())
                    .or_else(|| elem_type.extract_value_type(true).cloned());

                if let Some(ref vt) = resolved_type {
                    let resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
                        vt,
                        &ctx.current_class.name,
                        ctx.all_classes,
                        ctx.class_loader,
                    );
                    if !resolved.is_empty() {
                        scope.set(
                            &var_name,
                            ResolvedType::from_classes_with_hint(resolved, vt.clone()),
                        );
                    } else {
                        scope.set(&var_name, vec![ResolvedType::from_type_string(vt.clone())]);
                    }
                }
            }
        }
    }
}

/// Returns `true` when `expr` is a non-empty array literal such as
/// `["a", "b", "c"]` or `array(1, 2, 3)`.
///
/// Used by `process_foreach` to detect iterables that are guaranteed to
/// have at least one element, so that the pre-loop type of the target
/// variable does not survive into the post-loop scope.
pub(crate) fn is_non_empty_array_literal(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Array(arr) => !arr.elements.is_empty(),
        Expression::LegacyArray(arr) => !arr.elements.is_empty(),
        _ => false,
    }
}

/// Extract the variable name from a foreach value expression, unwrapping
/// a leading `&` (by-reference) if present.
pub(crate) fn extract_foreach_var_name(expr: &Expression<'_>) -> Option<String> {
    let inner = if let Expression::UnaryPrefix(up) = expr
        && matches!(up.operator, UnaryPrefixOperator::Reference(_))
    {
        up.operand
    } else {
        expr
    };
    if let Expression::Variable(Variable::Direct(dv)) = inner {
        Some(bytes_to_str(dv.name).to_string())
    } else {
        None
    }
}

/// Undo what the previous iteration wrote to a `foreach` target
/// variable, ahead of re-binding it for the next one.
///
/// The loop hands the target a fresh element at the top of every
/// iteration, so a write in the body — `$step = …`, and just as much
/// `$step['fo'] = …` — describes the element that iteration was given,
/// not the next one.  Merging it back over the loop's back edge leaves
/// the rebound variable carrying a type it cannot have, which then
/// defeats the guards in the body that would have narrowed it.
///
/// What the variable held *before* the loop is put back rather than
/// dropped: a `foreach` whose element type nothing can settle leaves the
/// name where it found it, and a loop that shadows an outer variable of
/// the same name is then no worse off than it was before the loop.
/// Clearing the entry first also drops the synthetic `$step['fo']` keys
/// and the proofs recorded against them, which the rebinding invalidates
/// whether or not a type replaces them.
fn reset_foreach_target(
    expr: &Expression<'_>,
    scope: &mut ScopeState,
    pre_loop_scope: &ScopeState,
) {
    let inner = if let Expression::UnaryPrefix(up) = expr
        && matches!(up.operator, UnaryPrefixOperator::Reference(_))
    {
        up.operand
    } else {
        expr
    };
    match inner {
        Expression::Variable(Variable::Direct(dv)) => {
            let var_name = bytes_to_str(dv.name);
            scope.remove(var_name);
            scope.invalidate_dependent_keys(var_name);
            if pre_loop_scope.contains(var_name) {
                let before = pre_loop_scope.get(var_name);
                if before.is_empty() {
                    scope.set_empty(var_name);
                } else {
                    scope.set(var_name, before.to_vec());
                }
            }
        }
        // Destructuring targets: `foreach ($rows as [$a, $b])` binds
        // every variable in the pattern, each one just as fresh.
        Expression::Array(arr) => {
            for elem in arr.elements.iter() {
                reset_foreach_destructured_element(elem, scope, pre_loop_scope);
            }
        }
        Expression::List(list) => {
            for elem in list.elements.iter() {
                reset_foreach_destructured_element(elem, scope, pre_loop_scope);
            }
        }
        _ => {}
    }
}

fn reset_foreach_destructured_element(
    elem: &ArrayElement<'_>,
    scope: &mut ScopeState,
    pre_loop_scope: &ScopeState,
) {
    match elem {
        ArrayElement::KeyValue(kv) => reset_foreach_target(kv.value, scope, pre_loop_scope),
        ArrayElement::Value(val) => reset_foreach_target(val.value, scope, pre_loop_scope),
        _ => {}
    }
}

/// Extract a string key from a foreach destructuring key expression.
///
/// Handles string literals (`'user'`, `"user"`) and integer literals.
pub(crate) fn extract_foreach_destr_key(key_expr: &Expression<'_>) -> Option<String> {
    match key_expr {
        Expression::Literal(Literal::String(lit_str)) => match lit_str.value {
            Some(bytes) => Some(literal_bytes_to_str(bytes)?.to_string()),
            None => {
                let raw = bytes_to_str(lit_str.raw).to_string();
                Some(raw.trim_matches('\'').trim_matches('"').to_string())
            }
        },
        Expression::Literal(Literal::Integer(lit_int)) => {
            Some(bytes_to_str(lit_int.raw).to_string())
        }
        _ => None,
    }
}

/// Bind a foreach key variable.
pub(crate) fn bind_foreach_key<'b>(
    key_expr: &'b Expression<'b>,
    iter_type: &Option<PhpType>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    if let Expression::Variable(Variable::Direct(dv)) = key_expr {
        let var_name = bytes_to_str(dv.name).to_string();
        // A bare `array` says nothing about its keys, and neither does an
        // untyped iterable: both leave the key `int|string`.
        let key_type = iter_type.as_ref().and_then(|it| {
            crate::type_engine::variable::foreach_resolution::iteration_key_type(
                it,
                &iterable_ctx(ctx),
            )
        });
        // Benevolent, because `int|string` here is not something the array
        // said — it is the whole of PHP's key domain, standing in for a key
        // type nobody wrote down.  Holding the user to both branches of a
        // union we invented turns every `substr($key, …)` into a false
        // positive, so a single branch satisfies it (`is_type_compatible`
        // implements that half).
        let key_type = key_type.unwrap_or_else(|| {
            PhpType::benevolent(PhpType::union(vec![PhpType::int(), PhpType::string()]))
        });
        scope.set(&var_name, ctx.resolved_types_for(key_type));
    }
}

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
    let loop_depth = enter_loop();

    // Hard limit: skip the body entirely at excessive nesting depth.
    if loop_depth > MAX_LOOP_DEPTH {
        leave_loop(loop_depth);
        return;
    }

    // Record `&&` and `||` chain snapshots for the while condition.
    record_short_circuit_snapshots(while_stmt.condition, scope, ctx);

    let pre_loop_scope = scope.clone();

    // Assignment in condition: `while ($x = expr())`.  Seeded before the
    // narrowing below so a condition that assigns and checks in one
    // expression (`while (($line = fgets($h)) !== false)`) finds the
    // variable in scope and can strip the sentinel from it.
    process_nested_assignments(while_stmt.condition, scope, ctx);

    // Pass-by-reference in condition: `while (preg_match(..., $matches))`
    seed_pass_by_ref_in_condition(while_stmt.condition, scope, ctx);

    // The while body executes when the condition is truthy, so apply
    // condition narrowing (instanceof, phpstan-assert-if-true, etc.).
    // This must happen AFTER saving pre_loop_scope so the narrowing
    // only affects the loop body, not the post-loop scope.
    apply_condition_narrowing(while_stmt.condition, scope, ctx);

    // When the cursor is inside the loop body (completion path), discovery
    // passes must walk the ENTIRE body; the final pass uses the real
    // cursor_offset so it stops at the cursor as usual.
    let body_span = match &while_stmt.body {
        WhileBody::Statement(inner) => inner.span(),
        WhileBody::ColonDelimited(body) => body.span(),
    };
    let cursor_in_body =
        ctx.cursor_offset >= body_span.start.offset && ctx.cursor_offset <= body_span.end.offset;
    let discovery_ctx = if cursor_in_body && !is_diagnostic_scope_active() {
        ctx.with_cursor_offset(u32::MAX)
    } else {
        ctx.with_cursor_offset(ctx.cursor_offset)
    };

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

    push_exit_frame();
    walk_loop_body_to_fixed_point(
        &body_stmts,
        scope,
        LoopWalk {
            pre_loop_scope: &pre_loop_scope,
            assignment_depth,
            fold_exit_edges: !cursor_in_body,
            ctx,
            discovery_ctx: &discovery_ctx,
        },
        |next_scope, point| {
            if point != LoopSeedPoint::Entry {
                return;
            }
            process_nested_assignments(while_stmt.condition, next_scope, ctx);
            seed_pass_by_ref_in_condition(while_stmt.condition, next_scope, ctx);
            apply_condition_narrowing(while_stmt.condition, next_scope, ctx);
        },
    );
    let exits = pop_exit_frame();

    // When the cursor is inside the loop body (completion path), keep
    // the scope with condition narrowing applied.  The post-loop
    // merge would erase the narrowing (since the loop might not execute),
    // but the cursor IS inside the body, so the condition is true.
    if cursor_in_body && !is_diagnostic_scope_active() {
        leave_loop(loop_depth);
        return;
    }

    // The loop body might not execute at all (condition false on
    // first check), so merge with the pre-loop scope.
    let post_loop = scope.clone();
    *scope = pre_loop_scope;
    scope.merge_branch(&post_loop);

    // After the loop, the condition evaluated to false (that's why the
    // loop exited).  Apply the inverse of the condition to narrow types.
    // For example: `while ($a) { $a = $a->parent; }` => after loop, $a is null.
    apply_condition_narrowing_inverse(while_stmt.condition, scope, ctx);

    // A `break` leaves without re-testing the condition, so its state
    // joins *after* the inverse narrowing rather than being narrowed by it.
    merge_exit_edges(scope, &exits.breaks);

    // Remove synthetic property access keys that were seeded by
    // condition narrowing.  These represent narrowed types that only
    // hold inside the loop body (where the condition is true).
    // After the loop, the condition may be false, so the narrowing
    // no longer applies.
    strip_synthetic_property_keys(scope);

    leave_loop(loop_depth);
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
    let loop_depth = enter_loop();

    // Hard limit: skip the body entirely at excessive nesting depth.
    if loop_depth > MAX_LOOP_DEPTH {
        leave_loop(loop_depth);
        return;
    }

    // Process initializer expressions (e.g. `$i = 0`).
    for init_expr in for_stmt.initializations.iter() {
        process_assignment_expr(init_expr, scope, ctx);
    }

    // Process condition assignments (e.g. `for (; $x = nextItem(); )`)
    // and pass-by-ref in conditions (e.g. `for (; preg_match(..., $m); )`).
    for cond_expr in for_stmt.conditions.iter() {
        process_nested_assignments(cond_expr, scope, ctx);
        seed_pass_by_ref_in_condition(cond_expr, scope, ctx);
    }

    // Record a snapshot at each condition expression so that member
    // accesses in the condition clause (which live on the `for` line,
    // before any body statement) see the variables bound by the init
    // clause.  Without this, a diagnostic on the condition would only
    // find the pre-`for` snapshot and treat init-clause variables as
    // unresolved.
    if is_diagnostic_scope_active() {
        for cond_expr in for_stmt.conditions.iter() {
            record_scope_snapshot(cond_expr.span().start.offset, scope);
        }
    }

    // A condition clause narrows its own operands the way an `if`
    // condition does: `for (; $n && $n->next(); )` reaches `$n->next()`
    // only with `$n` non-null.
    for cond_expr in for_stmt.conditions.iter() {
        record_short_circuit_snapshots(cond_expr, scope, ctx);
    }

    let pre_loop_scope = scope.clone();

    // The body executes when the conditions are truthy, so apply condition
    // narrowing (instanceof, isset, phpstan-assert-if-true, etc.) the same
    // way `process_while` does for its single condition. Comma-separated
    // conditions are evaluated left to right, so narrow them in that order;
    // only the last one's truthiness decides whether the body runs, but an
    // earlier clause can still narrow a variable that a later clause or the
    // body depends on.
    for cond_expr in for_stmt.conditions.iter() {
        apply_condition_narrowing(cond_expr, scope, ctx);
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
    let discovery_ctx = if cursor_in_body && !is_diagnostic_scope_active() {
        ctx.with_cursor_offset(u32::MAX)
    } else {
        ctx.with_cursor_offset(ctx.cursor_offset)
    };

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

    push_exit_frame();
    walk_loop_body_to_fixed_point(
        &body_stmts,
        scope,
        LoopWalk {
            pre_loop_scope: &pre_loop_scope,
            assignment_depth,
            fold_exit_edges: !cursor_in_body,
            ctx,
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
                for increment in for_stmt.increments.iter() {
                    process_assignment_expr(increment, next_scope, ctx);
                }
            }
            LoopSeedPoint::Entry => {
                for cond_expr in for_stmt.conditions.iter() {
                    process_nested_assignments(cond_expr, next_scope, ctx);
                    seed_pass_by_ref_in_condition(cond_expr, next_scope, ctx);
                    apply_condition_narrowing(cond_expr, next_scope, ctx);
                }
            }
        },
    );
    let exits = pop_exit_frame();

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
        leave_loop(loop_depth);
        return;
    }

    // A loop that ran its body at least once exited from the condition
    // check that follows the update clause, so the reassignments the update
    // clause makes are part of the post-loop state.
    for increment in for_stmt.increments.iter() {
        process_assignment_expr(increment, scope, ctx);
    }

    // The loop body might not execute at all (condition false on
    // first check), so merge with the pre-loop scope.
    let post_loop = scope.clone();
    *scope = pre_loop_scope;
    scope.merge_branch(&post_loop);

    // After the loop, only the last condition clause decided the exit (the
    // earlier clauses were evaluated for their side effects but don't gate
    // continuation), so apply the inverse of just that clause.
    // For example: `for (; ($row = fgetcsv($h)) !== false; )` => after the
    // loop, $row is false.
    if let Some(last_cond) = for_stmt.conditions.iter().last() {
        apply_condition_narrowing_inverse(last_cond, scope, ctx);
    }

    // A `break` leaves without re-testing the condition, so its state
    // joins after the inverse narrowing.
    merge_exit_edges(scope, &exits.breaks);

    // Remove synthetic property access keys that were seeded by condition
    // narrowing; they only hold inside the loop body where the conditions
    // were true.
    strip_synthetic_property_keys(scope);

    leave_loop(loop_depth);
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
    let loop_depth = enter_loop();

    // Hard limit: skip the body entirely at excessive nesting depth.
    if loop_depth > MAX_LOOP_DEPTH {
        leave_loop(loop_depth);
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

    push_exit_frame();
    walk_loop_body_to_fixed_point(
        &body_stmts,
        scope,
        LoopWalk {
            pre_loop_scope: &pre_loop_scope,
            assignment_depth,
            fold_exit_edges: !cursor_in_body,
            ctx,
            discovery_ctx: ctx,
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
                process_nested_assignments(dw.condition, next_scope, ctx);
                seed_pass_by_ref_in_condition(dw.condition, next_scope, ctx);
                // The assignment above re-runs `$c = $c->getParent()` on
                // the merged scope, which puts the declared `?Category`
                // back over the narrowing `AfterBody` applied. The loop
                // only re-enters when the condition held, so that
                // narrowing has to go back on top of it.
                apply_condition_narrowing(dw.condition, next_scope, ctx);
            }
        },
    );
    let exits = pop_exit_frame();

    // The condition runs after the body, so its own `&&`/`||` narrowing
    // is recorded against the scope the body leaves behind: that is where
    // `do { $n = next(); } while ($n && $n->ok());` reads `$n` from.
    record_short_circuit_snapshots(dw.condition, scope, ctx);

    // After the do-while loop, the condition evaluated to false (that's
    // why the loop exited).  Apply the inverse of the condition to narrow
    // types.  For example: `do { $a = getA(); } while ($a !== null);`
    // => after loop, $a is null.
    apply_condition_narrowing_inverse(dw.condition, scope, ctx);

    // A `break` leaves without re-testing the condition, so its state
    // joins after the inverse narrowing.  This is the only way the state
    // before an early `break` reaches the code after the loop: the body
    // always runs, so there is no pre-loop scope to merge with.
    if !cursor_in_body {
        merge_exit_edges(scope, &exits.breaks);
    }

    leave_loop(loop_depth);
}

/// Bind the exception variable a `catch` clause names into `scope`.
///
/// A clause that names none (`catch (LogicException)`) binds nothing, and
/// so does one whose hint resolves to no class: leaving the variable
/// unset beats recording it as untyped, which would mask whatever the
/// scope already knew about that name.
fn bind_catch_variable(
    catch: &TryCatchClause<'_>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let Some(ref var) = catch.variable else {
        return;
    };
    let parsed_hint = extract_hint_type(&catch.hint);
    let resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        &parsed_hint,
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    );
    let exception_types = ResolvedType::from_classes_with_hint(resolved, parsed_hint);
    if !exception_types.is_empty() {
        scope.set(bytes_to_str(var.name), exception_types);
    }
}

/// Process a `try-catch-finally` statement.
pub(crate) fn process_try<'b>(
    try_stmt: &'b Try<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let pre_try_scope = scope.clone();

    let try_body_span = try_stmt.block.span();
    let cursor_in_try = ctx.cursor_offset >= try_body_span.start.offset
        && ctx.cursor_offset <= try_body_span.end.offset;

    if cursor_in_try {
        walk_body_forward(try_stmt.block.statements.iter(), scope, ctx);
        return;
    }

    for catch in try_stmt.catch_clauses.iter() {
        let catch_span = catch.block.span();
        if ctx.cursor_offset >= catch_span.start.offset
            && ctx.cursor_offset <= catch_span.end.offset
        {
            // Start from the pre-try scope, since the exception could
            // have been thrown at any point in the try body, and bind the
            // caught exception variable on top of it.
            *scope = pre_try_scope.clone();
            bind_catch_variable(catch, scope, ctx);
            walk_body_forward(catch.block.statements.iter(), scope, ctx);
            return;
        }
    }

    if let Some(ref finally) = try_stmt.finally_clause {
        let finally_span = finally.block.span();
        if ctx.cursor_offset >= finally_span.start.offset
            && ctx.cursor_offset <= finally_span.end.offset
        {
            // In finally, merge all possible paths.
            walk_body_forward(try_stmt.block.statements.iter(), scope, ctx);
            walk_body_forward(finally.block.statements.iter(), scope, ctx);
            return;
        }
    }

    // Cursor is after the try/catch/finally.  Walk the try body and
    // merge all catch scopes.
    walk_body_forward(try_stmt.block.statements.iter(), scope, ctx);
    let try_scope = scope.clone();

    let mut all_scopes = vec![try_scope];
    for catch in try_stmt.catch_clauses.iter() {
        let mut catch_scope = pre_try_scope.clone();
        bind_catch_variable(catch, &mut catch_scope, ctx);
        walk_body_forward(catch.block.statements.iter(), &mut catch_scope, ctx);
        // A catch that rethrows or returns never reaches the statement
        // after the `try`, so the state it leaves must not be merged in:
        // that is what puts the pre-try type of a variable the try body
        // assigned back into the join.
        if branch_exits_stmts(catch.block.statements.iter(), &catch_scope, ctx) {
            continue;
        }
        all_scopes.push(catch_scope);
    }

    // Merge all scopes.
    let mut merged = all_scopes[0].clone();
    for s in &all_scopes[1..] {
        merged.merge_branch(s);
    }
    *scope = merged;

    // Walk the finally block if present.
    if let Some(ref finally) = try_stmt.finally_clause {
        walk_body_forward(finally.block.statements.iter(), scope, ctx);
    }
}

/// Process a `switch` statement.
///
/// Each case arm is walked on a clone of the pre-switch scope so that
/// assignments in one arm don't leak into another.  After all arms are
/// walked, the resulting scopes are merged (union of types), matching
/// the runtime behaviour where only one arm executes.
///
/// Fall-through cases (cases with no statements) share their scope
/// with the next non-empty case, mirroring PHP semantics.
pub(crate) fn process_switch<'b>(
    switch: &'b Switch<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let pre_switch_scope = scope.clone();
    let cases: Vec<_> = switch.body.cases().iter().collect();

    if cases.is_empty() {
        return;
    }

    // PHP counts a `switch` as a breakable structure: a `break;` in a case
    // arm leaves the switch (and must not be attributed to an enclosing
    // loop, which is what a `break 2` would target).  Each arm owns the
    // jumps written inside it, so the state a `break` left with is folded
    // straight back into that arm's own contribution — for the trailing
    // `break;` that closes almost every arm the two are the same state,
    // and `merge_branch` recognises that and does nothing.
    let mut branch_scopes: Vec<ScopeState> = Vec::new();
    let mut has_default = false;

    let walk_arm = |stmts: &[&Statement<'b>], branch_scopes: &mut Vec<ScopeState>| {
        let mut case_scope = pre_switch_scope.clone();
        push_exit_frame();
        walk_body_forward(stmts.iter().copied(), &mut case_scope, ctx);
        let arm_exits = pop_exit_frame();
        merge_exit_edges(&mut case_scope, &arm_exits.breaks);
        branch_scopes.push(case_scope);
    };

    // Walk cases, accumulating fall-through groups.
    let mut accumulated_stmts: Vec<&Statement<'b>> = Vec::new();
    for case in &cases {
        if case.is_default() {
            has_default = true;
        }

        let stmts: Vec<_> = case.statements().iter().collect();
        if stmts.is_empty() {
            // Fall-through: no statements, will share scope with next case.
            continue;
        }

        accumulated_stmts.extend(stmts);
        walk_arm(&accumulated_stmts, &mut branch_scopes);
        accumulated_stmts.clear();
    }

    // Handle trailing fall-through cases (empty cases at the end).
    if !accumulated_stmts.is_empty() {
        walk_arm(&accumulated_stmts, &mut branch_scopes);
    }

    if branch_scopes.is_empty() {
        return;
    }

    // Merge all branch scopes.
    let mut merged = branch_scopes[0].clone();
    for s in &branch_scopes[1..] {
        merged.merge_branch(s);
    }

    // If there is no default case, the switch might not execute any
    // arm at all, so merge with the pre-switch scope.
    if !has_default {
        merged.merge_branch(&pre_switch_scope);
    }

    *scope = merged;
}
