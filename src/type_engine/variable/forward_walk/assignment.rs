use super::*;

use mago_span::HasSpan;
use mago_syntax::cst::argument::Argument;

use crate::atom::{Atom, atom, bytes_to_str};
use crate::php_type::{LiteralValue, PhpType, TypeKind};
use crate::type_engine::types::narrowing;
use crate::types::{ClassInfo, ResolvedType};

use super::super::rhs_resolution::{
    ArithmeticOpKind, infer_addition_result_type, infer_arithmetic_result_type,
};

// ─── Statement processing ───────────────────────────────────────────────────

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
            record_exit_edge(exit_level(brk.level), true, scope);
        }
        Statement::Continue(cont) => {
            record_exit_edge(exit_level(cont.level), false, scope);
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

                // Record `&&` and `||` chain snapshots so that member
                // accesses after an instanceof/null guard see the
                // narrowed type.  E.g. `return $x instanceof Foo && $x->bar()`
                record_short_circuit_snapshots(val, scope, ctx);

                // Record narrowed snapshots inside match(true) arms
                // and ternary instanceof branches.
                if is_diagnostic_scope_active() {
                    record_match_ternary_snapshots(val, scope, ctx);
                }
            }

            // A `return` leaves the body with the types it holds *here*.
            // For a closure walked to see what it writes to its `use (&$x)`
            // captures, that state is part of the exit state even though
            // the branch merge drops the branch it sits in.
            record_return_edge(scope);
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
    record_short_circuit_snapshots(value, scope, ctx);
    if is_diagnostic_scope_active() {
        record_match_ternary_snapshots(value, scope, ctx);
    }
}

// ─── Expression statement handling ──────────────────────────────────────────

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
    // so that one pass is skipped.  Only that one: everything else the
    // statement carries — the ternary and short-circuit snapshots, assert
    // narrowing, by-ref captures — still has to run, now against the scope
    // the docblock just established.  Returning outright here left
    // `takesString($m->virtual ? $m->virtual : $m->title)` under a
    // preceding `/** @var Model $m */` with no branch snapshots at all, so
    // the truthy arm read the property's declared nullable type.
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

    // Record intermediate scope snapshots within `&&` and `||` chains
    // so that member accesses after an instanceof/null guard see the
    // narrowed type.  E.g. `$x instanceof Foo && $x->bar()` as an
    // expression statement.
    record_short_circuit_snapshots(expr, scope, ctx);

    // Record narrowed snapshots inside match(true) arms and ternary
    // instanceof branches within this expression.
    if is_diagnostic_scope_active() {
        record_match_ternary_snapshots(expr, scope, ctx);
    }

    if !skip_assignment {
        process_assignment_expr(expr, scope, ctx);
    }

    process_by_ref_closure_captures(expr, scope, ctx);

    process_pass_by_ref(expr, scope, ctx);

    // Sits between the passes that *read* the statement's expressions and
    // the passes that record what it *proves*.  The reads above see the
    // state the call itself saw; a proof below describes the value the
    // call handed back, so it must outlive the call's own invalidation
    // (`assertNotNull($holder->find('a'))` proves something about the very
    // call it makes).
    process_receiver_mutation(expr, scope, ctx);

    process_assert_narrowing(expr, scope, ctx);

    process_self_out_narrowing(expr, scope, ctx);

    // Process increment/decrement: $a++, ++$a, $a--, --$a.
    process_increment_decrement(expr, scope, ctx);
}

/// Identifies which callee parameter a call argument fills.
///
/// Positional arguments bind by their ordinal position; named arguments
/// (`foo(callback: ...)`) bind by the declared parameter name and may
/// appear out of their natural position, so they must be resolved by
/// name rather than by their slot in the argument list.
pub(crate) enum ArgSelector {
    Position(usize),
    Name(String),
}

/// Flatten a statement iterator, descending into `namespace Foo;` and
/// `namespace Foo { ... }` blocks so that function and class
/// declarations inside a namespace are visited alongside top-level
/// declarations. Nearly all real-world PHP declares its symbols inside
/// a namespace, so a search that only inspects `program.statements`
/// would never find the callee.
pub(crate) fn flatten_namespaced_statements<'b>(
    statements: impl Iterator<Item = &'b Statement<'b>>,
    out: &mut Vec<&'b Statement<'b>>,
) {
    for stmt in statements {
        if let Statement::Namespace(ns) = stmt {
            flatten_namespaced_statements(ns.statements().iter(), out);
        } else {
            out.push(stmt);
        }
    }
}

/// Process increment/decrement expressions (`$a++`, `++$a`, `$a--`, `--$a`).
///
/// For numeric types (int, float), the base type is preserved.
/// Numeric literals and refined numeric types are widened because the
/// operation changes the value and may invalidate the refinement.
/// For numeric strings, the result becomes `int|float`.
/// For general strings, PHP increments alphabetically (stays string), while
/// decrementing a known non-numeric string is a no-op and stays exact.
/// Incrementing `null` produces `1`, while decrementing it leaves `null`
/// unchanged.
#[derive(Clone, Copy)]
enum IncrementDecrementKind {
    Increment,
    Decrement,
}

fn type_after_increment_decrement(ty: &PhpType, operation: IncrementDecrementKind) -> PhpType {
    match ty.kind() {
        TypeKind::Union(members) => {
            let mut transformed = Vec::with_capacity(members.len());
            for member in members {
                let member = type_after_increment_decrement(member, operation);
                for alternative in member.union_members() {
                    if !transformed.iter().any(|existing| existing == alternative) {
                        transformed.push(alternative.clone());
                    }
                }
            }
            if transformed.is_empty() {
                ty.clone()
            } else {
                PhpType::union(transformed)
            }
        }
        TypeKind::Nullable(inner) => {
            let inner = type_after_increment_decrement(inner, operation);
            match operation {
                IncrementDecrementKind::Increment => {
                    let mut alternatives = vec![PhpType::int()];
                    for alternative in inner.union_members() {
                        if !alternatives.iter().any(|existing| existing == alternative) {
                            alternatives.push(alternative.clone());
                        }
                    }
                    if alternatives.len() == 1 {
                        alternatives.into_iter().next().unwrap()
                    } else {
                        PhpType::union(alternatives)
                    }
                }
                IncrementDecrementKind::Decrement => {
                    if matches!(inner.kind(), TypeKind::Union(_)) {
                        let mut alternatives: Vec<PhpType> =
                            inner.union_members().into_iter().cloned().collect();
                        alternatives.push(PhpType::null());
                        PhpType::union(alternatives)
                    } else {
                        PhpType::nullable(inner)
                    }
                }
            }
        }
        _ if ty.is_null() => match operation {
            IncrementDecrementKind::Increment => PhpType::int(),
            IncrementDecrementKind::Decrement => PhpType::null(),
        },
        _ if ty.is_named_ci("numeric")
            || ty.is_named("number")
            || ty.is_named_ci("numeric-string")
            || ty.is_subtype_of(&PhpType::named(atom("numeric-string"))) =>
        {
            PhpType::union(vec![PhpType::int(), PhpType::float()])
        }
        _ if ty.is_int_subtype() => PhpType::int(),
        _ if ty.is_float_subtype() => PhpType::float(),
        TypeKind::Literal(value) if matches!(&**value, LiteralValue::String(_)) => {
            match operation {
                IncrementDecrementKind::Increment => PhpType::string(),
                IncrementDecrementKind::Decrement => ty.clone(),
            }
        }
        // A broad string may be numeric at runtime. PHP converts numeric
        // strings to int or float for both operators; non-numeric strings
        // remain strings (apart from the deprecated increment behaviour).
        _ if ty.is_string_subtype() => {
            PhpType::union(vec![PhpType::int(), PhpType::float(), PhpType::string()])
        }
        _ => ty.clone(),
    }
}

pub(crate) fn process_increment_decrement<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    _ctx: &ForwardWalkCtx<'_>,
) {
    use mago_syntax::cst::unary::{UnaryPostfixOperator, UnaryPrefixOperator};

    let (var_expr, operation) = match expr {
        Expression::UnaryPostfix(postfix) => match &postfix.operator {
            UnaryPostfixOperator::PostIncrement(_) => {
                (postfix.operand, IncrementDecrementKind::Increment)
            }
            UnaryPostfixOperator::PostDecrement(_) => {
                (postfix.operand, IncrementDecrementKind::Decrement)
            }
        },
        Expression::UnaryPrefix(prefix) => match &prefix.operator {
            UnaryPrefixOperator::PreIncrement(_) => {
                (prefix.operand, IncrementDecrementKind::Increment)
            }
            UnaryPrefixOperator::PreDecrement(_) => {
                (prefix.operand, IncrementDecrementKind::Decrement)
            }
            _ => return,
        },
        _ => return,
    };

    let var_name = match var_expr {
        Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
        _ => return,
    };

    let existing = scope.get(&var_name).to_vec();
    if existing.is_empty() {
        return;
    }

    let current_type = ResolvedType::types_joined(&existing);
    let transformed = type_after_increment_decrement(&current_type, operation);
    if transformed != current_type {
        scope.set(&var_name, vec![ResolvedType::from_type_string(transformed)]);
    }
}

/// Get the byte offset of an expression (used for cursor comparisons).
pub(crate) fn stmt_offset(expr: &Expression<'_>) -> u32 {
    expr.span().start.offset
}

/// Result of [`try_process_inline_var_override`].
pub(crate) enum VarOverrideResult {
    /// No `@var` docblock found.
    None,
    /// A `@var Type $varName` block (with explicit variable name) was
    /// applied.  The caller should re-record the scope snapshot so that
    /// lookups within the same statement see the updated types.
    NamedVar,
    /// A `@var Type` block (without variable name) was applied to the
    /// assignment LHS.  The caller must NOT re-record the snapshot
    /// because the LHS variable should not be visible in the RHS.
    NoVar,
}

/// Extract the native type of an RHS expression using the current scope.
///
/// Used by [`try_process_inline_var_override`] to determine whether a
/// `@var` override should be blocked by a scalar native type.
///
/// This delegates to [`super::super::resolution::extract_native_type_from_rhs`]
/// via a `VarResolutionCtx` that has scope-based variable resolution.
/// That function already handles method calls, function calls, static
/// calls, casts, literals, and other patterns — including extracting
/// scalar return types from method signatures.
pub(crate) fn resolve_rhs_native_type(
    rhs: &Expression<'_>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<PhpType> {
    let scope_resolver = scope.snapshot_resolver();
    let var_ctx =
        ctx.var_ctx_for_with_scope("$__rhs_check", 0, &scope_resolver, Some(scope.proofs()));
    super::super::resolution::extract_native_type_from_rhs(rhs, &var_ctx)
}

/// Resolve a [`PhpType`] to a complete `Vec<ResolvedType>` with
/// `class_info` populated when possible.  Falls back to a
/// type-string-only entry for scalars and unresolvable types.
///
/// The type comes from a docblock the walker just read out of the source,
/// so its class names are still spelled the way the author wrote them.
/// They are qualified against the enclosing namespace first, matching how
/// PHP reads the same spelling and how the parser already resolved the
/// `@param`/`@return` tags these types get compared against.
pub(crate) fn resolve_type_to_resolved_types(
    php_type: &PhpType,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<ResolvedType> {
    let php_type = crate::util::resolve_source_php_type_names(
        php_type,
        ctx.current_class.file_namespace.as_deref(),
        ctx.class_loader,
    );
    ctx.resolved_types_for(php_type)
}

/// Process assignment expressions, updating the scope.
pub(crate) fn process_assignment_expr<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // `($a = expr);` is a parenthesized assignment statement — written by
    // hand, or produced by the Blade preprocessor for `@php($a = expr)`.
    if let Expression::Assignment(assignment) = crate::parser::unwrap_parens(expr) {
        // An assignment buried in the value runs before the target here is
        // written, and the rest of the value reads what it wrote:
        // `$ok = ($x = $map[$key])->truthy();`.  A right-hand side that
        // *is* an assignment is left to the chain handling below, which
        // knows shapes (destructuring, indexed writes) this does not.
        if !matches!(
            crate::parser::unwrap_parens(assignment.rhs),
            Expression::Assignment(_)
        ) {
            process_nested_assignments(assignment.rhs, scope, ctx);
        }

        if !assignment.operator.is_assign() {
            // Compound assignment: $x op= expr.
            // The type depends on the operator.
            process_compound_assignment(assignment, scope, ctx);
            return;
        }

        // Chain assignments: `$a = $b = expr` — the RHS is itself an
        // assignment expression.  Process it first so that the inner
        // variable (`$b`) gets its type before we resolve the outer one.
        if matches!(assignment.rhs, Expression::Assignment(_)) {
            process_assignment_expr(assignment.rhs, scope, ctx);
        }

        // Array destructuring: `[$a, $b] = …` / `list($a, $b) = …`
        if matches!(assignment.lhs, Expression::Array(_) | Expression::List(_)) {
            process_destructuring_assignment(assignment, scope, ctx);
            return;
        }

        // Array key assignment: `$var['key'] = expr;`
        if let Expression::ArrayAccess(array_access) = assignment.lhs {
            process_array_key_assignment(array_access, assignment, scope, ctx);
            return;
        }

        // Array push: `$var[] = expr;` and `$var['a'][$i][] = expr;`
        if let Expression::ArrayAppend(array_append) = assignment.lhs {
            process_array_append(array_append, assignment, scope, ctx);
            return;
        }

        // Property assignment: `$var->prop = expr;` (and null-safe
        // `$var?->prop = expr;`).  Record the assigned type under the
        // property-path key (e.g. `$settings->cache`) so that a later
        // read of that path resolves through the assignment rather than
        // the declaring class's declared property hints.  This is what
        // lets nested object property chains resolve, most notably on
        // `stdClass` which has no declared properties:
        //
        //     $s = new stdClass();
        //     $s->cache = new stdClass();
        //     $s->cache->ttl = 1;   // `$s->cache` now resolves to stdClass
        //
        // The key contains `->`, so it is treated as a synthetic
        // narrowing entry and stripped at loop boundaries — matching the
        // conservative behaviour of condition-based property narrowing.
        // A static property (`self::$repo = …`) is recorded the same way:
        // it is a member path with a declared type, and the lazy-init
        // idiom writes it in exactly the shape this branch handles.
        if matches!(
            assignment.lhs,
            Expression::Access(
                Access::Property(_) | Access::NullSafeProperty(_) | Access::StaticProperty(_)
            )
        ) {
            // Skip when the cursor is inside the RHS so that lookups
            // within the RHS see the pre-assignment state.
            let rhs_span = assignment.rhs.span();
            if ctx.cursor_offset >= rhs_span.start.offset
                && ctx.cursor_offset <= rhs_span.end.offset
            {
                return;
            }
            if let Some(key) = narrowing::expr_to_subject_key(assignment.lhs) {
                // A write that dispatches to `__set` is opaque: the
                // magic setter may transform, reroute, or drop the
                // value, and a later read goes through `__get`, which
                // decides what comes back.  Drop whatever was known
                // about the path instead of recording the written type.
                if property_write_dispatches_to_magic_set(assignment.lhs, scope, ctx) {
                    scope.remove(&key);
                    scope.invalidate_dependent_keys(&key);
                    return;
                }
                let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
                if rhs_types.is_empty() {
                    // The right-hand side did not resolve. Unlike a plain
                    // variable (`set_unknown`), a property's correct
                    // fallback is its *declared* type, not "unknown", so
                    // drop the key entirely rather than blanking it — a
                    // blank entry would leave the pre-write `instanceof`
                    // narrowing looking current instead of falling
                    // through to the declared type.
                    scope.remove(&key);
                } else {
                    scope.invalidate_proofs(&key);
                    scope.set(&key, rhs_types);
                }
            }
            return;
        }

        // Simple variable assignment: `$var = expr;`
        let lhs_name = match assignment.lhs {
            Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
            _ => return,
        };

        // When the cursor is inside the RHS of this assignment, skip
        // storing the new type so that variable lookups within the RHS
        // see the pre-assignment type.  E.g. in `$request = new Bar(
        // name: $request->)`, the cursor on `$request->` should see
        // the old `Foo` type, not the new `Bar` type.
        let rhs_span = assignment.rhs.span();
        let cursor_in_rhs =
            ctx.cursor_offset >= rhs_span.start.offset && ctx.cursor_offset <= rhs_span.end.offset;
        if cursor_in_rhs {
            return;
        }

        let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
        // Reassigning the variable replaces its object identity, so any
        // property/array-access key rooted at it (seeded by an earlier
        // assignment or condition narrowing) is now stale.  Drop them
        // after resolving the RHS, so `$x = $x->foo` still reads the old
        // key while resolving.
        scope.invalidate_dependent_keys(&lhs_name);
        scope.invalidate_proofs(&lhs_name);
        if !rhs_types.is_empty() {
            scope.set(&lhs_name, rhs_types);
        } else if !scope.get(&lhs_name).is_empty()
            && rhs_fails_on_resolved_receiver(assignment.rhs, scope)
        {
            // The right-hand side did not resolve, so nothing is known
            // about what the variable now holds — but the value it held
            // before the assignment is gone either way. Keeping the old
            // type is what made `$acc = $acc->merge($x)` (with `$x`
            // unresolved) report a member access on the `null` that
            // `$acc` was initialised with.
            //
            // Flagging it as unresolved is what keeps the loss local: a
            // join with a path that still knows the type takes that
            // path's answer, so `$acc = $acc->missing()` inside a loop
            // reports the missing member rather than turning `$acc`
            // unknown for the rest of the body.
            scope.set_unknown(&lhs_name);
        } else {
            // Nothing was known about the variable beforehand, or the
            // failure came from somewhere the answer was already "could
            // be anything". Neither is a type this walk lost, so the
            // entry is the plain unknown a join treats as top.
            scope.set_untyped(&lhs_name);
        }
        // `$isHtml = $raw instanceof HtmlString` makes `$isHtml` stand
        // for the check, so testing it later narrows `$raw`.
        record_assertion_variable(&lhs_name, assignment.rhs, scope);
        // `$period = $agreement?->latestPeriod()` makes `$period`'s null
        // stand for `$agreement`'s, so ruling out one rules out the other.
        record_nullsafe_origin(&lhs_name, assignment.rhs, scope);
        // `$ok = preg_match('/…/', $s, $m)` makes `$ok` stand for the
        // match's outcome, so testing it later narrows `$m`.
        record_preg_outcome(&lhs_name, assignment.rhs, scope, ctx);
    } else {
        // The expression assigns nothing at its root but may still assign
        // inside itself: `return ($x = $map[$key])->truthy();`.
        process_nested_assignments(expr, scope, ctx);
    }
}

/// Whether a right-hand side that resolved to nothing failed on a member
/// of a class the walker did resolve.
///
/// This is the one shape where the failure is the walker's own and it
/// says so out loud: the receiver is a known class, the member is not on
/// it, and `unknown_member` is reported on this very line. Anything else
/// — a value that had no type to start with, a chain that already lost
/// the thread further up — is a failure inherited from somewhere the
/// answer was already "could be anything", and passing it on is all the
/// assignment does.
///
/// The receiver is read straight out of the scope rather than resolved,
/// so this costs nothing on a path that is already a dead end. It also
/// keeps the answer to the accumulator idiom the flag exists for —
/// `$acc = $acc->…`, whose receiver is the variable being written — and
/// leaves a longer chain alone, which is the right way round: the deeper
/// the chain, the likelier it is that what failed was some link of it
/// rather than the member on the end.
fn rhs_fails_on_resolved_receiver(rhs: &Expression<'_>, scope: &ScopeState) -> bool {
    let receiver = match rhs {
        Expression::Call(Call::Method(call)) => call.object,
        Expression::Call(Call::NullSafeMethod(call)) => call.object,
        Expression::Access(Access::Property(access)) => access.object,
        Expression::Access(Access::NullSafeProperty(access)) => access.object,
        _ => return false,
    };
    let Expression::Variable(Variable::Direct(var)) = receiver else {
        return false;
    };
    scope
        .get(bytes_to_str(var.name))
        .iter()
        .any(|rt| rt.class_info.is_some())
}

/// Whether `$obj->prop = …` writes through the subject class's `__set`
/// magic method instead of storing the value in a real property.
///
/// Returns `false` whenever no subject class resolves: without a class
/// there is no evidence of a magic setter, and the write is recorded as
/// before.
fn property_write_dispatches_to_magic_set(
    lhs: &Expression<'_>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let (object, selector) = match lhs {
        Expression::Access(Access::Property(pa)) => (pa.object, &pa.property),
        Expression::Access(Access::NullSafeProperty(pa)) => (pa.object, &pa.property),
        _ => return false,
    };
    let ClassLikeMemberSelector::Identifier(ident) = selector else {
        return false;
    };
    let prop_name = bytes_to_str(ident.value);
    let is_magic = |cls: &ClassInfo| {
        crate::virtual_members::property_write_is_magic(
            cls,
            prop_name,
            ctx.class_loader,
            ctx.resolved_class_cache,
        )
    };

    // `$this` and plain variables are answered from the walker's own
    // state, so the common write shapes cost no resolution.  A union
    // subject is magic as soon as one member routes the write through
    // `__set`: the recorded type would have no authority over what that
    // member's `__get` returns.
    let object = crate::parser::unwrap_parens(object);
    if let Expression::Variable(Variable::Direct(dv)) = object {
        let var_name = bytes_to_str(dv.name);
        if var_name == "$this" {
            return is_magic(ctx.current_class);
        }
        return scope
            .get(var_name)
            .iter()
            .filter_map(|rt| rt.class_info.as_deref())
            .any(is_magic);
    }
    resolve_rhs_with_scope(object, scope, ctx)
        .iter()
        .filter_map(|rt| rt.class_info.as_deref())
        .any(is_magic)
}

/// What a `??=` leaves behind, given what its target and its fallback
/// resolve to.
///
/// `??=` keeps the target when it is not null and assigns the fallback
/// otherwise, so the value is the target's non-null half unioned with the
/// fallback. The resolved types are combined as they are, rather than
/// joined into one union *type string*, so the `class_info` already
/// attached to each operand survives: a rebuilt string carries none, and
/// a member access on the result would have nothing to resolve against.
///
/// Where both sides name the same type (commonly the target's declared
/// element type, which resolved no class, alongside an argument that did)
/// the class-backed entry speaks for the pair.
fn coalesce_assign_value(
    lhs_types: Vec<ResolvedType>,
    rhs_types: Vec<ResolvedType>,
) -> Vec<ResolvedType> {
    let mut combined: Vec<ResolvedType> = lhs_types
        .into_iter()
        .filter(|rt| !rt.type_string.is_null())
        .map(|mut rt| {
            if let Some(non_null) = rt.type_string.non_null_type() {
                rt.type_string = non_null;
            }
            rt
        })
        .collect();
    ResolvedType::extend_unique(&mut combined, rhs_types);
    let class_backed: Vec<PhpType> = combined
        .iter()
        .filter(|rt| rt.class_info.is_some())
        .map(|rt| rt.type_string.clone())
        .collect();
    combined.retain(|rt| rt.class_info.is_some() || !class_backed.contains(&rt.type_string));
    combined
}

/// Process compound assignment operators (`+=`, `-=`, `/=`, `*=`, etc.).
///
/// The result type depends on the operator kind:
/// - `.=` → string
/// - `%=` → int
/// - `<<=`, `>>=`, `&=`, `|=`, `^=` → int
/// - `+=`, `-=`, `*=`, `/=`, `**=` → int|float
/// - `??=` → union of LHS non-null type and RHS type
pub(crate) fn process_compound_assignment<'b>(
    assignment: &'b Assignment<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    use mago_syntax::cst::assignment::AssignmentOperator;

    let var_name = match assignment.lhs {
        Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
        // `$this->regexp ??= $this->generate();` leaves the property
        // non-null just as surely as the same operator leaves a local
        // non-null, and the scope names a member path the same way it
        // names a local.  Only `??=` is routed this way: the arithmetic
        // operators below read the target's current type, which a member
        // path the scope has never seen does not have.
        _ if matches!(assignment.operator, AssignmentOperator::Coalesce(_)) => {
            match crate::type_engine::types::narrowing::expr_to_subject_key(assignment.lhs) {
                Some(key) => key,
                None => return,
            }
        }
        _ => return,
    };
    if matches!(assignment.operator, AssignmentOperator::Coalesce(_)) {
        // A member path the scope has not narrowed yet still has a
        // declared type, and `??=` only keeps that type's non-null half —
        // reading nothing there would drop every alternative the
        // declaration allows besides the fallback's.
        let lhs_types = match scope.get(&var_name) {
            [] => resolve_rhs_with_scope(assignment.lhs, scope, ctx),
            existing => existing.to_vec(),
        };
        let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
        let combined = coalesce_assign_value(lhs_types, rhs_types);
        if !combined.is_empty() {
            scope.set(&var_name, combined);
        } else if !scope.contains(&var_name) {
            scope.set_empty(&var_name);
        }
        return;
    }

    // `??=` was handled above and plain `=` never reaches here.
    let Some(result_type) = compound_assignment_result(
        &assignment.operator,
        || scope.get(&var_name).to_vec(),
        || resolve_rhs_with_scope(assignment.rhs, scope, ctx),
    ) else {
        return;
    };

    scope.set(&var_name, vec![ResolvedType::from_type_string(result_type)]);
}

/// The type a compound assignment (`.=`, `+=`, `<<=`, …) leaves in its
/// target, given the types the target held and the types the right-hand
/// side produces.
///
/// The operand closures are only called for the operators that need them,
/// so a `.=` never pays for resolving its right-hand side.
///
/// `??=` and plain `=` return `None`: neither is decided by the operator
/// alone, so each caller settles them for itself.
fn compound_assignment_result(
    operator: &AssignmentOperator,
    lhs_types: impl FnOnce() -> Vec<ResolvedType>,
    rhs_types: impl FnOnce() -> Vec<ResolvedType>,
) -> Option<PhpType> {
    match operator {
        AssignmentOperator::Concat(_) => Some(PhpType::string()),
        AssignmentOperator::Modulo(_)
        | AssignmentOperator::LeftShift(_)
        | AssignmentOperator::RightShift(_)
        | AssignmentOperator::BitwiseAnd(_)
        | AssignmentOperator::BitwiseOr(_)
        | AssignmentOperator::BitwiseXor(_) => Some(PhpType::int()),
        AssignmentOperator::Addition(_) => {
            Some(infer_addition_result_type(&lhs_types(), &rhs_types()))
        }
        AssignmentOperator::Subtraction(_)
        | AssignmentOperator::Multiplication(_)
        | AssignmentOperator::Division(_)
        | AssignmentOperator::Exponentiation(_) => {
            let op_kind = match operator {
                AssignmentOperator::Division(_) => ArithmeticOpKind::Division,
                AssignmentOperator::Exponentiation(_) => ArithmeticOpKind::Exponentiation,
                _ => ArithmeticOpKind::Other,
            };
            Some(infer_arithmetic_result_type(
                &lhs_types(),
                &rhs_types(),
                op_kind,
            ))
        }
        AssignmentOperator::Coalesce(_) | AssignmentOperator::Assign(_) => None,
    }
}

/// Rewrite a `void` call result to `null` before it is stored as a
/// variable's type.
///
/// PHP has no `void` value: a `void`-declared function implicitly returns
/// `null`, so a variable assigned from such a call holds `null`, not
/// `void`. This is scoped to the assignment funnel rather than the RHS
/// pipeline itself, so a call's resolved type still reads as `void`
/// wherever that distinction matters outside of a variable holding it
/// (e.g. the argument-type-mismatch diagnostic reports a `void` argument
/// as always wrong, even against a nullable parameter).
fn normalize_void_assignment(mut resolved: Vec<ResolvedType>) -> Vec<ResolvedType> {
    for rt in &mut resolved {
        if rt.type_string.is_void() {
            rt.type_string = PhpType::null();
            rt.class_info = None;
        }
    }
    resolved
}

/// Resolve the type of an RHS expression using the current scope.
///
/// This is the key integration point: instead of calling
/// `resolve_variable_types` (which would recurse), we build a
/// `VarResolutionCtx` that already has the answer for any variable
/// references in the RHS — the forward walker has already resolved
/// them.
///
/// We delegate to `resolve_rhs_expression` with a `VarResolutionCtx`
/// whose `scope_var_resolver` reads directly from the forward walker's
/// in-progress `ScopeState`.  For bare variable references in the RHS,
/// we intercept them and return the scope-based result directly.
pub(crate) fn resolve_rhs_with_scope<'b>(
    rhs: &'b Expression<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<ResolvedType> {
    // Chain assignment: `$a = $b = expr` — the value of an assignment
    // expression is the value of its RHS.  Recurse into the inner RHS
    // so that `$a` resolves to the same type as `$b`.
    if let Expression::Assignment(assignment) = rhs
        && assignment.operator.is_assign()
    {
        return resolve_rhs_with_scope(assignment.rhs, scope, ctx);
    }

    // Compound assignment as RHS: `$a = ($x /= 2)` — the value of the
    // compound assignment is the result after the operation.  Infer the
    // type from the operator kind.
    if let Expression::Assignment(assignment) = rhs
        && !assignment.operator.is_assign()
    {
        use mago_syntax::cst::assignment::AssignmentOperator;
        // `$x = $cache[$k] ??= expensive();` — the value is whichever side
        // survives: the target's non-null half, or the fallback that
        // replaced it.
        if matches!(assignment.operator, AssignmentOperator::Coalesce(_)) {
            let lhs_types = resolve_rhs_with_scope(assignment.lhs, scope, ctx);
            let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
            let combined = coalesce_assign_value(lhs_types, rhs_types);
            if combined.is_empty() {
                return vec![ResolvedType::from_type_string(PhpType::mixed())];
            }
            return combined;
        }
        let result_type = compound_assignment_result(
            &assignment.operator,
            || match assignment.lhs {
                Expression::Variable(Variable::Direct(dv)) => {
                    scope.get(bytes_to_str(dv.name)).to_vec()
                }
                _ => Vec::new(),
            },
            || resolve_rhs_with_scope(assignment.rhs, scope, ctx),
        );
        if let Some(ty) = result_type {
            return vec![ResolvedType::from_type_string(ty)];
        }
    }

    // For bare variable references, read directly from scope.
    // This is the O(1) path that replaces the recursive backward scan.
    if let Expression::Variable(Variable::Direct(dv)) = rhs {
        let var_name = bytes_to_str(dv.name).to_string();
        let from_scope = scope.get(&var_name);
        if !from_scope.is_empty() {
            return from_scope.to_vec();
        }
        // Variable not in scope — fall through to rhs_resolution which
        // handles some special patterns.
    }

    // ── Foo::class → class-string<Foo> ──────────────────────────
    // `Foo::class` is parsed as `Access::ClassConstant` with the
    // identifier `class`.  resolve_rhs_expression doesn't return a
    // useful type for this (it looks for a constant named "class"
    // on the class and finds nothing).  Handle it here so that
    // subsequent `new $var` can resolve the class-string.
    if let Expression::Access(Access::ClassConstant(cca)) = rhs
        && let ClassLikeConstantSelector::Identifier(ident) = &cca.constant
        && ident.value == b"class"
    {
        let class_name = match cca.class {
            Expression::Identifier(id) => Some(bytes_to_str(id.value()).to_string()),
            Expression::Self_(_) | Expression::Static(_) => {
                if !ctx.current_class.name.is_empty() {
                    Some(ctx.current_class.name.to_string())
                } else {
                    None
                }
            }
            Expression::Parent(_) => ctx.current_class.parent_class.map(|a| a.to_string()),
            _ => None,
        };
        if let Some(name) = class_name {
            let resolved_name = name.strip_prefix('\\').unwrap_or(&name);
            // Resolve the class so we can store a proper ResolvedType
            // with class_info.  This allows `new $var` to work.
            let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
                &PhpType::named(atom(resolved_name)),
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            );
            // The identifier is spelled as the source writes it, which for a
            // name reached through a namespace import (`Support\Pen` behind
            // `use App\Support;`) is neither the FQCN nor resolvable once the
            // class-string is read somewhere else.  Prefer the resolved class.
            let class_string_type = PhpType::class_string(Some(match classes.first() {
                Some(cls) => PhpType::named(cls.fqn()),
                None => PhpType::named(atom(resolved_name)),
            }));
            if !classes.is_empty() {
                return ResolvedType::from_classes_with_hint(classes, class_string_type);
            }
            // Even if we can't resolve the class, return a type-string-only result
            // so the variable is non-empty in scope.
            return vec![ResolvedType::from_type_string(class_string_type)];
        }
    }

    // ── Fast paths for expressions whose type is known structurally ──
    // These avoid the full resolve_rhs_expression round-trip for
    // common patterns where the result type depends only on the
    // expression kind, not on the operand types.

    // Type casts (`(int) $x`), `!`, and `~`.  `-`/`+` are left to the
    // unified resolver below, which preserves signed numeric literals and
    // falls back to `int|float` for non-literal operands.
    if let Expression::UnaryPrefix(prefix) = rhs
        && let Some(ty) =
            super::super::rhs_resolution::unary_prefix_result_type(&prefix.operator, || {
                resolve_rhs_with_scope(prefix.operand, scope, ctx)
            })
    {
        return vec![ResolvedType::from_type_string(ty)];
    }

    // For all other expressions, delegate to the existing RHS resolver
    // with a scope-based variable resolver injected.  When
    // `resolve_rhs_expression` (or its sub-functions like
    // `resolve_rhs_method_call_inner`, `resolve_rhs_property_access`)
    // need to resolve a variable's type, they call `resolve_var_types`
    // which checks `scope_var_resolver` first.  This reads directly
    // from the forward walker's in-progress `ScopeState`, bypassing
    // `resolve_variable_types` entirely.
    let rhs_offset = rhs.span().start.offset;
    let dummy_var = "$__rhs";
    let scope_locals = &scope.locals;
    let scope_resolver = |var_name: &str| -> Vec<ResolvedType> {
        scope_locals
            .get(&atom(var_name))
            .cloned()
            .unwrap_or_default()
    };
    let var_ctx =
        ctx.var_ctx_for_with_scope(dummy_var, rhs_offset, &scope_resolver, Some(scope.proofs()));

    let result = super::super::rhs_resolution::resolve_rhs_expression(rhs, &var_ctx);
    if !result.is_empty() {
        return normalize_void_assignment(result);
    }

    // ── Structural fallbacks ────────────────────────────────────
    // When resolve_rhs_expression returns empty, infer the type
    // purely from the expression structure.  These only fire as a
    // last resort so they never override a more precise result.

    // Unwrap parenthesized expressions for structural inference.
    let rhs = crate::parser::unwrap_parens(rhs);

    // Composite strings are not scalar literals and may not be handled by the
    // canonical literal resolver. Exact scalar literals have no fallback here:
    // reintroducing broad int/string/float types would silently undo its
    // precision whenever resolution regressed or hit a recursion guard.
    if matches!(rhs, Expression::CompositeString(_)) {
        return vec![ResolvedType::from_type_string(PhpType::string())];
    }

    // ── Subject pipeline fallback ───────────────────────────────
    // When resolve_rhs_expression and the structural fallbacks both
    // return empty, try the full subject resolution pipeline
    // (resolve_target_classes).  This handles method calls and
    // static calls that resolve_rhs_expression cannot resolve
    // because the receiver or intermediate types are only reachable
    // through the subject pipeline's broader strategies (e.g.
    // docblock @return types, merged inheritance, virtual members).
    //
    // Property access (Expression::Access) is intentionally excluded
    // because resolve_target_classes resolves the *subject* (what
    // you'd complete after `->`) rather than the property's value
    // type.  For Eloquent relations like `$this->model->orderProducts`,
    // the subject pipeline returns the element type instead of the
    // collection, which breaks foreach value binding.  Property
    // access RHS resolution is handled by resolve_rhs_expression's
    // own property resolution path.
    if matches!(rhs, Expression::Call(_) | Expression::Instantiation(_)) {
        let rhs_span = rhs.span();
        let rhs_start = rhs_span.start.offset as usize;
        let rhs_end = rhs_span.end.offset as usize;
        if let Some(rhs_text) = ctx.content.get(rhs_start..rhs_end) {
            let rhs_text = rhs_text.trim();
            if !rhs_text.is_empty() {
                let subject_result = resolve_rhs_via_subject(rhs_text, scope, ctx);
                if !subject_result.is_empty() {
                    return subject_result;
                }
            }
        }
    }

    result
}

/// Resolve an RHS expression through the full subject pipeline.
///
/// This is a last-resort fallback for expressions that
/// `resolve_rhs_expression` can't handle.  It extracts the
/// expression text and passes it to `resolve_target_classes`, which
/// goes through SubjectExpr parsing, property/method chain
/// resolution, and the full type resolution infrastructure.
///
/// Only called for method calls, property access, static calls, and
/// instantiation — expression kinds that typically produce
/// object-typed results resolvable through the subject pipeline.
pub(crate) fn resolve_rhs_via_subject(
    rhs_text: &str,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<ResolvedType> {
    let scope_resolver = scope.snapshot_resolver();
    let var_ctx =
        ctx.var_ctx_for_with_scope("$__rhs_subject", 0, &scope_resolver, Some(scope.proofs()));
    let rctx = var_ctx.as_resolution_ctx();

    // Determine the access kind from the expression text.
    let access_kind = if rhs_text.contains("::") {
        crate::types::AccessKind::DoubleColon
    } else {
        crate::types::AccessKind::Arrow
    };

    crate::type_engine::resolver::resolve_target_classes(rhs_text, access_kind, &rctx)
}

/// Seed PHP superglobals (`$_SERVER`, `$_GET`, `$_POST`, etc.) into the
/// scope as `array` so that accesses on them resolve correctly.
/// PHP makes these available in every scope without
/// an explicit `global` declaration.
pub(crate) fn seed_superglobals(scope: &mut ScopeState) {
    let array_type = vec![ResolvedType::from_type_string(PhpType::named(atom(
        "array",
    )))];
    for name in [
        "$_SERVER",
        "$_GET",
        "$_POST",
        "$_COOKIE",
        "$_REQUEST",
        "$_FILES",
        "$_ENV",
        "$_SESSION",
        "$GLOBALS",
    ] {
        scope.set(name, array_type.clone());
    }
}

/// The guard functions whose first argument is a condition the call
/// leaves proven, paired with that parameter's name and with whether the
/// condition still holds once the call returns.
///
/// `assert()` proves its argument true outright.  Laravel's
/// `abort_unless()` / `throw_unless()` prove it true by bailing out when
/// it is false, and `abort_if()` / `throw_if()` prove its *negation* the
/// same way.  Neither the framework nor any stub annotates these four
/// with `@phpstan-assert`, so the name is the only signal there is, and
/// this is the set the Laravel PHPStan extensions special-case too.
const CONDITION_GUARD_FUNCTIONS: [(&str, &str, bool); 5] = [
    ("assert", "assertion", true),
    ("abort_if", "boolean", false),
    ("abort_unless", "boolean", true),
    ("throw_if", "condition", false),
    ("throw_unless", "condition", true),
];

/// The condition a [guard call](CONDITION_GUARD_FUNCTIONS) proves and
/// whether it holds after the call, or `None` when `expr` is not one.
///
/// Matches every spelling PHP accepts: unqualified, fully-qualified
/// (`\assert`), and any letter case.
fn guard_call_condition<'b>(expr: &'b Expression<'b>) -> Option<(&'b Expression<'b>, bool)> {
    let Expression::Call(Call::Function(fc)) = crate::parser::unwrap_parens(expr) else {
        return None;
    };
    let Expression::Identifier(ident) = fc.function else {
        return None;
    };
    let raw = bytes_to_str(ident.value());
    let called = raw.strip_prefix('\\').unwrap_or(raw);
    let (_, param, holds_after) = CONDITION_GUARD_FUNCTIONS
        .iter()
        .find(|(name, _, _)| called.eq_ignore_ascii_case(name))?;

    // A named argument may sit anywhere in the list, so `abort_if(code:
    // 404, boolean: $x === null)` still has to reach the condition.
    let condition = match fc.argument_list.arguments.first()? {
        Argument::Positional(pos) => pos.value,
        Argument::Named(_) => fc
            .argument_list
            .arguments
            .iter()
            .find_map(|arg| match arg {
                Argument::Named(named)
                    if bytes_to_str(named.name.value).eq_ignore_ascii_case(param) =>
                {
                    Some(named.value)
                }
                _ => None,
            })?,
    };
    Some((condition, *holds_after))
}

/// Process assert narrowing (assert($x instanceof Foo), @phpstan-assert, etc.)
pub(crate) fn process_assert_narrowing<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Every narrowing path below only fires for a (possibly parenthesized)
    // call expression, so a non-call statement can never be an assert() /
    // custom type-guard call. Bail out before the scope clone below, which
    // otherwise runs once per in-scope variable for every statement.
    let unwrapped = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    if !matches!(unwrapped, Expression::Call(_)) {
        return;
    }

    let guard = guard_call_condition(expr);

    // ── Handle assert($x instanceof Foo) for variables NOT yet in scope ──
    // When a foreach binds a variable but the iterable element type is
    // unknown, the variable won't be in the scope map.  A subsequent
    // `assert($x instanceof Foo)` (or `abort_unless($x instanceof Foo,
    // 403)`) should add it with the asserted type.
    if let Some((condition, true)) = guard
        && let Expression::Binary(bin) = condition
        && bin.operator.is_instanceof()
        && let Expression::Variable(Variable::Direct(dv)) = bin.lhs
    {
        let var_name = bytes_to_str(dv.name).to_string();
        if scope.get(&var_name).is_empty() {
            // Variable not in scope — seed it with the asserted type.
            let class_name = match bin.rhs {
                Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
                Expression::Self_(_) => Some(ctx.current_class.name.to_string()),
                Expression::Static(_) => Some(ctx.current_class.name.to_string()),
                Expression::Parent(_) => ctx.current_class.parent_class.map(|a| a.to_string()),
                _ => None,
            };
            if let Some(name) = class_name {
                let resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
                    &PhpType::named(atom(&name)),
                    &ctx.current_class.name,
                    ctx.all_classes,
                    ctx.class_loader,
                );
                if !resolved.is_empty() {
                    scope.set(
                        &var_name,
                        ResolvedType::from_classes_with_hint(resolved, PhpType::named(atom(&name))),
                    );
                } else {
                    scope.set(
                        &var_name,
                        vec![ResolvedType::from_type_string(PhpType::named(atom(&name)))],
                    );
                }
            }
        }
    }

    // Seed property/array-access subject keys that appear as arguments
    // to the assert call (e.g. `assertInstanceOf(X::class, $view->component)`
    // or a `@phpstan-assert` helper called on `$arg->value`) so the
    // narrowing loop below can find and narrow them.
    seed_assert_arg_subject_keys(expr, scope, ctx);

    // Re-export narrowing: PHPUnit's `assertTrue()` / `assertFalse()` carry
    // `@psalm-assert true/false $condition`.  When the argument is a boolean
    // condition expression (e.g. `property_exists($x, 'p')`), proving it
    // true/false is equivalent to a guard on that condition, so run the
    // standard condition-narrowing pipeline on the argument.
    let reexport_conditions = {
        let reexport_snapshot = scope.locals.clone();
        let reexport_resolver = |vn: &str| -> Vec<ResolvedType> {
            reexport_snapshot
                .get(&atom(vn))
                .cloned()
                .unwrap_or_default()
        };
        let reexport_ctx = build_var_ctx("", ctx, &reexport_resolver);
        narrowing::collect_assert_reexport_conditions(expr, &reexport_ctx)
    };
    for (condition, asserts_true) in reexport_conditions {
        if asserts_true {
            apply_condition_narrowing(condition, scope, ctx);
        } else {
            apply_condition_narrowing_inverse(condition, scope, ctx);
        }
    }

    // A conditional return type whose `never` branch some argument value
    // would have selected proves that value never reached the call.
    apply_never_branch_narrowing(unwrapped, scope, ctx);

    // `assert(<condition>)` proves its argument true for everything that
    // follows in the same scope, exactly the way entering `if (<condition>)`
    // proves it for the block body.  `abort_if(<condition>, 404)` and its
    // siblings prove the same thing about the branch that survives them,
    // just in the polarity their name picks.  Feeding the argument into the
    // same pipeline the `if` takes means every guard form is honoured in
    // both places: `$x !== null`, `$x !== false`, `is_string($x)`, `&&`
    // chains, and so on.
    if let Some((condition, holds_after)) = guard {
        if holds_after {
            apply_condition_narrowing(condition, scope, ctx);
        } else {
            apply_condition_narrowing_inverse(condition, scope, ctx);
        }
    }

    // Apply assert narrowing to each variable in scope.
    let scope_resolver = scope.snapshot_resolver();
    let var_names: Vec<Atom> = scope.locals.keys().copied().collect();
    for var_name in var_names {
        let var_ctx = ctx.var_ctx_for_with_scope(
            &var_name,
            ctx.cursor_offset,
            &scope_resolver,
            Some(scope.proofs()),
        );
        let before = scope.get(&var_name).to_vec();
        let mut results = before.clone();

        // @phpstan-assert / @psalm-assert
        let mut type_guard: Option<(narrowing::TypeGuardKind, bool)> = None;
        let mut intersected = false;
        ResolvedType::apply_narrowing(&mut results, |classes| {
            narrowing::try_apply_custom_assert_narrowing(
                expr,
                &var_ctx,
                classes,
                &mut type_guard,
                &mut intersected,
            )
        });
        // The assertion proved a class the subject does not nominally
        // implement, so the entries describe one value that is all of them
        // rather than a choice between them.  Untagged they join as a
        // union, which satisfies neither half's declared type.
        if intersected {
            ResolvedType::tag_as_intersection(&mut results);
        }

        // A scalar / pseudo-type assertion (`assertIsString`, `assertIsObject`,
        // `assertIsArray`, their `assertIsNot*` negations, or the `object`
        // fallback for an unresolvable `assertInstanceOf` class argument) is a
        // type guard, not a class narrowing.  Apply it on the full resolved
        // types so union members are kept or dropped by category — e.g.
        // `assertIsObject` drops null/scalar members while keeping the class,
        // and `assertIsNotObject` drops the class.
        if let Some((kind, exclude)) = type_guard {
            if exclude {
                narrowing::apply_type_guard_exclusion(kind, &mut results, Some(ctx.class_loader));
            } else {
                narrowing::apply_type_guard_inclusion(kind, &mut results, Some(ctx.class_loader));
            }
        }

        // A not-null assertion (`@phpstan-assert !null $x`, e.g. PHPUnit's
        // `assertNotNull`) removes the `null` pseudo-type, which the
        // class-based exclusion above cannot express.  Strip null from the
        // subject's resolved types directly so a value that was tracked as
        // exactly `null` (e.g. after `$obj->prop = null;`) no longer reads
        // as null after the assertion.
        if narrowing::call_asserts_not_null(expr, &var_ctx) {
            results.retain_mut(|rt| match rt.type_string.non_null_type() {
                Some(non_null) => {
                    rt.type_string = non_null;
                    true
                }
                None => rt.type_string != PhpType::null(),
            });
        }

        if resolved_types_differ(&results, &before) {
            if results.is_empty() {
                // Narrowing removed all types (e.g. assert($x instanceof
                // UnresolvableClass)).  Explicitly clear the variable so
                // that diagnostics see "unknown type" and suppress false
                // positives.  `scope.set()` is a no-op for empty vecs.
                scope.set_untyped(&var_name);
            } else {
                scope.set(&var_name, results);
            }
        }
    }
}

/// `@psalm-this-out` / `@phpstan-self-out`: a call to a method carrying
/// this annotation changes the type the walker tracks for its receiver,
/// the way an assignment changes a variable's type.  Method-level
/// template parameters bound from the call's arguments are substituted
/// into the annotation's type before it replaces the receiver's tracked
/// type: `$box->replace('x')` on a `MutableBox<int> $box`, where
/// `replace(U $value)` declares `@psalm-this-out self<U>`, re-binds
/// `$box` to `MutableBox<string>` for the rest of the block.
///
/// Only fires for a receiver that is a plain variable already in scope
/// with a resolved class — `$this` is excluded because there is no
/// receiver variable to re-bind.
pub(crate) fn process_self_out_narrowing<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let unwrapped = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    let (object, method, argument_list) = match unwrapped {
        Expression::Call(Call::Method(mc)) => (mc.object, &mc.method, &mc.argument_list),
        Expression::Call(Call::NullSafeMethod(mc)) => (mc.object, &mc.method, &mc.argument_list),
        _ => return,
    };
    let ClassLikeMemberSelector::Identifier(ident) = method else {
        return;
    };
    let Expression::Variable(Variable::Direct(dv)) = object else {
        return;
    };
    let var_name = bytes_to_str(dv.name);
    if var_name == "$this" {
        return;
    }
    let method_name = bytes_to_str(ident.value).to_string();

    let before = scope.get(var_name).to_vec();
    if before.is_empty() {
        return;
    }
    // Cheap check before the template substitution machinery below runs:
    // bail out unless at least one branch's class actually declares a
    // self-out type for this method.
    if !before.iter().any(|rt| {
        rt.class_info
            .as_ref()
            .and_then(|c| c.get_method_ci(&method_name))
            .is_some_and(|m| m.self_out.is_some())
    }) {
        return;
    }

    let arg_texts = crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast(
        argument_list,
        ctx.content,
    );
    let arg_refs: Vec<&str> = arg_texts.iter().map(|s| s.as_str()).collect();

    let scope_resolver = scope.snapshot_resolver();
    let var_ctx = ctx.var_ctx_for_with_scope(
        var_name,
        ctx.cursor_offset,
        &scope_resolver,
        Some(scope.proofs()),
    );
    let rctx = var_ctx.as_resolution_ctx();

    let mut changed = false;
    let mut results: Vec<ResolvedType> = Vec::with_capacity(before.len());
    for rt in &before {
        let mutated = rt.class_info.as_ref().and_then(|owner| {
            let self_out = owner.get_method_ci(&method_name)?.self_out.clone()?;
            let template_subs = crate::type_engine::call_resolution::build_call_template_subs(
                owner,
                &method_name,
                &arg_refs,
                Some(&rt.type_string),
                &rctx,
            );
            let substituted = self_out.substitute(&template_subs).simplified();
            let final_ty = if substituted.contains_self_ref() {
                substituted.replace_self_with_type(&rt.type_string)
            } else {
                substituted
            };
            // Re-resolve the class for the new type rather than keeping the
            // receiver's existing `class_info`: that one still carries the
            // *old* template substitution, so members typed by a template
            // parameter would keep resolving to the pre-call binding.
            let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
                &final_ty,
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            );
            Some(if classes.is_empty() {
                vec![ResolvedType::from_type_string(final_ty)]
            } else {
                ResolvedType::from_classes_with_hint(classes, final_ty)
            })
        });
        match mutated {
            Some(new_rts) => {
                changed = true;
                results.extend(new_rts);
            }
            None => results.push(rt.clone()),
        }
    }

    if changed {
        scope.set(var_name, results);
    }
}

/// Compare two `ResolvedType` slices by their observable identity
/// (type string + class FQN).  `ResolvedType` intentionally does not
/// implement `PartialEq` because `ClassInfo` is a large struct where
/// field-by-field equality is too expensive and semantically wrong.
/// This lightweight comparison detects when narrowing changed the
/// resolved type (e.g. replaced `BaseCatalogFeature` with `self`).
pub(crate) fn resolved_types_differ(a: &[ResolvedType], b: &[ResolvedType]) -> bool {
    if a.len() != b.len() {
        return true;
    }
    for (ra, rb) in a.iter().zip(b.iter()) {
        if ra.type_string != rb.type_string {
            return true;
        }
        match (&ra.class_info, &rb.class_info) {
            (Some(ca), Some(cb)) => {
                if ca.fqn() != cb.fqn() {
                    return true;
                }
            }
            (None, None) => {}
            _ => return true,
        }
    }
    false
}

/// Subtract from each argument the values a `never` branch of the callee's
/// conditional return type rules out.
///
/// `throw_unless()`, `throw_if()`, `abort_unless()` and their family carry
/// no `@phpstan-assert` tag; they declare their effect in the return type
/// itself:
///
/// ```text
/// @return ($condition is false ? never : ($condition is non-empty-mixed ? TValue : never))
/// ```
///
/// A `null` argument lands on a `never` branch there, so a run that gets
/// past the call did not pass one, and the following scope can drop it.
/// This is the same subtraction the `if (!$x) { throw …; }` form already
/// gets, derived from the declaration instead of from a body.
fn apply_never_branch_narrowing<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let Expression::Call(call) = expr else {
        return;
    };

    let snapshot = scope.locals.clone();
    let resolver =
        |vn: &str| -> Vec<ResolvedType> { snapshot.get(&atom(vn)).cloned().unwrap_or_default() };
    let var_ctx = build_var_ctx("", ctx, &resolver);
    let Some(info) = narrowing::extract_conditional_return_call(call, &var_ctx) else {
        return;
    };

    // A subject named by an argument is keyed the same way a condition's
    // subject is, so a property path or a call argument narrows too.
    seed_assert_arg_subject_keys(expr, scope, ctx);

    for (index, parameter) in info.parameters.iter().enumerate() {
        let Some(argument) = info.argument_list.arguments.iter().nth(index) else {
            continue;
        };
        let arg_expr = narrowing::argument_value(argument);
        let Some(key) = narrowing::expr_to_subject_key(arg_expr) else {
            continue;
        };
        let current = scope.get(&key).to_vec();
        if current.is_empty() {
            continue;
        }
        let joined = ResolvedType::types_joined(&current);
        let ruled_out = narrowing::never_ruled_out_members(
            &info.return_type,
            &parameter.name,
            &joined,
            ctx.class_loader,
        );
        if ruled_out.is_empty() {
            continue;
        }
        let kept: Vec<ResolvedType> = current
            .into_iter()
            .filter(|rt| !ruled_out.iter().any(|out| out.equivalent(&rt.type_string)))
            .map(|mut rt| {
                if let Some(narrowed) = subtract_members(&rt.type_string, &ruled_out) {
                    rt.type_string = narrowed;
                }
                rt
            })
            .collect();
        if !kept.is_empty() {
            scope.set(&key, kept);
        }
    }
}

/// `ty` with every alternative `ruled_out` names removed, or `None` when
/// it names none of them.
///
/// A type with a single alternative is left alone: removing its only
/// member would leave nothing to describe the value with.
fn subtract_members(ty: &PhpType, ruled_out: &[PhpType]) -> Option<PhpType> {
    let members = narrowing::split_into_runtime_members(ty);
    if members.len() < 2 {
        return None;
    }
    let kept: Vec<PhpType> = members
        .iter()
        .filter(|member| !ruled_out.iter().any(|out| out.equivalent(member)))
        .cloned()
        .collect();
    if kept.is_empty() || kept.len() == members.len() {
        return None;
    }
    Some(PhpType::union(kept))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increment_decrement_preserves_only_values_that_cannot_change() {
        let text = PhpType::literal_string_raw("'abc'");

        assert_eq!(
            type_after_increment_decrement(&text, IncrementDecrementKind::Increment),
            PhpType::string()
        );
        assert_eq!(
            type_after_increment_decrement(&text, IncrementDecrementKind::Decrement),
            text
        );
        assert_eq!(
            type_after_increment_decrement(
                &PhpType::named(atom("number")),
                IncrementDecrementKind::Increment
            ),
            PhpType::union(vec![PhpType::int(), PhpType::float()])
        );
        assert_eq!(
            type_after_increment_decrement(
                &PhpType::named(atom("Number")),
                IncrementDecrementKind::Increment
            ),
            PhpType::named(atom("Number"))
        );
    }
}
