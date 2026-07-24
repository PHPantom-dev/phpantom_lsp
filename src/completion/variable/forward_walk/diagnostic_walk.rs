use super::*;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::argument::Argument;
use mago_syntax::cst::sequence::TokenSeparatedSequence;

use crate::atom::bytes_to_str;
use crate::completion::resolver::Loaders;
use crate::parser::{extract_hint_type, with_parsed_program};
use crate::php_type::PhpType;
use crate::types::{ClassInfo, ResolvedType};

/// Walk a sequence of statements for diagnostic scope building.
///
/// Unlike [`walk_body_forward`] (which stops at the cursor), this walks
/// the **entire** body and records a scope snapshot at every statement
/// boundary.  The snapshots are stored in the thread-local
/// [`DIAGNOSTIC_SCOPE`] cache.
///
/// For each statement, this also discovers closure and arrow function
/// expressions and walks their bodies with properly seeded scopes so
/// that variables inside closures are fully resolved.
pub(crate) fn walk_body_for_diagnostics<'b>(
    statements: impl Iterator<Item = &'b Statement<'b>>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    for stmt in statements {
        // Record the scope at this statement's start offset so that
        // any member-access span inside this statement can look up
        // variable types that were established by prior statements.
        record_scope_snapshot(stmt.span().start.offset, scope);

        // Snapshot the pre-statement scope for the closure walk below.
        // Closures and other references inside this statement's own
        // expression evaluate *before* the statement's assignment takes
        // effect, so they must see the pre-assignment types.  E.g. in
        // `$x = f(fn() => ..., $x)` the trailing `$x` argument must
        // resolve to `$x`'s type before the reassignment, not `f()`'s
        // result.
        let pre_stmt_scope = scope.clone();

        process_statement(stmt, scope, ctx);

        // Walk closure and arrow function bodies found in this
        // statement.  Each closure gets a fresh scope seeded with
        // `use()` variables from the outer scope and its own parameter
        // types (with callable inference from the enclosing call's
        // signature).  Arrow functions inherit the outer scope with
        // their parameter types added on top.  The body is fully
        // walked so that scope snapshots are recorded for every
        // statement inside the closure/arrow function.
        walk_closures_in_statement(stmt, &pre_stmt_scope, ctx);

        // Also record at the statement's end offset, which covers
        // member accesses that appear after the last statement in
        // a block (e.g. the closing `}` region).
        record_scope_snapshot(stmt.span().end.offset, scope);
    }
}

/// Scan a statement for closure/arrow function expressions and walk
/// their bodies with properly seeded scopes.
///
/// For each closure found, a fresh scope is created and seeded with
/// `use()` variables from the outer scope plus the closure's own
/// parameters.  For each arrow function, the outer scope is cloned
/// and the arrow's parameters are added on top.  The body is then
/// fully walked so that scope snapshots are recorded for every
/// statement inside the closure/arrow function.
/// Scan a statement for closure/arrow function expressions and record
/// scope "shadow" snapshots inside their bodies.
///
/// When a closure/arrow function parameter shadows an outer variable
/// (e.g. `fn(Request $request)` where the outer scope has a different
/// `$request`), the scope cache would return the outer type for lookups
/// inside the closure body.  We fix this by recording a scope snapshot
/// at the closure body's start offset that removes shadowed variables.
/// The scope cache lookup then returns `None` for those variables,
/// calling `resolve_variable_types` which would re-enter the forward
/// walker.
///
/// This approach avoids walking the entire closure body (which would
/// override `resolve_variable_types` for ALL variables, including those
/// from foreach bindings over generic collections where the outer
/// resolver produces better types).
/// Scan a **single** statement's direct expressions for closure/arrow
/// function literals and walk their bodies with properly seeded scopes.
///
/// This function intentionally does **not** recurse into nested block
/// bodies (if/while/foreach/try/switch).  Those bodies are walked by
/// [`walk_body_forward`], which calls this function for each statement
/// it processes — at that point the scope already reflects narrowing,
/// foreach bindings, and other context from the enclosing block.
///
/// Only the expressions that are directly part of this statement (the
/// condition expression, the iteration expression, echo values, etc.)
/// are scanned for closures.  Closures inside nested block bodies will
/// be picked up when `walk_body_forward` processes the inner statements.
pub(crate) fn walk_closures_in_statement<'b>(
    stmt: &'b Statement<'b>,
    outer_scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    match stmt {
        Statement::Expression(expr_stmt) => {
            walk_closures_in_expr(expr_stmt.expression, outer_scope, ctx, None);
        }
        Statement::Return(ret) => {
            if let Some(val) = ret.value {
                walk_closures_in_expr(val, outer_scope, ctx, None);
            }
        }
        Statement::Echo(echo) => {
            for val in echo.values.iter() {
                walk_closures_in_expr(val, outer_scope, ctx, None);
            }
        }
        // For compound statements (if, while, foreach, etc.) only scan
        // the condition/iteration expression — not the block bodies.
        // Block bodies are walked by walk_body_forward which calls us
        // per-statement with the correct inner scope.
        Statement::If(if_stmt) => {
            walk_closures_in_expr(if_stmt.condition, outer_scope, ctx, None);
        }
        Statement::While(while_stmt) => {
            walk_closures_in_expr(while_stmt.condition, outer_scope, ctx, None);
        }
        Statement::DoWhile(dw) => {
            walk_closures_in_expr(dw.condition, outer_scope, ctx, None);
        }
        Statement::Foreach(foreach) => {
            walk_closures_in_expr(foreach.expression, outer_scope, ctx, None);
        }
        Statement::For(for_stmt) => {
            for init in for_stmt.initializations.iter() {
                walk_closures_in_expr(init, outer_scope, ctx, None);
            }
            for cond in for_stmt.conditions.iter() {
                walk_closures_in_expr(cond, outer_scope, ctx, None);
            }
            for update in for_stmt.increments.iter() {
                walk_closures_in_expr(update, outer_scope, ctx, None);
            }
        }
        Statement::Switch(switch) => {
            walk_closures_in_expr(switch.expression, outer_scope, ctx, None);
        }
        _ => {}
    }
}

/// Recursively scan an expression tree for closures/arrow functions
/// and walk their bodies with properly seeded scopes.
///
/// When a closure/arrow function is found:
/// 1. Build a scope for the closure body (fresh for closures, cloned
///    from outer for arrow functions).
/// 2. Seed the scope with parameter types (using callable inference
///    from the enclosing call's signature when parameters are untyped).
/// 3. Walk the body using [`walk_body_for_diagnostics`] so that scope
///    snapshots are recorded at every statement boundary.
///
/// The `inferred_params` argument carries callable parameter types
/// inferred from the enclosing call's signature.  When a closure is
/// found as a direct argument to a function/method call, the caller
/// passes the inferred types so untyped parameters get the correct
/// types.
pub(crate) fn walk_closures_in_expr<'b>(
    expr: &'b Expression<'b>,
    outer_scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    inferred_params: Option<&[PhpType]>,
) {
    match expr {
        Expression::Closure(closure) => {
            // Build a fresh scope for the closure.
            let mut closure_scope = ScopeState::new();

            // PHP closures implicitly capture `$this` from the
            // enclosing class method.  Seed it from the outer scope
            // so that `$this->prop` inside the closure resolves
            // without calling `resolve_variable_types`.
            let this_types = outer_scope.get("$this");
            if !this_types.is_empty() {
                closure_scope.set("$this", this_types.to_vec());
            }

            // Seed with `use(...)` variables from the outer scope.
            if let Some(ref use_clause) = closure.use_clause {
                for use_var in use_clause.variables.iter() {
                    let var_name = bytes_to_str(use_var.variable.name).to_string();
                    let from_outer = outer_scope.get(&var_name);
                    if !from_outer.is_empty() {
                        closure_scope.set(&var_name, from_outer.to_vec());
                    }
                }
            }

            // Seed with parameter types, using callable inference when
            // available.  Filter out any inferred params whose base
            // type is unresolvable (e.g. PHPStan pseudo-types like
            // `collection-of<T>`) so they don't poison the scope —
            // the param simply won't be seeded, which is better than
            // skipping the entire closure body.
            let inferred = inferred_params.unwrap_or(&[]);
            let filtered_inferred = filter_resolvable_inferred_params(inferred, ctx);
            seed_closure_params(
                &mut closure_scope,
                &closure.parameter_list,
                closure.span().start.offset,
                &filtered_inferred,
                ctx,
            );

            // Record the scope at the body start.
            let body_span = closure.body.span();
            record_scope_snapshot(body_span.start.offset, &closure_scope);

            // Walk the closure body.
            walk_body_for_diagnostics(closure.body.statements.iter(), &mut closure_scope, ctx);

            // Record at body end (closure scope).
            record_scope_snapshot(body_span.end.offset, &closure_scope);

            // Restore the outer scope immediately after the closure body
            // so that code following the closure in the same expression
            // (e.g. `->where('product_id', $product->id)` after a
            // `whereHas(function (Builder $q) { ... })`) sees the outer
            // scope's variables, not the closure's.
            record_scope_snapshot(body_span.end.offset + 1, outer_scope);
        }
        Expression::ArrowFunction(arrow) => {
            // Arrow functions inherit the enclosing scope.
            let mut arrow_scope = outer_scope.clone();

            // Seed with parameter types, using callable inference when
            // available.
            let inferred = inferred_params.unwrap_or(&[]);
            let filtered_inferred = filter_resolvable_inferred_params(inferred, ctx);
            seed_closure_params(
                &mut arrow_scope,
                &arrow.parameter_list,
                arrow.span().start.offset,
                &filtered_inferred,
                ctx,
            );

            // Record the scope at the body expression.
            let body_span = arrow.expression.span();
            record_scope_snapshot(body_span.start.offset, &arrow_scope);
            record_scope_snapshot(body_span.end.offset, &arrow_scope);

            // The arrow body is a single return-value expression, so
            // apply the same `&&` / `||` / match / ternary narrowing that
            // a `return $x instanceof Foo && $x->bar()` statement would
            // get.  Without this, a member access on a parameter narrowed
            // by an earlier conjunct (e.g. `fn($x) => $x instanceof Foo
            // && $x->bar()`) sees the un-narrowed parameter type.
            record_and_chain_snapshots(arrow.expression, &arrow_scope, ctx);
            record_or_chain_snapshots(arrow.expression, &arrow_scope, ctx);
            if is_diagnostic_scope_active() {
                record_match_ternary_snapshots(arrow.expression, &arrow_scope, ctx);
            }

            // Restore the outer scope after the arrow body (same
            // reasoning as for closures above).
            record_scope_snapshot(body_span.end.offset + 1, outer_scope);

            // Recurse into the body expression for nested closures.
            walk_closures_in_expr(arrow.expression, &arrow_scope, ctx, None);
        }
        // For call expressions, try to infer callable parameter types
        // from the function/method signature before recursing into
        // the arguments.
        Expression::Call(call) => {
            walk_closures_in_call(call, outer_scope, ctx);
        }
        // Recurse into sub-expressions that may contain closures.
        Expression::Parenthesized(inner) => {
            walk_closures_in_expr(inner.expression, outer_scope, ctx, None);
        }
        Expression::Assignment(assignment) => {
            walk_closures_in_expr(assignment.rhs, outer_scope, ctx, None);
        }
        Expression::Access(access) => match access {
            Access::Property(pa) => {
                walk_closures_in_expr(pa.object, outer_scope, ctx, None);
            }
            Access::NullSafeProperty(pa) => {
                walk_closures_in_expr(pa.object, outer_scope, ctx, None);
            }
            _ => {}
        },
        Expression::Array(arr) => {
            for elem in arr.elements.iter() {
                let elem_expr = match elem {
                    ArrayElement::KeyValue(kv) => {
                        walk_closures_in_expr(kv.key, outer_scope, ctx, None);
                        kv.value
                    }
                    ArrayElement::Value(val) => val.value,
                    ArrayElement::Variadic(v) => v.value,
                    ArrayElement::Missing(_) => continue,
                };
                walk_closures_in_expr(elem_expr, outer_scope, ctx, None);
            }
        }
        Expression::LegacyArray(arr) => {
            for elem in arr.elements.iter() {
                let elem_expr = match elem {
                    ArrayElement::KeyValue(kv) => {
                        walk_closures_in_expr(kv.key, outer_scope, ctx, None);
                        kv.value
                    }
                    ArrayElement::Value(val) => val.value,
                    ArrayElement::Variadic(v) => v.value,
                    ArrayElement::Missing(_) => continue,
                };
                walk_closures_in_expr(elem_expr, outer_scope, ctx, None);
            }
        }
        Expression::Binary(bin) => {
            walk_closures_in_expr(bin.lhs, outer_scope, ctx, None);
            walk_closures_in_expr(bin.rhs, outer_scope, ctx, None);
        }
        Expression::UnaryPrefix(prefix) => {
            walk_closures_in_expr(prefix.operand, outer_scope, ctx, None);
        }
        Expression::Conditional(cond) => {
            walk_closures_in_expr(cond.condition, outer_scope, ctx, None);
            if let Some(then_expr) = cond.then {
                walk_closures_in_expr(then_expr, outer_scope, ctx, None);
            }
            walk_closures_in_expr(cond.r#else, outer_scope, ctx, None);
        }
        Expression::Match(m) => {
            walk_closures_in_expr(m.expression, outer_scope, ctx, None);
            for arm in m.arms.iter() {
                walk_closures_in_expr(arm.expression(), outer_scope, ctx, None);
            }
        }
        Expression::Instantiation(inst) => {
            if let Some(ref args) = inst.argument_list {
                walk_closures_in_call_args(&args.arguments, outer_scope, ctx, |_| vec![]);
            }
        }
        Expression::AnonymousClass(anon) => {
            // Constructor arguments evaluate in the outer scope (with the
            // outer `$this`), so scan them for closures there.
            if let Some(ref args) = anon.argument_list {
                walk_closure_in_partial_call_args(&args.arguments, outer_scope, ctx, |_| vec![]);
            }
            // The anonymous class's own method bodies have their own
            // `$this` (the anonymous class), so walk them separately.
            walk_anonymous_class_member_bodies(anon, ctx);

            // Restore the outer scope immediately after the anonymous
            // class body so that code following it in the same expression
            // (e.g. a sibling call argument `f(new class {...}, $this->x)`)
            // sees the outer `$this`, not the anonymous class's.
            record_scope_snapshot(anon.right_brace.end.offset + 1, outer_scope);
        }
        Expression::Yield(y) => match y {
            Yield::Value(yv) => {
                if let Some(val) = &yv.value {
                    walk_closures_in_expr(val, outer_scope, ctx, None);
                }
            }
            Yield::Pair(yp) => {
                walk_closures_in_expr(yp.key, outer_scope, ctx, None);
                walk_closures_in_expr(yp.value, outer_scope, ctx, None);
            }
            Yield::From(yf) => {
                walk_closures_in_expr(yf.iterator, outer_scope, ctx, None);
            }
        },
        Expression::Throw(t) => {
            walk_closures_in_expr(t.exception, outer_scope, ctx, None);
        }
        Expression::Clone(c) => {
            walk_closures_in_expr(c.object, outer_scope, ctx, None);
        }
        Expression::Pipe(p) => {
            walk_closures_in_expr(p.input, outer_scope, ctx, None);
            walk_closures_in_expr(p.callable, outer_scope, ctx, None);
        }
        _ => {}
    }
}

/// Handle a call expression: infer callable parameter types from the
/// function/method signature and pass them when walking closure arguments.
pub(crate) fn walk_closures_in_call<'b>(
    call: &'b Call<'b>,
    outer_scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    match call {
        Call::Function(fc) => {
            // Recurse into the function expression (for closures in
            // chained calls like `$fn()($anotherClosure)`).
            walk_closures_in_expr(fc.function, outer_scope, ctx, None);

            let func_name = match fc.function {
                Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
                _ => None,
            };
            walk_closures_in_call_args(&fc.argument_list.arguments, outer_scope, ctx, |arg_idx| {
                if let Some(ref name) = func_name {
                    infer_callable_params_from_function_fw(
                        name,
                        arg_idx,
                        &fc.argument_list.arguments,
                        outer_scope,
                        ctx,
                    )
                } else {
                    vec![]
                }
            });
        }
        Call::Method(mc) => {
            walk_closures_in_expr(mc.object, outer_scope, ctx, None);

            let method_name = if let ClassLikeMemberSelector::Identifier(ident) = &mc.method {
                Some(bytes_to_str(ident.value).to_string())
            } else {
                None
            };
            let obj_span = mc.object.span();
            let first_arg = extract_first_arg_string_fw(&mc.argument_list.arguments, ctx.content);
            walk_closures_in_call_args(&mc.argument_list.arguments, outer_scope, ctx, |arg_idx| {
                if let Some(ref name) = method_name {
                    infer_callable_params_from_receiver_fw(
                        obj_span.start.offset,
                        obj_span.end.offset,
                        name,
                        arg_idx,
                        first_arg.as_deref(),
                        outer_scope,
                        ctx,
                    )
                } else {
                    vec![]
                }
            });
        }
        Call::NullSafeMethod(mc) => {
            walk_closures_in_expr(mc.object, outer_scope, ctx, None);

            let method_name = if let ClassLikeMemberSelector::Identifier(ident) = &mc.method {
                Some(bytes_to_str(ident.value).to_string())
            } else {
                None
            };
            let obj_span = mc.object.span();
            let first_arg = extract_first_arg_string_fw(&mc.argument_list.arguments, ctx.content);
            walk_closures_in_call_args(&mc.argument_list.arguments, outer_scope, ctx, |arg_idx| {
                if let Some(ref name) = method_name {
                    infer_callable_params_from_receiver_fw(
                        obj_span.start.offset,
                        obj_span.end.offset,
                        name,
                        arg_idx,
                        first_arg.as_deref(),
                        outer_scope,
                        ctx,
                    )
                } else {
                    vec![]
                }
            });
        }
        Call::StaticMethod(sc) => {
            walk_closures_in_expr(sc.class, outer_scope, ctx, None);

            let method_name = if let ClassLikeMemberSelector::Identifier(ident) = &sc.method {
                Some(bytes_to_str(ident.value).to_string())
            } else {
                None
            };
            let first_arg = extract_first_arg_string_fw(&sc.argument_list.arguments, ctx.content);
            walk_closures_in_call_args(&sc.argument_list.arguments, outer_scope, ctx, |arg_idx| {
                if let Some(ref name) = method_name {
                    infer_callable_params_from_static_receiver_fw(
                        sc.class,
                        name,
                        arg_idx,
                        first_arg.as_deref(),
                        outer_scope,
                        ctx,
                    )
                } else {
                    vec![]
                }
            });
        }
    }
}

/// Walk the arguments of a call expression, invoking `infer_fn` for
/// each argument index to get inferred callable parameter types.
/// When an argument is a closure/arrow function, the inferred types
/// are passed through so untyped parameters get the correct types.
pub(crate) fn walk_closures_in_call_args<'b, F>(
    arguments: &'b TokenSeparatedSequence<'b, Argument<'b>>,
    outer_scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    infer_fn: F,
) where
    F: Fn(usize) -> Vec<PhpType>,
{
    for (arg_idx, arg) in arguments.iter().enumerate() {
        let arg_expr = match arg {
            Argument::Positional(a) => a.value,
            Argument::Named(a) => a.value,
        };
        match arg_expr {
            Expression::Closure(_) | Expression::ArrowFunction(_) => {
                let inferred = infer_fn(arg_idx);
                walk_closures_in_expr(
                    arg_expr,
                    outer_scope,
                    ctx,
                    if inferred.is_empty() {
                        None
                    } else {
                        Some(&inferred)
                    },
                );
            }
            _ => {
                walk_closures_in_expr(arg_expr, outer_scope, ctx, None);
            }
        }
    }
}

/// Walk the partial arguments of a call expression, invoking `infer_fn` for
/// each argument index to get inferred callable parameter types.
/// When an argument is a closure/arrow function, the inferred types
/// are passed through so untyped parameters get the correct types.
pub(crate) fn walk_closure_in_partial_call_args<'b, F>(
    arguments: &'b TokenSeparatedSequence<'b, PartialArgument<'b>>,
    outer_scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    infer_fn: F,
) where
    F: Fn(usize) -> Vec<PhpType>,
{
    for (arg_idx, arg) in arguments.iter().enumerate() {
        let arg_expr = match arg {
            PartialArgument::Positional(a) => a.value,
            PartialArgument::Named(a) => a.value,
            _ => continue,
        };
        match arg_expr {
            Expression::Closure(_) | Expression::ArrowFunction(_) => {
                let inferred = infer_fn(arg_idx);
                walk_closures_in_expr(
                    arg_expr,
                    outer_scope,
                    ctx,
                    if inferred.is_empty() {
                        None
                    } else {
                        Some(&inferred)
                    },
                );
            }
            _ => {
                walk_closures_in_expr(arg_expr, outer_scope, ctx, None);
            }
        }
    }
}

/// Seed a closure/arrow function scope with parameter types, using
/// inferred callable types as fallback for untyped parameters.
///
/// This mirrors [`seed_params`] but additionally accepts `inferred_types`
/// from the enclosing call's callable signature.  When a parameter has
/// no explicit type hint, the corresponding inferred type (matched by
/// positional index) is used instead.
/// Check whether a `/** … */` docblock is directly attached to the
/// code at `fn_offset` — i.e. only whitespace separates the closing
/// `*/` from `fn_offset`.  This prevents `@param` annotations from
/// sibling closures/arrow functions from leaking across statement
/// boundaries.
pub(crate) fn is_docblock_adjacent(content: &str, fn_offset: usize) -> bool {
    let before = match content.get(..fn_offset) {
        Some(s) => s,
        None => return false,
    };
    // Walk backward over whitespace, then over optional keywords
    // (`static`, visibility modifiers) that may sit between the
    // docblock and `fn`.
    let trimmed = before.trim_end();
    if trimmed.ends_with("*/") {
        return true;
    }
    // Allow `static` keyword between docblock and `fn(…)`:
    //   /** @param T $x */ static fn(T $x) => …
    // Also allow the `function` keyword for regular closures.
    let trimmed = trimmed
        .trim_end_matches(|c: char| c.is_ascii_alphanumeric() || c == '_')
        .trim_end();
    trimmed.ends_with("*/")
}

pub(crate) fn seed_closure_params(
    scope: &mut ScopeState,
    parameter_list: &FunctionLikeParameterList<'_>,
    fn_span_start: u32,
    inferred_types: &[PhpType],
    ctx: &ForwardWalkCtx<'_>,
) {
    for (idx, param) in parameter_list.parameters.iter().enumerate() {
        let pname = bytes_to_str(param.variable.name).to_string();
        let is_variadic = param.ellipsis.is_some();

        let native_type = param.hint.as_ref().map(|h| extract_hint_type(h));

        // Check the `@param` docblock annotation.
        //
        // Only trust the result when the docblock is directly attached
        // to this closure/arrow function (no intervening code).  Without
        // this guard, sibling arrow functions that share a parameter
        // name (e.g. two `array_map(fn($row) => …)` calls) would leak
        // `@param` annotations from one closure to the other, because
        // arrow functions don't introduce `{`/`}` scope boundaries and
        // `find_iterable_raw_type_in_source` scans backward freely.
        let raw_docblock_type = crate::docblock::find_iterable_raw_type_in_source(
            ctx.content,
            fn_span_start as usize,
            &pname,
        )
        .filter(|_| is_docblock_adjacent(ctx.content, fn_span_start as usize))
        .map(|t| crate::util::resolve_php_type_names(&t, ctx.class_loader));

        let effective_type = crate::docblock::resolve_effective_type_typed(
            native_type.as_ref(),
            raw_docblock_type.as_ref(),
        );

        // Substitute method-level template params with their bounds.
        let effective_type = effective_type.map(|ty| {
            let ty = super::super::resolution::substitute_template_param_bounds(
                ty,
                ctx.content,
                fn_span_start as usize,
            );
            super::super::resolution::substitute_class_string_template_bounds(
                ty,
                ctx.content,
                fn_span_start as usize,
            )
        });

        let inferred_for_idx = inferred_types.get(idx);

        // When the explicit hint is a bare class name and the inferred
        // type is the same class WITH generic args, prefer the inferred
        // type (preserves template substitution).
        let use_inferred_over_explicit = if let Some(ref eff) = effective_type
            && let Some(inferred) = inferred_for_idx
        {
            super::super::closure_resolution::inferred_type_is_more_specific_pub(eff, inferred)
        } else {
            false
        };

        let mut param_results = if use_inferred_over_explicit {
            let pi = inferred_for_idx.unwrap();
            let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                pi,
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            );
            if !resolved.is_empty() {
                ResolvedType::from_classes_with_hint(resolved, pi.clone())
            } else if pi.is_informative() {
                // The inferred type is more specific (e.g.
                // `array<int, array<string, string>>` vs bare `array`)
                // but doesn't resolve to a class.  Preserve the type
                // string so the parameter is still seeded in scope.
                vec![ResolvedType::from_type_string(pi.clone())]
            } else {
                vec![]
            }
        } else if let Some(ref eff) = effective_type {
            let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                eff,
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            );
            if !resolved.is_empty() {
                // Check if inferred is a subtype and more specific.
                if let Some(inferred) = inferred_for_idx {
                    let inferred_resolved =
                        crate::completion::type_resolution::type_hint_to_classes_typed(
                            inferred,
                            &ctx.current_class.name,
                            ctx.all_classes,
                            ctx.class_loader,
                        );
                    // Narrow to the inferred type only when it is a
                    // genuine refinement of the *whole* declared type:
                    // every inferred class must be a subtype of some
                    // declared class (inferred ⊆ declared), AND every
                    // declared class must be refined by some inferred
                    // class (declared covered by inferred).  Without the
                    // second check a declared union like
                    // `A|B|C` collapses to a single inferred arm (`A`)
                    // when the subject is a union of differently
                    // parameterized collections, discarding the other
                    // declared possibilities.
                    let inferred_is_subtype = !inferred_resolved.is_empty()
                        && inferred_resolved.iter().all(|inferred_cls| {
                            resolved.iter().any(|explicit_cls| {
                                crate::class_lookup::is_subtype_of_names(
                                    &inferred_cls.fqn(),
                                    &explicit_cls.fqn(),
                                    ctx.class_loader,
                                )
                            })
                        });
                    let inferred_covers_declared = resolved.iter().all(|explicit_cls| {
                        inferred_resolved.iter().any(|inferred_cls| {
                            crate::class_lookup::is_subtype_of_names(
                                &inferred_cls.fqn(),
                                &explicit_cls.fqn(),
                                ctx.class_loader,
                            )
                        })
                    });
                    if inferred_is_subtype && inferred_covers_declared {
                        ResolvedType::from_classes_with_hint(inferred_resolved, inferred.clone())
                    } else {
                        ResolvedType::from_classes_with_hint(resolved, eff.clone())
                    }
                } else {
                    ResolvedType::from_classes_with_hint(resolved, eff.clone())
                }
            } else {
                // The explicit hint didn't resolve to a class.
                // Try docblock for a richer type.
                if let Some(ref parsed_dt) = raw_docblock_type {
                    let doc_resolved =
                        crate::completion::type_resolution::type_hint_to_classes_typed(
                            parsed_dt,
                            &ctx.current_class.name,
                            ctx.all_classes,
                            ctx.class_loader,
                        );
                    if !doc_resolved.is_empty() {
                        ResolvedType::from_classes_with_hint(doc_resolved, parsed_dt.clone())
                    } else {
                        let best_type = raw_docblock_type
                            .clone()
                            .or_else(|| effective_type.clone())
                            .unwrap_or_else(PhpType::untyped);
                        vec![ResolvedType::from_type_string(best_type)]
                    }
                } else {
                    let best_type = effective_type.clone().unwrap_or_else(PhpType::untyped);
                    vec![ResolvedType::from_type_string(best_type)]
                }
            }
        } else if let Some(inferred) = inferred_for_idx {
            // No explicit type — use the inferred type from the
            // callable signature.
            let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                inferred,
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            );
            if !resolved.is_empty() {
                ResolvedType::from_classes_with_hint(resolved, inferred.clone())
            } else if inferred.is_informative() {
                vec![ResolvedType::from_type_string(inferred.clone())]
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        // Variadic parameter wrapping.
        if is_variadic && !param_results.is_empty() {
            for rt in &mut param_results {
                rt.type_string = PhpType::list(rt.type_string.clone());
                rt.class_info = None;
            }
        }

        // Closure/arrow-function parameters shadow same-named outer
        // variables unconditionally.  Even when no type could be
        // determined, the outer variable's type must not leak into the
        // closure body — record the parameter as present-but-untyped
        // and drop any synthetic keys (`$p->x`, `$p["k"]`) tracked for
        // the shadowed outer variable.
        scope.remove(&pname);
        scope.invalidate_dependent_keys(&pname);
        if param_results.is_empty() {
            scope.set_empty(&pname);
        } else {
            scope.seed(&pname, param_results);
        }
    }
}

/// Build diagnostic scope snapshots for every function/method body in
/// the file.
///
/// Parses the file, iterates all top-level and class-level
/// function/method bodies, runs the forward walker on each, and stores
/// scope snapshots in the thread-local [`DIAGNOSTIC_SCOPE`] cache.
///
/// The caller must have activated the cache via
/// [`with_diagnostic_scope_cache`] before calling this function.
pub(crate) fn build_diagnostic_scopes(
    content: &str,
    local_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    loaders: Loaders<'_>,
    resolved_class_cache: Option<&crate::virtual_members::ResolvedClassCache>,
) {
    if !is_diagnostic_scope_active() {
        return;
    }

    // Skip if the scope cache is already populated (prevents double
    // walk when both the analyze loop and collect_slow_diagnostics
    // call this function).
    let already_populated =
        DIAGNOSTIC_SCOPE.with(|cell| cell.borrow().as_ref().is_some_and(|m| !m.is_empty()));
    if already_populated {
        return;
    }

    // Mark that we are building the scope cache so that nested
    // resolution calls (e.g. resolve_variable_types) do not read
    // from the partially-populated cache.
    BUILDING_SCOPES.with(|cell: &Cell<bool>| cell.set(true));
    let _building_guard = BuildingScopesGuard;

    let default_class = ClassInfo::default();
    let diag_ctx = DiagnosticWalkCtx {
        content,
        local_classes,
        class_loader,
        loaders,
        resolved_class_cache,
    };

    with_parsed_program(content, "build_diagnostic_scopes", |program, _content| {
        // Walk all top-level statements, analyzing function/method
        // bodies AND top-level code (assignments, expressions, if,
        // foreach, etc.) that lives outside any function body.
        walk_top_level_statements(program.statements.iter(), &default_class, &diag_ctx);
    });
}

/// Walk a sequence of top-level (or namespace-level) statements,
/// maintaining a shared `ScopeState` for code that lives outside any
/// function or class body.  Function/class/trait/interface/enum bodies
/// are analyzed in isolation (as before), but top-level assignments,
/// expressions, if/foreach/while/for/try/switch, and other statements
/// are walked through the forward walker so that scope snapshots are
/// recorded.  This ensures variable accesses resolve from the scope
/// cache.
pub(crate) fn walk_top_level_statements<'a, 'b: 'a>(
    statements: impl Iterator<Item = &'b Statement<'b>>,
    default_class: &ClassInfo,
    diag_ctx: &DiagnosticWalkCtx<'_>,
) {
    let ctx = ForwardWalkCtx {
        current_class: default_class,
        all_classes: diag_ctx.local_classes,
        content: diag_ctx.content,
        cursor_offset: u32::MAX,
        class_loader: diag_ctx.class_loader,
        loaders: diag_ctx.loaders,
        resolved_class_cache: diag_ctx.resolved_class_cache,
        enclosing_return_type: None,
        top_level_scope: None,
    };

    let mut top_level_scope = ScopeState::new();

    // Seed superglobals for top-level code.
    seed_superglobals(&mut top_level_scope);

    for stmt in statements {
        match stmt {
            Statement::Namespace(ns) => {
                // Recurse into namespace body with a fresh scope.
                walk_top_level_statements(ns.statements().iter(), default_class, diag_ctx);
            }
            Statement::Class(class) => {
                let enclosing = find_enclosing_class_for_offset(
                    diag_ctx.local_classes,
                    class.left_brace.start.offset,
                )
                .unwrap_or(default_class);
                for member in class.members.iter() {
                    walk_class_member_body(member, enclosing, diag_ctx);
                }
            }
            Statement::Interface(iface) => {
                let enclosing = find_enclosing_class_for_offset(
                    diag_ctx.local_classes,
                    iface.left_brace.start.offset,
                )
                .unwrap_or(default_class);
                for member in iface.members.iter() {
                    walk_class_member_body(member, enclosing, diag_ctx);
                }
            }
            Statement::Trait(trait_def) => {
                let enclosing = find_enclosing_class_for_offset(
                    diag_ctx.local_classes,
                    trait_def.left_brace.start.offset,
                )
                .unwrap_or(default_class);
                for member in trait_def.members.iter() {
                    walk_class_member_body(member, enclosing, diag_ctx);
                }
            }
            Statement::Enum(enum_def) => {
                let enclosing = find_enclosing_class_for_offset(
                    diag_ctx.local_classes,
                    enum_def.left_brace.start.offset,
                )
                .unwrap_or(default_class);
                for member in enum_def.members.iter() {
                    walk_class_member_body(member, enclosing, diag_ctx);
                }
            }
            Statement::Function(func) => {
                analyze_function_body(
                    func.parameter_list.parameters.iter(),
                    func.body.statements.iter(),
                    func.span().start.offset,
                    default_class,
                    None,
                    true, // standalone functions have no `$this`
                    diag_ctx,
                );
            }
            // Functions nested inside if blocks (common pattern:
            // `if (!function_exists('name')) { function name() {} }`)
            // must be analyzed the same way as top-level functions.
            Statement::If(if_stmt) => {
                record_scope_snapshot(stmt.span().start.offset, &top_level_scope);
                let pre_stmt_scope = top_level_scope.clone();
                process_statement(stmt, &mut top_level_scope, &ctx);
                walk_closures_in_statement(stmt, &pre_stmt_scope, &ctx);
                record_scope_snapshot(stmt.span().end.offset, &top_level_scope);
                walk_functions_in_if_body(&if_stmt.body, default_class, diag_ctx);
            }
            // Top-level code: walk it with the shared scope so that
            // variable assignments accumulate and subsequent accesses
            // can be served from the scope cache instead of remaining
            // unresolved.
            _ => {
                record_scope_snapshot(stmt.span().start.offset, &top_level_scope);
                let pre_stmt_scope = top_level_scope.clone();
                process_statement(stmt, &mut top_level_scope, &ctx);
                walk_closures_in_statement(stmt, &pre_stmt_scope, &ctx);
                record_scope_snapshot(stmt.span().end.offset, &top_level_scope);
            }
        }
    }
}

/// Recurse into an if-statement body looking for function declarations
/// and analyze each one.  Handles the common PHP pattern:
/// `if (!function_exists('name')) { function name(...) { ... } }`
pub(crate) fn walk_functions_in_if_body<'b>(
    body: &'b mago_syntax::cst::control_flow::r#if::IfBody<'b>,
    default_class: &ClassInfo,
    diag_ctx: &DiagnosticWalkCtx<'_>,
) {
    use mago_syntax::cst::control_flow::r#if::IfBody;

    let statements: &[Statement<'b>] = match body {
        IfBody::Statement(stmt_body) => {
            // Single statement body — check if it's a block.
            if let Statement::Block(block) = stmt_body.statement {
                block.statements.as_slice()
            } else if let Statement::Function(func) = stmt_body.statement {
                analyze_function_body(
                    func.parameter_list.parameters.iter(),
                    func.body.statements.iter(),
                    func.span().start.offset,
                    default_class,
                    None,
                    true,
                    diag_ctx,
                );
                return;
            } else {
                return;
            }
        }
        IfBody::ColonDelimited(colon_body) => colon_body.statements.as_slice(),
    };

    for inner_stmt in statements.iter() {
        if let Statement::Function(func) = inner_stmt {
            analyze_function_body(
                func.parameter_list.parameters.iter(),
                func.body.statements.iter(),
                func.span().start.offset,
                default_class,
                None,
                true,
                diag_ctx,
            );
        }
    }
}

/// Walk a class member to find method bodies and run the forward walker.
pub(crate) fn walk_class_member_body<'b>(
    member: &'b mago_syntax::cst::class_like::member::ClassLikeMember<'b>,
    enclosing_class: &ClassInfo,
    diag_ctx: &DiagnosticWalkCtx<'_>,
) {
    use mago_syntax::cst::class_like::member::ClassLikeMember;
    use mago_syntax::cst::class_like::method::MethodBody;

    if let ClassLikeMember::Method(method) = member
        && let MethodBody::Concrete(block) = &method.body
    {
        let method_name = bytes_to_str(method.name.value).to_string();
        let is_static = method.modifiers.contains_static();
        analyze_function_body(
            method.parameter_list.parameters.iter(),
            block.statements.iter(),
            method.span().start.offset,
            enclosing_class,
            Some(&method_name),
            is_static,
            diag_ctx,
        );
    }
}

/// Bundles the immutable context needed by [`analyze_function_body`] and
/// the AST walkers so we don't pass 5+ individual arguments everywhere.
pub(crate) struct DiagnosticWalkCtx<'a> {
    content: &'a str,
    local_classes: &'a [Arc<ClassInfo>],
    class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    loaders: Loaders<'a>,
    resolved_class_cache: Option<&'a crate::virtual_members::ResolvedClassCache>,
}

/// Run the forward walker on a single function/method body and record
/// scope snapshots for diagnostics.
///
/// `is_static` indicates whether this is a static method.  When `false`
/// and `current_class` has a non-empty name, `$this` is seeded in the
/// scope so that expressions like `$this->prop` and `foreach ($this->items as $item)`
/// can resolve without remaining unresolved.
pub(crate) fn analyze_function_body<'b>(
    parameters: impl Iterator<Item = &'b FunctionLikeParameter<'b>>,
    body_statements: impl Iterator<Item = &'b Statement<'b>>,
    fn_span_start: u32,
    current_class: &ClassInfo,
    method_name: Option<&str>,
    is_static: bool,
    diag_ctx: &DiagnosticWalkCtx<'_>,
) {
    let ctx = ForwardWalkCtx {
        current_class,
        all_classes: diag_ctx.local_classes,
        content: diag_ctx.content,
        cursor_offset: u32::MAX,
        class_loader: diag_ctx.class_loader,
        loaders: diag_ctx.loaders,
        resolved_class_cache: diag_ctx.resolved_class_cache,
        enclosing_return_type: None,
        top_level_scope: None,
    };

    seed_and_walk_function_body(
        parameters,
        body_statements,
        fn_span_start,
        method_name,
        is_static,
        &ctx,
    );
}

/// Seed a fresh scope for a function/method body and walk it for
/// diagnostic scope snapshots.
///
/// Shared by [`analyze_function_body`] (top-level functions and class
/// methods) and [`walk_anonymous_class_member_bodies`] (methods declared
/// inside an anonymous class expression).  Both need the same seeding:
/// `$this` for non-static methods, parameter types, and superglobals.
/// The only difference is the `current_class` carried by `ctx`.
pub(crate) fn seed_and_walk_function_body<'b>(
    parameters: impl Iterator<Item = &'b FunctionLikeParameter<'b>>,
    body_statements: impl Iterator<Item = &'b Statement<'b>>,
    fn_span_start: u32,
    method_name: Option<&str>,
    is_static: bool,
    ctx: &ForwardWalkCtx<'_>,
) {
    let mut scope = ScopeState::new();

    // Seed `$this` for non-static class methods so that expressions
    // referencing `$this` (e.g. `$this->prop`, `foreach ($this->items …)`)
    // resolve from the scope instead of falling through to the backward
    // scanner.
    if !is_static {
        seed_this(&mut scope, ctx.current_class);
    }

    // Seed scope with parameter types.
    // Detect whether this method has a #[Scope] attribute by scanning
    // the source text around the method span for `#[Scope]`.
    let has_scope_attr = method_name
        .map(|_| detect_scope_attribute_from_source(ctx.content, fn_span_start as usize))
        .unwrap_or(false);
    seed_params(
        &mut scope,
        parameters,
        fn_span_start,
        method_name,
        has_scope_attr,
        ctx,
    );

    // Seed superglobals so that accesses like `$_SERVER['key']` don't
    // remain unresolved.
    seed_superglobals(&mut scope);

    // Record the scope right at the function body start so that
    // member accesses on parameters before any assignment are covered.
    record_scope_snapshot(fn_span_start, &scope);

    // Walk the entire body, recording snapshots at each statement.
    walk_body_for_diagnostics(body_statements, &mut scope, ctx);
}

/// Walk the method bodies of an anonymous class expression, seeding
/// `$this` to the anonymous class itself.
///
/// Without this, the forward walker records `$this` snapshots for the
/// lexically enclosing method (whose `$this` is the outer class) and
/// those snapshots leak into the anonymous class's method bodies, since
/// they sit at higher offsets with no intervening re-seed.  Member
/// accesses like `$this->prop` inside the anonymous class would then
/// resolve against the outer class and be flagged as unknown.
pub(crate) fn walk_anonymous_class_member_bodies<'b>(
    anon: &'b AnonymousClass<'b>,
    ctx: &ForwardWalkCtx<'_>,
) {
    use mago_syntax::cst::class_like::member::ClassLikeMember;
    use mago_syntax::cst::class_like::method::MethodBody;

    // The parser extracts anonymous classes as `ClassInfo` with the
    // synthetic name `__anonymous@<left_brace_offset>`.  Look it up so
    // the walk sees the anonymous class's real members instead of the
    // enclosing class.
    let anon_name = format!("__anonymous@{}", anon.left_brace.start.offset);
    let Some(anon_class) = ctx.all_classes.iter().find(|c| *c.name == anon_name) else {
        return;
    };

    let anon_ctx = ForwardWalkCtx {
        current_class: anon_class.as_ref(),
        all_classes: ctx.all_classes,
        content: ctx.content,
        cursor_offset: ctx.cursor_offset,
        class_loader: ctx.class_loader,
        loaders: ctx.loaders,
        resolved_class_cache: ctx.resolved_class_cache,
        enclosing_return_type: None,
        top_level_scope: None,
    };

    for member in anon.members.iter() {
        if let ClassLikeMember::Method(method) = member
            && let MethodBody::Concrete(block) = &method.body
        {
            let method_name = bytes_to_str(method.name.value).to_string();
            let is_static = method.modifiers.contains_static();
            seed_and_walk_function_body(
                method.parameter_list.parameters.iter(),
                block.statements.iter(),
                method.span().start.offset,
                Some(&method_name),
                is_static,
                &anon_ctx,
            );
        }
    }
}

/// Find the innermost class whose body span contains `offset`.
///
/// This is the diagnostic-module equivalent of
/// [`find_innermost_enclosing_class`](crate::diagnostics::helpers::find_innermost_enclosing_class).
pub(crate) fn find_enclosing_class_for_offset(
    local_classes: &[Arc<ClassInfo>],
    offset: u32,
) -> Option<&ClassInfo> {
    local_classes
        .iter()
        .filter(|c| offset >= c.start_offset && offset <= c.end_offset)
        .min_by_key(|c| c.end_offset.saturating_sub(c.start_offset))
        .map(|c| c.as_ref())
}
