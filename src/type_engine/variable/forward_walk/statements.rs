//! The walker's statement dispatcher: applying one statement, or one
//! expression statement, to the scope, and handing nested blocks,
//! branches and loops to the modules that walk them.

use mago_span::HasSpan;

use crate::atom::{atom, bytes_to_str};
use crate::types::ResolvedType;

use super::*;

/// Process a single statement, updating `scope` with any variable
/// assignments, narrowing, or control-flow effects.
pub(crate) fn process_statement<'b>(
    stmt: &'b Statement<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // An expression statement runs its own `@var` handling, which has
    // extra rules (the LHS is left alone while the cursor sits in the
    // RHS, a scalar RHS blocks a class override).  Every other statement
    // kind only ever sees standalone annotations, and `global` restricts
    // those to the variables it imports.
    if !matches!(stmt, Statement::Expression(_) | Statement::Global(_)) {
        let stmt_offset = stmt.span().start.offset;
        if apply_standalone_var_docblocks(stmt_offset, scope, ctx) {
            // The diagnostic scope snapshot at this offset was recorded
            // by the caller *before* this call, so it still holds the
            // pre-docblock scope. Re-record it now so that a lookup for
            // an expression inside this same statement (e.g. `echo
            // $v->method()` right after a standalone `@var $v` block)
            // sees the type the docblock just applied instead of
            // falling through to the stale snapshot.
            record_scope_snapshot(stmt_offset, scope);
        }
    }

    match stmt {
        Statement::Expression(expr_stmt) => {
            process_expression_statement(expr_stmt, scope, ctx);
        }
        Statement::Foreach(foreach) => {
            process_foreach(foreach, scope, ctx);
        }
        Statement::If(if_stmt) => {
            process_if(if_stmt, stmt, scope, ctx);
        }
        Statement::While(while_stmt) => {
            process_while(while_stmt, scope, ctx);
        }
        Statement::For(for_stmt) => {
            process_for(for_stmt, scope, ctx);
        }
        Statement::DoWhile(dw) => {
            process_do_while(dw, scope, ctx);
        }
        Statement::Try(try_stmt) => {
            process_try(try_stmt, scope, ctx);
        }
        Statement::Switch(switch) => {
            process_switch(switch, scope, ctx);
        }
        Statement::Block(block) => {
            walk_body_forward(block.statements.iter(), scope, ctx);
        }
        // A jump out of a loop carries the types it holds *here* to the
        // loop's join, not to the statement that follows it.  The loop
        // that owns the edge folds it back in.
        Statement::Break(brk) => {
            record_exit_edge(ctx.flow, exit_level(brk.level), true, scope);
        }
        Statement::Continue(cont) => {
            record_exit_edge(ctx.flow, exit_level(cont.level), false, scope);
        }
        Statement::Unset(unset_stmt) => {
            for val in unset_stmt.values.iter() {
                match val {
                    Expression::Variable(Variable::Direct(dv)) => {
                        scope.remove(bytes_to_str(dv.name));
                    }
                    // `unset($arr['key'])` removes one element rather than
                    // the whole variable, so a `non-empty-array` or shape
                    // type must lose whatever emptiness guarantee that
                    // element was supplying — otherwise a later `foreach`
                    // over the same array still assumes its body runs.
                    Expression::ArrayAccess(array_access) => {
                        if let Some((base_name, key_chain)) =
                            super::super::array_shape_writes::extract_nested_array_access_chain(
                                array_access,
                            )
                        {
                            let Some(base_type) = scope
                                .get(&base_name)
                                .last()
                                .map(|rt| rt.type_string.clone())
                            else {
                                continue;
                            };
                            let keys: Vec<Option<String>> = key_chain
                                .iter()
                                .map(|idx| {
                                    super::super::array_shape_writes::extract_array_key_for_shape(
                                        idx,
                                    )
                                })
                                .collect();
                            let updated =
                                super::super::array_shape_writes::apply_nested_array_unset(
                                    &base_type, &keys,
                                );
                            scope.set(&base_name, vec![ResolvedType::from_type_string(updated)]);
                            let removed_from = super::array_assignment::array_write_synthetic_key(
                                &base_name,
                                &key_chain[..key_chain.len() - 1],
                            );
                            scope.note_element_write(
                                removed_from.as_deref().unwrap_or(&base_name),
                                true,
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
        Statement::Namespace(ns) => {
            walk_body_forward(ns.statements().iter(), scope, ctx);
        }
        Statement::Global(global) => {
            let mut imported: Vec<&str> = Vec::with_capacity(global.variables.len());
            for var in global.variables.iter() {
                if let Variable::Direct(dv) = var {
                    let var_name = bytes_to_str(dv.name);
                    imported.push(var_name);
                    if let Some(top_scope) = &ctx.top_level_scope {
                        if let Some(types) = top_scope.get(&atom(var_name)) {
                            scope.set(var_name, types.clone());
                        } else {
                            scope.set_empty(var_name);
                        }
                    } else {
                        scope.set_empty(var_name);
                    }
                }
            }
            let stmt_offset = stmt.span().start.offset;
            if apply_global_var_docblocks(stmt_offset, &imported, scope, ctx) {
                record_scope_snapshot(stmt_offset, scope);
            }
        }
        Statement::Return(ret) => {
            if let Some(val) = ret.value {
                process_assignment_expr(val, scope, ctx);
            }

            // A `return` leaves the body with the types it holds *here*.
            // For a closure walked to see what it writes to its `use (&$x)`
            // captures, that state is part of the exit state even though
            // the branch merge drops the branch it sits in.
            record_return_edge(ctx.flow, scope);
        }
        // An echoed expression narrows exactly the way a returned one
        // does: `echo $s ? strtoupper($s) : '';` proves `$s` a string
        // inside the arm that runs it.  Blade compiles every `{{ … }}`
        // to an `echo`, so a template's guards live here.
        Statement::Echo(echo) => {
            for value in echo.values.iter() {
                process_echoed_expression(value, scope, ctx);
            }
        }
        Statement::EchoTag(echo) => {
            for value in echo.values.iter() {
                process_echoed_expression(value, scope, ctx);
            }
        }
        _ => {}
    }
}

/// Apply one echoed expression's effects to the scope: the assignments it
/// makes, and the narrowing its short-circuit chains, ternaries and
/// `match (true)` arms prove for the code inside them.
fn process_echoed_expression<'b>(
    value: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    process_assignment_expr(value, scope, ctx);
}

/// Process an expression statement: handle assignments, assert narrowing,
/// pass-by-reference type inference, etc.
pub(crate) fn process_expression_statement<'b>(
    expr_stmt: &'b ExpressionStatement<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // `($a = expr);` is a parenthesized expression statement, written by
    // hand or produced by the Blade preprocessor for `@php($a = expr)`.
    // The statement offset stays on the outer expression so a preceding
    // `@var` docblock is still found, but everything that inspects the
    // expression's shape works on the inner one.
    let outer = expr_stmt.expression;
    let expr = crate::parser::unwrap_parens(outer);

    // Try inline `/** @var Type $x */` override first.
    // A `@var` block is authoritative over the assignment it annotates,
    // so that one write is skipped.  Only that one: everything else the
    // statement does (the writes and narrowing inside its expression,
    // assert narrowing, by-ref captures) still has to run, now against the
    // scope the docblock just established.  Returning outright here left
    // `takesString($m->virtual ? $m->virtual : $m->title)` under a
    // preceding `/** @var Model $m */` with nothing narrowed, so the truthy
    // arm read the property's declared nullable type.
    //
    // What the assignment target held before the docblock retyped it. A
    // `@var` above an assignment describes the variable *after* it runs,
    // so the right-hand side still reads the old value:
    // `/** @var Base $b */ $b = $b->inner;` resolves `$b->inner` against
    // whatever `$b` was on the way in, not against `Base`.
    let assigned_var = match expr {
        Expression::Assignment(assignment) => match assignment.lhs {
            Expression::Variable(Variable::Direct(dv)) => {
                let name = bytes_to_str(dv.name).to_string();
                let before = scope.get(&name).to_vec();
                Some((name, before))
            }
            _ => None,
        },
        _ => None,
    };

    // The target's entry as the right-hand side reads it, absent when the
    // statement is what first assigns it.
    let pre_override = assigned_var
        .as_ref()
        .and_then(|(name, _)| scope.locals.get(&atom(name)).cloned());

    let skip_assignment =
        match try_process_inline_var_override(expr, stmt_offset(outer), scope, ctx) {
            VarOverrideResult::NamedVar => {
                // Re-record the scope snapshot at this expression's offset
                // so that variable lookups within the same statement (e.g.
                // `$app` in `$client = $app->make(...)` where a preceding
                // `@var` block declared `$app`) see the updated types.
                // The snapshot recorded by `walk_body_for_diagnostics` at
                // the statement start was taken *before* the `@var`
                // override was applied.  The assignment target is put back
                // to its incoming type for that snapshot alone, so the
                // right-hand side is read the way it was written.
                match assigned_var {
                    Some((ref name, ref before)) if !before.is_empty() => {
                        let mut rhs_scope = scope.clone();
                        rhs_scope.set(name, before.clone());
                        record_scope_snapshot(stmt_offset(outer), &rhs_scope);
                    }
                    _ => record_scope_snapshot(stmt_offset(outer), scope),
                }
                true
            }
            // A `@var Type` (no variable name) was applied to the assignment
            // LHS.  The snapshot is deliberately *not* re-recorded: the LHS
            // variable must not be visible to lookups inside the RHS.
            VarOverrideResult::NoVar => true,
            VarOverrideResult::None => false,
        };

    if !skip_assignment {
        process_assignment_expr(expr, scope, ctx);
    } else {
        // The docblock stands in for the outermost write alone; whatever
        // the expression does on the way there still happens, and it reads
        // the target as it was before the write the docblock describes.
        let value = match expr {
            Expression::Assignment(assignment) => assignment.rhs,
            _ => expr,
        };
        match &assigned_var {
            Some((name, _)) => {
                let key = atom(name);
                let declared = scope.locals.get(&key).cloned();
                match &pre_override {
                    Some(types) => scope.locals.insert(key, types.clone()),
                    None => scope.locals.remove(&key),
                }
                process_expr(value, scope, ctx);
                match declared {
                    Some(types) => scope.locals.insert(key, types),
                    None => scope.locals.remove(&key),
                }
            }
            None => {
                process_expr(value, scope, ctx);
            }
        }
    }

    process_by_ref_closure_captures(expr, scope, ctx);

    // Sits between the passes that *read* the statement's expressions and
    // the passes that record what it *proves*: a proof below describes the
    // value a call handed back, so it must outlive the call's own
    // invalidation (`assertNotNull($holder->find('a'))` proves something
    // about the very call it makes).
    process_receiver_mutation(expr, scope, ctx);

    process_assert_narrowing(expr, scope, ctx);
}
