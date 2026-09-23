use super::*;

use mago_span::HasSpan;

use crate::atom::bytes_to_str;
use crate::parser::extract_hint_type;
use crate::types::ResolvedType;

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
        let exit_frame = ExitFrameGuard::push();
        walk_body_forward(stmts.iter().copied(), &mut case_scope, ctx);
        let arm_exits = exit_frame.pop();
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
