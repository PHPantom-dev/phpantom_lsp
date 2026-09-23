use super::*;

use mago_span::HasSpan;

use crate::atom::bytes_to_str;
use crate::parser::extract_hint_type;
use crate::type_engine::types::narrowing;
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
