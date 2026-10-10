use super::*;

use mago_span::HasSpan;

use crate::atom::bytes_to_str;
use crate::parser::extract_hint_type;
use crate::php_type::{PhpType, TypeKind};
use crate::types::ResolvedType;

/// Bind the exception variable a `catch` clause names into `scope`.
///
/// A clause that names none (`catch (LogicException)`) binds nothing.  A
/// hint naming a class that cannot be loaded still binds the named type,
/// the way `new UndeclaredFoo()` does, so the variable joins the scope
/// after the `try` rather than keeping whatever it held before.
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
    let exception_types = if resolved.is_empty() {
        vec![ResolvedType::from_type_string(parsed_hint)]
    } else {
        ResolvedType::from_classes_with_hint(resolved, parsed_hint)
    };
    scope.set(bytes_to_str(var.name), exception_types);
}

/// Whether one of the statement's catch clauses takes `\Throwable`, so no
/// exception from the try body leaves it uncaught.
fn catches_everything(try_stmt: &Try<'_>) -> bool {
    try_stmt.catch_clauses.iter().any(|catch| {
        let hint = extract_hint_type(&catch.hint);
        let names_throwable = |ty: &PhpType| {
            ty.base_name().is_some_and(|name| {
                name.trim_start_matches('\\')
                    .eq_ignore_ascii_case("Throwable")
            })
        };
        match hint.kind() {
            TypeKind::Union(members) => members.iter().any(names_throwable),
            _ => names_throwable(&hint),
        }
    })
}

/// Walk a `try` body into `scope` and return the scope a `catch` clause
/// starts from.
///
/// A `catch` sees the join of the state before each statement that can
/// throw: the assignments the body made before the throw, but not the one
/// the throwing statement was about to make (`$x = mayThrow();` leaves `$x`
/// as it was).  A body where nothing can throw leaves the catch with the
/// state before the `try`.
///
/// When `throws` is given, what each statement can throw is added to it.
fn walk_try_body<'b>(
    try_stmt: &'b Try<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    mut throws: Option<&mut ThrowPoints>,
) -> ScopeState {
    let pre_try = scope.clone();
    let mut catch_entry: Option<ScopeState> = None;
    for stmt in try_stmt.block.statements.iter() {
        let flat = matches!(
            stmt,
            Statement::Expression(_) | Statement::Return(_) | Statement::Echo(_)
        );
        let mut stmt_throws = ThrowPoints::default();
        stmt_throws.collect(stmt, flat.then_some(&*scope), ctx);
        if stmt_throws.may_throw() {
            match catch_entry.as_mut() {
                Some(entry) => entry.merge_branch(scope),
                None => catch_entry = Some(scope.clone()),
            }
        }
        if let Some(throws) = throws.as_deref_mut() {
            throws.absorb(stmt_throws);
        }
        walk_body_forward(std::iter::once(stmt), scope, ctx);
    }
    catch_entry.unwrap_or(pre_try)
}

/// Process a `try-catch-finally` statement.
pub(crate) fn process_try<'b>(
    try_stmt: &'b Try<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let try_body_span = try_stmt.block.span();
    let cursor_in_try = ctx.cursor_offset >= try_body_span.start.offset
        && ctx.cursor_offset <= try_body_span.end.offset;

    if cursor_in_try {
        walk_body_forward(try_stmt.block.statements.iter(), scope, ctx);
        return;
    }

    let cursor_in_catch = try_stmt.catch_clauses.iter().find(|catch| {
        let catch_span = catch.block.span();
        ctx.cursor_offset >= catch_span.start.offset && ctx.cursor_offset <= catch_span.end.offset
    });
    if let Some(catch) = cursor_in_catch {
        *scope = walk_try_body(try_stmt, scope, ctx, None);
        bind_catch_variable(catch, scope, ctx);
        walk_body_forward(catch.block.statements.iter(), scope, ctx);
        return;
    }

    if let Some(ref finally) = try_stmt.finally_clause {
        let finally_span = finally.block.span();
        if ctx.cursor_offset >= finally_span.start.offset
            && ctx.cursor_offset <= finally_span.end.offset
        {
            // Every path through the statement runs the finally block: the
            // try body finishing, each catch finishing (even one that
            // returns or rethrows, since the block runs before it leaves),
            // and an exception no catch takes, which arrives with whatever
            // the try body had done when it threw.
            let mut throws = ThrowPoints::default();
            let catch_entry = walk_try_body(try_stmt, scope, ctx, Some(&mut throws));
            let mut merged = scope.clone();
            for catch in try_stmt.catch_clauses.iter() {
                if !throws.reach(catch, ctx) {
                    continue;
                }
                let mut catch_scope = catch_entry.clone();
                bind_catch_variable(catch, &mut catch_scope, ctx);
                walk_body_forward(catch.block.statements.iter(), &mut catch_scope, ctx);
                merged.merge_branch(&catch_scope);
            }
            if !catches_everything(try_stmt) {
                merged.merge_branch(&catch_entry);
            }
            *scope = merged;
            walk_body_forward(finally.block.statements.iter(), scope, ctx);
            return;
        }
    }

    // Cursor is after the try/catch/finally.  Walk the try body and
    // merge all catch scopes.
    let mut throws = ThrowPoints::default();
    let collect = (!try_stmt.catch_clauses.is_empty()).then_some(&mut throws);
    let catch_entry = walk_try_body(try_stmt, scope, ctx, collect);
    let try_scope = scope.clone();

    let mut all_scopes = vec![try_scope];
    for catch in try_stmt.catch_clauses.iter() {
        if !throws.reach(catch, ctx) {
            continue;
        }
        let mut catch_scope = catch_entry.clone();
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

    // Every literal label, which the `default` arm has seen fail.
    let all_labels: Vec<&Expression<'b>> = cases
        .iter()
        .filter_map(|case| match case {
            SwitchCase::Expression(c) => Some(c.expression),
            SwitchCase::Default(_) => None,
        })
        .collect();

    // An arm is narrowed by the labels that lead into it, unless the arm
    // before it can run on into it without a `break`: then the labels say
    // nothing about the values that arrive that way.
    let mut falls_into_next = false;
    let mut walk_arm = |stmts: &[&Statement<'b>],
                        labels: &[&Expression<'b>],
                        is_default: bool,
                        branch_scopes: &mut Vec<ScopeState>| {
        let mut case_scope = pre_switch_scope.clone();
        if !falls_into_next {
            if !is_default {
                apply_switch_arm_narrowing(switch.expression, labels, &[], &mut case_scope, ctx);
            } else if labels.is_empty() {
                apply_switch_arm_narrowing(
                    switch.expression,
                    &[],
                    &all_labels,
                    &mut case_scope,
                    ctx,
                );
            }
        }
        let arm_jumps_out = branch_exits_stmts(stmts.iter().copied(), &case_scope, ctx);
        falls_into_next = !arm_jumps_out;
        let exit_frame = ExitFrameGuard::push();
        walk_body_forward(stmts.iter().copied(), &mut case_scope, ctx);
        let arm_exits = exit_frame.pop();
        // The cursor sits in this arm, so what the arm knows at the
        // cursor is the answer, not the join of every arm after it.
        let holds_cursor = stmts
            .first()
            .zip(stmts.last())
            .is_some_and(|(first, last)| {
                ctx.cursor_offset >= first.span().start.offset
                    && ctx.cursor_offset <= last.span().end.offset
            });
        if holds_cursor {
            return Some(case_scope);
        }
        // An arm that jumps out never runs off its end: what it hands the
        // code after the `switch` is only what its `break`s carried, and
        // an arm that throws or returns hands it nothing.  PHP treats a
        // `continue` that targets the `switch` as a `break`.
        if arm_jumps_out {
            case_scope.unreachable = true;
        }
        merge_exit_edges(&mut case_scope, &arm_exits.breaks);
        merge_exit_edges(&mut case_scope, &arm_exits.continues);
        branch_scopes.push(case_scope);
        None
    };

    // Walk cases, accumulating fall-through groups.
    let mut group_labels: Vec<&Expression<'b>> = Vec::new();
    let mut group_has_default = false;
    for case in &cases {
        match case {
            SwitchCase::Expression(c) => group_labels.push(c.expression),
            SwitchCase::Default(_) => {
                has_default = true;
                group_has_default = true;
            }
        }

        let stmts: Vec<_> = case.statements().iter().collect();
        if stmts.is_empty() {
            // Fall-through: no statements, will share scope with next case.
            continue;
        }

        if let Some(at_cursor) =
            walk_arm(&stmts, &group_labels, group_has_default, &mut branch_scopes)
        {
            *scope = at_cursor;
            return;
        }
        group_labels.clear();
        group_has_default = false;
    }

    let Some(mut merged) = ScopeState::join_all(branch_scopes) else {
        return;
    };

    // If there is no default case, the switch might not execute any
    // arm at all, so merge with the pre-switch scope.
    if !has_default {
        merged.merge_branch(&pre_switch_scope);
    }

    // Every arm returns or throws and a `default` catches every value:
    // nothing reaches the code after the `switch`.  As with an `if` whose
    // branches all exit, the pre-switch types are the least surprising
    // answer for a cursor in that dead code.
    if merged.unreachable && !pre_switch_scope.unreachable {
        *scope = pre_switch_scope;
        scope.unreachable = true;
        return;
    }

    *scope = merged;
}
