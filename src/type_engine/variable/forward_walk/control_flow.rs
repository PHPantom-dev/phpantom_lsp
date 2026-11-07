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
    let parsed_hint = ctx
        .written_names()
        .resolve_hint(&extract_hint_type(&catch.hint));
    let resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        &parsed_hint,
        &ctx.current_class.fqn(),
        ctx.file.all_classes,
        ctx.file.class_loader,
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

/// Walk `statements` into `scope` and return the join of the state before
/// each statement that can throw, or `None` when none of them can.
///
/// That join is what a handler for an exception thrown from the
/// statements starts from: the assignments made before the throw, but not
/// the one the throwing statement was about to make (`$x = mayThrow();`
/// leaves `$x` as it was).
///
/// When `throws` is given, what each statement can throw is added to it.
fn walk_throwing_statements<'b>(
    statements: impl Iterator<Item = &'b Statement<'b>>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    mut throws: Option<&mut ThrowPoints>,
) -> Option<ScopeState> {
    let mut catch_entry: Option<ScopeState> = None;
    for stmt in statements {
        let flat = matches!(
            stmt,
            Statement::Expression(_)
                | Statement::Return(_)
                | Statement::Echo(_)
                | Statement::EchoTag(_)
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
    catch_entry
}

/// Walk the `try` body into `scope` and return the scope a `catch` clause
/// starts from (see [`walk_throwing_statements`]), or `None` when nothing
/// in the body can throw.
fn walk_try_body<'b>(
    try_stmt: &'b Try<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    throws: Option<&mut ThrowPoints>,
) -> Option<ScopeState> {
    walk_throwing_statements(try_stmt.block.statements.iter(), scope, ctx, throws)
}

/// Walk a `catch` clause from `catch_entry` and return the scope it
/// finishes with.
///
/// When `thrown` is given, the state before each statement of the clause
/// that can throw is merged into it: an exception the clause throws
/// partway through still runs the `finally` block.
fn walk_catch<'b>(
    catch: &'b TryCatchClause<'b>,
    catch_entry: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    thrown: Option<&mut Option<ScopeState>>,
) -> ScopeState {
    let mut catch_scope = catch_entry.clone();
    bind_catch_variable(catch, &mut catch_scope, ctx);
    let statements = catch.block.statements.iter();
    match thrown {
        Some(thrown) => {
            if let Some(entry) = walk_throwing_statements(statements, &mut catch_scope, ctx, None) {
                merge_into(thrown, entry);
            }
        }
        None => walk_body_forward(statements, &mut catch_scope, ctx),
    }
    catch_scope
}

fn merge_into(target: &mut Option<ScopeState>, state: ScopeState) {
    match target.as_mut() {
        Some(target) => target.merge_branch(&state),
        None => *target = Some(state),
    }
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
        let pre_try = scope.clone();
        *scope = walk_try_body(try_stmt, scope, ctx, None).unwrap_or(pre_try);
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
            // an exception no catch takes, which arrives with whatever the
            // try body or catch had done when it threw, and a `return` out
            // of the try body.
            let mut throws = ThrowPoints::default();
            let return_frame = push_return_frame(ctx.flow);
            let pre_try = scope.clone();
            let thrown = walk_try_body(try_stmt, scope, ctx, Some(&mut throws));
            let mut uncaught = thrown
                .as_ref()
                .filter(|_| !catches_everything(try_stmt))
                .cloned();
            let catch_entry = thrown.unwrap_or(pre_try);
            let mut merged = scope.clone();
            for catch in try_stmt.catch_clauses.iter() {
                if !throws.reach(catch, ctx) {
                    continue;
                }
                let catch_scope = walk_catch(catch, &catch_entry, ctx, Some(&mut uncaught));
                merged.merge_branch(&catch_scope);
            }
            if let Some(uncaught) = &uncaught {
                merged.merge_branch(uncaught);
            }
            if let Some(returned) = return_frame.finish() {
                merged.merge_branch(&returned);
                record_return_edge(ctx.flow, &returned);
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
    // A `finally` block also runs on the way out of a `return` in the try
    // body or a catch, so what each one carries out is collected for it.
    let return_frame = try_stmt
        .finally_clause
        .is_some()
        .then(|| push_return_frame(ctx.flow));
    let pre_try = scope.clone();
    let thrown = walk_try_body(try_stmt, scope, ctx, collect);
    let has_finally = try_stmt.finally_clause.is_some();
    // The exceptions that leave the statement through the `finally` block:
    // one from the try body that no catch takes, or one a catch throws
    // partway through.
    let mut uncaught = thrown
        .as_ref()
        .filter(|_| has_finally && !catches_everything(try_stmt))
        .cloned();
    let catch_entry = thrown.unwrap_or(pre_try);
    let mut try_scope = scope.clone();
    // A try body that always returns or throws never runs off its end, so
    // only the catches that carry on reach the statement after the `try`.
    // The join treats an unreachable scope as contributing nothing.
    if branch_exits_stmts(try_stmt.block.statements.iter(), &try_scope, ctx) {
        try_scope.unreachable = true;
    }

    let mut all_scopes = vec![try_scope];
    // The catches that return or rethrow, which leave through the
    // `finally` block without reaching the statement after the `try`.
    let mut exited: Option<ScopeState> = None;
    for catch in try_stmt.catch_clauses.iter() {
        throws.record_catch_types(catch, ctx);
        if !throws.reach(catch, ctx) {
            continue;
        }
        let catch_scope = walk_catch(
            catch,
            &catch_entry,
            ctx,
            has_finally.then_some(&mut uncaught),
        );
        // A catch that rethrows or returns never reaches the statement
        // after the `try`, so the state it leaves must not be merged in:
        // that is what puts the pre-try type of a variable the try body
        // assigned back into the join.
        if branch_exits_stmts(catch.block.statements.iter(), &catch_scope, ctx) {
            if has_finally {
                merge_into(&mut exited, catch_scope);
            }
            continue;
        }
        all_scopes.push(catch_scope);
    }
    let returned = return_frame.and_then(ReturnFrameGuard::finish);

    // Merge all scopes.
    let mut merged = all_scopes[0].clone();
    for s in &all_scopes[1..] {
        merged.merge_branch(s);
    }
    *scope = merged;

    if let Some(ref finally) = try_stmt.finally_clause {
        walk_finally_block(
            finally,
            scope,
            [exited.as_ref(), returned.as_ref(), uncaught.as_ref()],
            ctx,
        );
    }
    // The `return`s collected above still leave the enclosing body.
    if let Some(returned) = &returned {
        record_return_edge(ctx.flow, returned);
    }
}

/// Walk a `finally` block for the code after its `try` statement.
///
/// `scope` holds the paths that carry on past the statement, and `exits`
/// the ones that leave it by a `return`, a rethrowing `catch` or an
/// uncaught exception, which run the block on their way out too.  The code after the statement must only
/// see the first, while the block's own statements are read with both.  So when
/// the walk records the scopes statements are read with, the block is
/// walked twice: once for `scope`, recording nothing, and once more from
/// the join with `exits`, for the record.  A walk that records nothing
/// has no use for the second one.
///
/// Only the recording walk ever takes the second path, and a `try` nested
/// in the block is walked by the quiet one as a single walk again, so a
/// block nested `n` deep is walked `n + 1` times rather than `2^n`.
fn walk_finally_block<'b>(
    finally: &'b TryFinallyClause<'b>,
    scope: &mut ScopeState,
    exits: [Option<&ScopeState>; 3],
    ctx: &ForwardWalkCtx<'_>,
) {
    let statements = &finally.block.statements;
    let recording = is_diagnostic_scope_active() && !snapshot_recording_suspended();
    let entry = (recording && exits.iter().any(Option::is_some))
        .then(|| {
            let mut entry = scope.clone();
            for exit in exits.into_iter().flatten() {
                entry.merge_branch(exit);
            }
            entry
        })
        .filter(|entry| !entry.walks_the_same_as(scope));
    let Some(mut entry) = entry else {
        walk_body_forward(statements.iter(), scope, ctx);
        return;
    };
    {
        let _quiet = suspend_snapshot_recording();
        walk_body_forward(statements.iter(), scope, ctx);
    }
    // This walk is only there for the record, so it must leave nothing else
    // behind: the returns the block makes were recorded by the one above.
    let _barrier = suspend_return_edges(ctx.flow);
    walk_body_forward(statements.iter(), &mut entry, ctx);
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
        let exit_frame = ExitFrameGuard::push(ctx.flow);
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
